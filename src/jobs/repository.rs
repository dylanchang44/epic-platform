use super::domain::*;
use sqlx::{FromRow, SqlitePool};

#[derive(Clone)]
pub struct JobRepository {
    pool: SqlitePool,
}

// The single SELECT keeps current state and past failures in one read snapshot.
macro_rules! select {
    ($tail:literal) => { concat!("SELECT jobs.*, (SELECT json_group_array(json_object('attempt',attempt,'status',status,'step',step,'started_at',started_at,'completed_at',completed_at,'error',error)) FROM (SELECT * FROM job_failures WHERE job_id = jobs.id ORDER BY attempt)) AS failures_json FROM jobs", $tail) };
}

#[derive(FromRow)]
struct Row {
    id: i64,
    kind: String,
    input_json: String,
    status: String,
    created_at: String,
    queued_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    attempt_count: i64,
    step: String,
    error: Option<String>,
    result_id: Option<i64>,
    failures_json: String,
}
impl Row {
    fn decode(self) -> Result<Job, JobError> {
        fn decode<T: serde::de::DeserializeOwned>(s: String) -> Result<T, JobError> {
            serde_json::from_value(serde_json::Value::String(s)).map_err(failure)
        }
        let kind = decode(self.kind)?;
        let input: JobInput = serde_json::from_str(&self.input_json).map_err(failure)?;
        if input.kind() != kind {
            return Err(JobError::Repository);
        }
        Ok(Job {
            id: self.id,
            kind,
            input,
            status: decode(self.status)?,
            created_at: self.created_at,
            queued_at: self.queued_at,
            started_at: self.started_at,
            completed_at: self.completed_at,
            attempt_count: self.attempt_count,
            step: decode(self.step)?,
            error: self.error,
            result: self.result_id.map(|id| match kind {
                JobKind::PortfolioReview => JobResult::Review { id },
                JobKind::WatchlistBriefing => JobResult::WatchlistBriefing { id },
            }),
            previous_failures: serde_json::from_str(&self.failures_json).map_err(failure)?,
        })
    }
}

impl JobRepository {
    /// Pool is supplied by the Review execution database after its migrations.
    /// This repository owns only jobs/job_failures SQL.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn usable(&self) -> Result<(), JobError> {
        sqlx::query("SELECT id FROM jobs LIMIT 1")
            .execute(&self.pool)
            .await
            .map_err(failure)?;
        Ok(())
    }

    pub async fn by_key(&self, key: &str) -> Result<Option<Job>, JobError> {
        self.by_input_key(&JobInput::PortfolioReview, key).await
    }
    pub async fn by_input_key(&self, input: &JobInput, key: &str) -> Result<Option<Job>, JobError> {
        sqlx::query_as::<_, Row>(select!(
            " WHERE kind=? AND input_json=? AND idempotency_key = ?"
        ))
        .bind(input.kind().as_str())
        .bind(serde_json::to_string(input).map_err(failure)?)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(failure)?
        .map(Row::decode)
        .transpose()
    }

    pub async fn enqueue(&self, key: Option<&str>) -> Result<Job, JobError> {
        self.enqueue_input(&JobInput::PortfolioReview, key).await
    }
    pub async fn enqueue_input(
        &self,
        input: &JobInput,
        key: Option<&str>,
    ) -> Result<Job, JobError> {
        let now = now();
        // A concurrent repeated submission loses the unique-key race and reads
        // the existing job. A NULL key always means a separate intentional job.
        let id: Option<i64> = sqlx::query_scalar("INSERT INTO jobs(kind,input_json,status,created_at,queued_at,step,idempotency_key) VALUES (?,?,'queued',?,?,'queued',?) ON CONFLICT(kind,input_json,idempotency_key) DO NOTHING RETURNING id")
            .bind(input.kind().as_str()).bind(serde_json::to_string(input).map_err(failure)?).bind(&now).bind(&now).bind(key).fetch_optional(&self.pool).await.map_err(failure)?;
        match id {
            Some(id) => self.get(id).await,
            None => self
                .by_input_key(input, key.ok_or(JobError::Repository)?)
                .await?
                .ok_or(JobError::Repository),
        }
    }

    pub async fn get(&self, id: i64) -> Result<Job, JobError> {
        if id <= 0 {
            return Err(JobError::InvalidId);
        }
        sqlx::query_as::<_, Row>(select!(" WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(failure)?
            .ok_or(JobError::NotFound)?
            .decode()
    }

    pub async fn recent(&self) -> Result<Vec<Job>, JobError> {
        self.recent_kind(None).await
    }
    pub async fn recent_kind(&self, kind: Option<JobKind>) -> Result<Vec<Job>, JobError> {
        sqlx::query_as::<_, Row>(select!(
            " WHERE (? IS NULL OR kind=?) ORDER BY id DESC LIMIT 30"
        ))
        .bind(kind.map(JobKind::as_str))
        .bind(kind.map(JobKind::as_str))
        .fetch_all(&self.pool)
        .await
        .map_err(failure)?
        .into_iter()
        .map(Row::decode)
        .collect()
    }

    pub async fn running(&self) -> Result<Vec<Job>, JobError> {
        sqlx::query_as::<_, Row>(select!(" WHERE status = 'running' ORDER BY id"))
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?
            .into_iter()
            .map(Row::decode)
            .collect()
    }

    pub async fn claim(&self) -> Result<Option<Job>, JobError> {
        // One SQLite write statement obtains the writer lock before selection.
        // No SELECT-then-UPDATE race, even with two callers.
        let id: Option<i64> = sqlx::query_scalar("UPDATE jobs SET status='running',started_at=?,attempt_count=attempt_count+1,step=CASE kind WHEN 'portfolio_review' THEN 'snapshotting_portfolio' ELSE 'reading_watchlist' END WHERE id=(SELECT id FROM jobs WHERE status='queued' ORDER BY queued_at,id LIMIT 1) AND status='queued' RETURNING id")
            .bind(now()).fetch_optional(&self.pool).await.map_err(failure)?;
        match id {
            Some(id) => self.get(id).await.map(Some),
            None => Ok(None),
        }
    }

    pub async fn progress(&self, id: i64, attempt: i64, step: JobStep) -> Result<(), JobError> {
        let job = self.get(id).await?;
        let previous = match (job.kind, step) {
            (
                JobKind::PortfolioReview,
                JobStep::SnapshottingPortfolio | JobStep::LoadingResearch,
            ) => "snapshotting_portfolio",
            (JobKind::PortfolioReview, JobStep::CalculatingReview) => "loading_research",
            (JobKind::PortfolioReview, JobStep::PersistingReview) => "calculating_review",
            (JobKind::WatchlistBriefing, JobStep::ReadingWatchlist | JobStep::LoadingResearch) => {
                "reading_watchlist"
            }
            (JobKind::WatchlistBriefing, JobStep::PersistingBriefing) => "loading_research",
            _ => return Err(JobError::InvalidTransition),
        };
        changed(sqlx::query("UPDATE jobs SET step=? WHERE id=? AND attempt_count=? AND status='running' AND step=?")
            .bind(step.as_str()).bind(id).bind(attempt).bind(previous).execute(&self.pool).await.map_err(failure)?.rows_affected())
    }

    pub async fn succeed(&self, id: i64, attempt: i64, review_id: i64) -> Result<(), JobError> {
        self.succeed_result(id, attempt, JobResult::Review { id: review_id })
            .await
    }
    pub async fn succeed_result(
        &self,
        id: i64,
        attempt: i64,
        result: JobResult,
    ) -> Result<(), JobError> {
        changed(sqlx::query("UPDATE jobs SET status='succeeded',step='completed',completed_at=?,result_id=?,error=NULL WHERE id=? AND attempt_count=? AND status='running' AND kind=?")
            .bind(now()).bind(result.id()).bind(id).bind(attempt).bind(result.kind().as_str()).execute(&self.pool).await.map_err(failure)?.rows_affected())
    }

    pub async fn fail(&self, id: i64, attempt: i64, error: ExecutionError) -> Result<(), JobError> {
        let status = if matches!(error, ExecutionError::Interrupted) {
            "interrupted"
        } else {
            "failed"
        };
        let mut tx = self.pool.begin().await.map_err(failure)?;
        changed(sqlx::query("UPDATE jobs SET status=?,completed_at=?,error=? WHERE id=? AND attempt_count=? AND status='running'")
            .bind(status).bind(now()).bind(error.to_string()).bind(id).bind(attempt).execute(&mut *tx).await.map_err(failure)?.rows_affected())?;
        sqlx::query("INSERT INTO job_failures(job_id,attempt,status,step,started_at,completed_at,error) SELECT id,attempt_count,status,step,started_at,completed_at,error FROM jobs WHERE id=?")
            .bind(id).execute(&mut *tx).await.map_err(failure)?;
        tx.commit().await.map_err(failure)
    }

    pub async fn retry(&self, id: i64) -> Result<Job, JobError> {
        self.get(id).await?;
        changed(sqlx::query("UPDATE jobs SET status='queued',queued_at=?,started_at=NULL,completed_at=NULL,error=NULL,step='queued' WHERE id=? AND status IN ('failed','interrupted')")
            .bind(now()).bind(id).execute(&self.pool).await.map_err(failure)?.rows_affected())?;
        // attempt_count counts actual executions: the next atomic claim increments it.
        self.get(id).await
    }
}

fn changed(rows: u64) -> Result<(), JobError> {
    if rows == 1 {
        Ok(())
    } else {
        Err(JobError::InvalidTransition)
    }
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}
fn failure(error: impl std::fmt::Display) -> JobError {
    tracing::error!(%error, "job repository operation failed");
    JobError::Repository
}
