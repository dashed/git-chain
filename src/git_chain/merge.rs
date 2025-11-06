use std::process::Command;

use colored::*;
use git2::{Error, RepositoryState};

use super::GitChain;
use crate::error::ErrorExt;
use crate::types::*;
use crate::Chain;

impl GitChain {
    pub fn is_squashed_merged(
        &self,
        common_ancestor: &str,
        parent_branch: &str,
        current_branch: &str,
    ) -> Result<bool, Error> {
        // References:
        // https://blog.takanabe.tokyo/en/2020/04/remove-squash-merged-local-git-branches/
        // https://github.com/not-an-aardvark/git-delete-squashed

        // common_ancestor should be pre-computed beforehand, ideally with self.merge_base_fork_point()
        // common_ancestor is commit sha

        // tree_id = git rev-parse current_branch^{tree}
        let tree_id = self.get_tree_id_from_branch_name(current_branch)?;

        // dangling_commit_id = git commit-tree tree_id -p common_ancestor -m "Temp commit for checking is_squashed_merged for branch current_branch"
        let output = Command::new("git")
            .arg("commit-tree")
            .arg(&tree_id)
            .arg("-p")
            .arg(common_ancestor)
            .arg("-m")
            .arg(format!(
                "Temp commit for checking is_squashed_merged for branch {}",
                current_branch
            ))
            .output()
            .unwrap_or_else(|_| {
                panic!(
                    "Unable to generate commit-tree of branch {}",
                    current_branch.bold()
                )
            });

        let dangling_commit_id = if output.status.success() {
            let raw_output = String::from_utf8(output.stdout).unwrap();
            let dangling_commit_id = raw_output.trim().to_string();
            dangling_commit_id
        } else {
            return Err(Error::from_str(&format!(
                "Unable to generate commit-tree of branch {}",
                current_branch.bold()
            )));
        };

        // output = git cherry parent_branch dangling_commit_id
        let output = Command::new("git")
            .arg("cherry")
            .arg(parent_branch)
            .arg(&dangling_commit_id)
            .output()
            .unwrap_or_else(|_| {
                panic!(
                    "Unable to determine if branch {} was squashed and merged into {}",
                    current_branch.bold(),
                    parent_branch.bold()
                )
            });

        let cherry_output = if output.status.success() {
            let raw_output = String::from_utf8(output.stdout).unwrap();
            raw_output.trim().to_string()
        } else {
            return Err(Error::from_str(&format!(
                "Unable to determine if branch {} was squashed and merged into {}",
                current_branch.bold(),
                parent_branch.bold()
            )));
        };

        let lines: Vec<String> = cherry_output.lines().map(|x| x.to_string()).collect();
        if lines.is_empty() {
            return Ok(true);
        }

        if lines.len() == 1 {
            // check if output is a single line containing "- dangling_commit_id"
            let line = &lines[0].trim();
            let is_squashed_merged = line.starts_with(&format!("- {}", dangling_commit_id));
            return Ok(is_squashed_merged);
        }

        for line in lines {
            if line.trim().starts_with('-') {
                continue;
            } else {
                return Ok(false);
            }
        }

        Ok(true)
    }
    pub fn smart_merge_base(
        &self,
        ancestor_branch: &str,
        descendant_branch: &str,
    ) -> Result<String, Error> {
        if self.is_ancestor(ancestor_branch, descendant_branch)? {
            // Can "fast forward" from ancestor_branch to descendant_branch
            return self.merge_base(ancestor_branch, descendant_branch);
        }
        self.merge_base_fork_point(ancestor_branch, descendant_branch)
    }
    pub fn merge_base(
        &self,
        ancestor_branch: &str,
        descendant_branch: &str,
    ) -> Result<String, Error> {
        // git merge-base <ancestor_branch> <descendant_branch>

        let output = Command::new("git")
            .arg("merge-base")
            .arg(ancestor_branch)
            .arg(descendant_branch)
            .output()
            .unwrap_or_else(|_| {
                panic!(
                    "Unable to run: git merge-base {} {}",
                    ancestor_branch.bold(),
                    descendant_branch.bold()
                )
            });

        if output.status.success() {
            let raw_output = String::from_utf8(output.stdout).unwrap();
            let common_point = raw_output.trim().to_string();
            return Ok(common_point);
        }
        Err(Error::from_str(&format!(
            "Unable to get common ancestor of {} and {}",
            ancestor_branch.bold(),
            descendant_branch.bold()
        )))
    }
    pub fn merge_base_fork_point(
        &self,
        ancestor_branch: &str,
        descendant_branch: &str,
    ) -> Result<String, Error> {
        // git merge-base --fork-point <ancestor_branch> <descendant_branch>

        let output = Command::new("git")
            .arg("merge-base")
            .arg("--fork-point")
            .arg(ancestor_branch)
            .arg(descendant_branch)
            .output()
            .unwrap_or_else(|_| {
                panic!(
                    "Unable to run: git merge-base --fork-point {} {}",
                    ancestor_branch.bold(),
                    descendant_branch.bold()
                )
            });

        if output.status.success() {
            let raw_output = String::from_utf8(output.stdout).unwrap();
            let common_point = raw_output.trim().to_string();
            Ok(common_point)
        } else {
            // fork-point not found, try git merge-base
            self.merge_base(ancestor_branch, descendant_branch)
        }
    }
    pub fn is_ancestor(
        &self,
        ancestor_branch: &str,
        descendant_branch: &str,
    ) -> Result<bool, Error> {
        match self.merge_base(ancestor_branch, descendant_branch) {
            Ok(common_point) => {
                let (common_point_obj, _) = self.repo.revparse_ext(&common_point)?;
                let (ancestor_object, _reference) = self.repo.revparse_ext(ancestor_branch)?;
                Ok(common_point_obj.id() == ancestor_object.id())
            }
            Err(_) => Ok(false),
        }
    }
    pub fn get_previous_branch(&self, chain: &Chain, index: usize) -> String {
        if index == 0 {
            chain.root_branch.clone()
        } else {
            chain.branches[index - 1].branch_name.clone()
        }
    }
    pub fn calculate_basic_merge_bases(&self, chain: &Chain) -> Result<Vec<String>, Error> {
        let mut common_ancestors = vec![];

        for (index, branch) in chain.branches.iter().enumerate() {
            let prev_branch = self.get_previous_branch(chain, index);

            // Use regular merge-base without fork-point
            let common_point = self.merge_base(&prev_branch, &branch.branch_name)?;
            common_ancestors.push(common_point);
        }

        Ok(common_ancestors)
    }
    pub fn calculate_smart_merge_bases(&self, chain: &Chain) -> Result<Vec<String>, Error> {
        let mut common_ancestors = vec![];

        for (index, branch) in chain.branches.iter().enumerate() {
            let prev_branch = self.get_previous_branch(chain, index);

            // Use smart merge-base with potential fork-point
            let common_point = self.smart_merge_base(&prev_branch, &branch.branch_name)?;
            common_ancestors.push(common_point);
        }

        Ok(common_ancestors)
    }
    pub fn execute_merge(
        &self,
        upstream: &str,
        merge_flags: &[String],
    ) -> Result<MergeResult, Error> {
        // Build command with all the specified flags
        let mut command = Command::new("git");
        command.arg("merge");

        // Add any custom merge flags
        for flag in merge_flags {
            command.arg(flag);
        }

        command.arg(upstream);

        // Collect output
        let output = command
            .output()
            .map_err(|e| Error::from_str(&format!("IO error: {}", e)))?;

        if output.status.success() {
            // Check if it was a no-op merge
            if String::from_utf8_lossy(&output.stdout).contains("Already up to date") {
                return Ok(MergeResult::AlreadyUpToDate);
            }

            // Successfully merged
            Ok(MergeResult::Success(
                String::from_utf8_lossy(&output.stdout).to_string(),
            ))
        } else {
            // Check if it's a merge conflict
            if self.repo.state() != RepositoryState::Clean {
                return Ok(MergeResult::Conflict(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }

            // Other error
            Err(Error::git_command_failed(
                format!("git merge {}", upstream),
                output.status.code().unwrap_or(1),
                String::from_utf8_lossy(&output.stdout).to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }
    pub fn get_merge_commit_info(
        &self,
        parent_branch: &str,
        branch_name: &str,
    ) -> Result<Vec<MergeCommitInfo>, Error> {
        // Get the latest commit on the branch
        let mut command = Command::new("git");
        command.args(["log", "--oneline", "-1", branch_name]);
        let output = match command.output() {
            Ok(output) => output,
            Err(_) => return Ok(vec![]), // Return empty vec on error
        };

        if !output.status.success() {
            return Ok(vec![]);
        }

        let latest_commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if latest_commit.is_empty() {
            return Ok(vec![]);
        }

        // Check if it's a merge commit by looking for parent commits
        let commit_hash = latest_commit.split_whitespace().next().unwrap_or("");
        if commit_hash.is_empty() {
            return Ok(vec![]);
        }

        // Get commit information
        let mut command = Command::new("git");
        command.args(["show", "--stat", commit_hash]);
        let output = match command.output() {
            Ok(output) => output,
            Err(_) => return Ok(vec![]),
        };

        if !output.status.success() {
            return Ok(vec![]);
        }

        let commit_info = String::from_utf8_lossy(&output.stdout).to_string();

        // Check if it's a merge commit, which typically contains "Merge" in the commit message
        if commit_info.contains(&format!("Merge branch '{}'", parent_branch))
            || commit_info.contains("Merge branch")
        {
            // Extract commit message (first line after commit hash)
            let commit_lines: Vec<&str> = commit_info.lines().collect();
            let message = commit_lines
                .iter()
                .position(|line| line.trim().starts_with("Merge branch"))
                .map(|idx| commit_lines[idx].trim().to_string());

            // Extract stats
            let stats_line = commit_lines
                .iter()
                .find(|line| line.contains("files changed") || line.contains("file changed"));

            let stats = stats_line.map(|line| {
                let mut files_changed = 0;
                let mut insertions = 0;
                let mut deletions = 0;

                if let Some(files_idx) = line.find("file changed") {
                    if let Some(files_num) = line[..files_idx].split_whitespace().last() {
                        files_changed = files_num.parse().unwrap_or(0);
                    }
                } else if let Some(files_idx) = line.find("files changed") {
                    if let Some(files_num) = line[..files_idx].split_whitespace().last() {
                        files_changed = files_num.parse().unwrap_or(0);
                    }
                }

                if let Some(ins_idx) = line.find("insertion") {
                    if let Some(ins_end) = line[..ins_idx].rfind(' ') {
                        if let Some(ins_start) = line[..ins_end].rfind(' ') {
                            let ins_str = &line[ins_start + 1..ins_end];
                            insertions = ins_str.parse().unwrap_or(0);
                        }
                    }
                }

                if let Some(del_idx) = line.find("deletion") {
                    if let Some(del_end) = line[..del_idx].rfind(' ') {
                        if let Some(del_start) = line[..del_end].rfind(' ') {
                            let del_str = &line[del_start + 1..del_end];
                            deletions = del_str.parse().unwrap_or(0);
                        }
                    }
                }

                MergeStats {
                    files_changed,
                    insertions,
                    deletions,
                }
            });

            return Ok(vec![MergeCommitInfo { message, stats }]);
        }

        // It's not a merge commit
        Ok(vec![])
    }
    pub fn report_merge_results(
        &self,
        chain_name: &str,
        merge_operations: usize,
        merge_conflicts: Vec<(String, String)>,
        skipped_branches: Vec<(String, String)>,
        squashed_merges: Vec<(String, String)>,
        options: &MergeOptions,
    ) -> Result<(), Error> {
        println!("\n📊 Merge Summary for Chain: {}", chain_name.bold());
        println!("  ✅ Successful merges: {}", merge_operations);

        if !merge_conflicts.is_empty() {
            println!("  ⚠️  Merge conflicts: {}", merge_conflicts.len());
            for (upstream, branch) in &merge_conflicts {
                println!("     - {} into {}", upstream.bold(), branch.bold());
            }
        }

        if !skipped_branches.is_empty() {
            println!("  ℹ️  Skipped branches: {}", skipped_branches.len());
            for (upstream, branch) in &skipped_branches {
                println!("     - {} into {}", upstream.bold(), branch.bold());
            }
        }

        if !squashed_merges.is_empty() {
            println!("  🔄 Squashed merges handled: {}", squashed_merges.len());
            for (upstream, branch) in &squashed_merges {
                println!("     - Reset {} to {}", branch.bold(), upstream.bold());
            }
        }

        // For detailed reporting, show information about each branch merge
        if matches!(options.report_level, ReportLevel::Detailed) && merge_operations > 0 {
            println!("\n📝 Detailed Merge Information:");

            // Get the chain's branches
            if let Ok(chain) = Chain::get_chain(self, chain_name) {
                for (index, branch) in chain.branches.iter().enumerate() {
                    if index == 0 && options.ignore_root {
                        continue; // Skip first branch if ignore_root is true
                    }

                    let prev_branch = if index == 0 {
                        chain.root_branch.clone()
                    } else {
                        chain.branches[index - 1].branch_name.clone()
                    };

                    // Skip printing detailed info for skipped branches and squashed merges
                    let is_skipped = skipped_branches
                        .iter()
                        .any(|(up, br)| *up == prev_branch && *br == branch.branch_name);
                    let is_squashed = squashed_merges
                        .iter()
                        .any(|(up, br)| *up == prev_branch && *br == branch.branch_name);
                    let is_conflict = merge_conflicts
                        .iter()
                        .any(|(up, br)| *up == prev_branch && *br == branch.branch_name);

                    if is_skipped {
                        println!(
                            "  {} ➔ {}: {}",
                            prev_branch.bold(),
                            branch.branch_name.bold(),
                            "Skipped".dimmed()
                        );
                        continue;
                    }

                    if is_squashed {
                        println!(
                            "  {} ➔ {}: {}",
                            prev_branch.bold(),
                            branch.branch_name.bold(),
                            "Squashed and reset".dimmed()
                        );
                        continue;
                    }

                    if is_conflict {
                        println!(
                            "  {} ➔ {}: {}",
                            prev_branch.bold(),
                            branch.branch_name.bold(),
                            "Merge conflict".red()
                        );
                        continue;
                    }

                    // Try to get commit information for successful merges
                    if let Ok(commits) =
                        self.get_merge_commit_info(&prev_branch, &branch.branch_name)
                    {
                        if commits.is_empty() {
                            // Branch was already up to date
                            println!(
                                "  {} ➔ {}: {}",
                                prev_branch.bold(),
                                branch.branch_name.bold(),
                                "Already up to date".dimmed()
                            );
                        } else {
                            for commit in commits {
                                println!(
                                    "  {} ➔ {}: {}",
                                    prev_branch.bold(),
                                    branch.branch_name.bold(),
                                    commit
                                        .message
                                        .unwrap_or_else(|| "No commit message".to_string())
                                        .green()
                                );

                                if let Some(stat) = commit.stats {
                                    println!(
                                        "    {} insertions(+), {} deletions(-) across {} files",
                                        stat.insertions, stat.deletions, stat.files_changed
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Overall status message
        if merge_operations > 0 {
            println!("\n🎉 Successfully merged chain {}", chain_name.bold());
        } else if merge_conflicts.is_empty() {
            println!("\nℹ️  Chain {} is already up-to-date.", chain_name.bold());
        } else {
            println!(
                "\n⚠️  Chain {} was partially merged with conflicts.",
                chain_name.bold()
            );
            println!("   Run `git status` to see conflicted files.");
            println!("   After resolving conflicts, continue with regular git commands:");
            println!("     git add <resolved-files>");
            println!("     git commit -m \"Merge conflict resolution\"");
        }

        Ok(())
    }
    pub fn validate_chain_and_repository_state(&self, chain_name: &str) -> Result<(), Error> {
        // Get the chain and ensure it exists
        let chain = Chain::get_chain(self, chain_name)?;

        // Ensure root branch exists
        if !self.git_branch_exists(&chain.root_branch)? {
            return Err(Error::from_str(&format!(
                "Root branch does not exist: {}",
                chain.root_branch.bold()
            )));
        }

        // Ensure each branch exists
        for branch in &chain.branches {
            if !self.git_local_branch_exists(&branch.branch_name)? {
                return Err(Error::from_str(&format!(
                    "Branch does not exist: {}",
                    branch.branch_name.bold()
                )));
            }
        }

        // Ensure repository is in a clean state
        match self.repo.state() {
            RepositoryState::Clean => {
                // Repository is in a clean state, proceed
            }
            _ => {
                return Err(Error::from_str(
                    "🛑 Repository needs to be in a clean state before merging.",
                ));
            }
        }

        // Check for uncommitted changes
        if self.dirty_working_directory()? {
            return Err(Error::from_str(&format!(
            "🛑 Unable to merge branches for the chain: {}\nYou have uncommitted changes in your working directory.\nPlease commit or stash them.",
            chain_name.bold()
        )));
        }

        Ok(())
    }
    pub fn reset_hard_to_branch(&self, branch_name: &str) -> Result<(), Error> {
        let command = format!("git reset --hard {}", branch_name);

        let output = Command::new("git")
            .arg("reset")
            .arg("--hard")
            .arg(branch_name)
            .output()
            .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

        if !output.status.success() {
            return Err(Error::from_str(&format!("Unable to run: {}", command)));
        }

        Ok(())
    }
    pub fn merge_chain_with_options(
        &self,
        chain_name: &str,
        options: MergeOptions,
    ) -> Result<(), Error> {
        // Validate inputs and check repository state
        self.validate_chain_and_repository_state(chain_name)?;

        let chain = Chain::get_chain(self, chain_name)?;
        let orig_branch = self.get_current_branch_name()?;

        // Calculate merge bases with smart fork point detection if enabled
        let merge_bases = if options.simple_mode || !options.use_fork_point {
            self.calculate_basic_merge_bases(&chain)?
        } else {
            self.calculate_smart_merge_bases(&chain)?
        };

        // Keep track of what happened
        let mut merge_operations = 0;
        let mut merge_conflicts = Vec::new();
        let mut skipped_branches = Vec::new();
        let mut squashed_merges = Vec::new();

        // Iterate through branches
        for (index, branch) in chain.branches.iter().enumerate() {
            let prev_branch = self.get_previous_branch(&chain, index);

            // Skip root merge if configured
            if index == 0 && options.ignore_root {
                if options.verbose {
                    println!(
                        "\n⚠️  Not merging branch {} against root branch {}. Skipping.",
                        branch.branch_name.bold(),
                        prev_branch.bold()
                    );
                }
                skipped_branches.push((prev_branch.to_string(), branch.branch_name.clone()));
                continue;
            }

            // Check out the branch to merge into
            self.checkout_branch(&branch.branch_name)?;

            if options.verbose {
                println!("\nProcessing branch: {}", branch.branch_name.bold());
            }

            // Store hash before merge for change detection
            let _before_sha1 = self.get_commit_hash_of_head()?;

            // Handle special cases (e.g., squashed merges) unless in simple mode
            if !options.simple_mode
                && self.is_squashed_merged(
                    &merge_bases[index],
                    &prev_branch,
                    &branch.branch_name,
                )?
            {
                if options.verbose {
                    println!(
                        "⚠️  Branch {} is detected to be squashed and merged onto {}.",
                        branch.branch_name.bold(),
                        prev_branch.bold()
                    );
                }

                // Handle the squashed merge case according to configuration
                match options.squashed_merge_handling {
                    SquashedMergeHandling::Reset => {
                        // Reset the branch to the previous branch
                        self.reset_hard_to_branch(&prev_branch)?;
                        squashed_merges.push((prev_branch.to_string(), branch.branch_name.clone()));
                        if options.verbose {
                            println!(
                                "Resetting branch {} to {}",
                                branch.branch_name.bold(),
                                prev_branch.bold()
                            );
                        }
                        continue;
                    }
                    SquashedMergeHandling::Skip => {
                        if options.verbose {
                            println!(
                                "Skipping merge as branch appears to be already squashed-merged."
                            );
                        }
                        skipped_branches
                            .push((prev_branch.to_string(), branch.branch_name.clone()));
                        continue;
                    }
                    SquashedMergeHandling::Merge => {
                        if options.verbose {
                            println!("Proceeding with merge despite squashed merge detection.");
                        }
                        // Continue with the merge despite squashed merge detection
                    }
                }
            }

            // Perform the merge with all the specified options
            match self.execute_merge(&prev_branch, &options.merge_flags)? {
                MergeResult::Success(summary) => {
                    merge_operations += 1;
                    if options.verbose {
                        println!("{}", summary);
                    }
                }
                MergeResult::AlreadyUpToDate => {
                    if options.verbose {
                        println!(
                            "Branch {} is already up-to-date with {}.",
                            branch.branch_name.bold(),
                            prev_branch.bold()
                        );
                    }
                }
                MergeResult::Conflict(message) => {
                    merge_conflicts.push((prev_branch.to_string(), branch.branch_name.clone()));
                    if options.verbose {
                        println!(
                            "🛑 Merge conflict between {} and {}:",
                            prev_branch.bold(),
                            branch.branch_name.bold()
                        );
                        println!("{}", message);
                    }

                    return Err(Error::merge_conflict(
                        branch.branch_name.clone(),
                        prev_branch.clone(),
                        Some(message),
                    ));
                }
            }
        }

        // Return to original branch if configured and needed
        if options.return_to_original && self.get_current_branch_name()? != orig_branch {
            if options.verbose {
                println!("\nSwitching back to branch: {}", orig_branch.bold());
            }
            self.checkout_branch(&orig_branch)?;
        }

        // Generate detailed report of what happened based on report level
        match options.report_level {
            ReportLevel::Minimal => {
                // Minimal reporting
                if merge_operations > 0 {
                    println!("Successfully merged chain {}", chain_name.bold());
                } else if merge_conflicts.is_empty() {
                    println!("Chain {} is already up-to-date.", chain_name.bold());
                } else {
                    println!("Failed to merge chain {}", chain_name.bold());
                }
            }
            ReportLevel::Standard | ReportLevel::Detailed => {
                // Standard/Detailed reporting
                self.report_merge_results(
                    chain_name,
                    merge_operations,
                    merge_conflicts,
                    skipped_branches,
                    squashed_merges,
                    &options,
                )?;
            }
        }

        Ok(())
    }
}
