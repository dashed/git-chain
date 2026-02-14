use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Command;

use colored::*;
use git2::{Error, RepositoryState};

use super::GitChain;
use crate::error::ErrorExt;
use crate::rebase_state::{delete_state, read_state, state_exists, write_state};
use crate::types::{
    BranchRebaseStatus, BranchState, ChainRebaseState, RebaseStateOptions, SquashedRebaseHandling,
};
use crate::{check_gh_cli_installed, Chain};

impl GitChain {
    pub fn rebase(
        &self,
        chain_name: &str,
        step_rebase: bool,
        ignore_root: bool,
        squashed_merge_handling: SquashedRebaseHandling,
    ) -> Result<(), Error> {
        // Check for existing chain rebase state (not for step mode)
        if !step_rebase && state_exists(&self.repo) {
            let existing_state = read_state(&self.repo);
            let chain_info = match &existing_state {
                Ok(state) => format!(" for chain '{}'", state.chain_name),
                Err(_) => String::new(),
            };
            return Err(Error::from_str(&format!(
                "🛑 A chain rebase is already in progress{}.\n\
                 Use '{} rebase --continue' to resume after resolving conflicts,\n\
                     '{} rebase --skip' to skip the conflicted branch,\n\
                 or  '{} rebase --abort' to cancel and restore all branches.",
                chain_info, self.executable_name, self.executable_name, self.executable_name
            )));
        }

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
            if index == 0 {
                let common_point = self.smart_merge_base(&root_branch, &branch.branch_name)?;
                common_ancestors.push(common_point);
                continue;
            }

            let prev_branch = &chain.branches[index - 1];

            let common_point =
                self.smart_merge_base(&prev_branch.branch_name, &branch.branch_name)?;
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
                version: 1,
                chain_name: chain_name.to_string(),
                original_branch: orig_branch.clone(),
                root_branch: root_branch.clone(),
                options: RebaseStateOptions {
                    step_rebase: false,
                    ignore_root,
                    squashed_merge_handling: squash_str.to_string(),
                },
                original_refs,
                merge_bases: common_ancestors.clone(),
                branches: branch_states,
                current_index: 0,
                completed_count: 0,
                total_count: chain.branches.len(),
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
                    &branch.branch_name.bold(),
                    prev_branch_name.bold()
                );
                if !step_rebase {
                    self.update_branch_state(index, BranchRebaseStatus::Skipped)?;
                }
                continue;
            }

            // git rebase --onto <onto> <upstream> <branch>
            // git rebase --onto parent_branch fork_point branch.name

            self.checkout_branch(&branch.branch_name)?;

            let before_sha1 = self.get_commit_hash_of_head()?;

            let common_point = &common_ancestors[index];

            // check if current branch is squashed merged to prev_branch_name
            if self.is_squashed_merged(common_point, prev_branch_name, &branch.branch_name)? {
                match squashed_merge_handling {
                    SquashedRebaseHandling::Skip => {
                        println!();
                        println!(
                            "⏭️  Skipping branch {} — detected as squash-merged onto {}.",
                            &branch.branch_name.bold(),
                            prev_branch_name.bold()
                        );
                        if !step_rebase {
                            self.update_branch_state(index, BranchRebaseStatus::Skipped)?;
                        }
                        continue;
                    }
                    SquashedRebaseHandling::Rebase => {
                        println!();
                        println!(
                            "⚠️  Branch {} detected as squash-merged onto {}, but forcing rebase as requested.",
                            &branch.branch_name.bold(),
                            prev_branch_name.bold()
                        );
                        // Fall through to normal rebase below
                    }
                    SquashedRebaseHandling::Reset => {
                        println!();
                        println!(
                            "⚠️  Branch {} is detected to be squashed and merged onto {}.",
                            &branch.branch_name.bold(),
                            prev_branch_name.bold()
                        );

                        // Create backup before destructive reset
                        branch.backup(self)?;
                        let backup_name = format!("backup-{}/{}", chain_name, &branch.branch_name);
                        println!("📦 Created backup branch: {}", backup_name.bold());

                        let command = format!("git reset --hard {}", &prev_branch_name);

                        // git reset --hard <prev_branch_name>
                        let output = Command::new("git")
                            .arg("reset")
                            .arg("--hard")
                            .arg(prev_branch_name)
                            .output()
                            .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

                        if !output.status.success() {
                            let _ = self.checkout_branch(&orig_branch);
                            return Err(Error::from_str(&format!("Unable to run: {}", &command)));
                        }

                        println!(
                            "Resetting branch {} to {}",
                            &branch.branch_name.bold(),
                            prev_branch_name.bold()
                        );
                        println!("{}", command);

                        if !step_rebase {
                            self.update_branch_state(index, BranchRebaseStatus::SquashReset)?;
                        }
                        continue;
                    }
                }
            }

            let command = format!(
                "git rebase --keep-empty --onto {} {} {}",
                &prev_branch_name, common_point, &branch.branch_name
            );

            let output = Command::new("git")
                .arg("rebase")
                .arg("--keep-empty")
                .arg("--onto")
                .arg(prev_branch_name)
                .arg(common_point)
                .arg(&branch.branch_name)
                .output()
                .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

            println!();
            println!("{}", command);

            // ensure repository is in a clean state
            match self.repo.state() {
                RepositoryState::Clean => {
                    if !output.status.success() {
                        io::stdout().write_all(&output.stdout).unwrap();
                        io::stderr().write_all(&output.stderr).unwrap();
                        if !step_rebase {
                            self.update_branch_state(index, BranchRebaseStatus::Failed)?;
                            let _ = delete_state(&self.repo);
                        }
                        let _ = self.checkout_branch(&orig_branch);
                        return Err(Error::git_command_failed(
                            command,
                            output.status.code().unwrap_or(1),
                            String::from_utf8_lossy(&output.stdout).to_string(),
                            String::from_utf8_lossy(&output.stderr).to_string(),
                        ));
                    }
                    io::stdout().write_all(&output.stdout).unwrap();
                    io::stderr().write_all(&output.stderr).unwrap();

                    let after_sha1 = self.get_commit_hash_of_head()?;

                    if before_sha1 != after_sha1 {
                        num_of_rebase_operations += 1;
                    }

                    if !step_rebase {
                        self.update_branch_state(index, BranchRebaseStatus::Completed)?;
                    }
                    // go ahead to rebase next branch.
                }
                _ => {
                    if !step_rebase {
                        self.update_branch_state(index, BranchRebaseStatus::Conflict)?;
                    }
                    return Err(Error::from_str(&format!(
                        "🛑 Unable to completely rebase {} to {}\n\
                         ⚠️  Resolve conflicts, then run:\n\
                         \x20  1. git add <resolved files>\n\
                         \x20  2. git rebase --continue\n\
                         \x20  3. {} rebase --continue\n\
                         \n\
                         Or run '{} rebase --skip' to skip this branch,\n\
                         or  '{} rebase --abort' to cancel and restore all branches.",
                        &branch.branch_name,
                        prev_branch_name,
                        self.executable_name,
                        self.executable_name,
                        self.executable_name
                    )));
                }
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
            let state = read_state(&self.repo)?;
            self.print_rebase_summary(&state, num_of_rebase_operations);
            let _ = delete_state(&self.repo);
        }

        let current_branch = self.get_current_branch_name()?;

        if current_branch != orig_branch {
            println!();
            println!("Switching back to branch: {}", orig_branch.bold());
            self.checkout_branch(&orig_branch)?;
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
            }
        }

        Ok(())
    }

    /// Helper to update a branch's status in the persisted state file.
    fn update_branch_state(
        &self,
        branch_index: usize,
        status: BranchRebaseStatus,
    ) -> Result<(), Error> {
        let mut state = read_state(&self.repo)?;
        if branch_index < state.branches.len() {
            state.branches[branch_index].status = status;
            state.current_index = branch_index;
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

    pub fn rebase_continue(&self) -> Result<(), Error> {
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
            return Err(Error::from_str(
                "You have uncommitted changes in your working directory.\n\
                 Please commit or stash them before continuing the chain rebase.",
            ));
        }

        // 4. Load state
        let mut state = read_state(&self.repo)?;

        // 5. Find branch with Conflict status and mark as Completed
        let conflict_index = state
            .branches
            .iter()
            .position(|b| b.status == BranchRebaseStatus::Conflict);

        let resume_from = match conflict_index {
            Some(idx) => {
                let branch_name = &state.branches[idx].name;

                // Detect external git rebase --abort: if the branch's current OID
                // matches the original_ref, the user aborted the rebase externally
                if let Some(original_oid) = state.original_refs.get(branch_name) {
                    let current_oid = self.get_branch_commit_oid(branch_name)?;
                    if &current_oid == original_oid {
                        return Err(Error::from_str(&format!(
                            "It appears the rebase for branch '{}' was aborted externally \
                             (via git rebase --abort).\n\
                             Use '{} rebase --skip' to skip this branch and continue with \
                             the rest of the chain,\n\
                             or  '{} rebase --abort' to cancel the entire chain rebase.",
                            branch_name, self.executable_name, self.executable_name
                        )));
                    }
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
                // No conflict found — look for InProgress
                let in_progress_index = state
                    .branches
                    .iter()
                    .position(|b| b.status == BranchRebaseStatus::InProgress);
                match in_progress_index {
                    Some(idx) => {
                        let branch_name = &state.branches[idx].name;

                        // Detect external git rebase --abort
                        if let Some(original_oid) = state.original_refs.get(branch_name) {
                            let current_oid = self.get_branch_commit_oid(branch_name)?;
                            if &current_oid == original_oid {
                                return Err(Error::from_str(&format!(
                                    "It appears the rebase for branch '{}' was aborted externally \
                                     (via git rebase --abort).\n\
                                     Use '{} rebase --skip' to skip this branch and continue with \
                                     the rest of the chain,\n\
                                     or  '{} rebase --abort' to cancel the entire chain rebase.",
                                    branch_name, self.executable_name, self.executable_name
                                )));
                            }
                        }

                        state.branches[idx].status = BranchRebaseStatus::Completed;
                        write_state(&self.repo, &state)?;
                        idx + 1
                    }
                    None => {
                        // Find first pending branch
                        state
                            .branches
                            .iter()
                            .position(|b| b.status == BranchRebaseStatus::Pending)
                            .unwrap_or(state.branches.len())
                    }
                }
            }
        };

        // Parse squashed_merge_handling from state
        let squashed_merge_handling = match state.options.squashed_merge_handling.as_str() {
            "skip" => SquashedRebaseHandling::Skip,
            "rebase" => SquashedRebaseHandling::Rebase,
            _ => SquashedRebaseHandling::Reset,
        };

        // Validate pending branches still exist
        for i in resume_from..state.branches.len() {
            if state.branches[i].status != BranchRebaseStatus::Pending {
                continue;
            }
            if !self.git_local_branch_exists(&state.branches[i].name)? {
                println!(
                    "⚠️  Branch '{}' no longer exists, skipping",
                    state.branches[i].name.bold()
                );
                self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Skipped)?;
            }
        }

        // 6. Resume the rebase loop from resume_from
        let mut num_of_rebase_operations = 0;

        println!(
            "Continuing chain rebase for chain {}...",
            state.chain_name.bold()
        );

        for i in resume_from..state.branches.len() {
            if state.branches[i].status != BranchRebaseStatus::Pending {
                continue;
            }

            let branch_name = state.branches[i].name.clone();
            let parent_name = state.branches[i].parent.clone();
            let common_point = state.merge_bases[i].clone();

            // Progress reporting
            println!();
            println!(
                "📌 [{}/{}] Rebasing {} onto {}...",
                i + 1,
                state.total_count,
                branch_name.bold(),
                parent_name.bold()
            );

            self.checkout_branch(&branch_name)?;

            let before_sha1 = self.get_commit_hash_of_head()?;

            // Check for squash-merge
            if self.is_squashed_merged(&common_point, &parent_name, &branch_name)? {
                match squashed_merge_handling {
                    SquashedRebaseHandling::Skip => {
                        println!(
                            "⏭️  Skipping branch {} — detected as squash-merged onto {}.",
                            branch_name.bold(),
                            parent_name.bold()
                        );
                        self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Skipped)?;
                        continue;
                    }
                    SquashedRebaseHandling::Rebase => {
                        println!(
                            "⚠️  Branch {} detected as squash-merged onto {}, but forcing rebase as requested.",
                            branch_name.bold(),
                            parent_name.bold()
                        );
                        // Fall through to normal rebase
                    }
                    SquashedRebaseHandling::Reset => {
                        println!(
                            "⚠️  Branch {} is detected to be squashed and merged onto {}.",
                            branch_name.bold(),
                            parent_name.bold()
                        );

                        let command = format!("git reset --hard {}", parent_name);
                        let output = Command::new("git")
                            .arg("reset")
                            .arg("--hard")
                            .arg(parent_name.as_str())
                            .output()
                            .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

                        if !output.status.success() {
                            return Err(Error::from_str(&format!("Unable to run: {}", &command)));
                        }

                        println!(
                            "Resetting branch {} to {}",
                            branch_name.bold(),
                            parent_name.bold()
                        );
                        println!("{}", command);

                        self.update_branch_state_in(
                            &mut state,
                            i,
                            BranchRebaseStatus::SquashReset,
                        )?;
                        continue;
                    }
                }
            }

            let command = format!(
                "git rebase --keep-empty --onto {} {} {}",
                parent_name, common_point, branch_name
            );

            let output = Command::new("git")
                .arg("rebase")
                .arg("--keep-empty")
                .arg("--onto")
                .arg(parent_name.as_str())
                .arg(common_point.as_str())
                .arg(branch_name.as_str())
                .output()
                .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

            println!("{}", command);

            match self.repo.state() {
                RepositoryState::Clean => {
                    if !output.status.success() {
                        io::stdout().write_all(&output.stdout).unwrap();
                        io::stderr().write_all(&output.stderr).unwrap();
                        self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Failed)?;
                        let _ = delete_state(&self.repo);
                        return Err(Error::from_str(&format!(
                            "🛑 Rebase failed for branch {} onto {}",
                            branch_name, parent_name
                        )));
                    }
                    io::stdout().write_all(&output.stdout).unwrap();
                    io::stderr().write_all(&output.stderr).unwrap();

                    let after_sha1 = self.get_commit_hash_of_head()?;
                    if before_sha1 != after_sha1 {
                        num_of_rebase_operations += 1;
                    }

                    self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Completed)?;
                }
                _ => {
                    self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Conflict)?;
                    return Err(Error::from_str(&format!(
                        "🛑 Unable to completely rebase {} to {}\n\
                         ⚠️  Resolve conflicts, then run:\n\
                         \x20  1. git add <resolved files>\n\
                         \x20  2. git rebase --continue\n\
                         \x20  3. {} rebase --continue\n\
                         \n\
                         Or run '{} rebase --skip' to skip this branch,\n\
                         or  '{} rebase --abort' to cancel and restore all branches.",
                        branch_name,
                        parent_name,
                        self.executable_name,
                        self.executable_name,
                        self.executable_name
                    )));
                }
            }
        }

        // Print summary and clean up
        self.print_rebase_summary(&state, num_of_rebase_operations);
        let _ = delete_state(&self.repo);

        // Return to original branch
        let current_branch = self.get_current_branch_name()?;
        if current_branch != state.original_branch {
            println!();
            println!("Switching back to branch: {}", state.original_branch.bold());
            self.checkout_branch(&state.original_branch)?;
        }

        Ok(())
    }

    /// Helper to update branch state directly in a mutable state reference and persist.
    fn update_branch_state_in(
        &self,
        state: &mut ChainRebaseState,
        branch_index: usize,
        status: BranchRebaseStatus,
    ) -> Result<(), Error> {
        if branch_index < state.branches.len() {
            state.branches[branch_index].status = status;
            state.current_index = branch_index;
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

    pub fn rebase_skip(&self) -> Result<(), Error> {
        // 1. Verify state file exists
        if !state_exists(&self.repo) {
            return Err(Error::from_str(
                "No chain rebase in progress. Nothing to skip.",
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
        let mut state = read_state(&self.repo)?;

        // 4. Find branch with Conflict or InProgress status
        let skip_index = state.branches.iter().position(|b| {
            b.status == BranchRebaseStatus::Conflict || b.status == BranchRebaseStatus::InProgress
        });

        let resume_from = match skip_index {
            Some(idx) => {
                let branch_name = state.branches[idx].name.clone();

                // 5. Restore branch to its original position
                if let Some(original_oid) = state.original_refs.get(&branch_name) {
                    let output = Command::new("git")
                        .arg("update-ref")
                        .arg(format!("refs/heads/{}", branch_name))
                        .arg(original_oid)
                        .output();

                    match output {
                        Ok(result) if result.status.success() => {}
                        Ok(result) => {
                            let stderr = String::from_utf8_lossy(&result.stderr);
                            eprintln!(
                                "  ⚠️  Failed to restore {}: {}",
                                branch_name.bold(),
                                stderr.trim()
                            );
                        }
                        Err(e) => {
                            eprintln!("  ⚠️  Failed to restore {}: {}", branch_name.bold(), e);
                        }
                    }
                }

                // 6. Mark as Skipped
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
            }
            None => {
                return Err(Error::from_str("No conflicted branch to skip."));
            }
        };

        // Parse squashed_merge_handling from state
        let squashed_merge_handling = match state.options.squashed_merge_handling.as_str() {
            "skip" => SquashedRebaseHandling::Skip,
            "rebase" => SquashedRebaseHandling::Rebase,
            _ => SquashedRebaseHandling::Reset,
        };

        // Validate pending branches still exist
        for i in resume_from..state.branches.len() {
            if state.branches[i].status != BranchRebaseStatus::Pending {
                continue;
            }
            if !self.git_local_branch_exists(&state.branches[i].name)? {
                println!(
                    "⚠️  Branch '{}' no longer exists, skipping",
                    state.branches[i].name.bold()
                );
                self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Skipped)?;
            }
        }

        // 7. Resume the rebase loop from the next pending branch
        let mut num_of_rebase_operations = 0;

        println!(
            "Continuing chain rebase for chain {}...",
            state.chain_name.bold()
        );

        for i in resume_from..state.branches.len() {
            if state.branches[i].status != BranchRebaseStatus::Pending {
                continue;
            }

            let branch_name = state.branches[i].name.clone();
            let parent_name = state.branches[i].parent.clone();
            let common_point = state.merge_bases[i].clone();

            // Progress reporting
            println!();
            println!(
                "📌 [{}/{}] Rebasing {} onto {}...",
                i + 1,
                state.total_count,
                branch_name.bold(),
                parent_name.bold()
            );

            self.checkout_branch(&branch_name)?;

            let before_sha1 = self.get_commit_hash_of_head()?;

            // Check for squash-merge
            if self.is_squashed_merged(&common_point, &parent_name, &branch_name)? {
                match squashed_merge_handling {
                    SquashedRebaseHandling::Skip => {
                        println!(
                            "⏭️  Skipping branch {} — detected as squash-merged onto {}.",
                            branch_name.bold(),
                            parent_name.bold()
                        );
                        self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Skipped)?;
                        continue;
                    }
                    SquashedRebaseHandling::Rebase => {
                        println!(
                            "⚠️  Branch {} detected as squash-merged onto {}, but forcing rebase as requested.",
                            branch_name.bold(),
                            parent_name.bold()
                        );
                        // Fall through to normal rebase
                    }
                    SquashedRebaseHandling::Reset => {
                        println!(
                            "⚠️  Branch {} is detected to be squashed and merged onto {}.",
                            branch_name.bold(),
                            parent_name.bold()
                        );

                        let command = format!("git reset --hard {}", parent_name);
                        let output = Command::new("git")
                            .arg("reset")
                            .arg("--hard")
                            .arg(parent_name.as_str())
                            .output()
                            .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

                        if !output.status.success() {
                            return Err(Error::from_str(&format!("Unable to run: {}", &command)));
                        }

                        println!(
                            "Resetting branch {} to {}",
                            branch_name.bold(),
                            parent_name.bold()
                        );
                        println!("{}", command);

                        self.update_branch_state_in(
                            &mut state,
                            i,
                            BranchRebaseStatus::SquashReset,
                        )?;
                        continue;
                    }
                }
            }

            let command = format!(
                "git rebase --keep-empty --onto {} {} {}",
                parent_name, common_point, branch_name
            );

            let output = Command::new("git")
                .arg("rebase")
                .arg("--keep-empty")
                .arg("--onto")
                .arg(parent_name.as_str())
                .arg(common_point.as_str())
                .arg(branch_name.as_str())
                .output()
                .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

            println!("{}", command);

            match self.repo.state() {
                RepositoryState::Clean => {
                    if !output.status.success() {
                        io::stdout().write_all(&output.stdout).unwrap();
                        io::stderr().write_all(&output.stderr).unwrap();
                        self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Failed)?;
                        let _ = delete_state(&self.repo);
                        return Err(Error::from_str(&format!(
                            "🛑 Rebase failed for branch {} onto {}",
                            branch_name, parent_name
                        )));
                    }
                    io::stdout().write_all(&output.stdout).unwrap();
                    io::stderr().write_all(&output.stderr).unwrap();

                    let after_sha1 = self.get_commit_hash_of_head()?;
                    if before_sha1 != after_sha1 {
                        num_of_rebase_operations += 1;
                    }

                    self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Completed)?;
                }
                _ => {
                    self.update_branch_state_in(&mut state, i, BranchRebaseStatus::Conflict)?;
                    return Err(Error::from_str(&format!(
                        "🛑 Unable to completely rebase {} to {}\n\
                         ⚠️  Resolve conflicts, then run:\n\
                         \x20  1. git add <resolved files>\n\
                         \x20  2. git rebase --continue\n\
                         \x20  3. {} rebase --continue\n\
                         \n\
                         Or run '{} rebase --skip' to skip this branch,\n\
                         or  '{} rebase --abort' to cancel and restore all branches.",
                        branch_name,
                        parent_name,
                        self.executable_name,
                        self.executable_name,
                        self.executable_name
                    )));
                }
            }
        }

        // Print summary and clean up
        self.print_rebase_summary(&state, num_of_rebase_operations);
        let _ = delete_state(&self.repo);

        // Return to original branch
        let current_branch = self.get_current_branch_name()?;
        if current_branch != state.original_branch {
            println!();
            println!("Switching back to branch: {}", state.original_branch.bold());
            self.checkout_branch(&state.original_branch)?;
        }

        Ok(())
    }

    /// Print a summary report after rebase completion.
    fn print_rebase_summary(&self, state: &ChainRebaseState, num_of_rebase_operations: usize) {
        let completed = state
            .branches
            .iter()
            .filter(|b| b.status == BranchRebaseStatus::Completed)
            .count();
        let skipped = state
            .branches
            .iter()
            .filter(|b| b.status == BranchRebaseStatus::Skipped)
            .count();
        let squash_reset = state
            .branches
            .iter()
            .filter(|b| b.status == BranchRebaseStatus::SquashReset)
            .count();

        println!();
        println!("📊 Rebase Summary for Chain: {}", state.chain_name.bold());

        if completed > 0 {
            println!("  ✅ Rebased: {}", completed);
        }
        if skipped > 0 {
            println!("  ⏭️  Skipped: {}", skipped);
        }
        if squash_reset > 0 {
            println!("  🔄 Reset (squash-merge): {}", squash_reset);
        }

        println!();
        if num_of_rebase_operations > 0 {
            println!("🎉 Successfully rebased chain {}", state.chain_name.bold());
        } else {
            println!("Chain {} is already up-to-date.", state.chain_name.bold());
        }
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

        // 4. Restore all branches from original_refs
        println!(
            "Restoring branches for chain {}...",
            state.chain_name.bold()
        );

        for (branch_name, original_oid) in &state.original_refs {
            let short_oid = &original_oid[..7.min(original_oid.len())];
            let output = Command::new("git")
                .arg("update-ref")
                .arg(format!("refs/heads/{}", branch_name))
                .arg(original_oid)
                .output();

            match output {
                Ok(result) if result.status.success() => {
                    println!("  Restored {} to {}", branch_name.bold(), short_oid);
                }
                Ok(result) => {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    eprintln!(
                        "  ⚠️  Failed to restore {}: {}",
                        branch_name.bold(),
                        stderr.trim()
                    );
                }
                Err(e) => {
                    eprintln!("  ⚠️  Failed to restore {}: {}", branch_name.bold(), e);
                }
            }
        }

        // 5. Checkout original branch
        println!();
        println!("Switching back to branch: {}", state.original_branch.bold());
        self.checkout_branch(&state.original_branch)?;

        // 6. Delete state file
        delete_state(&self.repo)?;

        // 7. Print summary
        println!();
        println!(
            "🔄 Aborted chain rebase for chain {}. All branches restored to their original state.",
            state.chain_name.bold()
        );

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
                return Err(Error::from_str(&format!(
                    "🛑 Unable to back up branches for the chain: {}\nYou have uncommitted changes in your working directory.\nPlease commit or stash them.",
                    chain.name
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
            return Err(Error::from_str(
                "You have uncommitted changes in your working directory.",
            ));
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
