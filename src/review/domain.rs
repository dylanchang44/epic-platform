use crate::{
    portfolio::LoadStatus,
    research::domain::{ResearchError, ResearchSnapshot},
    symbol::StockSymbol,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Review reports its real execution boundaries without depending on Jobs.
#[derive(Clone, Copy)]
pub enum ReviewStep {
    SnapshottingPortfolio,
    LoadingResearch,
    CalculatingReview,
    PersistingReview,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Available,
    Missing,
    Unmatched,
    Unsupported,
    RepositoryFailure,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewResearch {
    /// Provenance within the research database; reads use the copied snapshot.
    pub snapshot_id: i64,
    pub snapshot: ResearchSnapshot,
    /// Age at review creation, in whole seconds; never recalculated on display.
    pub retrieval_age_seconds: i64,
    pub source_age_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewPosition {
    pub symbol: String,
    pub stock_symbol: Option<StockSymbol>,
    pub description: String,
    pub asset_type: String,
    pub quantity: Option<Decimal>,
    pub market_value: Decimal,
    /// Signed market value / positive signed portfolio total. None otherwise.
    pub weight_percent: Option<Decimal>,
    pub coverage: CoverageStatus,
    pub research: Option<ReviewResearch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LargestPosition {
    pub symbol: String,
    pub market_value: Decimal,
    pub gross_weight_percent: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub total_market_value: Decimal,
    pub gross_market_value: Decimal,
    pub position_count: usize,
    /// Largest absolute exposure; symbol provides a deterministic tie breaker.
    pub largest_position: LargestPosition,
    pub top_three_concentration_percent: Option<Decimal>,
    pub supported_position_count: usize,
    pub covered_position_count: usize,
    pub missing_position_count: usize,
    pub unmatched_position_count: usize,
    pub unsupported_position_count: usize,
    pub supported_gross_market_value: Decimal,
    pub covered_gross_market_value: Decimal,
    pub coverage_count_percent: Option<Decimal>,
    pub coverage_value_percent: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewDocument {
    pub calculation_version: u32,
    pub created_at: String,
    pub portfolio_captured_at: String,
    pub research_captured_at: String,
    pub portfolio_loaded_at: String,
    pub portfolio_load_status: LoadStatus,
    pub portfolio_source_file: String,
    pub research_error: Option<ResearchError>,
    pub summary: ReviewSummary,
    pub positions: Vec<ReviewPosition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedReview {
    pub id: i64,
    pub document: ReviewDocument,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewHistoryEntry {
    pub id: i64,
    pub created_at: String,
    pub summary: ReviewSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewComparison {
    pub previous_id: i64,
    pub total_value_change: Decimal,
    pub position_count_change: i64,
    pub previous_largest_symbol: String,
    pub current_largest_symbol: String,
    pub concentration_change_points: Option<Decimal>,
    pub coverage_count_change_points: Option<Decimal>,
    pub coverage_value_change_points: Option<Decimal>,
    pub added_symbols: Vec<String>,
    pub removed_symbols: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewDetail {
    pub review: SavedReview,
    pub comparison: Option<ReviewComparison>,
    pub comparison_error: Option<ReviewError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewError {
    NoPortfolio,
    InvalidPortfolio,
    Initialization,
    Migration,
    Repository,
    NotFound,
    InvalidId,
    Calculation,
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoPortfolio => "Load local Schwab data on Portfolio before creating a review.",
            Self::InvalidPortfolio => "The portfolio is empty or inconsistent. Reload valid local data before creating a review.",
            Self::Initialization => "Review database could not be opened. Check REVIEW_DB_PATH and permissions, then restart EPIC.",
            Self::Migration => "Review database migration failed. Check the server log and database version.",
            Self::Repository => "Review database operation failed. No partial review has been saved.",
            Self::NotFound => "This saved review does not exist.",
            Self::InvalidId => "Review ID must be a positive integer.",
            Self::Calculation => "Review calculation could not be completed safely; a numeric value or date is outside the supported range.",
        })
    }
}
impl std::error::Error for ReviewError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewApiError {
    pub error: ReviewError,
    pub message: String,
}
