//! Repository identity and native-command adapters.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use git2::Repository;

use crate::process::CommandSpec;

pub mod rebase;

/// Captured repository locations and the invocation directory for native commands.
///
/// The invocation directory is kept separately from the worktree root to preserve commands
/// started in a subdirectory. This context does not change the process's working directory.
#[derive(Debug, Clone)]
pub struct RepoContext {
    command_dir: PathBuf,
    worktree_root: Option<PathBuf>,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

impl RepoContext {
    /// Captures locations from a discovered repository and an absolute invocation directory.
    pub fn from_repository(repo: &Repository, command_dir: PathBuf) -> io::Result<Self> {
        if !command_dir.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Repository command directory must be absolute",
            ));
        }
        Ok(Self {
            command_dir,
            worktree_root: repo.workdir().map(Path::to_path_buf),
            git_dir: repo.path().to_path_buf(),
            common_dir: repo.commondir().to_path_buf(),
        })
    }

    /// Returns the directory from which native commands execute.
    pub fn command_dir(&self) -> &Path {
        &self.command_dir
    }

    /// Returns the worktree root, or `None` for a bare repository.
    pub fn worktree_root(&self) -> Option<&Path> {
        self.worktree_root.as_deref()
    }

    /// Returns this worktree's Git directory.
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Returns the common Git directory shared by linked worktrees.
    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    fn git_command(&self, args: Vec<OsString>) -> CommandSpec {
        CommandSpec {
            program: "git".into(),
            args,
            cwd: self.command_dir.clone(),
            env: Vec::new(),
        }
    }
}
