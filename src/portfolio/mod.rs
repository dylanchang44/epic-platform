//! Shared domain and API data. Filesystem/parser code is native-only.
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ssr")]
pub mod loader;
#[cfg(feature = "ssr")]
mod parser;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub description: String,
    pub quantity: Option<Decimal>,
    pub price: Option<Decimal>,
    pub market_value: Decimal,
    pub cost_basis: Option<Decimal>,
    pub asset_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioSummary {
    pub position_count: usize,
    /// Signed sum of exported rows, including cash if present; not account equity.
    pub total_market_value: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Portfolio {
    pub positions: Vec<Position>,
    pub summary: PortfolioSummary,
    pub source_file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadError {
    pub code: String,
    pub message: String,
}

impl LoadError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadStatus {
    NotLoaded,
    Loading,
    Loaded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub data_directory: String,
    pub status: LoadStatus,
    pub last_successful_load: Option<String>,
    pub error: Option<LoadError>,
    pub portfolio: Option<Portfolio>,
}
