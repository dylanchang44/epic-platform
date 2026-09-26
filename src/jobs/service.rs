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
    repository: Result<JobRepository, JobError>,
    pub(crate) notify: Notify,
    pub(crate) worker_started: AtomicBool,
    pub(crate) worker_reserved: AtomicBool,
}
impl JobService {
    pub fn new(review: &ReviewService) -> Arc<Self> {
        Arc::new(Self {
            repository: review
                .execution_pool()
                .map(JobRepository::new)
                .map_err(|_| JobError::Unavailable),
            notify: Notify::new(),
            worker_started: AtomicBool::new(false),
            worker_reserved: AtomicBool::new(false),
        })
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
        if let Some(key) = key {
            if key.is_empty()
                || key.len() > 128
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                return Err(JobError::InvalidKey);
            }
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
            job_kind = "portfolio_review",
            state = "queued",
            attempt = job.attempt_count,
            "manual retry queued"
        );
        self.notify.notify_one();
        Ok((&job).into())
    }
}
