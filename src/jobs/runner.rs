use super::{domain::*, service::JobService};
use crate::{
    research::service::ResearchService,
    review::{
        domain::{ReviewError, ReviewStep},
        service::ReviewService,
    },
    state::PortfolioState,
};
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::{
    sync::watch,
    task::{JoinHandle, JoinSet},
};
use tracing::Instrument;

pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

pub struct Worker {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}
impl Worker {
    pub fn request_stop(&self) {
        let _ = self.stop.send(true);
    }
    pub async fn shutdown(self) {
        self.request_stop();
        let _ = self.task.await;
    }
}

struct WorkerPermit(Arc<JobService>);
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.0.worker_started.store(false, Ordering::Release);
        self.0.worker_reserved.store(false, Ordering::Release);
    }
}

/// Must run before accepting work; assumes the single prior process has exited.
pub async fn reconcile(jobs: &JobService, review: &ReviewService) -> Result<(), JobError> {
    let repository = jobs.repository()?;
    for job in repository.running().await? {
        match saved_result(jobs, review, &job).await? {
            Some(saved) => {
                repository
                    .succeed_result(job.id, job.attempt_count, saved.clone())
                    .await?;
                tracing::info!(
                    job_id = job.id,
                    job_kind = job.kind.as_str(),
                    attempt = job.attempt_count,
                    state = "succeeded",
                    result_id = saved.id(),
                    "recovered committed result"
                );
            }
            None => {
                repository
                    .fail(job.id, job.attempt_count, ExecutionError::Interrupted)
                    .await?;
                tracing::warn!(
                    job_id = job.id,
                    job_kind = job.kind.as_str(),
                    attempt = job.attempt_count,
                    state = "interrupted",
                    "recovered abandoned execution"
                );
            }
        }
    }
    Ok(())
}

pub async fn start(
    jobs: Arc<JobService>,
    portfolio: Arc<PortfolioState>,
    research: Arc<ResearchService>,
    review: Arc<ReviewService>,
) -> Result<Worker, JobError> {
    jobs.worker_reserved
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| JobError::Unavailable)?;
    let permit = WorkerPermit(jobs.clone());
    reconcile(&jobs, &review).await?;
    jobs.repository()?.usable().await?;
    let (stop, mut stopping) = watch::channel(false);
    jobs.worker_started.store(true, Ordering::Release);
    let task = tokio::spawn(async move {
        let _permit = permit;
        loop {
            if *stopping.borrow() {
                break;
            }
            let started = std::sync::atomic::AtomicBool::new(false);
            let execution = async {
                started.store(true, Ordering::Release);
                run_one(
                    jobs.clone(),
                    portfolio.clone(),
                    research.clone(),
                    review.clone(),
                )
                .await
            };
            tokio::pin!(execution);
            let result = tokio::select! {
                biased;
                _ = stopping.changed() => {
                    jobs.worker_started.store(false, Ordering::Release);
                    // Dropping execution aborts its JoinSet child. SQLite may already
                    // have committed: leave Running for startup reconciliation.
                    if started.load(Ordering::Acquire) && tokio::time::timeout(SHUTDOWN_GRACE, &mut execution).await.is_err() {
                        tracing::warn!("worker shutdown grace expired; startup will reconcile running work");
                    }
                    break;
                },
                result = &mut execution => result,
            };
            match result {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    tracing::error!(%error, "worker stopped; database transition could not be recorded");
                    break;
                }
            }
            tokio::select! {
                biased;
                _ = stopping.changed() => break,
                _ = jobs.notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_secs(5)) => {},
            }
        }
        tracing::info!("job worker stopped");
    });
    Ok(Worker { stop, task })
}

/// Execute at most one durable job. Also used by deterministic offline tests.
pub async fn run_one(
    jobs: Arc<JobService>,
    portfolio: Arc<PortfolioState>,
    research: Arc<ResearchService>,
    review: Arc<ReviewService>,
) -> Result<bool, JobError> {
    let Some(job) = jobs.repository()?.claim().await? else {
        return Ok(false);
    };
    let span = tracing::info_span!(
        "job",
        job_id = job.id,
        job_kind = job.kind.as_str(),
        attempt = job.attempt_count
    );
    async move {
        let elapsed = Instant::now();
        tracing::info!(state="running", step=job.step.as_str(), "job claimed");
        let task_jobs = jobs.clone();
        let task_review = review.clone();
        let task_input = job.input.clone();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            match job.kind {
                JobKind::PortfolioReview => task_review.create_for_job(&portfolio, &research, Some(job.id), |step| {
                    let jobs = task_jobs.clone();
                    async move {
                        let step = match step {
                            ReviewStep::SnapshottingPortfolio => JobStep::SnapshottingPortfolio,
                            ReviewStep::LoadingResearch => JobStep::LoadingResearch,
                            ReviewStep::CalculatingReview => JobStep::CalculatingReview,
                            ReviewStep::PersistingReview => JobStep::PersistingReview,
                        };
                        jobs.repository().map_err(|_| ReviewError::Repository)?.progress(job.id, job.attempt_count, step).await.map_err(|_| ReviewError::Repository)?;
                        tracing::info!(state="running", step=step.as_str(), "job progress");
                        Ok(())
                    }
                }).await.map(|saved| JobResult::Review { id:saved.id }).map_err(ExecutionError::Review),
                JobKind::WatchlistBriefing => {
                    let JobInput::WatchlistBriefing { symbols } = task_input else { unreachable!("repository validates kind and input") };
                    task_jobs.watchlist().create_for_job(&research,symbols,job.id,|step| {
                        let jobs = task_jobs.clone();
                        async move {
                            use crate::watchlist::{domain::WatchlistError,service::BriefingStep};
                            let step = match step {
                                BriefingStep::ReadingWatchlist => JobStep::ReadingWatchlist,
                                BriefingStep::LoadingResearch => JobStep::LoadingResearch,
                                BriefingStep::PersistingBriefing => JobStep::PersistingBriefing,
                            };
                            jobs.repository().map_err(|_| WatchlistError::Repository)?.progress(job.id,job.attempt_count,step).await.map_err(|_| WatchlistError::Repository)?;
                            tracing::info!(state="running",step=step.as_str(),"job progress");
                            Ok(())
                        }
                    }).await.map(|saved| JobResult::WatchlistBriefing { id:saved.id }).map_err(ExecutionError::Watchlist)
                },
            }
        }.in_current_span());
        let result = tasks.join_next().await.expect("one execution task");
        match result {
            Ok(Ok(saved)) => {
                jobs.repository()?.succeed_result(job.id, job.attempt_count, saved.clone()).await?;
                tracing::info!(state="succeeded", elapsed_ms=elapsed.elapsed().as_millis() as u64, result_id=saved.id(), "job completed");
            }
            failure => {
                // A failed/ambiguous COMMIT acknowledgement must not hide a
                // committed result or encourage a duplicate on manual retry.
                if let Some(saved) = saved_result(&jobs,&review,&job).await? {
                    jobs.repository()?.succeed_result(job.id, job.attempt_count, saved.clone()).await?;
                    tracing::info!(state="succeeded", elapsed_ms=elapsed.elapsed().as_millis() as u64, result_id=saved.id(), "reconciled execution result");
                } else {
                    let error = match failure { Ok(Err(error)) => error, _ => ExecutionError::Panicked };
                    jobs.repository()?.fail(job.id, job.attempt_count, error.clone()).await?;
                    tracing::warn!(state="failed", elapsed_ms=elapsed.elapsed().as_millis() as u64, %error, "job failed");
                }
            }
        }
        Ok(true)
    }.instrument(span).await
}

/// The same lookup protects both restart recovery and ambiguous commit errors.
async fn saved_result(
    jobs: &JobService,
    review: &ReviewService,
    job: &Job,
) -> Result<Option<JobResult>, JobError> {
    match job.kind {
        JobKind::PortfolioReview => review
            .by_origin_job(job.id)
            .await
            .map(|saved| saved.map(|s| JobResult::Review { id: s.id }))
            .map_err(|_| JobError::Repository),
        JobKind::WatchlistBriefing => jobs
            .watchlist()
            .repository()
            .map_err(|_| JobError::Repository)?
            .by_origin_job(job.id)
            .await
            .map(|saved| saved.map(|s| JobResult::WatchlistBriefing { id: s.id }))
            .map_err(|_| JobError::Repository),
    }
}
