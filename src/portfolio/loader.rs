//! Read only the configured directory. Adapted from Schwab Tracker's discovery convention.
use std::{fs, path::Path};

use super::{LoadError, Portfolio, parser::parse_positions};

pub fn load_portfolio(directory: &Path) -> Result<Portfolio, LoadError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        let code = match error.kind() {
            std::io::ErrorKind::NotFound => "directory_missing",
            std::io::ErrorKind::PermissionDenied => "directory_unreadable",
            _ => "directory_invalid",
        };
        LoadError::new(
            code,
            format!(
                "Cannot read data directory {}: {error}",
                directory.display()
            ),
        )
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            LoadError::new(
                "directory_unreadable",
                format!("Cannot list data directory: {e}"),
            )
        })?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.contains("position") || !name.ends_with(".csv") {
            continue;
        }
        // Do not follow symlinks out of the configured directory.
        let file_type = entry
            .file_type()
            .map_err(|e| LoadError::new("file_unreadable", e.to_string()))?;
        if !file_type.is_file() {
            continue;
        }
        let modified = entry.metadata().and_then(|m| m.modified()).map_err(|e| {
            LoadError::new(
                "file_unreadable",
                format!("Cannot inspect {}: {e}", path.display()),
            )
        })?;
        candidates.push((modified, path));
    }
    // Path is a deterministic tie breaker when modification timestamps match.
    candidates.sort();
    let (_, path) = candidates.pop().ok_or_else(|| LoadError::new(
        "positions_missing", "No positions CSV found. Add a regular .csv file with 'position' in its name to the configured directory."
    ))?;
    let content = fs::read_to_string(&path).map_err(|e| {
        LoadError::new(
            "file_unreadable",
            format!("Cannot read {} as UTF-8: {e}", path.display()),
        )
    })?;
    parse_positions(
        &content,
        &path.file_name().unwrap_or_default().to_string_lossy(),
    )
}
