use std::collections::HashMap;
use std::process::{self, Command};

use colored::*;
use git2::Error;
use regex::Regex;

use crate::types::*;
use crate::{check_gh_cli_installed, Branch, GitChain};

#[derive(Clone)]
pub struct Chain {
    pub name: String,
    pub root_branch: String,
    pub branches: Vec<Branch>,
}

impl Chain {
    fn get_all_branch_configs(git_chain: &GitChain) -> Result<Vec<(String, String)>, Error> {
        let key_regex = Regex::new(r"^branch\.(?P<branch_name>.+)\.chain-name$".trim()).unwrap();
        git_chain.get_git_configs_matching_key(&key_regex)
    }

    pub fn get_all_chains(git_chain: &GitChain) -> Result<Vec<Chain>, Error> {
        let entries = Chain::get_all_branch_configs(git_chain)?;

        let mut chains: HashMap<String, Chain> = HashMap::new();

        for (_key, chain_name) in entries {
            if chains.contains_key(&chain_name) {
                continue;
            }

            let chain = Chain::get_chain(git_chain, &chain_name)?;
            chains.insert(chain_name, chain);
        }

        let mut list: Vec<Chain> = chains.values().cloned().collect();
        list.sort_by_key(|c| c.name.clone());
        Ok(list)
    }

    fn get_branches_for_chain(
        git_chain: &GitChain,
        chain_name: &str,
    ) -> Result<Vec<Branch>, Error> {
        let key_regex = Regex::new(r"^branch\.(?P<branch_name>.+)\.chain-name$".trim()).unwrap();
        let mut branches: Vec<Branch> = vec![];

        let entries = Chain::get_all_branch_configs(git_chain)?;
        for (key, value) in entries {
            if value != chain_name {
                continue;
            }

            let captures = key_regex.captures(&key).unwrap();
            let branch_name = &captures["branch_name"];

            let results = Branch::get_branch_with_chain(git_chain, branch_name)?;

            match results {
                BranchSearchResult::NotPartOfAnyChain => {
                    // TODO: could this fail silently?
                    eprintln!(
                        "Branch not correctly set up as part of a chain: {}",
                        branch_name.bold()
                    );
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => {
                    branches.push(branch);
                }
            };
        }

        Ok(branches)
    }

    pub fn chain_exists(git_chain: &GitChain, chain_name: &str) -> Result<bool, Error> {
        let branches = Chain::get_branches_for_chain(git_chain, chain_name)?;
        Ok(!branches.is_empty())
    }

    pub fn get_chain(git_chain: &GitChain, chain_name: &str) -> Result<Self, Error> {
        let mut branches = Chain::get_branches_for_chain(git_chain, chain_name)?;

        if branches.is_empty() {
            return Err(Error::from_str(&format!(
                "Unable to get branches attached to chain: {}",
                chain_name
            )));
        }

        // TODO: ensure all branches have the same root

        branches.sort_by_key(|b| b.chain_order.clone());

        // use first branch as the source of the root branch
        let root_branch = branches[0].root_branch.clone();

        let chain = Chain {
            name: chain_name.to_string(),
            root_branch,
            branches,
        };

        Ok(chain)
    }

    pub fn has_chain_order(&self, chain_order: &str) -> bool {
        for branch in &self.branches {
            if branch.chain_order == chain_order {
                return true;
            }
        }
        false
    }

    fn display_ahead_behind(
        &self,
        git_chain: &GitChain,
        upstream: &str,
        branch: &str,
    ) -> Result<String, Error> {
        let (upstream_obj, _reference) = git_chain.repo.revparse_ext(upstream)?;
        let (branch_obj, _reference) = git_chain.repo.revparse_ext(branch)?;

        let ahead_behind = git_chain
            .repo
            .graph_ahead_behind(branch_obj.id(), upstream_obj.id())?;

        let status = match ahead_behind {
            (0, 0) => "".to_string(),
            (ahead, 0) => {
                format!("{} ahead", ahead)
            }
            (0, behind) => {
                format!("{} behind", behind)
            }
            (ahead, behind) => {
                format!("{} ahead ⦁ {} behind", ahead, behind)
            }
        };

        Ok(status)
    }

    pub fn display_list(
        &self,
        git_chain: &GitChain,
        current_branch: &str,
        show_prs: bool,
    ) -> Result<(), Error> {
        println!("{}", self.name);

        let mut branches = self.branches.clone();
        branches.reverse();

        for (index, branch) in branches.iter().enumerate() {
            let (marker, branch_name) = if branch.branch_name == current_branch {
                ("➜ ", branch.branch_name.bold().to_string())
            } else {
                ("", branch.branch_name.clone())
            };

            let upstream = if index == branches.len() - 1 {
                &self.root_branch
            } else {
                &branches[index + 1].branch_name
            };

            let ahead_behind_status =
                self.display_ahead_behind(git_chain, upstream, &branch.branch_name)?;

            let mut status_line = if ahead_behind_status.is_empty() {
                format!("{:>6}{}", marker, branch_name)
            } else {
                format!("{:>6}{} ⦁ {}", marker, branch_name, ahead_behind_status)
            };

            if show_prs && check_gh_cli_installed().is_ok() {
                // Check for open pull requests for each branch
                let output = Command::new("gh")
                    .arg("pr")
                    .arg("list")
                    .arg("--state")
                    .arg("all")
                    .arg("--head")
                    .arg(&branch.branch_name)
                    .arg("--json")
                    .arg("url,state")
                    .output();

                match output {
                    Ok(output) if output.status.success() => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let pr_objects: Vec<serde_json::Value> =
                            serde_json::from_str(&stdout).unwrap_or_default();
                        let pr_details: Vec<String> = pr_objects
                            .iter()
                            .filter_map(|pr| {
                                let url = pr.get("url").and_then(|url| url.as_str());
                                let state = pr.get("state").and_then(|state| state.as_str());
                                match (url, state) {
                                    (Some(url), Some(state)) => {
                                        let colored_state = match state {
                                            "MERGED" => "Merged".purple().to_string(),
                                            "OPEN" => "Open".green().to_string(),
                                            "CLOSED" => "Closed".red().to_string(),
                                            _ => state.to_string(),
                                        };
                                        Some(format!("{} [{}]", url, colored_state))
                                    }
                                    _ => None,
                                }
                            })
                            .collect();

                        if !pr_details.is_empty() {
                            let pr_list = pr_details.join("; ");
                            status_line.push_str(&format!(" ({})", pr_list));
                        }
                    }
                    _ => {
                        eprintln!(
                            "  Failed to retrieve PRs for branch {}.",
                            branch.branch_name.bold()
                        );
                    }
                }
            }

            println!("{}", status_line.trim_end());
        }

        if self.root_branch == current_branch {
            println!("{:>6}{} (root branch)", "➜ ", self.root_branch.bold());
        } else {
            println!("{:>6}{} (root branch)", "", self.root_branch);
        };

        Ok(())
    }

    pub fn before(&self, needle_branch: &Branch) -> Option<Branch> {
        if self.branches.is_empty() {
            return None;
        }

        let maybe_index = self.branches.iter().position(|b| b == needle_branch);

        match maybe_index {
            None => None,
            Some(index) => {
                if index > 0 {
                    let before_branch = self.branches[index - 1].clone();
                    return Some(before_branch);
                }
                None
            }
        }
    }

    pub fn after(&self, needle_branch: &Branch) -> Option<Branch> {
        if self.branches.is_empty() {
            return None;
        }

        let maybe_index = self.branches.iter().position(|b| b == needle_branch);

        match maybe_index {
            None => None,
            Some(index) => {
                if index == (self.branches.len() - 1) {
                    return None;
                }
                let after_branch = self.branches[index + 1].clone();
                Some(after_branch)
            }
        }
    }

    pub fn change_root_branch(
        &self,
        git_chain: &GitChain,
        new_root_branch: &str,
    ) -> Result<(), Error> {
        // verify that none of the branches of the chain are equal to new_root_branch
        for branch in &self.branches {
            if new_root_branch == branch.branch_name {
                eprintln!(
                    "Unable to update the root branch for the branches in the chain: {}",
                    self.name.bold()
                );
                eprintln!(
                    "Branch cannot be the root branch: {}",
                    branch.branch_name.bold()
                );
                process::exit(1);
            }
        }

        for branch in &self.branches {
            branch.change_root_branch(git_chain, new_root_branch)?;
        }

        Ok(())
    }

    pub fn delete(self, git_chain: &GitChain) -> Result<Vec<String>, Error> {
        let mut deleted_branches: Vec<String> = vec![];
        for branch in self.branches {
            deleted_branches.push(branch.branch_name.clone());
            branch.remove_from_chain(git_chain)?;
        }

        Ok(deleted_branches)
    }

    pub fn backup(&self, git_chain: &GitChain) -> Result<(), Error> {
        for branch in &self.branches {
            branch.backup(git_chain)?;
        }
        Ok(())
    }

    pub fn push(&self, git_chain: &GitChain, force_push: bool) -> Result<usize, Error> {
        let mut num_of_pushes = 0;
        for branch in &self.branches {
            if branch.push(git_chain, force_push)? {
                num_of_pushes += 1;
            }
        }
        Ok(num_of_pushes)
    }

    pub fn prune(&self, git_chain: &GitChain, dry_run: bool) -> Result<Vec<String>, Error> {
        let mut pruned_branches = vec![];
        for branch in self.branches.clone() {
            // branch is an ancestor of the root branch if:
            // - it is the root branch, or
            // - the branch is a commit that occurs before the root branch.
            if git_chain.is_ancestor(&branch.branch_name, &self.root_branch)? {
                let branch_name = branch.branch_name.clone();

                if !dry_run {
                    branch.remove_from_chain(git_chain)?;
                }

                pruned_branches.push(branch_name);
            }
        }
        Ok(pruned_branches)
    }

    pub fn rename(self, git_chain: &GitChain, new_chain_name: &str) -> Result<(), Error> {
        // invariant: new_chain_name chain does not exist
        assert!(!Chain::chain_exists(git_chain, new_chain_name)?);

        for branch in self.branches {
            Branch::setup_branch(
                git_chain,
                new_chain_name,
                &branch.root_branch,
                &branch.branch_name,
                &SortBranch::Last,
            )?;
        }
        Ok(())
    }
}
