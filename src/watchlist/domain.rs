use crate::{research::domain::SavedResearchSnapshot, symbol::StockSymbol};
use serde::{Deserialize, Serialize};

/// Canonical, bounded input. Order and duplicate spelling do not change identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct WatchlistInput(Vec<StockSymbol>);
impl WatchlistInput {
    pub fn parse(value: &str) -> Result<Self, WatchlistError> {
        if value.trim().is_empty() {
            return Ok(Self(Vec::new()));
        }
        Self::try_from(value.split(',').map(str::to_string).collect::<Vec<_>>())
    }
    pub fn symbols(&self) -> &[StockSymbol] {
        &self.0
    }
    pub fn validate_submission(&self) -> Result<(), WatchlistError> {
        if self.0.is_empty() {
            Err(WatchlistError::Empty)
        } else {
            Ok(())
        }
    }
}
impl TryFrom<Vec<String>> for WatchlistInput {
    type Error = WatchlistError;
    fn try_from(values: Vec<String>) -> Result<Self, Self::Error> {
        if values.len() > 32 {
            return Err(WatchlistError::InvalidConfiguration);
        }
        let mut symbols = values
            .iter()
            .map(|s| StockSymbol::parse(s).map_err(|_| WatchlistError::InvalidConfiguration))
            .collect::<Result<Vec<_>, _>>()?;
        symbols.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        symbols.dedup();
        Ok(Self(symbols))
    }
}
impl From<WatchlistInput> for Vec<String> {
    fn from(value: WatchlistInput) -> Self {
        value.0.into_iter().map(Into::into).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefingStatus {
    Available,
    NeverRefreshed,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BriefingEntry {
    pub symbol: StockSymbol,
    pub company_name: Option<String>,
    pub status: BriefingStatus,
    pub research: Option<SavedResearchSnapshot>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BriefingDocument {
    pub created_at: String,
    pub entries: Vec<BriefingEntry>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedBriefing {
    pub id: i64,
    pub document: BriefingDocument,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefingHistoryEntry {
    pub id: i64,
    pub created_at: String,
    pub symbols: Vec<StockSymbol>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistConfiguration {
    pub input: Option<WatchlistInput>,
    pub error: Option<WatchlistError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchlistError {
    Empty,
    InvalidConfiguration,
    Repository,
    ResearchUnavailable,
    NotFound,
    InvalidId,
}
impl std::fmt::Display for WatchlistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "Watchlist is empty. Set WATCHLIST_SYMBOLS on the server and restart.",
            Self::InvalidConfiguration => "Invalid WATCHLIST_SYMBOLS. Use at most 32 comma-separated ordinary stock symbols; empty entries and option symbols are not supported.",
            Self::Repository => "Briefing database operation failed. Existing saved briefings remain unchanged.",
            Self::ResearchUnavailable => "Saved research could not be read. No briefing was saved; this is not a Never refreshed result.",
            Self::NotFound => "This briefing does not exist.",
            Self::InvalidId => "Briefing ID must be a positive integer.",
        })
    }
}
impl std::error::Error for WatchlistError {}
