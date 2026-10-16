use git2::Repository;

use crate::process::SystemProcessRunner;
use crate::repository::rebase::RebaseCommands;

pub struct GitChain {
    pub repo: Repository,
    pub executable_name: String,
    pub rebase_commands: RebaseCommands<SystemProcessRunner>,
}

// Re-export impl blocks
mod core;
mod merge;
mod operations;
mod rebase_executor;
