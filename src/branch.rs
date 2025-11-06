use std::io::{self, Write};
use std::iter::FromIterator;
use std::process::Command;

use between::Between;
use colored::*;
use git2::{BranchType, Error, ErrorCode};
use rand::Rng;

use crate::types::*;
use crate::{Chain, GitChain};

fn chain_name_key(branch_name: &str) -> String {
    format!("branch.{}.chain-name", branch_name)
}

fn chain_order_key(branch_name: &str) -> String {
    format!("branch.{}.chain-order", branch_name)
}

fn root_branch_key(branch_name: &str) -> String {
    format!("branch.{}.root-branch", branch_name)
}

fn generate_chain_order() -> String {
    let between = Between::init();
    let chars = between.chars();
    let chars_length = chars.len();
    assert!(chars_length >= 3);
    let last_chars_index = chars_length - 1;

    // Use character that is not either between.low() or between.high().
    // This guarantees that the next generated string sorts before or after the string generated in this function.
    let character_range = 1..=(last_chars_index - 1);
    let mut rng = rand::thread_rng();

    let mut len = 5;
    let mut str: Vec<char> = vec![];

    while len >= 1 {
        let index: usize = rng.gen_range(character_range.clone());
        let character_candidate = *chars.get(index).unwrap();
        str.push(character_candidate);
        len -= 1;
    }

    String::from_iter(str)
}

fn generate_chain_order_after(chain_order: &str) -> Option<String> {
    let between = Between::init();
    between.after(chain_order)
}

fn generate_chain_order_before(chain_order: &str) -> Option<String> {
    let between = Between::init();
    between.before(chain_order)
}

fn generate_chain_order_between(before: &str, after: &str) -> Option<String> {
    let between = Between::init();
    between.between(before, after)
}

#[derive(Clone, PartialEq)]
pub struct Branch {
    pub branch_name: String,
    pub chain_name: String,
    pub chain_order: String,
    pub root_branch: String,
}

impl Branch {
    pub fn delete_all_configs(git_chain: &GitChain, branch_name: &str) -> Result<(), Error> {
        git_chain.delete_git_config(&chain_name_key(branch_name))?;
        git_chain.delete_git_config(&chain_order_key(branch_name))?;
        git_chain.delete_git_config(&root_branch_key(branch_name))?;
        Ok(())
    }

    pub fn remove_from_chain(self, git_chain: &GitChain) -> Result<(), Error> {
        Branch::delete_all_configs(git_chain, &self.branch_name)
    }

    pub fn get_branch_with_chain(
        git_chain: &GitChain,
        branch_name: &str,
    ) -> Result<BranchSearchResult, Error> {
        let chain_name = git_chain.get_git_config(&chain_name_key(branch_name))?;
        let chain_order = git_chain.get_git_config(&chain_order_key(branch_name))?;
        let root_branch = git_chain.get_git_config(&root_branch_key(branch_name))?;

        if chain_name.is_none()
            || chain_order.is_none()
            || root_branch.is_none()
            || !git_chain.git_local_branch_exists(branch_name)?
        {
            Branch::delete_all_configs(git_chain, branch_name)?;
            return Ok(BranchSearchResult::NotPartOfAnyChain);
        }

        let branch = Branch {
            branch_name: branch_name.to_string(),
            chain_name: chain_name.unwrap(),
            chain_order: chain_order.unwrap(),
            root_branch: root_branch.unwrap(),
        };

        Ok(BranchSearchResult::Branch(branch))
    }

    fn generate_chain_order(
        git_chain: &GitChain,
        chain_name: &str,
        sort_option: &SortBranch,
    ) -> Result<String, Error> {
        let chain_order = if Chain::chain_exists(git_chain, chain_name)? {
            // invariant: a chain exists if and only if it has at least one branch.
            let chain = Chain::get_chain(git_chain, chain_name)?;
            assert!(!chain.branches.is_empty());

            let maybe_chain_order = match sort_option {
                SortBranch::First => {
                    let first_branch = chain.branches.first().unwrap();
                    generate_chain_order_before(&first_branch.chain_order)
                }
                SortBranch::Last => {
                    let last_branch = chain.branches.last().unwrap();
                    generate_chain_order_after(&last_branch.chain_order)
                }
                SortBranch::Before(after_branch) => match chain.before(after_branch) {
                    None => generate_chain_order_before(&after_branch.chain_order),
                    Some(before_branch) => generate_chain_order_between(
                        &before_branch.chain_order,
                        &after_branch.chain_order,
                    ),
                },
                SortBranch::After(before_branch) => match chain.after(before_branch) {
                    None => generate_chain_order_after(&before_branch.chain_order),
                    Some(after_branch) => generate_chain_order_between(
                        &before_branch.chain_order,
                        &after_branch.chain_order,
                    ),
                },
            };

            match maybe_chain_order {
                Some(chain_order) => chain_order,
                None => {
                    let mut chain_order = generate_chain_order();
                    // last resort
                    while chain.has_chain_order(&chain_order) {
                        chain_order = generate_chain_order();
                    }
                    chain_order
                }
            }
        } else {
            generate_chain_order()
        };

        Ok(chain_order)
    }

    pub fn setup_branch(
        git_chain: &GitChain,
        chain_name: &str,
        root_branch: &str,
        branch_name: &str,
        sort_option: &SortBranch,
    ) -> Result<(), Error> {
        Branch::delete_all_configs(git_chain, branch_name)?;

        let chain_order = Branch::generate_chain_order(git_chain, chain_name, sort_option)?;
        git_chain.set_git_config(&chain_order_key(branch_name), &chain_order)?;
        git_chain.set_git_config(&root_branch_key(branch_name), root_branch)?;
        git_chain.set_git_config(&chain_name_key(branch_name), chain_name)?;

        Ok(())
    }

    pub fn display_status(&self, git_chain: &GitChain, show_prs: bool) -> Result<(), Error> {
        let chain = Chain::get_chain(git_chain, &self.chain_name)?;

        let current_branch = git_chain.get_current_branch_name()?;

        chain.display_list(git_chain, &current_branch, show_prs)?;

        Ok(())
    }

    pub fn change_root_branch(
        &self,
        git_chain: &GitChain,
        new_root_branch: &str,
    ) -> Result<(), Error> {
        git_chain.set_git_config(&root_branch_key(&self.branch_name), new_root_branch)?;
        Ok(())
    }

    pub fn move_branch(
        &self,
        git_chain: &GitChain,
        chain_name: &str,
        sort_option: &SortBranch,
    ) -> Result<(), Error> {
        Branch::setup_branch(
            git_chain,
            chain_name,
            &self.root_branch,
            &self.branch_name,
            sort_option,
        )?;
        Ok(())
    }

    pub fn backup(&self, git_chain: &GitChain) -> Result<(), Error> {
        let (object, _reference) = git_chain.repo.revparse_ext(&self.branch_name)?;
        let commit = git_chain.repo.find_commit(object.id())?;

        let backup_branch = format!("backup-{}/{}", self.chain_name, self.branch_name);

        git_chain.repo.branch(&backup_branch, &commit, true)?;

        Ok(())
    }

    pub fn push(&self, git_chain: &GitChain, force_push: bool) -> Result<bool, Error> {
        // get branch's upstream

        let branch = match git_chain
            .repo
            .find_branch(&self.branch_name, BranchType::Local)
        {
            Ok(branch) => branch,
            Err(e) => {
                if e.code() == ErrorCode::NotFound {
                    // do nothing
                    return Ok(false);
                }
                return Err(e);
            }
        };

        match branch.upstream() {
            Ok(_remote_branch) => {
                let remote = git_chain
                    .repo
                    .branch_upstream_remote(branch.get().name().unwrap())?;
                let remote = remote.as_str().unwrap();

                let output = if force_push {
                    // git push --force-with-lease <remote> <branch>
                    Command::new("git")
                        .arg("push")
                        .arg("--force-with-lease")
                        .arg(remote)
                        .arg(&self.branch_name)
                        .output()
                        .unwrap_or_else(|_| {
                            panic!(
                                "Unable to push branch to their upstream: {}",
                                self.branch_name.bold()
                            )
                        })
                } else {
                    // git push <remote> <branch>
                    Command::new("git")
                        .arg("push")
                        .arg(remote)
                        .arg(&self.branch_name)
                        .output()
                        .unwrap_or_else(|_| {
                            panic!(
                                "Unable to push branch to their upstream: {}",
                                self.branch_name.bold()
                            )
                        })
                };

                if output.status.success() {
                    if force_push {
                        println!("✅ Force pushed {}", self.branch_name.bold());
                    } else {
                        println!("✅ Pushed {}", self.branch_name.bold());
                    }

                    Ok(true)
                } else {
                    io::stdout().write_all(&output.stdout).unwrap();
                    io::stderr().write_all(&output.stderr).unwrap();
                    println!("🛑 Unable to push {}", self.branch_name.bold());
                    Ok(false)
                }
            }
            Err(e) => {
                if e.code() == ErrorCode::NotFound {
                    println!(
                        "🛑 Cannot push. Branch has no upstream: {}",
                        self.branch_name.bold()
                    );
                    // do nothing
                    return Ok(false);
                }
                Err(e)
            }
        }
    }
}
