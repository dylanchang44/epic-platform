//! Process configuration; never accept a filesystem path from an HTTP request.
use std::path::PathBuf;

const DEFAULT_SCHWAB_DATA_DIR: &str = "/home/dylan/schwab-data";
const DEFAULT_RESEARCH_DB_PATH: &str = "data/research.db";

#[derive(Clone)]
pub struct AppConfig {
    pub schwab_data_dir: PathBuf,
    pub research_db_path: PathBuf,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schwab_data_dir: DEFAULT_SCHWAB_DATA_DIR.into(),
            research_db_path: DEFAULT_RESEARCH_DB_PATH.into(),
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            schwab_data_dir: std::env::var_os("SCHWAB_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_SCHWAB_DATA_DIR)),
            research_db_path: std::env::var_os("RESEARCH_DB_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| DEFAULT_RESEARCH_DB_PATH.into()),
        }
    }
}
