//! Process configuration; never accept a filesystem path from an HTTP request.
use std::path::PathBuf;

const DEFAULT_SCHWAB_DATA_DIR: &str = "/home/dylan/schwab-data";

#[derive(Clone)]
pub struct AppConfig {
    pub schwab_data_dir: PathBuf,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            schwab_data_dir: std::env::var_os("SCHWAB_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_SCHWAB_DATA_DIR)),
        }
    }
}
