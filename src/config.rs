//! Process configuration; never accept a filesystem path from an HTTP request.
use std::path::PathBuf;

const DEFAULT_SCHWAB_DATA_DIR: &str = "/home/dylan/schwab-data";
const DEFAULT_RESEARCH_DB_PATH: &str = "data/research.db";
const DEFAULT_REVIEW_DB_PATH: &str = "data/reviews.db";

#[derive(Clone)]
pub struct AppConfig {
    pub schwab_data_dir: PathBuf,
    pub research_db_path: PathBuf,
    pub review_db_path: PathBuf,
    pub watchlist:
        Result<crate::watchlist::domain::WatchlistInput, crate::watchlist::domain::WatchlistError>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schwab_data_dir: DEFAULT_SCHWAB_DATA_DIR.into(),
            research_db_path: DEFAULT_RESEARCH_DB_PATH.into(),
            review_db_path: DEFAULT_REVIEW_DB_PATH.into(),
            watchlist: crate::watchlist::domain::WatchlistInput::parse(""),
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            watchlist: match std::env::var("WATCHLIST_SYMBOLS") {
                Ok(value) => crate::watchlist::domain::WatchlistInput::parse(&value),
                Err(std::env::VarError::NotPresent) => {
                    crate::watchlist::domain::WatchlistInput::parse("")
                }
                Err(_) => Err(crate::watchlist::domain::WatchlistError::InvalidConfiguration),
            },
            schwab_data_dir: std::env::var_os("SCHWAB_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_SCHWAB_DATA_DIR)),
            research_db_path: std::env::var_os("RESEARCH_DB_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| DEFAULT_RESEARCH_DB_PATH.into()),
            review_db_path: std::env::var_os("REVIEW_DB_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| DEFAULT_REVIEW_DB_PATH.into()),
        }
    }
}
