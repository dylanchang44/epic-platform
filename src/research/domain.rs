use crate::{
    portfolio::{HoldingSummary, LoadStatus},
    symbol::StockSymbol,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Initial coverage registry adapted from ConsensX. No alias or fuzzy matching.
const COMPANIES: &[(&str, &str)] = &[
    ("NVDA", "NVIDIA"),
    ("MSFT", "Microsoft"),
    ("GOOGL", "Alphabet"),
    ("NET", "Cloudflare"),
    ("AMD", "AMD"),
    ("ARM", "Arm Holdings"),
    ("SPCX", "SpaceX"),
    ("MU", "Micron"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Company {
    pub symbol: StockSymbol,
    pub name: String,
}

impl Company {
    pub fn find(symbol: &StockSymbol) -> Option<Self> {
        COMPANIES
            .iter()
            .find(|(s, _)| *s == symbol.as_str())
            .map(|(_, name)| Self {
                symbol: symbol.clone(),
                name: (*name).into(),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EarningsPeriod {
    pub label: String,
    pub ended_on: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchSnapshot {
    pub company: Company,
    pub period: EarningsPeriod,
    pub revenue: Decimal,
    pub diluted_eps: Decimal,
    pub currency: String,
    /// Timestamp reported by the source, not necessarily earnings publication time.
    pub source_updated_at: String,
    /// Retrieval that produced this immutable record; unchanged on same-period refresh.
    pub retrieved_at: String,
    pub sources: Vec<SourceLink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResearchError {
    InvalidSymbol,
    UnsupportedSymbol,
    Initialization,
    Migration,
    Repository,
    SourceUnavailable,
    Timeout,
    SourceHttp { status: u16 },
    MalformedSource,
    Busy,
}

impl std::fmt::Display for ResearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSymbol => f.write_str("Invalid stock symbol. Options and other complex instruments are not supported."),
            Self::UnsupportedSymbol => f.write_str("No matching company in the Stage 3 research registry."),
            Self::Initialization => f.write_str("Research database could not be opened. Check RESEARCH_DB_PATH and permissions, then restart EPIC."),
            Self::Migration => f.write_str("Research database migration failed. Check the server log and database version; portfolio remains available."),
            Self::Repository => f.write_str("Research database read or write failed. Any previously saved snapshot has not been replaced."),
            Self::SourceUnavailable => f.write_str("Stock Analysis is unavailable. Saved research remains available; try again later."),
            Self::Timeout => f.write_str("Stock Analysis request timed out. Saved research remains available."),
            Self::SourceHttp { status } => write!(f, "Stock Analysis returned HTTP {status}. Saved research remains available."),
            Self::MalformedSource => f.write_str("Stock Analysis returned malformed, incomplete, or unsupported data. Nothing was saved."),
            Self::Busy => f.write_str("Another research refresh is in progress. Try again after it finishes."),
        }
    }
}
impl std::error::Error for ResearchError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchStatus {
    Available,
    NeverRefreshed,
    Unmatched,
    Unsupported,
    RepositoryFailure,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanyView {
    pub symbol: String,
    pub company: Option<Company>,
    pub status: ResearchStatus,
    pub snapshot: Option<ResearchSnapshot>,
    pub error: Option<ResearchError>,
    /// The latest attempt in this process; separate from immutable saved facts.
    pub last_refresh_at: Option<String>,
    pub refresh_error: Option<ResearchError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingResearch {
    pub holding: HoldingSummary,
    pub research: CompanyView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingsResearch {
    pub portfolio_status: LoadStatus,
    pub portfolio_available: bool,
    pub repository_error: Option<ResearchError>,
    pub holdings: Vec<HoldingResearch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    Saved,
    Unchanged,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefreshResponse {
    pub outcome: RefreshOutcome,
    pub company: CompanyView,
}

#[cfg(feature = "ssr")]
impl ResearchSnapshot {
    pub fn validate(&self) -> Result<(), ResearchError> {
        let bad = ResearchError::MalformedSource;
        let end = chrono::NaiveDate::parse_from_str(&self.period.ended_on, "%Y-%m-%d")
            .map_err(|_| bad.clone())?;
        if Company::find(&self.company.symbol).is_none()
            || self.company.name.trim().is_empty()
            || self.period.label.trim().is_empty()
            || end.format("%Y-%m-%d").to_string() != self.period.ended_on
            || end > chrono::Utc::now().date_naive()
            || self.currency != "USD"
            || self.revenue.is_sign_negative()
            || chrono::DateTime::parse_from_rfc3339(&self.source_updated_at).is_err()
            || chrono::DateTime::parse_from_rfc3339(&self.retrieved_at).is_err()
            || self.sources.is_empty()
        {
            return Err(bad);
        }
        for source in &self.sources {
            let url = reqwest::Url::parse(&source.url).map_err(|_| bad.clone())?;
            if !matches!(url.scheme(), "https" | "http")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || source.label.trim().is_empty()
            {
                return Err(bad);
            }
        }
        Ok(())
    }
}
