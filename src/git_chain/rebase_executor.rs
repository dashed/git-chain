//! One branch's rebase mechanics, shared by fresh runs and both resume commands.

use std::io::{self, Write};
use std::process::{Command, Output};

use colored::*;
use git2::{Error, RepositoryState};

use super::GitChain;
use crate::error::ErrorExt;
use crate::types::{BranchRebaseStatus, ChainRebaseState, SquashedRebaseHandling};

/// Frozen replay inputs and policy for one member of the chain.
#[derive(Clone, Copy)]
pub(super) struct BranchRebase<'a> {
    pub chain_name: &'a str,
    pub name: &'a str,
    pub parent: &'a str,
    pub frozen_base: &'a str,
    pub squashed_merge_handling: SquashedRebaseHandling,
    pub no_fork_point: bool,
}

/// A step has no saved session; initial and resumed runs retain their existing IO paths.
pub(super) enum BranchExecution<'a> {
    Step,
    Initial {
        index: usize,
    },
    Resume {
        state: &'a mut ChainRebaseState,
        index: usize,
    },
}

impl BranchExecution<'_> {
    fn mark(&mut self, git: &GitChain, status: BranchRebaseStatus) -> Result<(), Error> {
        match self {
            Self::Step => Ok(()),
            Self::Initial { index } => git.update_branch_state(*index, status),
            Self::Resume { state, index } => git.update_branch_state_in(state, *index, status),
        }
    }

    fn record_backup(&mut self, git: &GitChain, name: &str) -> Result<(), Error> {
        match self {
            Self::Step => Ok(()),
            Self::Initial { .. } => git.record_created_backup(name),
            Self::Resume { state, .. } => git.record_created_backup_in(state, name),
        }
    }

    fn separator(&self) {
        if !matches!(self, Self::Resume { .. }) {
            println!();
        }
    }
}

/// Only `Rebased` consumes the initial step-mode limit.
#[derive(PartialEq, Eq)]
pub(super) enum BranchOutcome {
    Rebased,
    UpToDate,
    Skipped,
    SquashReset,
}

/// Failures are classified here; entry handlers retain their reporting/switch-back policy.
pub(super) enum BranchFailure {
    Checkout(Error),
    RebaseCommand { command: String, output: Output },
    Conflict,
    Reset(String),
    Other(Error),
}

impl From<Error> for BranchFailure {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}

impl GitChain {
    /// Checks out and executes one branch, saving checkpoints before protected effects.
    pub(super) fn execute_rebase_branch(
        &self,
        branch: BranchRebase<'_>,
        mut execution: BranchExecution<'_>,
    ) -> Result<BranchOutcome, BranchFailure> {
        if let Err(error) = self.checkout_branch(branch.name) {
            execution.mark(self, BranchRebaseStatus::Failed)?;
            return Err(BranchFailure::Checkout(error));
        }
        let before = self.get_commit_hash_of_head()?;

        if self.is_squashed_merged(branch.frozen_base, branch.parent, branch.name)? {
            execution.separator();
            match branch.squashed_merge_handling {
                SquashedRebaseHandling::Skip => {
                    println!(
                        "⏭️  Skipping branch {} — detected as squash-merged onto {}.",
                        branch.name.bold(),
                        branch.parent.bold()
                    );
                    execution.mark(self, BranchRebaseStatus::Skipped)?;
                    return Ok(BranchOutcome::Skipped);
                }
                SquashedRebaseHandling::Rebase => {
                    println!(
                        "⚠️  Branch {} detected as squash-merged onto {}, but forcing rebase as requested.",
                        branch.name.bold(), branch.parent.bold()
                    );
                }
                SquashedRebaseHandling::Reset => {
                    println!(
                        "⚠️  Branch {} is detected to be squashed and merged onto {}.",
                        branch.name.bold(),
                        branch.parent.bold()
                    );
                    self.create_backup_branch(branch.chain_name, branch.name)?;
                    let backup_name = format!("backup-{}/{}", branch.chain_name, branch.name);
                    println!("📦 Created backup branch: {}", backup_name.bold());
                    execution.record_backup(self, &backup_name)?;

                    let command = format!("git reset --hard {}", branch.parent);
                    let output = Command::new("git")
                        .arg("reset")
                        .arg("--hard")
                        .arg(branch.parent)
                        .output()
                        .unwrap_or_else(|_| panic!("Unable to run: {}", command));
                    if !output.status.success() {
                        return Err(BranchFailure::Reset(command));
                    }
                    println!(
                        "Resetting branch {} to {}",
                        branch.name.bold(),
                        branch.parent.bold()
                    );
                    println!("{}", command);
                    execution.mark(self, BranchRebaseStatus::SquashReset)?;
                    return Ok(BranchOutcome::SquashReset);
                }
            }
        }

        // This must precede invocation: interruption needs to remain distinguishable
        // from an untouched branch. Keep state after both clean failures and conflicts.
        execution.mark(self, BranchRebaseStatus::InProgress)?;
        let (command, spec) = self.rebase_commands.build(
            branch.parent,
            branch.frozen_base,
            branch.name,
            !branch.no_fork_point,
        );
        let output = self
            .rebase_commands
            .execute(&spec)
            .unwrap_or_else(|_| panic!("Unable to run: {}", command));
        execution.separator();
        println!("{}", command);

        if self.repo.state() != RepositoryState::Clean {
            execution.mark(self, BranchRebaseStatus::Conflict)?;
            return Err(BranchFailure::Conflict);
        }
        io::stdout().write_all(&output.stdout).unwrap();
        io::stderr().write_all(&output.stderr).unwrap();
        if !output.status.success() {
            execution.mark(self, BranchRebaseStatus::Failed)?;
            return Err(BranchFailure::RebaseCommand { command, output });
        }
        let moved = before != self.get_commit_hash_of_head()?;
        let (status, outcome) = if moved {
            (BranchRebaseStatus::Completed, BranchOutcome::Rebased)
        } else {
            (BranchRebaseStatus::UpToDate, BranchOutcome::UpToDate)
        };
        execution.mark(self, status)?;
        Ok(outcome)
    }

    /// Formats an initial-run failure and switches back only on the historical failure paths.
    pub(super) fn initial_branch_failure(
        &self,
        failure: BranchFailure,
        branch: BranchRebase<'_>,
        original_branch: &str,
        step_rebase: bool,
    ) -> Error {
        let switch_back = matches!(
            &failure,
            BranchFailure::Checkout(_)
                | BranchFailure::RebaseCommand { .. }
                | BranchFailure::Reset(_)
        );
        let error = match failure {
            BranchFailure::Checkout(error) => {
                let mut message = error.message().to_string();
                if !step_rebase {
                    message.push_str("\n\n");
                    message.push_str(&self.rebase_failure_advice());
                }
                Error::from_str(&message)
            }
            BranchFailure::RebaseCommand { command, output } => {
                let mut message = Error::git_command_failed(
                    command,
                    output.status.code().unwrap_or(1),
                    String::from_utf8_lossy(&output.stdout).to_string(),
                    String::from_utf8_lossy(&output.stderr).to_string(),
                )
                .message()
                .to_string();
                if !step_rebase {
                    message.push_str("\n\n");
                    message.push_str(&self.rebase_failure_advice());
                }
                Error::from_str(&message)
            }
            BranchFailure::Reset(command) => {
                Error::from_str(&format!("Unable to run: {}", command))
            }
            BranchFailure::Conflict => self.branch_conflict_error(branch),
            BranchFailure::Other(error) => error,
        };
        if switch_back {
            let _ = self.checkout_branch(original_branch);
        }
        error
    }

    /// Formats a resume failure without switching away from the affected branch.
    pub(super) fn resumed_branch_failure(
        &self,
        failure: BranchFailure,
        branch: BranchRebase<'_>,
    ) -> Error {
        match failure {
            BranchFailure::Checkout(error) => Error::from_str(&format!(
                "{}\n\n{}",
                error.message(),
                self.rebase_failure_advice()
            )),
            BranchFailure::RebaseCommand { .. } => Error::from_str(&format!(
                "🛑 Rebase failed for branch {} onto {}\n\n{}",
                branch.name,
                branch.parent,
                self.rebase_failure_advice()
            )),
            BranchFailure::Reset(command) => {
                Error::from_str(&format!("Unable to run: {}", command))
            }
            BranchFailure::Conflict => self.branch_conflict_error(branch),
            BranchFailure::Other(error) => error,
        }
    }

    fn branch_conflict_error(&self, branch: BranchRebase<'_>) -> Error {
        Error::from_str(&format!(
            "🛑 Unable to completely rebase {} to {}\n\
             ⚠️  Resolve conflicts, then run:\n\
             \x20  1. git add <resolved files>\n\
             \x20  2. git rebase --continue\n\
             \x20  3. {} rebase --continue\n\
             \n\
             Or run '{} rebase --skip' to skip this branch,\n\
             or  '{} rebase --abort' to cancel and restore all branches.",
            branch.name,
            branch.parent,
            self.executable_name,
            self.executable_name,
            self.executable_name
        ))
    }

    fn rebase_failure_advice(&self) -> String {
        format!(
            "Chain rebase state saved. Run '{} rebase --abort' to restore all branches to \
             their original state.",
            self.executable_name
        )
    }

    fn create_backup_branch(&self, chain_name: &str, branch_name: &str) -> Result<(), Error> {
        let (object, _reference) = self.repo.revparse_ext(branch_name)?;
        let commit = self.repo.find_commit(object.id())?;
        let backup_branch = format!("backup-{}/{}", chain_name, branch_name);
        self.repo.branch(&backup_branch, &commit, true)?;
        Ok(())
    }
}
