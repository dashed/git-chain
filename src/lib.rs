//! Application entry and typed CLI requests for git-chain.
//!
//! Repository and operation modules remain private while their boundaries are refactored.

mod branch;
mod chain;
pub mod cli;
mod commands;
mod error;
mod git_chain;
pub mod process;
mod rebase_state;
pub mod repository;
mod types;

pub(crate) use branch::Branch;
pub(crate) use chain::Chain;
pub(crate) use git_chain::GitChain;
pub use types::{
    MergeOptions, RebaseOptions, ReportLevel, SquashedMergeHandling, SquashedRebaseHandling,
};

/// Executes a parsed command in the current repository.
pub fn run(request: cli::CommandRequest) -> Result<(), git2::Error> {
    commands::run(request)
}

fn executable_name() -> String {
    let name = std::env::current_exe()
        .expect("Cannot get the path of current executable.")
        .file_name()
        .expect("Cannot get the executable name.")
        .to_string_lossy()
        .into_owned();
    if name.starts_with("git-") && name.len() > 4 {
        let tmp: Vec<String> = name.split("git-").map(|x| x.to_string()).collect();
        let git_cmd = &tmp[1];
        return format!("git {}", git_cmd);
    }
    name
}

fn check_gh_cli_installed() -> Result<(), git2::Error> {
    let output = std::process::Command::new("gh").arg("--version").output();
    match output {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(git2::Error::from_str(
            "The GitHub CLI (gh) is not installed or not found in the PATH.\nPlease install it from https://cli.github.com/ and ensure it's available in your PATH.",
        )),
    }
}
