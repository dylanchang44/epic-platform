use super::domain::*;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::{path::Path, time::Duration};

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./src/review/migrations");

pub struct ReviewRepository {
    pool: SqlitePool,
}

impl ReviewRepository {
    /// Jobs shares this execution database, but owns its own repository/tables.
    pub fn execution_pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    pub async fn by_origin_job(&self, job_id: i64) -> Result<Option<SavedReview>, ReviewError> {
        let id: Option<i64> = sqlx::query_scalar("SELECT id FROM reviews WHERE origin_job_id=?")
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(failure)?;
        match id {
            Some(id) => self.get(id).await.map(Some),
            None => Ok(None),
        }
    }
    pub async fn open(path: &Path) -> Result<Self, ReviewError> {
        if path.as_os_str().is_empty() {
            return Err(ReviewError::Initialization);
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                eprintln!("Review directory initialization: {error}");
                ReviewError::Initialization
            })?;
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(3)
            .acquire_timeout(Duration::from_secs(3))
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .busy_timeout(Duration::from_secs(3)),
            )
            .await
            .map_err(|error| {
                eprintln!("Review database initialization: {error}");
                ReviewError::Initialization
            })?;
        MIGRATIONS.run(&pool).await.map_err(|error| {
            eprintln!("Review migration: {error}");
            ReviewError::Migration
        })?;
        Ok(Self { pool })
    }

    pub async fn save(&self, document: ReviewDocument) -> Result<SavedReview, ReviewError> {
        self.save_for_job(document, None).await
    }

    pub async fn save_for_job(
        &self,
        document: ReviewDocument,
        job_id: Option<i64>,
    ) -> Result<SavedReview, ReviewError> {
        let json = serde_json::to_string(&document).map_err(failure)?;
        let summary = serde_json::to_string(&document.summary).map_err(failure)?;
        // All input reads/calculations precede BEGIN. One row is one complete review.
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        let id: Option<i64> = sqlx::query_scalar(
            "INSERT INTO reviews(created_at, summary_json, document_json, origin_job_id) VALUES (?, ?, ?, ?) ON CONFLICT(origin_job_id) DO NOTHING RETURNING id",
        )
        .bind(&document.created_at)
        .bind(summary)
        .bind(json)
        .bind(job_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(failure)?;
        transaction.commit().await.map_err(failure)?;
        match id {
            Some(id) => Ok(SavedReview { id, document }),
            None => self
                .by_origin_job(job_id.ok_or(ReviewError::Repository)?)
                .await?
                .ok_or(ReviewError::Repository),
        }
    }

    pub async fn get(&self, id: i64) -> Result<SavedReview, ReviewError> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT document_json FROM reviews WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(failure)?;
        let document =
            serde_json::from_str(&json.ok_or(ReviewError::NotFound)?).map_err(failure)?;
        Ok(SavedReview { id, document })
    }

    pub async fn history(&self) -> Result<Vec<ReviewHistoryEntry>, ReviewError> {
        let rows: Vec<(i64, String, String)> =
            sqlx::query_as("SELECT id, created_at, summary_json FROM reviews ORDER BY id DESC")
                .fetch_all(&self.pool)
                .await
                .map_err(failure)?;
        rows.into_iter()
            .map(|(id, created_at, json)| {
                Ok(ReviewHistoryEntry {
                    id,
                    created_at,
                    summary: serde_json::from_str(&json).map_err(failure)?,
                })
            })
            .collect()
    }

    pub async fn previous(&self, id: i64) -> Result<Option<SavedReview>, ReviewError> {
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, document_json FROM reviews WHERE id < ? ORDER BY id DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(failure)?;
        row.map(|(id, json)| {
            Ok(SavedReview {
                id,
                document: serde_json::from_str(&json).map_err(failure)?,
            })
        })
        .transpose()
    }
}

fn failure(error: impl std::fmt::Display) -> ReviewError {
    eprintln!("Review repository: {error}");
    ReviewError::Repository
}
