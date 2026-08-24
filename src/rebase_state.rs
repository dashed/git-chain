use std::fs;
use std::path::PathBuf;

use git2::{Error, Repository};

use crate::executable_name;
use crate::types::ChainRebaseState;

/// Schema version this build writes and accepts.
pub const STATE_VERSION: u32 = 1;

/// Returns the path the chain rebase state is written to.
///
/// This is the repository's *common* directory, not the per-worktree git directory. The two
/// are the same in the main worktree; in a linked worktree they differ, and a per-worktree
/// state file meant a rebase started in one worktree was invisible to every other — while the
/// branches it was rewriting were shared by all of them. One state per repository makes the
/// in-progress guard, `--status` and the recovery commands agree with the refs they protect
/// (REBASE_AUDIT M8).
pub fn state_file_path(repo: &Repository) -> PathBuf {
    repo.commondir().join("chain-rebase-state.json")
}

/// The per-worktree location used before the state moved to the common directory.
///
/// Identical to [`state_file_path`] in the main worktree, so it only names a distinct file
/// when running from a linked worktree.
fn legacy_state_file_path(repo: &Repository) -> PathBuf {
    repo.path().join("chain-rebase-state.json")
}

/// Returns the state file to read: the shared one, or a leftover from before the move.
///
/// A rebase paused by an older build of git-chain inside a linked worktree left its state at
/// the legacy path. Reading it there keeps that rebase recoverable; writes and deletes always
/// use the shared path, so the leftover is cleaned up rather than perpetuated.
pub fn state_file_path_for_read(repo: &Repository) -> PathBuf {
    let path = state_file_path(repo);
    if path.exists() {
        return path;
    }

    let legacy = legacy_state_file_path(repo);
    if legacy.exists() {
        return legacy;
    }

    path
}

/// Checks if a chain rebase state file exists.
pub fn state_exists(repo: &Repository) -> bool {
    state_file_path_for_read(repo).exists()
}

/// Reads and deserializes the chain rebase state file.
pub fn read_state(repo: &Repository) -> Result<ChainRebaseState, Error> {
    let path = state_file_path_for_read(repo);
    let contents = fs::read_to_string(&path).map_err(|e| {
        Error::from_str(&format!(
            "Failed to read chain rebase state file at {}: {}",
            path.display(),
            e
        ))
    })?;
    let state: ChainRebaseState = serde_json::from_str(&contents).map_err(|e| {
        Error::from_str(&format!(
            "Failed to parse chain rebase state file at {}: {}\n\
             Run '{} rebase --quit' to discard it without touching any branch.",
            path.display(),
            e,
            executable_name()
        ))
    })?;

    if state.version != STATE_VERSION {
        return Err(Error::from_str(&format!(
            "Unsupported chain rebase state version {} in {} (this build understands version {}).\n\
             The file was written by a different version of git-chain.\n\
             Run '{} rebase --quit' to discard it without touching any branch.",
            state.version,
            path.display(),
            STATE_VERSION,
            executable_name()
        )));
    }

    Ok(state)
}

/// Serializes and writes the chain rebase state to file.
///
/// Uses atomic write (write-to-temp-then-rename) to prevent corruption
/// if the process is killed mid-write.
pub fn write_state(repo: &Repository, state: &ChainRebaseState) -> Result<(), Error> {
    let path = state_file_path(repo);
    // Include the pid: two git-chain processes writing state in the same repository would
    // otherwise share one temp file, and the loser's partial write could be renamed into
    // place. The rename itself stays the atomic step (REBASE_AUDIT L6).
    let tmp_path = path.with_extension(format!("json.tmp.{}", std::process::id()));
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
///
/// Both locations are cleared, so finishing or abandoning a rebase also removes a leftover
/// written by an older build inside a linked worktree.
pub fn delete_state(repo: &Repository) -> Result<(), Error> {
    for path in [state_file_path(repo), legacy_state_file_path(repo)] {
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                Error::from_str(&format!(
                    "Failed to delete chain rebase state file at {}: {}",
                    path.display(),
                    e
                ))
            })?;
        }
    }
    Ok(())
}
