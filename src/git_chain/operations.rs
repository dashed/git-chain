use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Command;

use colored::*;
use git2::{Error, RepositoryState};

use super::rebase_executor::{BranchExecution, BranchOutcome, BranchRebase};
use super::GitChain;
use crate::rebase_state::{
    delete_state, is_usable_commit_id, read_state, read_state_for_resume, state_exists,
    state_file_path_for_read, write_state, ResumePlan, STATE_VERSION,
};
use crate::types::{
    BranchRebaseStatus, BranchState, ChainRebaseState, RebaseOptions, RebaseStateOptions,
    SquashedRebaseHandling,
};
use crate::{check_gh_cli_installed, Chain};

impl GitChain {
    pub fn rebase(&self, chain_name: &str, options: RebaseOptions) -> Result<(), Error> {
        let RebaseOptions {
            step_rebase,
            ignore_root,
            squashed_merge_handling,
            cleanup_backups,
            no_fork_point,
        } = options;
        self.ensure_no_chain_rebase_in_progress(step_rebase)?;

        match self.preliminary_checks(chain_name) {
            Ok(_) => {}
            Err(e) => {
                return Err(Error::from_str(&format!(
                    "🛑 Unable to rebase chain {}: {}",
                    chain_name, e
                )));
            }
        }

        let chain = Chain::get_chain(self, chain_name)?;
        let orig_branch = self.get_current_branch_name()?;
        let root_branch = chain.root_branch;

        // List of common ancestors between each branch and its parent branch.
        // For the first branch, a common ancestor is generated between it and the root branch.
        //
        // The following command is used to generate the common ancestors:
        // git merge-base --fork-point <ancestor_branch> <descendant_branch>
        let mut common_ancestors = vec![];

        for (index, branch) in chain.branches.iter().enumerate() {
            let parent = if index == 0 {
                root_branch.as_str()
            } else {
                chain.branches[index - 1].branch_name.as_str()
            };

            // `--no-fork-point` skips git's reflog-based calculation entirely and takes the
            // plain merge-base, for repositories where that reflog is missing or misleading.
            let common_point = if no_fork_point {
                self.merge_base(parent, &branch.branch_name)?
            } else {
                self.smart_merge_base(parent, &branch.branch_name)?
            };
            common_ancestors.push(common_point);
        }

        assert_eq!(chain.branches.len(), common_ancestors.len());

        // Save initial state for --continue/--abort support (skip for step mode)
        if !step_rebase {
            let mut original_refs = HashMap::new();
            for branch in &chain.branches {
                let oid = self.get_branch_commit_oid(&branch.branch_name)?;
                original_refs.insert(branch.branch_name.clone(), oid);
            }

            let branch_states: Vec<BranchState> = chain
                .branches
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let parent = if i == 0 {
                        root_branch.clone()
                    } else {
                        chain.branches[i - 1].branch_name.clone()
                    };
                    BranchState {
                        name: b.branch_name.clone(),
                        parent,
                        status: BranchRebaseStatus::Pending,
                    }
                })
                .collect();

            let squash_str = match squashed_merge_handling {
                SquashedRebaseHandling::Reset => "reset",
                SquashedRebaseHandling::Skip => "skip",
                SquashedRebaseHandling::Rebase => "rebase",
            };

            let state = ChainRebaseState {
                version: STATE_VERSION,
                chain_name: chain_name.to_string(),
                original_branch: orig_branch.clone(),
                root_branch: root_branch.clone(),
                options: RebaseStateOptions {
                    step_rebase: false,
                    ignore_root,
                    squashed_merge_handling: squash_str.to_string(),
                    no_fork_point,
                },
                original_refs,
                merge_bases: common_ancestors.clone(),
                branches: branch_states,
                completed_count: 0,
                total_count: chain.branches.len(),
                created_backups: Vec::new(),
            };

            write_state(&self.repo, &state)?;
        }

        let mut num_of_rebase_operations = 0;
        let mut num_of_branches_visited = 0;

        let total_branches = chain.branches.len();

        for (index, branch) in chain.branches.iter().enumerate() {
            if step_rebase && num_of_rebase_operations == 1 {
                // performed at most one rebase.
                break;
            }

            num_of_branches_visited += 1;

            let prev_branch_name = if index == 0 {
                &root_branch
            } else {
                &chain.branches[index - 1].branch_name
            };

            // Progress reporting
            if !step_rebase {
                println!();
                println!(
                    "📌 [{}/{}] Rebasing {} onto {}...",
                    index + 1,
                    total_branches,
                    branch.branch_name.bold(),
                    prev_branch_name.bold()
                );
            }

            if index == 0 && ignore_root {
                // Skip the rebase operation for the first branch of the chain.
                // Essentially, we do not rebase the first branch against the root branch.
                println!();
                println!(
                    "⚠️  Not rebasing branch {} against root branch {}. Skipping.",
                    branch.branch_name.bold(),
                    prev_branch_name.bold()
                );
                if !step_rebase {
                    self.update_branch_state(index, BranchRebaseStatus::Skipped)?;
                }
                continue;
            }

            let replay = BranchRebase {
                chain_name,
                name: &branch.branch_name,
                parent: prev_branch_name,
                frozen_base: &common_ancestors[index],
                squashed_merge_handling,
                no_fork_point,
            };
            let execution = if step_rebase {
                BranchExecution::Step
            } else {
                BranchExecution::Initial { index }
            };
            let outcome = self
                .execute_rebase_branch(replay, execution)
                .map_err(|failure| {
                    self.initial_branch_failure(failure, replay, &orig_branch, step_rebase)
                })?;
            if outcome == BranchOutcome::Rebased {
                num_of_rebase_operations += 1;
            }
        }

        // Print summary and clean up state file on successful completion
        if !step_rebase {
            if ignore_root {
                println!();
                println!(
                    "⚠️ Did not rebase chain against root branch: {}",
                    root_branch.bold()
                );
            }
            // The rebase itself has succeeded. A summary we cannot render is a cosmetic
            // loss, not a reason to report failure (REBASE_AUDIT L8).
            match read_state(&self.repo) {
                Ok(state) => {
                    self.print_rebase_summary(&state);
                    if cleanup_backups {
                        self.cleanup_backup_branches(&state.created_backups);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "  ⚠️  Could not read the chain rebase state for the summary: {}",
                        e
                    );
                }
            }
            self.clear_state_after_success();
        }

        let current_branch = self.get_current_branch_name()?;

        if current_branch != orig_branch {
            println!();
            println!("Switching back to branch: {}", orig_branch.bold());
            // The rebase itself is done; failing to switch back must not turn a successful
            // run into a non-zero exit.
            if let Err(e) = self.checkout_branch(&orig_branch) {
                println!(
                    "  ⚠️  Could not switch back to {}: {}",
                    orig_branch.bold(),
                    e
                );
            }
        }

        if step_rebase {
            if num_of_rebase_operations == 1 && num_of_branches_visited != chain.branches.len() {
                println!();
                println!("Performed one rebase on branch: {}", current_branch.bold());
                println!();
                println!(
                    "To continue rebasing, run {} rebase --step",
                    self.executable_name
                );
            } else {
                println!();
                if ignore_root {
                    println!(
                        "⚠️ Did not rebase chain against root branch: {}",
                        root_branch.bold()
                    );
                }
                if num_of_rebase_operations > 0 {
                    println!("🎉 Successfully rebased chain {}", chain.name.bold());
                } else {
                    println!("Chain {} is already up-to-date.", chain.name.bold());
                }

                self.print_prune_suggestion(
                    &root_branch,
                    chain.branches.iter().map(|b| b.branch_name.as_str()),
                );
            }
        }

        Ok(())
    }

    /// Require the chain to still look the way it did when the paused rebase was planned.
    ///
    /// Every `merge_bases` entry and every branch's recorded `parent` were computed from the
    /// chain as it stood when the run started. Editing the chain mid-pause — removing a
    /// branch, adding one, reordering, or changing the root — makes those stale, and resuming
    /// would replay commits onto the wrong parents while reporting success (REBASE_AUDIT M5).
    ///
    /// A branch deleted from git outright is not a chain edit: `git branch -D` takes the
    /// branch's config with it, and the resume loop already copes by marking it skipped. When
    /// any recorded branch's ref has gone, the chain shape is not checked at all — reading the
    /// chain in that state is itself unreliable, since a half-removed branch config aborts the
    /// chain lookup. Every case this guard is for (a branch removed from the chain, one added,
    /// a reorder, a new root, the chain deleted) leaves the refs themselves in place.
    ///
    /// `--abort` is unaffected either way: it restores from the state snapshot, not from the
    /// chain.
    fn validate_chain_unchanged(&self, state: &ChainRebaseState) -> Result<(), Error> {
        for branch in &state.branches {
            if !self.git_local_branch_exists(&branch.name).unwrap_or(false) {
                return Ok(());
            }
        }

        let stale = |detail: String| {
            Error::from_str(&format!(
                "🛑 The chain {} has changed since this rebase started: {}\n\
                 The recorded merge bases and parents no longer describe this chain, so \
                 resuming would rebase onto the wrong commits.\n\
                 Run '{} rebase --abort' to restore every branch to where it was, then start \
                 again.",
                state.chain_name, detail, self.executable_name
            ))
        };

        if !Chain::chain_exists(self, &state.chain_name)? {
            return Err(stale(String::from("the chain no longer exists")));
        }

        let chain = Chain::get_chain(self, &state.chain_name)?;

        if chain.root_branch != state.root_branch {
            return Err(stale(format!(
                "the root branch is now {} but the rebase was planned against {}",
                chain.root_branch, state.root_branch
            )));
        }

        let current: Vec<&str> = chain
            .branches
            .iter()
            .map(|b| b.branch_name.as_str())
            .collect();
        let recorded: Vec<&str> = state.branches.iter().map(|b| b.name.as_str()).collect();

        if current == recorded {
            return Ok(());
        }

        let added: Vec<&str> = current
            .iter()
            .filter(|name| !recorded.contains(name))
            .copied()
            .collect();
        let removed: Vec<&str> = recorded
            .iter()
            .filter(|name| !current.contains(name))
            .copied()
            .collect();

        if !added.is_empty() {
            return Err(stale(format!(
                "branch(es) added to the chain: {}",
                added.join(", ")
            )));
        }
        if !removed.is_empty() {
            return Err(stale(format!(
                "branch(es) removed from the chain: {}",
                removed.join(", ")
            )));
        }

        // Same membership, different order.
        let mut current_sorted = current.clone();
        let mut recorded_sorted = recorded.clone();
        current_sorted.sort_unstable();
        recorded_sorted.sort_unstable();
        if current_sorted == recorded_sorted {
            return Err(stale(format!(
                "the branches were reordered (now {}, was {})",
                current.join(" → "),
                recorded.join(" → ")
            )));
        }

        Ok(())
    }

    /// The branch a git-level rebase is operating on, read from git's own rebase state.
    ///
    /// Both backends record it in `head-name` as a full ref; the file is absent when no
    /// rebase is in progress, and unreadable state simply yields `None` rather than failing
    /// a read-only status report.
    fn git_level_rebase_branch(&self) -> Option<String> {
        for dir in ["rebase-merge", "rebase-apply"] {
            let head_name = self.repo.path().join(dir).join("head-name");
            if let Ok(contents) = std::fs::read_to_string(&head_name) {
                let name = contents.trim();
                let name = name.strip_prefix("refs/heads/").unwrap_or(name);
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
        None
    }

    /// Remove the state file after a successful run, warning rather than failing.
    ///
    /// The rebase is already done, so an undeletable state file must not turn success into an
    /// error — but it does block every later chain rebase, so silently ignoring it (as this
    /// used to) leaves the user stuck with no idea why (REBASE_AUDIT L5).
    fn clear_state_after_success(&self) {
        if let Err(e) = delete_state(&self.repo) {
            eprintln!("  ⚠️  {}", e);
            eprintln!(
                "     The rebase finished, but the leftover state will block the next one. \
                 Run '{} rebase --quit' to clear it.",
                self.executable_name
            );
        }
    }

    /// Refuse to start a new chain rebase while one is already recorded.
    ///
    /// Called from the command layer *before* the current branch's chain is resolved, so a
    /// paused rebase is reported even when HEAD sits on a branch outside any chain — the
    /// chain lookup used to run first and answer "not part of any chain" instead.
    ///
    /// Step mode keeps no state of its own, so without this it would happily rebase branches
    /// the paused run is still tracking, invalidating that run's recorded originals and
    /// pre-computed merge bases (REBASE_AUDIT M1).
    pub fn ensure_no_chain_rebase_in_progress(&self, step_rebase: bool) -> Result<(), Error> {
        if !state_exists(&self.repo) {
            return Ok(());
        }

        // An unreadable state file cannot be resumed, skipped, aborted or reported on —
        // every one of those parses it first. Say so, and point at the one command that
        // does not: --quit.
        let chain_info = match read_state(&self.repo) {
            Ok(state) => format!(" for chain '{}'", state.chain_name),
            Err(e) => {
                return Err(Error::from_str(&format!(
                    "🛑 A chain rebase state file exists but cannot be read:\n\
                     {}\n\n\
                     '{} rebase --continue', '--skip', '--abort' and '--status' all read \
                     this file, so none of them can run either.\n\
                     Run '{} rebase --quit' to discard it without touching any branch.",
                    e.message(),
                    self.executable_name,
                    self.executable_name
                )));
            }
        };

        if step_rebase {
            return Err(Error::from_str(&format!(
                "🛑 A chain rebase is already in progress{}.\n\
                 '{} rebase --step' cannot run while it is paused.\n\
                 Use '{} rebase --continue' to resume it,\n\
                     '{} rebase --abort' to cancel and restore all branches,\n\
                 or  '{} rebase --quit' to discard the state and leave every branch as-is.",
                chain_info,
                self.executable_name,
                self.executable_name,
                self.executable_name,
                self.executable_name
            )));
        }

        Err(Error::from_str(&format!(
            "🛑 A chain rebase is already in progress{}.\n\
             Use '{} rebase --continue' to resume after resolving conflicts,\n\
                 '{} rebase --skip' to skip the conflicted branch,\n\
                 '{} rebase --abort' to cancel and restore all branches,\n\
             or  '{} rebase --quit' to discard the state and leave every branch as-is.",
            chain_info,
            self.executable_name,
            self.executable_name,
            self.executable_name,
            self.executable_name
        )))
    }

    /// Record a backup ref created by this run in the persisted state file.
    ///
    /// `--cleanup-backups` deletes exactly this list, so a backup made by
    /// `git chain backup` or by an earlier run is never touched.
    pub(super) fn record_created_backup(&self, backup_name: &str) -> Result<(), Error> {
        let mut state = read_state(&self.repo)?;
        self.record_created_backup_in(&mut state, backup_name)
    }

    /// Same, for callers that already hold the state in memory.
    pub(super) fn record_created_backup_in(
        &self,
        state: &mut ChainRebaseState,
        backup_name: &str,
    ) -> Result<(), Error> {
        if !state.created_backups.iter().any(|b| b == backup_name) {
            state.created_backups.push(backup_name.to_string());
            write_state(&self.repo, state)?;
        }
        Ok(())
    }

    /// Helper to update a branch's status in the persisted state file.
    pub(super) fn update_branch_state(
        &self,
        branch_index: usize,
        status: BranchRebaseStatus,
    ) -> Result<(), Error> {
        let mut state = read_state(&self.repo)?;
        if branch_index < state.branches.len() {
            state.branches[branch_index].status = status;
            state.completed_count = state
                .branches
                .iter()
                .filter(|b| {
                    matches!(
                        b.status,
                        BranchRebaseStatus::Completed
                            | BranchRebaseStatus::Skipped
                            | BranchRebaseStatus::SquashReset
                    )
                })
                .count();
            write_state(&self.repo, &state)?;
        }
        Ok(())
    }

    pub fn rebase_continue(&self, cleanup_backups: bool) -> Result<(), Error> {
        // 1. Verify state file exists
        if !state_exists(&self.repo) {
            return Err(Error::from_str(
                "No chain rebase in progress. Nothing to continue.",
            ));
        }

        // 2. Check repo state — git-level rebase must be resolved first
        match self.repo.state() {
            RepositoryState::Clean => {
                // Good — git rebase is complete
            }
            _ => {
                return Err(Error::from_str(
                    "A git rebase is still in progress.\n\
                     Complete it first:\n\
                     \x20 1. Resolve conflicts\n\
                     \x20 2. git add <resolved files>\n\
                     \x20 3. git rebase --continue\n\
                     Then run 'git chain rebase --continue'.",
                ));
            }
        }

        // 3. Check for dirty working directory
        if self.dirty_working_directory()? {
            return Err(Error::from_str(&format!(
                "You have uncommitted changes {}.\n\
                 Please commit or stash them before continuing the chain rebase.",
                self.describe_current_position()
            )));
        }

        // 4. Load state
        let (mut state, plan) = read_state_for_resume(&self.repo)?;
        self.validate_chain_unchanged(&state)?;

        // 5. Put a branch left `Failed` or `InProgress` back in the queue.
        //
        // `Failed` means the rebase was attempted and refused; `InProgress` means the process
        // died between marking the branch and finishing it (REBASE_AUDIT M3). Neither branch
        // completed, and the repository is clean — checked above — so re-attempting from the
        // frozen merge base is exactly what a fresh run would do. git's patch-id detection
        // absorbs a partially-applied replay whose content is unchanged.
        //
        // Resuming *past* such a branch would replant its children onto its stale tip and
        // report success for a chain that is no longer internally consistent.
        let requeue_index = state.branches.iter().position(|b| {
            matches!(
                b.status,
                BranchRebaseStatus::Failed | BranchRebaseStatus::InProgress
            )
        });

        let retried_branch = match requeue_index {
            Some(idx) => {
                let reason = match state.branches[idx].status {
                    BranchRebaseStatus::InProgress => "was interrupted before it finished",
                    _ => "failed to rebase on the previous attempt",
                };
                state.branches[idx].status = BranchRebaseStatus::Pending;
                write_state(&self.repo, &state)?;
                Some((state.branches[idx].name.clone(), reason))
            }
            None => None,
        };

        // 6. Find branch with Conflict status and mark as Completed
        let conflict_index = state
            .branches
            .iter()
            .position(|b| b.status == BranchRebaseStatus::Conflict);

        let resume_from = match conflict_index {
            Some(idx) => {
                let branch_name = &state.branches[idx].name;

                // Detect external git rebase --abort: if the branch's current OID
                // matches the original_ref, the user aborted the rebase externally
                let original_oid = plan.branches()[idx].original_tip();
                let current_oid = self.get_branch_commit_oid(branch_name)?;
                if current_oid == original_oid {
                    return Err(Error::from_str(&format!(
                        "It appears the rebase for branch '{}' was aborted externally \
                         (via git rebase --abort).\n\
                         Use '{} rebase --skip' to skip this branch and continue with \
                         the rest of the chain,\n\
                         or  '{} rebase --abort' to cancel the entire chain rebase.",
                        branch_name, self.executable_name, self.executable_name
                    )));
                }

                state.branches[idx].status = BranchRebaseStatus::Completed;
                state.completed_count = state
                    .branches
                    .iter()
                    .filter(|b| {
                        matches!(
                            b.status,
                            BranchRebaseStatus::Completed
                                | BranchRebaseStatus::Skipped
                                | BranchRebaseStatus::SquashReset
                        )
                    })
                    .count();
                write_state(&self.repo, &state)?;
                idx + 1
            }
            None => {
                // No conflict. An `InProgress` branch was already requeued to `Pending` in
                // step 5, so the first pending branch is the right place to pick up.
                state
                    .branches
                    .iter()
                    .position(|b| b.status == BranchRebaseStatus::Pending)
                    .unwrap_or(state.branches.len())
            }
        };

        // The requeued branch must not be skipped over: resume at whichever comes first.
        let resume_from = match requeue_index {
            Some(idx) => resume_from.min(idx),
            None => resume_from,
        };

        self.skip_missing_pending_branches(&mut state, &plan, resume_from)?;

        // 7. Resume the rebase loop from resume_from
        println!(
            "Continuing chain rebase for chain {}...",
            state.chain_name.bold()
        );

        if let Some((branch_name, reason)) = &retried_branch {
            println!(
                "🔁 Retrying branch {}, which {}.",
                branch_name.bold(),
                reason
            );
        }

        self.run_pending_branches(&mut state, &plan, resume_from)?;
        self.finish_resumed_rebase(&state, cleanup_backups)
    }

    /// Helper to update branch state directly in a mutable state reference and persist.
    pub(super) fn update_branch_state_in(
        &self,
        state: &mut ChainRebaseState,
        branch_index: usize,
        status: BranchRebaseStatus,
    ) -> Result<(), Error> {
        if branch_index < state.branches.len() {
            state.branches[branch_index].status = status;
            state.completed_count = state
                .branches
                .iter()
                .filter(|b| {
                    matches!(
                        b.status,
                        BranchRebaseStatus::Completed
                            | BranchRebaseStatus::Skipped
                            | BranchRebaseStatus::SquashReset
                    )
                })
                .count();
            write_state(&self.repo, state)?;
        }
        Ok(())
    }

    pub fn rebase_skip(&self, cleanup_backups: bool) -> Result<(), Error> {
        // 1. Verify state file exists
        if !state_exists(&self.repo) {
            return Err(Error::from_str(
                "No chain rebase in progress. Nothing to skip.",
            ));
        }

        // 2. Load state and find the branch to skip BEFORE touching anything. Aborting the
        //    git-level rebase first would destroy work even when there is nothing to skip
        //    (REBASE_AUDIT M3).
        let (mut state, plan) = read_state_for_resume(&self.repo)?;
        self.validate_chain_unchanged(&state)?;

        let skip_index = state.branches.iter().position(|b| {
            b.status == BranchRebaseStatus::Conflict || b.status == BranchRebaseStatus::InProgress
        });

        let skip_index = match skip_index {
            Some(idx) => idx,
            None => {
                return Err(Error::from_str(
                    "No conflicted or interrupted branch to skip.",
                ));
            }
        };

        // 3. Only now, with something to skip, abort the git-level rebase.
        match self.repo.state() {
            RepositoryState::Clean => {
                // No git rebase to abort
            }
            _ => {
                println!("Aborting in-progress git rebase...");
                let output = Command::new("git")
                    .arg("rebase")
                    .arg("--abort")
                    .output()
                    .map_err(|e| {
                        Error::from_str(&format!("Failed to run git rebase --abort: {}", e))
                    })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(Error::from_str(&format!(
                        "Failed to abort git rebase: {}",
                        stderr
                    )));
                }
            }
        }

        let resume_from = {
            let idx = skip_index;
            let branch_name = state.branches[idx].name.clone();

            // 4. Restore branch to its original position. A failed restore leaves the branch
            //    somewhere the rest of the run would build on, so it is an error rather than a
            //    warning — keep the state so the user can fix it and retry (REBASE_AUDIT M6).
            let original_oid = plan.branches()[idx].original_tip();
            let output = Command::new("git")
                .arg("update-ref")
                .arg("-m")
                .arg(format!("chain rebase (skip): restoring {}", branch_name))
                .arg(format!("refs/heads/{}", branch_name))
                .arg(original_oid)
                .output();

            let failure = match output {
                Ok(result) if result.status.success() => None,
                Ok(result) => Some(String::from_utf8_lossy(&result.stderr).trim().to_string()),
                Err(e) => Some(e.to_string()),
            };

            if let Some(reason) = failure {
                return Err(Error::from_str(&format!(
                    "🛑 Could not restore {} to its original commit: {}\n\
                     The branch has not been skipped and the chain rebase state has been \
                     kept — resolve the problem and run '{} rebase --skip' again, or \
                     '{} rebase --abort' to cancel the whole rebase.",
                    branch_name, reason, self.executable_name, self.executable_name
                )));
            }

            // 5. Mark as Skipped
            println!(
                "⏭️  Skipping branch {}, restoring to original position",
                branch_name.bold()
            );
            state.branches[idx].status = BranchRebaseStatus::Skipped;
            state.completed_count = state
                .branches
                .iter()
                .filter(|b| {
                    matches!(
                        b.status,
                        BranchRebaseStatus::Completed
                            | BranchRebaseStatus::Skipped
                            | BranchRebaseStatus::SquashReset
                    )
                })
                .count();
            write_state(&self.repo, &state)?;
            idx + 1
        };

        self.skip_missing_pending_branches(&mut state, &plan, resume_from)?;

        // 7. Resume the rebase loop from the next pending branch
        println!(
            "Continuing chain rebase for chain {}...",
            state.chain_name.bold()
        );

        self.run_pending_branches(&mut state, &plan, resume_from)?;
        self.finish_resumed_rebase(&state, cleanup_backups)
    }

    /// Checkpoints missing pending members before any surviving branch is replayed.
    fn skip_missing_pending_branches(
        &self,
        state: &mut ChainRebaseState,
        plan: &ResumePlan,
        resume_from: usize,
    ) -> Result<(), Error> {
        for (i, planned) in plan.branches().iter().enumerate().skip(resume_from) {
            if state.branches[i].status != BranchRebaseStatus::Pending {
                continue;
            }
            if !self.git_local_branch_exists(planned.name())? {
                println!(
                    "⚠️  Branch '{}' no longer exists, skipping",
                    planned.name().bold()
                );
                self.update_branch_state_in(state, i, BranchRebaseStatus::Skipped)?;
            }
        }

        Ok(())
    }

    /// Replays the remaining pending members using their frozen plan and current statuses.
    fn run_pending_branches(
        &self,
        state: &mut ChainRebaseState,
        plan: &ResumePlan,
        resume_from: usize,
    ) -> Result<(), Error> {
        let squashed_merge_handling = plan.squashed_merge_handling();
        let chain_name = state.chain_name.clone();

        for (i, planned) in plan.branches().iter().enumerate().skip(resume_from) {
            if state.branches[i].status != BranchRebaseStatus::Pending {
                continue;
            }

            let replay = BranchRebase {
                chain_name: &chain_name,
                name: planned.name(),
                parent: planned.parent(),
                frozen_base: planned.frozen_base(),
                squashed_merge_handling,
                no_fork_point: state.options.no_fork_point,
            };
            println!();
            println!(
                "📌 [{}/{}] Rebasing {} onto {}...",
                i + 1,
                state.total_count,
                replay.name.bold(),
                replay.parent.bold()
            );
            self.execute_rebase_branch(replay, BranchExecution::Resume { state, index: i })
                .map_err(|failure| self.resumed_branch_failure(failure, replay))?;
        }

        Ok(())
    }

    /// Completes a resumed run only after every pending branch has finished successfully.
    fn finish_resumed_rebase(
        &self,
        state: &ChainRebaseState,
        cleanup_backups: bool,
    ) -> Result<(), Error> {
        // Print summary and clean up
        self.print_rebase_summary(state);
        if cleanup_backups {
            self.cleanup_backup_branches(&state.created_backups);
        }
        self.clear_state_after_success();

        // Return to original branch
        let current_branch = self.get_current_branch_name()?;
        if current_branch != state.original_branch {
            println!();
            println!("Switching back to branch: {}", state.original_branch.bold());
            // The rebase itself is done; failing to switch back must not turn a successful
            // run into a non-zero exit.
            if let Err(e) = self.checkout_branch(&state.original_branch) {
                println!(
                    "  ⚠️  Could not switch back to {}: {}",
                    state.original_branch.bold(),
                    e
                );
            }
        }

        Ok(())
    }

    /// Display the current chain rebase status.
    pub fn rebase_status(&self) -> Result<(), Error> {
        if !state_exists(&self.repo) {
            println!("No chain rebase in progress.");
            return Ok(());
        }

        let state = read_state(&self.repo)?;

        println!();

        // The state file records what git-chain believes; the repository records what git is
        // actually doing. A live or orphaned git rebase is invisible in the JSON, so report it
        // first — it changes which recovery command the user should reach for
        // (REBASE_AUDIT M7).
        if !matches!(self.repo.state(), RepositoryState::Clean) {
            match self.git_level_rebase_branch() {
                Some(branch_name) => println!(
                    "⚠️  A git rebase is in progress in this repository (branch {}).",
                    branch_name.bold()
                ),
                None => println!("⚠️  A git rebase is in progress in this repository."),
            }
            println!(
                "   Finish or abort it with git before resuming: 'git rebase --continue' or \
                 'git rebase --abort'."
            );
            println!();
        }

        println!("📊 Chain Rebase Status: {}", state.chain_name.bold());
        println!("   Root: {}", state.root_branch.bold());
        println!();

        for (i, branch) in state.branches.iter().enumerate() {
            let (icon, status_label) = match branch.status {
                BranchRebaseStatus::Completed => ("✅", "Completed"),
                BranchRebaseStatus::Skipped => ("⏭️ ", "Skipped"),
                BranchRebaseStatus::SquashReset => ("🔄", "Reset (squash-merge)"),
                BranchRebaseStatus::Conflict => ("❌", "Conflict"),
                BranchRebaseStatus::InProgress => ("🔧", "In Progress"),
                BranchRebaseStatus::Failed => ("💥", "Failed"),
                BranchRebaseStatus::UpToDate => ("✔️ ", "Up-to-date"),
                BranchRebaseStatus::Pending => ("⏳", "Pending"),
            };

            let current_marker = if branch.status == BranchRebaseStatus::Conflict
                || branch.status == BranchRebaseStatus::InProgress
            {
                "  ← current"
            } else {
                ""
            };

            println!(
                "   {} {} ({}/{}) onto {} — {}{}",
                icon,
                branch.name.bold(),
                i + 1,
                state.total_count,
                branch.parent,
                status_label,
                current_marker
            );
        }

        let completed = state
            .branches
            .iter()
            .filter(|b| {
                matches!(
                    b.status,
                    BranchRebaseStatus::Completed
                        | BranchRebaseStatus::Skipped
                        | BranchRebaseStatus::SquashReset
                )
            })
            .count();

        println!();
        println!("   Progress: {}/{} completed", completed, state.total_count);
        println!("   Original branch: {}", state.original_branch.bold());

        Ok(())
    }

    /// Suggest running `prune` for chain branches whose tips are already contained
    /// in (ancestors of) the root branch. Best-effort: errors are treated as
    /// "not an ancestor" — a missed suggestion must never fail a successful rebase.
    fn print_prune_suggestion<'a>(
        &self,
        root_branch: &str,
        branch_names: impl IntoIterator<Item = &'a str>,
    ) {
        let prunable: Vec<&str> = branch_names
            .into_iter()
            .filter(|branch_name| self.is_ancestor(branch_name, root_branch).unwrap_or(false))
            .collect();

        if prunable.is_empty() {
            return;
        }

        println!();
        println!(
            "💡 The following branches are ancestors of the root branch {} and can be removed from the chain:",
            root_branch.bold()
        );
        println!();

        for branch_name in prunable {
            println!("{}", branch_name);
        }

        println!();
        println!(
            "To remove them from the chain, run {} prune",
            self.executable_name
        );
    }

    /// Print a summary report after rebase completion.
    fn print_rebase_summary(&self, state: &ChainRebaseState) {
        let count_of = |status: BranchRebaseStatus| {
            state.branches.iter().filter(|b| b.status == status).count()
        };

        let completed = count_of(BranchRebaseStatus::Completed);
        let up_to_date = count_of(BranchRebaseStatus::UpToDate);
        let skipped = count_of(BranchRebaseStatus::Skipped);
        let squash_reset = count_of(BranchRebaseStatus::SquashReset);

        println!();
        println!("📊 Rebase Summary for Chain: {}", state.chain_name.bold());

        if completed > 0 {
            println!("  ✅ Rebased: {}", completed);
        }
        if up_to_date > 0 {
            println!("  ✔️  Up-to-date: {}", up_to_date);
        }
        if skipped > 0 {
            println!("  ⏭️  Skipped: {}", skipped);
        }
        if squash_reset > 0 {
            println!("  🔄 Reset (squash-merge): {}", squash_reset);
        }

        // Derived from what the branches actually record, rather than from a parallel
        // counter — a branch that never moved must not read as "rebased", and a run that
        // skipped a branch must not claim unqualified success (REBASE_AUDIT M6).
        let rebased = completed + squash_reset;
        let skip_note = if skipped == 1 {
            String::from(" (1 branch skipped)")
        } else {
            format!(" ({} branches skipped)", skipped)
        };

        println!();
        if rebased > 0 && skipped > 0 {
            println!("🎉 Rebased chain {}{}", state.chain_name.bold(), skip_note);
        } else if rebased > 0 {
            println!("🎉 Successfully rebased chain {}", state.chain_name.bold());
        } else if skipped > 0 {
            println!(
                "Chain {} is up-to-date{}",
                state.chain_name.bold(),
                skip_note
            );
        } else {
            println!("Chain {} is already up-to-date.", state.chain_name.bold());
        }

        self.print_prune_suggestion(
            &state.root_branch,
            state.branches.iter().map(|b| b.name.as_str()),
        );
    }

    /// Delete the backup branches this rebase run created, after it completes.
    ///
    /// Only refs recorded in `created_backups` are removed. Backups made by
    /// `git chain backup` or by an earlier run share the same namespace but are not
    /// this command's to delete — they are often the only remaining pre-rebase pointers.
    fn cleanup_backup_branches(&self, created_backups: &[String]) {
        let mut cleaned = 0;

        for backup_name in created_backups {
            // Check if backup branch exists
            if self.git_local_branch_exists(backup_name).unwrap_or(false) {
                let output = Command::new("git")
                    .arg("branch")
                    .arg("-D")
                    .arg(backup_name)
                    .output();

                match output {
                    Ok(result) if result.status.success() => {
                        if cleaned == 0 {
                            println!();
                            println!("🧹 Cleaning up backup branches...");
                        }
                        println!("  Deleted {}", backup_name.bold());
                        cleaned += 1;
                    }
                    Ok(result) => {
                        let stderr = String::from_utf8_lossy(&result.stderr);
                        eprintln!(
                            "  ⚠️  Failed to delete {}: {}",
                            backup_name.bold(),
                            stderr.trim()
                        );
                    }
                    Err(e) => {
                        eprintln!("  ⚠️  Failed to delete {}: {}", backup_name.bold(), e);
                    }
                }
            }
        }

        if cleaned > 0 {
            println!(
                "  Cleaned up {} backup branch{}.",
                cleaned,
                if cleaned == 1 { "" } else { "es" }
            );
        }
    }

    /// Discard the chain rebase state without touching any branch.
    ///
    /// git's `rebase --quit` counterpart, and the escape hatch for a state file the other
    /// recovery commands cannot parse — so this deliberately never reads it.
    pub fn rebase_quit(&self) -> Result<(), Error> {
        if !state_exists(&self.repo) {
            return Err(Error::from_str(
                "No chain rebase in progress. Nothing to quit.",
            ));
        }

        let path = state_file_path_for_read(&self.repo);

        // Deliberately not parsed: working on a corrupt or unsupported file is the point.
        delete_state(&self.repo)?;

        println!("🚪 Discarded the chain rebase state:");
        println!("  {}", path.display());
        println!();
        println!("No branches were touched. They are wherever the interrupted rebase left them.");

        if !matches!(self.repo.state(), RepositoryState::Clean) {
            println!();
            println!(
                "⚠️  A git rebase is still in progress. Run 'git rebase --abort' to undo it, \
                 or 'git rebase --quit' to leave it as-is."
            );
        }

        Ok(())
    }

    pub fn rebase_abort(&self) -> Result<(), Error> {
        // 1. Verify state file exists
        if !state_exists(&self.repo) {
            return Err(Error::from_str(
                "No chain rebase in progress. Nothing to abort.",
            ));
        }

        // 2. If git rebase is in progress, abort it first
        match self.repo.state() {
            RepositoryState::Clean => {
                // No git rebase to abort
            }
            _ => {
                println!("Aborting in-progress git rebase...");
                let output = Command::new("git")
                    .arg("rebase")
                    .arg("--abort")
                    .output()
                    .map_err(|e| {
                        Error::from_str(&format!("Failed to run git rebase --abort: {}", e))
                    })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(Error::from_str(&format!(
                        "Failed to abort git rebase: {}",
                        stderr
                    )));
                }
            }
        }

        // 3. Load state
        let state = read_state(&self.repo)?;

        // 4. Restore branches in chain order — iterating `original_refs` (a HashMap)
        //    would report them in a different order on every run.
        println!(
            "Restoring branches for chain {}...",
            state.chain_name.bold()
        );

        let mut restored_refs: Vec<(String, String)> = Vec::new();
        let mut already_in_place = 0usize;
        let mut left_as_is: Vec<String> = Vec::new();
        let mut failed: Vec<String> = Vec::new();

        for branch in &state.branches {
            let branch_name = &branch.name;

            let original_oid = match state.original_refs.get(branch_name) {
                Some(oid) => oid,
                None => {
                    eprintln!(
                        "  ⚠️  No original commit recorded for {} — leaving it as-is",
                        branch_name.bold()
                    );
                    left_as_is.push(branch_name.clone());
                    continue;
                }
            };

            if !is_usable_commit_id(original_oid) {
                eprintln!(
                    "  ⚠️  Not restoring {}: the recorded original commit {:?} is not a usable \
                     object id",
                    branch_name.bold(),
                    original_oid
                );
                left_as_is.push(branch_name.clone());
                continue;
            }

            let current_oid = self.get_branch_commit_oid(branch_name).ok();

            // A branch this rebase never reached is not ours to rewind. If it has moved
            // since the rebase started, the only thing that could have moved it is the
            // user, and abort must not throw that work away.
            if branch.status == BranchRebaseStatus::Pending {
                match current_oid.as_deref() {
                    Some(current) if current == original_oid => {
                        already_in_place += 1;
                        continue;
                    }
                    Some(_) => {
                        println!(
                            "  ⚠️  Leaving {} as-is: this chain rebase never touched it, but it \
                             has moved since the rebase started",
                            branch_name.bold()
                        );
                        left_as_is.push(branch_name.clone());
                        continue;
                    }
                    None => {
                        println!(
                            "  ⚠️  Leaving {} as-is: this chain rebase never touched it, and it \
                             no longer exists",
                            branch_name.bold()
                        );
                        left_as_is.push(branch_name.clone());
                        continue;
                    }
                }
            }

            let short_oid = &original_oid[..7.min(original_oid.len())];
            let output = Command::new("git")
                .arg("update-ref")
                .arg("-m")
                .arg(format!("chain rebase (abort): restoring {}", branch_name))
                .arg(format!("refs/heads/{}", branch_name))
                .arg(original_oid)
                .output();

            match output {
                Ok(result) if result.status.success() => {
                    println!("  Restored {} to {}", branch_name.bold(), short_oid);
                    restored_refs.push((branch_name.clone(), original_oid.clone()));
                }
                Ok(result) => {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    eprintln!(
                        "  ⚠️  Failed to restore {}: {}",
                        branch_name.bold(),
                        stderr.trim()
                    );
                    failed.push(branch_name.clone());
                }
                Err(e) => {
                    eprintln!("  ⚠️  Failed to restore {}: {}", branch_name.bold(), e);
                    failed.push(branch_name.clone());
                }
            }
        }

        // 5. A failed restore leaves the chain half-way. Keep the state so the user can
        //    fix the cause and run --abort again, and fail loudly rather than claiming
        //    everything was restored.
        if !failed.is_empty() {
            return Err(Error::from_str(&format!(
                "🛑 Could not restore {} of chain {}: {}.\n\
                 The chain rebase state has been kept — resolve the problem and run \
                 '{} rebase --abort' again.",
                if failed.len() == 1 {
                    "branch"
                } else {
                    "branches"
                },
                state.chain_name,
                failed.join(", "),
                self.executable_name
            )));
        }

        // 6. Bring the working tree and index back in line with the restored refs.
        //
        //    Restoring a ref that is currently checked out moves the branch out from under
        //    HEAD's tree and index, so whatever the interrupted rebase produced is left
        //    staged: every later git-chain command is then blocked by the dirty check, and
        //    a stray `git commit` would re-apply the very work this abort discarded. git's
        //    own `rebase --abort` hard-resets the working tree for the same reason, and
        //    abort's contract is explicitly discard-and-restore for the branches this
        //    rebase managed.
        //
        //    Only a branch this abort actually restored is reset. One deliberately left
        //    as-is — user-moved, or carrying an unusable recorded original — is not ours to
        //    touch, so its uncommitted changes stay put and we say so. Untracked files are
        //    unaffected either way: `git reset --hard` does not remove them.
        //
        //    A detached HEAD needs nothing here: the git-level `git rebase --abort` in step
        //    2 already reset the working tree before we reached this point.
        let current_branch = self.get_current_branch_name().unwrap_or_default();

        if let Some((branch_name, original_oid)) = restored_refs
            .iter()
            .find(|(branch_name, _)| branch_name == &current_branch)
        {
            let short_oid = &original_oid[..7.min(original_oid.len())];
            let output = Command::new("git")
                .arg("reset")
                .arg("--hard")
                .arg(original_oid)
                .output();

            match output {
                Ok(result) if result.status.success() => {
                    println!(
                        "  Reset the working tree of {} to {}",
                        branch_name.bold(),
                        short_oid
                    );
                }
                Ok(result) => {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    eprintln!(
                        "  ⚠️  Restored {} but could not reset its working tree: {}",
                        branch_name.bold(),
                        stderr.trim()
                    );
                }
                Err(e) => {
                    eprintln!(
                        "  ⚠️  Restored {} but could not reset its working tree: {}",
                        branch_name.bold(),
                        e
                    );
                }
            }
        } else if left_as_is.contains(&current_branch)
            && self.dirty_working_directory().unwrap_or(false)
        {
            println!(
                "  ⚠️  Left the uncommitted changes on {} in place: this abort did not restore \
                 that branch, so its working tree is not this abort's to reset.",
                current_branch.bold()
            );
        }

        // 7. Delete the state before the final checkout. The abort itself is done; a
        //    checkout failure (the original branch held by another worktree, say) must
        //    not leave the state behind for a retry that would redo the whole sweep.
        delete_state(&self.repo)?;

        // 8. Return to the original branch.
        println!();
        println!("Switching back to branch: {}", state.original_branch.bold());
        if let Err(e) = self.checkout_branch(&state.original_branch) {
            println!(
                "  ⚠️  Could not switch back to {}: {}",
                state.original_branch.bold(),
                e
            );
        }

        // 9. Report what actually happened.
        println!();
        if left_as_is.is_empty() {
            println!(
                "🔄 Aborted chain rebase for chain {}. All branches restored to their original state.",
                state.chain_name.bold()
            );
        } else {
            println!(
                "🔄 Aborted chain rebase for chain {}.",
                state.chain_name.bold()
            );
            if !restored_refs.is_empty() {
                println!("   Restored: {}", restored_refs.len());
            }
            if already_in_place > 0 {
                println!("   Already at their original state: {}", already_in_place);
            }
            println!("   Left as-is: {}", left_as_is.join(", "));
        }

        Ok(())
    }

    pub fn backup(&self, chain_name: &str) -> Result<(), Error> {
        if Chain::chain_exists(self, chain_name)? {
            let chain = Chain::get_chain(self, chain_name)?;

            // ensure repository is in a clean state
            match self.repo.state() {
                RepositoryState::Clean => {
                    // go ahead to back up chain.
                }
                _ => {
                    return Err(Error::from_str(&format!(
                        "🛑 Repository needs to be in a clean state before backing up chain: {}",
                        chain_name
                    )));
                }
            }

            if self.dirty_working_directory()? {
                let current_branch = self.get_current_branch_name()?;
                return Err(Error::from_str(&format!(
                    "🛑 Unable to back up branches for the chain: {}\nYou have uncommitted changes {}.\nPlease commit or stash them.",
                    chain.name,
                    current_branch.bold()
                )));
            }

            let orig_branch = self.get_current_branch_name()?;

            chain.backup(self)?;

            let current_branch = self.get_current_branch_name()?;

            if current_branch != orig_branch {
                println!("Switching back to branch: {}", orig_branch.bold());
                self.checkout_branch(&orig_branch)?;
            }

            println!("🎉 Successfully backed up chain: {}", chain.name.bold());
        } else {
            return Err(Error::from_str(&format!(
                "Unable to back up chain.\nChain does not exist: {}",
                chain_name
            )));
        }
        Ok(())
    }
    pub fn push(&self, chain_name: &str, force_push: bool) -> Result<(), Error> {
        if Chain::chain_exists(self, chain_name)? {
            let chain = Chain::get_chain(self, chain_name)?;

            let branches_pushed = chain.push(self, force_push)?;

            println!("Pushed {} branches.", format!("{}", branches_pushed).bold());
        } else {
            return Err(Error::from_str(&format!(
                "Unable to push branches of the chain.\nChain does not exist: {}",
                chain_name
            )));
        }
        Ok(())
    }
    pub fn prune(&self, chain_name: &str, dry_run: bool) -> Result<(), Error> {
        if Chain::chain_exists(self, chain_name)? {
            let chain = Chain::get_chain(self, chain_name)?;

            let pruned_branches = chain.prune(self, dry_run)?;
            if !pruned_branches.is_empty() {
                println!(
                    "Removed the following branches from chain: {}",
                    chain_name.bold()
                );
                println!();

                for branch in &pruned_branches {
                    println!("{}", branch);
                }

                println!();
                println!(
                    "Pruned {} branches.",
                    format!("{}", pruned_branches.len()).bold()
                );

                if dry_run {
                    println!();
                    println!("{}", "This was a dry-run, no branches pruned!".bold());
                }
            } else if dry_run {
                println!(
                    "This was a dry-run, no branches pruned for chain: {}",
                    chain_name.bold()
                );
            } else {
                println!("No branches pruned for chain: {}", chain_name.bold());
            }
        } else {
            return Err(Error::from_str(&format!(
                "Unable to prune branches of the chain.\nChain does not exist: {}",
                chain_name
            )));
        }
        Ok(())
    }
    pub fn preliminary_checks(&self, chain_name: &str) -> Result<(), Error> {
        if !Chain::chain_exists(self, chain_name)? {
            return Err(Error::from_str(&format!(
                "Chain {} does not exist",
                chain_name
            )));
        }

        // invariant: chain_name chain exists
        let chain = Chain::get_chain(self, chain_name)?;

        // ensure root branch exists
        if !self.git_branch_exists(&chain.root_branch)? {
            return Err(Error::from_str(&format!(
                "Root branch does not exist: {}",
                chain.root_branch.bold()
            )));
        }

        // ensure each branch exists
        for branch in &chain.branches {
            if !self.git_local_branch_exists(&branch.branch_name)? {
                return Err(Error::from_str(&format!(
                    "Branch does not exist: {}",
                    branch.branch_name.bold()
                )));
            }
        }

        // ensure no branch of the chain is held by another worktree.
        //
        // The rebase loop checks out every chain branch in turn, so one held elsewhere
        // strands the run part-way through. Catching it here — before any ref moves and
        // before any state file is written — is what makes "then retry" safe advice.
        // The root branch is not checked: nothing in the rebase ever checks it out.
        let mut occupied: Vec<String> = Vec::new();
        for branch in &chain.branches {
            if let Some(worktree_path) =
                self.branch_checked_out_in_other_worktree(&branch.branch_name)?
            {
                occupied.push(format!(
                    "  {} — {}",
                    branch.branch_name.bold(),
                    worktree_path.display()
                ));
            }
        }

        if !occupied.is_empty() {
            return Err(Error::from_str(&format!(
                "Cannot rebase chain {}.\n\
                 The following chain {} checked out in another worktree:\n\
                 {}\n\
                 Remove that worktree (git worktree remove <path>), or prune stale worktrees \
                 (git worktree prune), then retry.",
                chain_name.bold(),
                if occupied.len() == 1 {
                    "branch is"
                } else {
                    "branches are"
                },
                occupied.join("\n")
            )));
        }

        // ensure repository is in a clean state
        match self.repo.state() {
            RepositoryState::Clean => {
                // safe to proceed
            }
            _ => {
                return Err(Error::from_str(
                    "Repository needs to be in a clean state before merging.",
                ));
            }
        }

        if self.dirty_working_directory()? {
            return Err(Error::from_str(&format!(
                "You have uncommitted changes {}.",
                self.describe_current_position()
            )));
        }

        Ok(())
    }
    pub fn pr(&self, chain_name: &str, draft: bool) -> Result<(), Error> {
        check_gh_cli_installed()?;
        if Chain::chain_exists(self, chain_name)? {
            let chain = Chain::get_chain(self, chain_name)?;

            for (i, branch) in chain.branches.iter().enumerate() {
                let base_branch = if i == 0 {
                    &chain.root_branch
                } else {
                    &chain.branches[i - 1].branch_name
                };

                // Check for existing open PRs for the branch
                let output = Command::new("gh")
                    .arg("pr")
                    .arg("list")
                    .arg("--head")
                    .arg(&branch.branch_name)
                    .arg("--json")
                    .arg("url")
                    .output();

                match output {
                    Ok(output) if output.status.success() => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let pr_objects: Vec<serde_json::Value> =
                            serde_json::from_str(&stdout).unwrap_or_default();
                        if !pr_objects.is_empty() {
                            if let Some(pr_url) = pr_objects
                                .first()
                                .and_then(|pr| pr.get("url"))
                                .and_then(|url| url.as_str())
                            {
                                println!(
                                    "🔗 Open PR already exists for branch {}: {}",
                                    branch.branch_name.bold(),
                                    pr_url
                                );
                            } else {
                                println!(
                                    "🔗 Open PR already exists for branch {}",
                                    branch.branch_name.bold()
                                );
                            }
                            continue;
                        }
                    }
                    _ => {
                        eprintln!(
                            "  Failed to check existing PRs for branch {}.",
                            branch.branch_name.bold()
                        );
                        continue;
                    }
                }

                // Ensure the branch is pushed before creating a PR, because gh pr create --web drops into an interactive shell that this script doesn't handle correctly
                let push_output = Command::new("git")
                    .arg("push")
                    .arg("origin")
                    .arg(&branch.branch_name)
                    .output();

                if let Err(e) = push_output {
                    eprintln!("Failed to push branch {}: {}", branch.branch_name.bold(), e);
                    continue;
                } else {
                    let unwrapped_push_output = push_output.unwrap();
                    if !unwrapped_push_output.status.success() {
                        eprintln!(
                            "Failed to push branch {}: {}",
                            branch.branch_name.bold(),
                            String::from_utf8_lossy(&unwrapped_push_output.stderr)
                        );
                        continue;
                    }
                }

                println!(
                    "Pushed branch {}, creating PR...",
                    branch.branch_name.bold()
                );

                let mut gh_command = Command::new("gh");
                gh_command
                    .arg("pr")
                    .arg("create")
                    .arg("--base")
                    .arg(base_branch)
                    .arg("--head")
                    .arg(&branch.branch_name);

                // For draft PRs, we can't use --web flag due to GitHub CLI limitation
                // Instead, we'll create the draft PR and then open it separately
                if draft {
                    gh_command.arg("--draft");
                } else {
                    gh_command.arg("--web");
                }

                let output = gh_command.output().unwrap_or_else(|_| {
                    panic!(
                        "Unable to create pull request for branch {}",
                        branch.branch_name.bold()
                    )
                });

                if output.status.success() {
                    println!(
                        "✅ Created PR for {} -> {}",
                        branch.branch_name.bold(),
                        base_branch.bold()
                    );

                    // If draft mode, open the PR in browser separately
                    if draft {
                        let pr_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if let Some(pr_number) = pr_url.split('/').next_back() {
                            let browse_output =
                                Command::new("gh").arg("browse").arg(pr_number).output();

                            match browse_output {
                                Ok(browse_result) if browse_result.status.success() => {
                                    println!("🌐 Opened draft PR in browser");
                                }
                                _ => {
                                    println!("ℹ️  Draft PR created: {}", pr_url);
                                }
                            }
                        }
                    }
                } else {
                    io::stdout().write_all(&output.stdout).unwrap();
                    io::stderr().write_all(&output.stderr).unwrap();
                    println!("🛑 Failed to create PR for {}", branch.branch_name.bold());
                }
            }
        } else {
            return Err(Error::from_str(&format!(
                "Unable to create PRs for the chain.\nChain does not exist: {}",
                chain_name
            )));
        }
        Ok(())
    }
}
