use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    PortfolioReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl JobStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
    pub fn can_retry(self) -> bool {
        matches!(self, Self::Failed | Self::Interrupted)
    }
    pub fn allows(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Running)
                | (
                    Self::Running,
                    Self::Succeeded | Self::Failed | Self::Interrupted
                )
                | (Self::Failed | Self::Interrupted, Self::Queued)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStep {
    Queued,
    SnapshottingPortfolio,
    LoadingResearch,
    CalculatingReview,
    PersistingReview,
    Completed,
}

impl JobStep {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::SnapshottingPortfolio => "snapshotting_portfolio",
            Self::LoadingResearch => "loading_research",
            Self::CalculatingReview => "calculating_review",
            Self::PersistingReview => "persisting_review",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobFailure {
    pub attempt: i64,
    pub status: JobStatus,
    pub step: JobStep,
    pub started_at: String,
    pub completed_at: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobResult {
    Review { id: i64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: i64,
    pub kind: JobKind,
    pub status: JobStatus,
    pub created_at: String,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub attempt_count: i64,
    pub step: JobStep,
    pub error: Option<String>,
    pub result: Option<JobResult>,
    pub previous_failures: Vec<JobFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSubmission {
    pub job_id: i64,
    pub status: JobStatus,
    pub status_url: String,
}
impl From<&Job> for JobSubmission {
    fn from(job: &Job) -> Self {
        Self {
            job_id: job.id,
            status: job.status,
            status_url: format!("/api/jobs/{}", job.id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobError {
    Repository,
    NotFound,
    InvalidId,
    InvalidKey,
    InvalidTransition,
    Unavailable,
    NoPortfolio,
    InvalidPortfolio,
}
impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Repository => "Job database operation failed. Check job status before resubmitting.",
            Self::NotFound => "This job does not exist.",
            Self::InvalidId => "Job ID must be a positive integer.",
            Self::InvalidKey => "Idempotency-Key must contain 1–128 ASCII letters, digits, dots, underscores or hyphens.",
            Self::InvalidTransition => "This job cannot make that state transition. Only failed or interrupted jobs can be retried.",
            Self::Unavailable => "The review worker is unavailable. Check readiness and restart after resolving the server error.",
            Self::NoPortfolio => "Load local Schwab data on Portfolio before creating a review.",
            Self::InvalidPortfolio => "The portfolio is empty or inconsistent. Reload valid local data.",
        })
    }
}
impl std::error::Error for JobError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobApiError {
    pub error: JobError,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum ExecutionError {
    Review(crate::review::domain::ReviewError),
    Interrupted,
    Panicked,
}
impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Review(error) => error.fmt(f),
            Self::Interrupted => f.write_str("Execution stopped before a saved review was found. You may retry manually."),
            Self::Panicked => f.write_str("The review task stopped unexpectedly. Check the server log; you may retry manually."),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Readiness {
    pub ready: bool,
    pub migrations_completed: bool,
    pub job_repository_usable: bool,
    pub worker_started: bool,
}
