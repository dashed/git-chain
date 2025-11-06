use std::ffi::OsString;
use std::process;
use std::process::Command;

use colored::*;
use git2::Error;

mod branch;
mod chain;
mod cli;
mod commands;
mod error;
mod git_chain;
mod types;

use cli::parse_arg_matches;
use commands::run;

// Re-export for use by other modules
pub use branch::Branch;
pub use chain::Chain;
pub use git_chain::GitChain;

pub fn executable_name() -> String {
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

fn run_app<I, T>(arguments: I)
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arg_matches = parse_arg_matches(arguments);

    match run(arg_matches) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("{} {}", "error:".red().bold(), err);
            process::exit(1);
        }
    }
}

fn main() {
    run_app(std::env::args_os());
}

pub fn check_gh_cli_installed() -> Result<(), Error> {
    let output = Command::new("gh").arg("--version").output();
    match output {
        Ok(output) if output.status.success() => Ok(()),
        _ => {
            eprintln!("The GitHub CLI (gh) is not installed or not found in the PATH.");
            eprintln!("Please install it from https://cli.github.com/ and ensure it's available in your PATH.");
            process::exit(1);
        }
    }
}
