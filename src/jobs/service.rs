use super::{domain::*, repository::JobRepository};
use crate::{
    review::{domain::ReviewError, service::ReviewService},
    state::PortfolioState,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

pub struct JobService {
    watchlist: crate::watchlist::service::WatchlistService,
    repository: Result<JobRepository, JobError>,
    pub(crate) notify: Notify,
    pub(crate) worker_started: AtomicBool,
    pub(crate) worker_reserved: AtomicBool,
}
impl JobService {
    pub fn new(review: &ReviewService) -> Arc<Self> {
        Self::with_watchlist(review, crate::watchlist::domain::WatchlistInput::parse(""))
    }
    /// Runtime composition: workflow services keep their own domain/repository logic.
    pub fn with_watchlist(
        review: &ReviewService,
        input: Result<
            crate::watchlist::domain::WatchlistInput,
            crate::watchlist::domain::WatchlistError,
        >,
    ) -> Arc<Self> {
        Arc::new(Self {
            watchlist: crate::watchlist::service::WatchlistService::new(
                review
                    .execution_pool()
                    .map_err(|_| crate::watchlist::domain::WatchlistError::Repository),
                input,
            ),
            repository: review
                .execution_pool()
                .map(JobRepository::new)
                .map_err(|_| JobError::Unavailable),
            notify: Notify::new(),
            worker_started: AtomicBool::new(false),
            worker_reserved: AtomicBool::new(false),
        })
    }
    pub fn watchlist(&self) -> &crate::watchlist::service::WatchlistService {
        &self.watchlist
    }
    pub async fn submit_briefing(&self, key: Option<&str>) -> Result<JobSubmission, JobError> {
        validate_key(key)?;
        let input = JobInput::WatchlistBriefing {
            symbols: self
                .watchlist
                .submission_input()
                .map_err(|error| JobError::Watchlist { error })?,
        };
        if let Some(key) = key
            && let Some(job) = self.repository()?.by_input_key(&input, key).await?
        {
            return Ok((&job).into());
        }
        if !self.worker_started() {
            return Err(JobError::Unavailable);
        }
        let job = self.repository()?.enqueue_input(&input, key).await?;
        tracing::info!(job_id=job.id,job_kind=job.kind.as_str(),state=?job.status,attempt=job.attempt_count,"job submitted");
        self.notify.notify_one();
        Ok((&job).into())
    }
    pub fn repository(&self) -> Result<&JobRepository, JobError> {
        self.repository.as_ref().map_err(Clone::clone)
    }
    pub fn worker_started(&self) -> bool {
        self.worker_started.load(Ordering::Acquire)
    }
    pub async fn usable(&self) -> bool {
        match &self.repository {
            Ok(repo) => repo.usable().await.is_ok(),
            Err(_) => false,
        }
    }
    pub async fn submit(
        &self,
        portfolio: &PortfolioState,
        key: Option<&str>,
    ) -> Result<JobSubmission, JobError> {
        validate_key(key)?;
        if let Some(key) = key {
            // A lost response can be recovered even if portfolio/worker is now unavailable.
            if let Some(job) = self.repository()?.by_key(key).await? {
                return Ok((&job).into());
            }
        }
        if !self.worker_started() {
            return Err(JobError::Unavailable);
        }
        ReviewService::validate_submission(portfolio)
            .await
            .map_err(|e| match e {
                ReviewError::NoPortfolio => JobError::NoPortfolio,
                _ => JobError::InvalidPortfolio,
            })?;
        let job = self.repository()?.enqueue(key).await?;
        tracing::info!(job_id=job.id, job_kind="portfolio_review", state=?job.status, attempt=job.attempt_count, "job submitted");
        // A notification is only a wake-up hint. Commit already happened.
        self.notify.notify_one();
        Ok((&job).into())
    }
    pub async fn get(&self, id: i64) -> Result<Job, JobError> {
        self.repository()?.get(id).await
    }
    pub async fn recent(&self) -> Result<Vec<Job>, JobError> {
        self.repository()?.recent().await
    }
    pub async fn retry(&self, id: i64) -> Result<JobSubmission, JobError> {
        let existing = self.get(id).await?;
        if !existing.status.can_retry() {
            return Err(JobError::InvalidTransition);
        }
        if !self.worker_started() {
            return Err(JobError::Unavailable);
        }
        let job = self.repository()?.retry(id).await?;
        tracing::info!(
            job_id = id,
            job_kind = job.kind.as_str(),
            state = "queued",
            attempt = job.attempt_count,
            "manual retry queued"
        );
        self.notify.notify_one();
        Ok((&job).into())
    }
}

fn validate_key(key: Option<&str>) -> Result<(), JobError> {
    if let Some(key) = key
        && (key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
    {
        return Err(JobError::InvalidKey);
    }
    Ok(())
}
