use std::io::{self, Write};
use std::process::{self, Command};

use colored::*;
use git2::{Error, RepositoryState};

use super::GitChain;
use crate::{check_gh_cli_installed, Chain};

pub fn print_rebase_error(executable_name: &str, branch: &str, upstream_branch: &str) {
    eprintln!(
        "🛑 Unable to completely rebase {} to {}",
        branch.bold(),
        upstream_branch.bold()
    );
    eprintln!(
        "⚠️  Resolve any rebase merge conflicts, and then run {} rebase",
        executable_name
    );
}
impl GitChain {
    pub fn rebase(
        &self,
        chain_name: &str,
        step_rebase: bool,
        ignore_root: bool,
    ) -> Result<(), Error> {
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

        let mut num_of_rebase_operations = 0;
        let mut num_of_branches_visited = 0;

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

            if index == 0 && ignore_root {
                // Skip the rebase operation for the first branch of the chain.
                // Essentially, we do not rebase the first branch against the root branch.
                println!();
                println!(
                    "⚠️  Not rebasing branch {} against root branch {}. Skipping.",
                    &branch.branch_name.bold(),
                    prev_branch_name.bold()
                );
                continue;
            }

            // git rebase --onto <onto> <upstream> <branch>
            // git rebase --onto parent_branch fork_point branch.name

            self.checkout_branch(&branch.branch_name)?;

            let before_sha1 = self.get_commit_hash_of_head()?;

            let common_point = &common_ancestors[index];

            // check if current branch is squashed merged to prev_branch_name
            if self.is_squashed_merged(common_point, prev_branch_name, &branch.branch_name)? {
                println!();
                println!(
                    "⚠️  Branch {} is detected to be squashed and merged onto {}.",
                    &branch.branch_name.bold(),
                    prev_branch_name.bold()
                );

                let command = format!("git reset --hard {}", &prev_branch_name);

                // git reset --hard <prev_branch_name>
                let output = Command::new("git")
                    .arg("reset")
                    .arg("--hard")
                    .arg(prev_branch_name)
                    .output()
                    .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

                if !output.status.success() {
                    eprintln!("Unable to run: {}", &command);
                    process::exit(1);
                }

                println!(
                    "Resetting branch {} to {}",
                    &branch.branch_name.bold(),
                    prev_branch_name.bold()
                );
                println!("{}", command);

                continue;
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
                        eprintln!("Command returned non-zero exit status: {}", command);
                        eprintln!("It returned: {}", output.status.code().unwrap());
                        io::stdout().write_all(&output.stdout).unwrap();
                        io::stderr().write_all(&output.stderr).unwrap();
                        process::exit(1);
                    }
                    io::stdout().write_all(&output.stdout).unwrap();
                    io::stderr().write_all(&output.stderr).unwrap();

                    let after_sha1 = self.get_commit_hash_of_head()?;

                    if before_sha1 != after_sha1 {
                        num_of_rebase_operations += 1;
                    }
                    // go ahead to rebase next branch.
                }
                _ => {
                    print_rebase_error(
                        &self.executable_name,
                        &branch.branch_name,
                        prev_branch_name,
                    );
                    process::exit(1);
                }
            }
        }

        let current_branch = self.get_current_branch_name()?;

        if current_branch != orig_branch {
            println!();
            println!("Switching back to branch: {}", orig_branch.bold());
            self.checkout_branch(&orig_branch)?;
        }

        println!();
        if step_rebase
            && num_of_rebase_operations == 1
            && num_of_branches_visited != chain.branches.len()
        {
            println!("Performed one rebase on branch: {}", current_branch.bold());
            println!();
            println!(
                "To continue rebasing, run {} rebase --step",
                self.executable_name
            );

            return Ok(());
        }

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
                    eprintln!(
                        "🛑 Repository needs to be in a clean state before backing up chain: {}",
                        chain_name
                    );
                    process::exit(1);
                }
            }

            if self.dirty_working_directory()? {
                eprintln!(
                    "🛑 Unable to back up branches for the chain: {}",
                    chain.name.bold()
                );
                eprintln!("You have uncommitted changes in your working directory.");
                eprintln!("Please commit or stash them.");
                process::exit(1);
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
            eprintln!("Unable to back up chain.");
            eprintln!("Chain does not exist: {}", chain_name);
            process::exit(1);
        }
        Ok(())
    }
    pub fn push(&self, chain_name: &str, force_push: bool) -> Result<(), Error> {
        if Chain::chain_exists(self, chain_name)? {
            let chain = Chain::get_chain(self, chain_name)?;

            let branches_pushed = chain.push(self, force_push)?;

            println!("Pushed {} branches.", format!("{}", branches_pushed).bold());
        } else {
            eprintln!("Unable to push branches of the chain.");
            eprintln!("Chain does not exist: {}", chain_name);
            process::exit(1);
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
            eprintln!("Unable to prune branches of the chain.");
            eprintln!("Chain does not exist: {}", chain_name);
            process::exit(1);
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
            eprintln!("Unable to create PRs for the chain.");
            eprintln!("Chain does not exist: {}", chain_name);
            process::exit(1);
        }
        Ok(())
    }
}
