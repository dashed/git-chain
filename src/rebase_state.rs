use std::fs;
use std::path::PathBuf;

use git2::{Error, Repository};

use crate::types::ChainRebaseState;

/// Returns the path to the chain rebase state file.
pub fn state_file_path(repo: &Repository) -> PathBuf {
    repo.path().join("chain-rebase-state.json")
}

/// Checks if a chain rebase state file exists.
pub fn state_exists(repo: &Repository) -> bool {
    state_file_path(repo).exists()
}

/// Reads and deserializes the chain rebase state file.
pub fn read_state(repo: &Repository) -> Result<ChainRebaseState, Error> {
    let path = state_file_path(repo);
    let contents = fs::read_to_string(&path).map_err(|e| {
        Error::from_str(&format!(
            "Failed to read chain rebase state file at {}: {}",
            path.display(),
            e
        ))
    })?;
    serde_json::from_str(&contents)
        .map_err(|e| Error::from_str(&format!("Failed to parse chain rebase state file: {}", e)))
}

/// Serializes and writes the chain rebase state to file.
///
/// Uses atomic write (write-to-temp-then-rename) to prevent corruption
/// if the process is killed mid-write.
pub fn write_state(repo: &Repository, state: &ChainRebaseState) -> Result<(), Error> {
    let path = state_file_path(repo);
    let tmp_path = path.with_extension("json.tmp");
    let contents = serde_json::to_string_pretty(state)
        .map_err(|e| Error::from_str(&format!("Failed to serialize chain rebase state: {}", e)))?;
    fs::write(&tmp_path, &contents).map_err(|e| {
        Error::from_str(&format!(
            "Failed to write temporary chain rebase state file at {}: {}",
            tmp_path.display(),
            e
        ))
    })?;
    fs::rename(&tmp_path, &path).map_err(|e| {
        Error::from_str(&format!(
            "Failed to rename temporary state file {} to {}: {}",
            tmp_path.display(),
            path.display(),
            e
        ))
    })
}

/// Deletes the chain rebase state file if it exists.
pub fn delete_state(repo: &Repository) -> Result<(), Error> {
    let path = state_file_path(repo);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| {
            Error::from_str(&format!(
                "Failed to delete chain rebase state file at {}: {}",
                path.display(),
                e
            ))
        })?;
    }
    Ok(())
}
