use super::domain::*;
use sqlx::SqlitePool;

pub struct BriefingRepository {
    pool: SqlitePool,
}
impl BriefingRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
    pub async fn by_origin_job(
        &self,
        job_id: i64,
    ) -> Result<Option<SavedBriefing>, WatchlistError> {
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT id,document_json FROM watchlist_briefings WHERE origin_job_id=?",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(failure)?;
        row.map(decode).transpose()
    }
    pub async fn save(
        &self,
        document: BriefingDocument,
        job_id: i64,
    ) -> Result<SavedBriefing, WatchlistError> {
        let json = serde_json::to_string(&document).map_err(failure)?;
        let symbols = serde_json::to_string(
            &document
                .entries
                .iter()
                .map(|e| &e.symbol)
                .collect::<Vec<_>>(),
        )
        .map_err(failure)?;
        let mut tx = self.pool.begin().await.map_err(failure)?;
        let id: Option<i64> = sqlx::query_scalar("INSERT INTO watchlist_briefings(created_at,symbols_json,document_json,origin_job_id) VALUES (?,?,?,?) ON CONFLICT(origin_job_id) DO NOTHING RETURNING id")
            .bind(&document.created_at).bind(symbols).bind(json).bind(job_id).fetch_optional(&mut *tx).await.map_err(failure)?;
        tx.commit().await.map_err(failure)?;
        match id {
            Some(id) => Ok(SavedBriefing { id, document }),
            None => self
                .by_origin_job(job_id)
                .await?
                .ok_or(WatchlistError::Repository),
        }
    }
    pub async fn get(&self, id: i64) -> Result<SavedBriefing, WatchlistError> {
        if id <= 0 {
            return Err(WatchlistError::InvalidId);
        }
        let row = sqlx::query_as("SELECT id,document_json FROM watchlist_briefings WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(failure)?
            .ok_or(WatchlistError::NotFound)?;
        decode(row)
    }
    pub async fn history(&self) -> Result<Vec<BriefingHistoryEntry>, WatchlistError> {
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT id,created_at,symbols_json FROM watchlist_briefings ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(failure)?;
        rows.into_iter()
            .map(|(id, created_at, json)| {
                Ok(BriefingHistoryEntry {
                    id,
                    created_at,
                    symbols: serde_json::from_str(&json).map_err(failure)?,
                })
            })
            .collect()
    }
}
fn decode((id, json): (i64, String)) -> Result<SavedBriefing, WatchlistError> {
    Ok(SavedBriefing {
        id,
        document: serde_json::from_str(&json).map_err(failure)?,
    })
}
fn failure(error: impl std::fmt::Display) -> WatchlistError {
    tracing::error!(%error,"briefing repository operation failed");
    WatchlistError::Repository
}
