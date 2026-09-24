//! The sole SQLite owner. SQL rows never escape this module.
use super::domain::{ResearchError, ResearchSnapshot};
use crate::symbol::StockSymbol;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::{path::Path, time::Duration};

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./src/research/migrations");

pub struct ResearchRepository {
    pool: SqlitePool,
}

impl ResearchRepository {
    pub async fn open(path: &Path) -> Result<Self, ResearchError> {
        if path.as_os_str().is_empty() {
            return Err(ResearchError::Initialization);
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                eprintln!("Research directory initialization: {error}");
                ResearchError::Initialization
            })?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(3));
        let pool = SqlitePoolOptions::new()
            .max_connections(3)
            .acquire_timeout(Duration::from_secs(3))
            .connect_with(options)
            .await
            .map_err(|error| {
                eprintln!("Research database initialization: {error}");
                ResearchError::Initialization
            })?;
        MIGRATIONS.run(&pool).await.map_err(|error| {
            eprintln!("Research database migration: {error}");
            ResearchError::Migration
        })?;
        Ok(Self { pool })
    }

    pub async fn latest(
        &self,
        symbol: &StockSymbol,
    ) -> Result<Option<ResearchSnapshot>, ResearchError> {
        let json: Option<String> = sqlx::query_scalar("SELECT snapshot_json FROM research_snapshots WHERE symbol = ? ORDER BY period_end DESC LIMIT 1")
            .bind(symbol.as_str()).fetch_optional(&self.pool).await.map_err(repository_error)?;
        json.map(|json| decode(&json)).transpose()
    }

    pub async fn history(
        &self,
        symbol: &StockSymbol,
    ) -> Result<Vec<ResearchSnapshot>, ResearchError> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT snapshot_json FROM research_snapshots WHERE symbol = ? ORDER BY period_end DESC")
            .bind(symbol.as_str()).fetch_all(&self.pool).await.map_err(repository_error)?;
        rows.iter().map(|json| decode(json)).collect()
    }

    /// A same-period response never overwrites facts. Latest is selected by
    /// period end, so an older source response cannot roll the current view back.
    pub async fn save(&self, snapshot: &ResearchSnapshot) -> Result<bool, ResearchError> {
        snapshot.validate()?;
        let json = serde_json::to_string(snapshot).map_err(repository_error)?;
        // Network work and validation have finished before the transaction begins.
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;
        let inserted = sqlx::query("INSERT INTO research_snapshots (symbol, earnings_period, period_end, retrieved_at, snapshot_json) VALUES (?, ?, ?, ?, ?) ON CONFLICT DO NOTHING")
            .bind(snapshot.company.symbol.as_str()).bind(&snapshot.period.label)
            .bind(&snapshot.period.ended_on).bind(&snapshot.retrieved_at).bind(json)
            .execute(&mut *transaction).await.map_err(repository_error)?.rows_affected() == 1;
        transaction.commit().await.map_err(repository_error)?;
        Ok(inserted)
    }
}

fn decode(json: &str) -> Result<ResearchSnapshot, ResearchError> {
    let snapshot: ResearchSnapshot = serde_json::from_str(json).map_err(repository_error)?;
    snapshot.validate().map_err(repository_error)?;
    Ok(snapshot)
}

fn repository_error(error: impl std::fmt::Display) -> ResearchError {
    eprintln!("Research repository: {error}");
    ResearchError::Repository
}
