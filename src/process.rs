//! Native process execution with explicit arguments and child-local configuration.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Output};

/// An executable invocation. Arguments are passed directly, without shell interpolation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    /// Overrides applied to the child's inherited environment only.
    pub env: Vec<(OsString, OsString)>,
}

/// Runs a command, preserving raw output and distinguishing spawn errors from exit status.
pub trait ProcessRunner {
    /// Captures stdout/stderr. A nonzero exit is an `Ok(Output)`, not an IO error.
    fn output(&self, command: &CommandSpec) -> io::Result<Output>;
}

/// Executes native processes using the operating system.
#[derive(Debug, Default)]
pub struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn output(&self, command: &CommandSpec) -> io::Result<Output> {
        Command::new(&command.program)
            .args(&command.args)
            .current_dir(&command.cwd)
            .envs(command.env.iter().map(|(key, value)| (key, value)))
            .output()
    }
}
