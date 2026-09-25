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
        let json = serde_json::to_string(&document).map_err(failure)?;
        let summary = serde_json::to_string(&document.summary).map_err(failure)?;
        // All input reads/calculations precede BEGIN. One row is one complete review.
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        let id = sqlx::query(
            "INSERT INTO reviews(created_at, summary_json, document_json) VALUES (?, ?, ?)",
        )
        .bind(&document.created_at)
        .bind(summary)
        .bind(json)
        .execute(&mut *transaction)
        .await
        .map_err(failure)?
        .last_insert_rowid();
        transaction.commit().await.map_err(failure)?;
        Ok(SavedReview { id, document })
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
