use std::collections::HashSet;
use std::io::{self, Write};
use std::process;
use std::process::Command;
use std::{collections::HashMap, iter::FromIterator};

use between::Between;
use clap::{App, Arg, ArgMatches, SubCommand};
use colored::*;
use git2::{
    BranchType, Config, ConfigLevel, Error, ErrorCode, ObjectType, Repository, RepositoryState,
};
use regex::Regex;

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
    assert!(chars_length >= 2);

    let mut len = 5;

    let mut str: Vec<char> = vec![];

    while len >= 2 {
        let x = rand::random::<f64>();
        let index = (x * (chars_length as f64)).floor() as usize;
        str.push(*chars.get(index).unwrap());
        len -= 1;
    }

    // add last character that is not between.low()
    let x = rand::random::<f64>();
    let range = (chars_length - 2) as f64;
    let index = ((x * range).floor() as usize) + 1;
    let last_char = *chars.get(index).unwrap();
    assert!(last_char != between.low());
    str.push(last_char);

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

fn print_rebase_error(executable_name: &str, branch: &str, upstream_branch: &str) {
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

enum BranchSearchResult {
    NotPartOfAnyChain(String),
    Branch(Branch),
}

enum SortBranch {
    Last,
    Before(Branch),
    After(Branch),
}

#[derive(Clone, PartialEq)]
struct Branch {
    branch_name: String,
    chain_name: String,
    chain_order: String,
    root_branch: String,
}

impl Branch {
    fn delete_all_configs(git_chain: &GitChain, branch_name: &str) -> Result<(), Error> {
        git_chain.delete_git_config(&chain_name_key(branch_name))?;
        git_chain.delete_git_config(&chain_order_key(branch_name))?;
        git_chain.delete_git_config(&root_branch_key(branch_name))?;
        Ok(())
    }

    fn remove_from_chain(self, git_chain: &GitChain) -> Result<(), Error> {
        Branch::delete_all_configs(git_chain, &self.branch_name)
    }

    fn get_branch_with_chain(
        git_chain: &GitChain,
        branch_name: &str,
    ) -> Result<BranchSearchResult, Error> {
        let chain_name = git_chain.get_git_config(&chain_name_key(branch_name))?;
        let chain_order = git_chain.get_git_config(&chain_order_key(branch_name))?;
        let root_branch = git_chain.get_git_config(&root_branch_key(branch_name))?;

        if chain_name.is_none()
            || chain_order.is_none()
            || root_branch.is_none()
            || !git_chain.git_branch_exists(branch_name)?
        {
            Branch::delete_all_configs(git_chain, branch_name)?;
            return Ok(BranchSearchResult::NotPartOfAnyChain(
                branch_name.to_string(),
            ));
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

    fn setup_branch(
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

    fn display_status(&self, git_chain: &GitChain) -> Result<(), Error> {
        let chain = Chain::get_chain(git_chain, &self.chain_name)?;

        let current_branch = git_chain.get_current_branch_name()?;

        chain.display_list(git_chain, &current_branch)?;

        Ok(())
    }

    fn change_root_branch(&self, git_chain: &GitChain, new_root_branch: &str) -> Result<(), Error> {
        git_chain.set_git_config(&root_branch_key(&self.branch_name), new_root_branch)?;
        Ok(())
    }

    fn move_branch(
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

    fn backup(&self, git_chain: &GitChain) -> Result<(), Error> {
        let (object, _reference) = git_chain.repo.revparse_ext(&self.branch_name)?;
        let commit = git_chain.repo.find_commit(object.id())?;

        let backup_branch = format!("backup-{}/{}", self.chain_name, self.branch_name);

        git_chain.repo.branch(&backup_branch, &commit, true)?;

        Ok(())
    }

    fn push(&self, git_chain: &GitChain, force_push: bool) -> Result<bool, Error> {
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

#[derive(Clone)]
struct Chain {
    name: String,
    root_branch: String,
    branches: Vec<Branch>,
}

impl Chain {
    fn get_all_branch_configs(git_chain: &GitChain) -> Result<Vec<(String, String)>, Error> {
        let key_regex = Regex::new(r"^branch\.(?P<branch_name>.+)\.chain-name$".trim()).unwrap();
        git_chain.get_git_configs_matching_key(&key_regex)
    }

    fn get_all_chains(git_chain: &GitChain) -> Result<Vec<Chain>, Error> {
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
                BranchSearchResult::NotPartOfAnyChain(_) => {
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

    fn chain_exists(git_chain: &GitChain, chain_name: &str) -> Result<bool, Error> {
        let branches = Chain::get_branches_for_chain(git_chain, chain_name)?;
        Ok(!branches.is_empty())
    }

    fn get_chain(git_chain: &GitChain, chain_name: &str) -> Result<Self, Error> {
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

    fn has_chain_order(&self, chain_order: &str) -> bool {
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

    fn display_list(&self, git_chain: &GitChain, current_branch: &str) -> Result<(), Error> {
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

            let status_line = if ahead_behind_status.is_empty() {
                format!("{:>6}{}", marker, branch_name)
            } else {
                format!("{:>6}{} ⦁ {}", marker, branch_name, ahead_behind_status)
            };

            println!("{}", status_line.trim_end());
        }

        if self.root_branch == current_branch {
            println!("{:>6}{} (root branch)", "➜ ", self.root_branch.bold());
        } else {
            println!("{:>6}{} (root branch)", "", self.root_branch);
        };

        Ok(())
    }

    fn before(&self, needle_branch: &Branch) -> Option<Branch> {
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

    fn after(&self, needle_branch: &Branch) -> Option<Branch> {
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

    fn change_root_branch(&self, git_chain: &GitChain, new_root_branch: &str) -> Result<(), Error> {
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

    fn delete(self, git_chain: &GitChain) -> Result<Vec<String>, Error> {
        let mut deleted_branches: Vec<String> = vec![];
        for branch in self.branches {
            deleted_branches.push(branch.branch_name.clone());
            branch.remove_from_chain(git_chain)?;
        }

        Ok(deleted_branches)
    }

    fn backup(&self, git_chain: &GitChain) -> Result<(), Error> {
        for branch in &self.branches {
            branch.backup(git_chain)?;
        }
        Ok(())
    }

    fn push(&self, git_chain: &GitChain, force_push: bool) -> Result<usize, Error> {
        let mut num_of_pushes = 0;
        for branch in &self.branches {
            if branch.push(git_chain, force_push)? {
                num_of_pushes += 1;
            }
        }
        Ok(num_of_pushes)
    }

    fn prune(&self, git_chain: &GitChain, dry_run: bool) -> Result<Vec<String>, Error> {
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

    fn rename(self, git_chain: &GitChain, new_chain_name: &str) -> Result<(), Error> {
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

struct GitChain {
    executable_name: String,
    repo: Repository,
}

impl GitChain {
    fn init() -> Result<Self, Error> {
        let name_of_current_executable = executable_name();

        let repo = Repository::discover(".")?;

        if repo.is_bare() {
            eprintln!(
                "Cannot run {} on bare git repository.",
                name_of_current_executable
            );
            process::exit(1);
        }

        let git_chain = GitChain {
            repo,
            executable_name: name_of_current_executable,
        };
        Ok(git_chain)
    }

    fn get_current_branch_name(&self) -> Result<String, Error> {
        let head = match self.repo.head() {
            Ok(head) => Some(head),
            Err(ref e)
                if e.code() == ErrorCode::UnbornBranch || e.code() == ErrorCode::NotFound =>
            {
                None
            }
            Err(e) => return Err(e),
        };

        let head = head.as_ref().and_then(|h| h.shorthand());

        match head {
            Some(branch_name) => Ok(branch_name.to_string()),
            None => Err(Error::from_str("Unable to get current branch name.")),
        }
    }

    fn get_local_git_config(&self) -> Result<Config, Error> {
        self.repo.config()?.open_level(ConfigLevel::Local)
    }

    fn get_git_config(&self, key: &str) -> Result<Option<String>, Error> {
        let local_config = self.get_local_git_config()?;
        match local_config.get_string(key) {
            Ok(value) => Ok(Some(value)),
            Err(ref e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn get_git_configs_matching_key(&self, regexp: &Regex) -> Result<Vec<(String, String)>, Error> {
        let local_config = self.get_local_git_config()?;
        let mut entries = vec![];
        for entry in &local_config.entries(None)? {
            let entry = entry?;
            if let Some(key) = entry.name() {
                if regexp.is_match(key) && entry.has_value() {
                    let key = key.to_string();
                    let value = entry.value().unwrap().to_string();
                    entries.push((key, value));
                }
            }
        }
        Ok(entries)
    }

    fn set_git_config(&self, key: &str, value: &str) -> Result<(), Error> {
        let mut local_config = self.get_local_git_config()?;
        local_config.set_str(key, value)?;
        Ok(())
    }

    fn delete_git_config(&self, key: &str) -> Result<(), Error> {
        let mut local_config = self.get_local_git_config()?;
        match local_config.remove(key) {
            Ok(()) => Ok(()),
            Err(ref e) if e.code() == ErrorCode::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn checkout_branch(&self, branch_name: &str) -> Result<(), Error> {
        let (object, reference) = self.repo.revparse_ext(branch_name)?;

        // set working directory
        self.repo.checkout_tree(&object, None)?;

        // set HEAD to branch_name
        match reference {
            // ref_name is an actual reference like branches or tags
            Some(ref_name) => self.repo.set_head(ref_name.name().unwrap()),
            // this is a commit, not a reference
            None => self.repo.set_head_detached(object.id()),
        }
        .unwrap_or_else(|_| panic!("Failed to set HEAD to branch {}", branch_name));

        Ok(())
    }

    fn git_branch_exists(&self, branch_name: &str) -> Result<bool, Error> {
        match self.repo.find_branch(branch_name, BranchType::Local) {
            Ok(_branch) => Ok(true),
            Err(ref e) if e.code() == ErrorCode::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn display_branch_not_part_of_chain_error(&self, branch_name: &str) {
        eprintln!("❌ Branch is not part of any chain: {}", branch_name.bold());
        eprintln!(
            "To initialize a chain for this branch, run {} init <chain_name> <root_branch>",
            self.executable_name
        );
    }

    fn run_status(&self) -> Result<(), Error> {
        let branch_name = self.get_current_branch_name()?;
        println!("On branch: {}", branch_name.bold());
        println!();

        let results = Branch::get_branch_with_chain(self, &branch_name)?;

        match results {
            BranchSearchResult::NotPartOfAnyChain(_) => {
                self.display_branch_not_part_of_chain_error(&branch_name);
                process::exit(1);
            }
            BranchSearchResult::Branch(branch) => {
                branch.display_status(self)?;
            }
        }

        Ok(())
    }

    fn init_chain(
        &self,
        chain_name: &str,
        root_branch: &str,
        branch_name: &str,
        sort_option: SortBranch,
    ) -> Result<(), Error> {
        let results = Branch::get_branch_with_chain(self, branch_name)?;

        match results {
            BranchSearchResult::NotPartOfAnyChain(_) => {
                Branch::setup_branch(self, chain_name, root_branch, branch_name, &sort_option)?;

                match Branch::get_branch_with_chain(self, branch_name)? {
                    BranchSearchResult::NotPartOfAnyChain(_) => {
                        eprintln!("Unable to set up chain for branch: {}", branch_name.bold());
                        process::exit(1);
                    }
                    BranchSearchResult::Branch(branch) => {
                        println!("🔗 Succesfully set up branch: {}", branch_name.bold());
                        println!();
                        branch.display_status(self)?;
                    }
                };
            }
            BranchSearchResult::Branch(branch) => {
                eprintln!("❌ Unable to initialize branch to a chain.",);
                eprintln!();
                eprintln!("Branch already part of a chain: {}", branch_name.bold());
                eprintln!("It is part of the chain: {}", branch.chain_name.bold());
                eprintln!("With root branch: {}", branch.root_branch.bold());
                process::exit(1);
            }
        };

        Ok(())
    }

    fn remove_branch_from_chain(&self, branch_name: String) -> Result<(), Error> {
        let results = Branch::get_branch_with_chain(self, &branch_name)?;

        match results {
            BranchSearchResult::NotPartOfAnyChain(_) => {
                Branch::delete_all_configs(self, &branch_name)?;

                println!(
                    "Unable to remove branch from its chain: {}",
                    branch_name.bold()
                );
                println!("It is not part of any chain. Nothing to do.");
            }
            BranchSearchResult::Branch(branch) => {
                let chain_name = branch.chain_name.clone();
                let root_branch = branch.root_branch.clone();
                branch.remove_from_chain(self)?;

                println!(
                    "Removed branch {} from chain {}",
                    branch_name.bold(),
                    chain_name.bold()
                );
                println!("Its root branch was: {}", root_branch.bold());
            }
        };
        Ok(())
    }

    fn list_chains(&self, current_branch: &str) -> Result<(), Error> {
        let list = Chain::get_all_chains(self)?;

        if list.is_empty() {
            println!("No chains to list.");
            println!(
                "To initialize a chain for this branch, run {} init <root_branch> <chain_name>",
                self.executable_name
            );
            return Ok(());
        }

        for (index, chain) in list.iter().enumerate() {
            chain.display_list(self, current_branch)?;

            if index != list.len() - 1 {
                println!();
            }
        }

        Ok(())
    }

    fn move_branch(
        &self,
        chain_name: &str,
        branch_name: &str,
        sort_option: &SortBranch,
    ) -> Result<(), Error> {
        match Branch::get_branch_with_chain(self, branch_name)? {
            BranchSearchResult::NotPartOfAnyChain(_) => {
                self.display_branch_not_part_of_chain_error(branch_name);
                process::exit(1);
            }
            BranchSearchResult::Branch(branch) => {
                branch.move_branch(self, chain_name, sort_option)?;

                match Branch::get_branch_with_chain(self, &branch.branch_name)? {
                    BranchSearchResult::NotPartOfAnyChain(_) => {
                        eprintln!("Unable to move branch: {}", branch.branch_name.bold());
                        process::exit(1);
                    }
                    BranchSearchResult::Branch(branch) => {
                        println!("🔗 Succesfully moved branch: {}", branch.branch_name.bold());
                        println!();
                        branch.display_status(self)?;
                    }
                };
            }
        };

        Ok(())
    }

    fn get_commit_hash_of_head(&self) -> Result<String, Error> {
        let head = self.repo.head()?;
        let oid = head.target().unwrap();
        let commit = self.repo.find_commit(oid).unwrap();
        Ok(commit.id().to_string())
    }

    fn rebase(&self, chain_name: &str, step_rebase: bool) -> Result<(), Error> {
        // invariant: chain_name chain exists
        let chain = Chain::get_chain(self, chain_name)?;

        // ensure root branch exists
        if !self.git_branch_exists(&chain.root_branch)? {
            eprintln!("Root branch does not exist: {}", chain.root_branch.bold());
            process::exit(1);
        }

        // ensure each branch exists
        for branch in &chain.branches {
            if !self.git_branch_exists(&branch.branch_name)? {
                eprintln!("Root branch does not exist: {}", chain.root_branch.bold());
                process::exit(1);
            }
        }

        // ensure repository is in a clean state
        match self.repo.state() {
            RepositoryState::Clean => {
                // go ahead to rebase.
            }
            _ => {
                eprintln!("🛑 Repository needs to be in a clean state before rebasing.");
                process::exit(1);
            }
        }

        if self.dirty_working_directory()? {
            eprintln!(
                "🛑 Unable to rebase branches for the chain: {}",
                chain.name.bold()
            );
            eprintln!("You have uncommitted changes in your working directory.");
            eprintln!("Please commit or stash them.");
            process::exit(1);
        }

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
                let common_point = self.merge_base_fork_point(&root_branch, &branch.branch_name)?;
                common_ancestors.push(common_point);
                continue;
            }

            let prev_branch = &chain.branches[index - 1];

            let common_point =
                self.merge_base_fork_point(&prev_branch.branch_name, &branch.branch_name)?;
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

            // git rebase --onto <onto> <upstream> <branch>
            // git rebase --onto parent_branch fork_point branch.name

            self.checkout_branch(&branch.branch_name)?;

            let before_sha1 = self.get_commit_hash_of_head()?;

            let common_point = &common_ancestors[index];

            let command = format!(
                "git rebase --keep-empty --onto {} {} {}",
                &prev_branch_name, common_point, &branch.branch_name
            );

            let output = Command::new("git")
                .arg("rebase")
                .arg("--keep-empty")
                .arg("--onto")
                .arg(&prev_branch_name)
                .arg(&common_point)
                .arg(&branch.branch_name)
                .output()
                .unwrap_or_else(|_| panic!("Unable to run: {}", &command));

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

        if num_of_rebase_operations > 0 {
            println!("🎉 Successfully rebased chain {}", chain.name.bold());
        } else {
            println!("Chain {} is already up-to-date.", chain.name.bold());
        }

        Ok(())
    }

    fn dirty_working_directory(&self) -> Result<bool, Error> {
        // perform equivalent to git diff-index HEAD
        let obj = self.repo.revparse_single("HEAD")?;
        let tree = obj.peel(ObjectType::Tree)?;

        // This is used for diff formatting for diff-index. But we're only interested in the diff stats.
        // let mut opts = DiffOptions::new();
        // opts.id_abbrev(40);

        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(tree.as_tree(), None)?;

        let diff_stats = diff.stats()?;
        let has_changes = diff_stats.files_changed() > 0
            || diff_stats.insertions() > 0
            || diff_stats.deletions() > 0;

        Ok(has_changes)
    }

    fn backup(&self, chain_name: &str) -> Result<(), Error> {
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

    fn push(&self, chain_name: &str, force_push: bool) -> Result<(), Error> {
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

    fn prune(&self, chain_name: &str, dry_run: bool) -> Result<(), Error> {
        if Chain::chain_exists(self, chain_name)? {
            let chain = Chain::get_chain(self, chain_name)?;

            let pruned_branches = chain.prune(self, dry_run)?;
            if !pruned_branches.is_empty() {
                println!(
                    "Removed the following branches from chain: {}",
                    chain_name.bold()
                );

                for branch in &pruned_branches {
                    println!("{}", branch);
                }

                println!(
                    "Pruned {} branches.",
                    format!("{}", pruned_branches.len()).bold()
                );

                if dry_run {
                    println!(
                        "This was a dry-run, no branches pruned for chain: {}",
                        chain_name.bold()
                    );
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

    fn merge_base_fork_point(
        &self,
        ancestor_branch: &str,
        descendant_branch: &str,
    ) -> Result<String, Error> {
        // git merge-base --fork-point <ancestor_branch> <descendant_branch>

        let output = Command::new("git")
            .arg("merge-base")
            .arg("--fork-point")
            .arg(&ancestor_branch)
            .arg(&descendant_branch)
            .output()
            .unwrap_or_else(|_| {
                panic!(
                    "Unable to get forkpoint of {} and {}",
                    ancestor_branch.bold(),
                    descendant_branch.bold()
                )
            });

        if output.status.success() {
            let raw_output = String::from_utf8(output.stdout).unwrap();
            let common_point = raw_output.trim().to_string();
            return Ok(common_point);
        }
        return Err(Error::from_str(&format!(
            "Unable to get forkpoint of {} and {}",
            ancestor_branch.bold(),
            descendant_branch.bold()
        )));
    }

    fn is_ancestor(&self, ancestor_branch: &str, descendant_branch: &str) -> Result<bool, Error> {
        let (ancestor_object, _reference) = self.repo.revparse_ext(ancestor_branch)?;
        let (descendant_object, _reference) = self.repo.revparse_ext(descendant_branch)?;

        let common_point = self
            .repo
            .merge_base(ancestor_object.id(), descendant_object.id())?;

        Ok(common_point == ancestor_object.id())
    }
}

fn parse_sort_option(
    git_chain: &GitChain,
    chain_name: &str,
    before_branch: Option<&str>,
    after_branch: Option<&str>,
) -> Result<SortBranch, Error> {
    if let Some(before_branch) = before_branch {
        if !git_chain.git_branch_exists(before_branch)? {
            return Err(Error::from_str(&format!(
                "Branch does not exist: {}",
                before_branch.bold()
            )));
        }

        let before_branch = match Branch::get_branch_with_chain(git_chain, before_branch)? {
            BranchSearchResult::NotPartOfAnyChain(_) => {
                git_chain.display_branch_not_part_of_chain_error(before_branch);
                process::exit(1);
            }
            BranchSearchResult::Branch(before_branch) => {
                if before_branch.chain_name != chain_name {
                    return Err(Error::from_str(&format!(
                        "Branch {} is not part of chain {}",
                        before_branch.branch_name.bold(),
                        chain_name.bold()
                    )));
                }
                before_branch
            }
        };

        Ok(SortBranch::Before(before_branch))
    } else if let Some(after_branch) = after_branch {
        if !git_chain.git_branch_exists(after_branch)? {
            return Err(Error::from_str(&format!(
                "Branch does not exist: {}",
                after_branch.bold()
            )));
        }

        let after_branch = match Branch::get_branch_with_chain(git_chain, after_branch)? {
            BranchSearchResult::NotPartOfAnyChain(_) => {
                git_chain.display_branch_not_part_of_chain_error(after_branch);
                process::exit(1);
            }
            BranchSearchResult::Branch(after_branch) => {
                if after_branch.chain_name != chain_name {
                    return Err(Error::from_str(&format!(
                        "Branch {} is not part of chain {}",
                        after_branch.branch_name.bold(),
                        chain_name.bold()
                    )));
                }
                after_branch
            }
        };

        Ok(SortBranch::After(after_branch))
    } else {
        Ok(SortBranch::Last)
    }
}

fn run(arg_matches: ArgMatches) -> Result<(), Error> {
    let git_chain = GitChain::init()?;

    match arg_matches.subcommand() {
        ("init", Some(sub_matches)) => {
            // Initialize the current branch to a chain.

            let chain_name = sub_matches.value_of("chain_name").unwrap().to_string();
            let root_branch = sub_matches.value_of("root_branch");

            let before_branch = sub_matches.value_of("before");
            let after_branch = sub_matches.value_of("after");

            let branch_name = git_chain.get_current_branch_name()?;

            let root_branch = if Chain::chain_exists(&git_chain, &chain_name)? {
                // Derive root branch from an existing chain
                let chain = Chain::get_chain(&git_chain, &chain_name)?;

                if let Some(user_provided_root_branch) = root_branch {
                    if user_provided_root_branch != chain.root_branch {
                        println!(
                            "Using root branch {} of chain {} instead of {}",
                            chain.root_branch.bold(),
                            chain_name.bold(),
                            user_provided_root_branch.bold()
                        );
                    }
                }

                chain.root_branch
            } else if let Some(root_branch) = root_branch {
                root_branch.to_string()
            } else {
                eprintln!("Please provide the root branch.");
                process::exit(1);
            };

            if !git_chain.git_branch_exists(&root_branch)? {
                eprintln!("Root branch does not exist: {}", root_branch.bold());
                process::exit(1);
            }

            if root_branch == branch_name {
                eprintln!(
                    "Current branch cannot be the root branch: {}",
                    branch_name.bold()
                );
                process::exit(1);
            }

            let sort_option =
                parse_sort_option(&git_chain, &chain_name, before_branch, after_branch)?;

            git_chain.init_chain(&chain_name, &root_branch, &branch_name, sort_option)?
        }
        ("remove", Some(sub_matches)) => {
            // Remove current branch from its chain.

            let chain_name = sub_matches.value_of("chain_name");

            let branch_name = git_chain.get_current_branch_name()?;

            if let Some(chain_name) = chain_name {
                // Only delete a specific chain
                if Chain::chain_exists(&git_chain, chain_name)? {
                    let chain = Chain::get_chain(&git_chain, chain_name)?;
                    let deleted_branches = chain.delete(&git_chain)?;

                    if !deleted_branches.is_empty() {
                        println!("Removed the following branches from their chains:");
                        for branch_name in deleted_branches {
                            println!("{}", branch_name)
                        }
                    }
                    println!("Successfully deleted chain: {}", chain_name.bold());
                    return Ok(());
                }

                println!(
                    "Unable to delete chain that does not exist: {}",
                    chain_name.bold()
                );
                println!("Nothing to do.");

                return Ok(());
            }

            git_chain.remove_branch_from_chain(branch_name)?
        }
        ("list", Some(_sub_matches)) => {
            // List all chains.
            let current_branch = git_chain.get_current_branch_name()?;
            git_chain.list_chains(&current_branch)?
        }
        ("move", Some(sub_matches)) => {
            // Move current branch or chain.

            let before_branch = sub_matches.value_of("before");
            let after_branch = sub_matches.value_of("after");
            let root_branch = sub_matches.value_of("root");
            let chain_name = sub_matches.value_of("chain_name");

            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain(_) => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            if let Some(root_branch) = root_branch {
                // invariant: chain_name is None
                // clap ensures this invariant
                assert!(chain_name.is_none());

                if !git_chain.git_branch_exists(root_branch)? {
                    eprintln!("Root branch does not exist: {}", root_branch.bold());
                    process::exit(1);
                }

                if root_branch == branch_name {
                    eprintln!(
                        "Current branch cannot be the root branch: {}",
                        branch_name.bold()
                    );
                    process::exit(1);
                }

                let chain = Chain::get_chain(&git_chain, &branch.chain_name)?;

                let old_root_branch = chain.root_branch.clone();

                chain.change_root_branch(&git_chain, root_branch)?;

                println!(
                    "Changed root branch for the chain {} from {} to {}",
                    chain.name.bold(),
                    old_root_branch.bold(),
                    root_branch.bold()
                );
            }

            match chain_name {
                None => {
                    let chain_name = branch.chain_name;
                    if before_branch.is_some() || after_branch.is_some() {
                        let sort_option = parse_sort_option(
                            &git_chain,
                            &chain_name,
                            before_branch,
                            after_branch,
                        )?;
                        git_chain.move_branch(&chain_name, &branch_name, &sort_option)?
                    } else {
                        // nothing to do
                        println!("Nothing to do. ☕");
                    }
                }
                Some(new_chain_name) => {
                    let old_chain_name = branch.chain_name;
                    if before_branch.is_some()
                        || after_branch.is_some()
                        || new_chain_name != old_chain_name
                    {
                        let sort_option = parse_sort_option(
                            &git_chain,
                            new_chain_name,
                            before_branch,
                            after_branch,
                        )?;
                        git_chain.move_branch(new_chain_name, &branch_name, &sort_option)?
                    } else {
                        // nothing to do
                        println!("Nothing to do. ☕");
                    }
                }
            };
        }
        ("rebase", Some(sub_matches)) => {
            // Rebase all branches for the current chain.
            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain(_) => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            if Chain::chain_exists(&git_chain, &branch.chain_name)? {
                let step_rebase = sub_matches.is_present("step");
                git_chain.rebase(&branch.chain_name, step_rebase)?;
            } else {
                eprintln!("Unable to rebase chain.");
                eprintln!("Chain does not exist: {}", branch.chain_name.bold());
                process::exit(1);
            }
        }
        ("backup", Some(_sub_matches)) => {
            // Back up all branches of the current chain.

            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain(_) => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            git_chain.backup(&branch.chain_name)?;
        }
        ("push", Some(sub_matches)) => {
            // Push all branches of the current chain to their upstreams.

            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain(_) => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            let force_push = sub_matches.is_present("force");
            git_chain.push(&branch.chain_name, force_push)?;
        }
        ("prune", Some(sub_matches)) => {
            // Prune any branches of the current chain.

            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain(_) => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            let dry_run = sub_matches.is_present("dry_run");

            git_chain.prune(&branch.chain_name, dry_run)?;
        }
        ("rename", Some(sub_matches)) => {
            // Rename current chain.

            let new_chain_name = sub_matches.value_of("chain_name").unwrap().to_string();

            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain(_) => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            if Chain::chain_exists(&git_chain, &new_chain_name)? {
                eprintln!(
                    "Unable to rename chain {} to {}",
                    branch.chain_name.bold(),
                    new_chain_name.bold()
                );
                eprintln!("Chain already exists: {}", branch.chain_name.bold());
                process::exit(1);
            }

            if Chain::chain_exists(&git_chain, &branch.chain_name)? {
                let chain = Chain::get_chain(&git_chain, &branch.chain_name)?;
                let old_chain_name = chain.name.clone();
                chain.rename(&git_chain, &new_chain_name)?;
                println!(
                    "Renamed chain from {} to {}",
                    old_chain_name.bold(),
                    new_chain_name.bold()
                );
            } else {
                eprintln!("Unable to rename chain.");
                eprintln!("Chain does not exist: {}", new_chain_name.bold());
                process::exit(1);
            }
        }
        ("setup", Some(sub_matches)) => {
            // Set up a chain.

            let chain_name = sub_matches.value_of("chain_name").unwrap().to_string();
            let root_branch = sub_matches.value_of("root_branch").unwrap().to_string();

            let branches: Vec<String> = sub_matches
                .values_of("branch")
                .unwrap()
                .map(|x| x.to_string())
                .collect();

            // ensure root branch exists
            if !git_chain.git_branch_exists(&root_branch)? {
                eprintln!("Root branch does not exist: {}", root_branch.bold());
                process::exit(1);
            }

            let mut visited_branches = HashSet::new();

            for branch_name in &branches {
                if branch_name == &root_branch {
                    eprintln!(
                        "Branch being added to the chain cannot be the root branch: {}",
                        branch_name.bold()
                    );
                    process::exit(1);
                }

                if !git_chain.git_branch_exists(branch_name)? {
                    eprintln!("Branch does not exist: {}", branch_name.bold());
                    process::exit(1);
                }

                let results = Branch::get_branch_with_chain(&git_chain, branch_name)?;

                match results {
                    BranchSearchResult::Branch(branch) => {
                        eprintln!("❌ Unable to initialize branch to a chain.");
                        eprintln!();
                        eprintln!("Branch already part of a chain: {}", branch_name.bold());
                        eprintln!("It is part of the chain: {}", branch.chain_name.bold());
                        eprintln!("With root branch: {}", branch.root_branch.bold());
                        process::exit(1);
                    }
                    BranchSearchResult::NotPartOfAnyChain(_) => {}
                }

                if visited_branches.contains(branch_name) {
                    eprintln!(
                        "Branch defined on the chain at least twice: {}",
                        branch_name.bold()
                    );
                    eprintln!("Branches should be unique when setting up a new chain.");
                    process::exit(1);
                }
                visited_branches.insert(branch_name);
            }

            for branch_name in &branches {
                Branch::setup_branch(
                    &git_chain,
                    &chain_name,
                    &root_branch,
                    branch_name,
                    &SortBranch::Last,
                )?;
            }

            println!("🔗 Succesfully set up chain: {}", chain_name.bold());
            println!();

            let chain = Chain::get_chain(&git_chain, &chain_name)?;
            let current_branch = git_chain.get_current_branch_name()?;
            chain.display_list(&git_chain, &current_branch)?;
        }
        _ => {
            git_chain.run_status()?;
        }
    }

    Ok(())
}

fn main() {
    let init_subcommand = SubCommand::with_name("init")
        .about("Initialize the current branch to a chain.")
        .arg(
            Arg::with_name("before")
                .short("b")
                .long("before")
                .value_name("branch_name")
                .help("Sort current branch before another branch.")
                .conflicts_with("after")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("after")
                .short("a")
                .long("after")
                .value_name("branch_name")
                .help("Sort current branch after another branch.")
                .conflicts_with("before")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("chain_name")
                .help("The name of the chain.")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("root_branch")
                .help("The root branch which the chain of branches will merge into.")
                .required(false)
                .index(2),
        );

    let remove_subcommand = SubCommand::with_name("remove")
        .about("Remove current branch from its chain.")
        .arg(
            Arg::with_name("chain_name")
                .short("c")
                .long("chain")
                .value_name("chain_name")
                .help("Delete chain by removing all of its branches.")
                .takes_value(true),
        );

    let move_subcommand = SubCommand::with_name("move")
        .about("Move current branch or chain.")
        .arg(
            Arg::with_name("before")
                .short("b")
                .long("before")
                .value_name("branch_name")
                .help("Sort current branch before another branch.")
                .conflicts_with("after")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("after")
                .short("a")
                .long("after")
                .value_name("branch_name")
                .help("Sort current branch after another branch.")
                .conflicts_with("before")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("root")
                .short("r")
                .long("root")
                .value_name("root_branch")
                .help("Set root branch of current branch and the chain it is a part of.")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("chain_name")
                .short("c")
                .long("chain")
                .value_name("chain_name")
                .help("Move current branch to another chain.")
                .conflicts_with("root")
                .takes_value(true),
        );

    let rebase_subcommand = SubCommand::with_name("rebase")
        .about("Rebase all branches for the current chain.")
        .arg(
            Arg::with_name("step")
                .short("s")
                .long("step")
                .value_name("step")
                .help("Stop at the first rebase.")
                .takes_value(false),
        );

    let push_subcommand = SubCommand::with_name("push")
        .about("Push all branches of the current chain to their upstreams.")
        .arg(
            Arg::with_name("force")
                .short("f")
                .long("force")
                .value_name("force")
                .help("Push branches with --force-with-lease")
                .takes_value(false),
        );

    let prune_subcommand = SubCommand::with_name("prune")
        .about("Prune any branches of the current chain that are ancestors of the root branch.")
        .arg(
            Arg::with_name("dry_run")
                .short("d")
                .long("dry-run")
                .value_name("dry_run")
                .help("Output branches that will be pruned.")
                .takes_value(false),
        );

    let rename_subcommand = SubCommand::with_name("rename")
        .about("Rename current chain.")
        .arg(
            Arg::with_name("chain_name")
                .help("The new name of the chain.")
                .required(true)
                .index(1),
        );

    let setup_subcommand = SubCommand::with_name("setup")
        .about("Set up a chain.")
        .arg(
            Arg::with_name("chain_name")
                .help("The new name of the chain.")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("root_branch")
                .help("The root branch which the chain of branches will merge into.")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::with_name("branch")
                .help("A branch to add to the chain")
                .required(true)
                .multiple(true)
                .index(3),
        );

    let arg_matches = App::new("git-chain")
        .bin_name(executable_name())
        .version("0.01")
        .author("Alberto Leal <mailforalberto@gmail.com>")
        .about("Tool for rebasing a chain of local git branches.")
        .subcommand(init_subcommand)
        .subcommand(remove_subcommand)
        .subcommand(move_subcommand)
        .subcommand(rebase_subcommand)
        .subcommand(push_subcommand)
        .subcommand(prune_subcommand)
        .subcommand(setup_subcommand)
        .subcommand(SubCommand::with_name("list").about("List all chains."))
        .subcommand(
            SubCommand::with_name("backup").about("Back up all branches of the current chain."),
        )
        .subcommand(rename_subcommand)
        .get_matches_from(std::env::args_os());

    match run(arg_matches) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("{} {}", "error:".red().bold(), err);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use git2::{BranchType, ConfigLevel, IndexAddOption, Oid, Repository};
    use std::fs;
    use std::fs::File;
    use std::io::prelude::*;
    use std::path::{Path, PathBuf};

    fn generate_path_to_repo<S>(repo_name: S) -> PathBuf
    where
        S: Into<String>,
    {
        let repo_name: String = repo_name.into();
        let test_fixture_path = Path::new("./test_sandbox/");
        let path_to_repo = test_fixture_path.join(repo_name);
        assert!(path_to_repo.is_relative());
        path_to_repo
    }

    fn setup_git_repo<S>(repo_name: S) -> Repository
    where
        S: Into<String>,
    {
        let path_to_repo = generate_path_to_repo(repo_name);

        fs::remove_dir_all(&path_to_repo).ok();
        fs::create_dir_all(&path_to_repo).unwrap();

        let repo = match Repository::init(path_to_repo) {
            Ok(repo) => repo,
            Err(err) => panic!("failed to init repo: {}", err),
        };

        let mut config = repo.config().unwrap();
        config.set_str("user.name", "name").unwrap();
        config.set_str("user.email", "email").unwrap();

        repo
    }

    fn teardown_git_repo<S>(repo_name: S)
    where
        S: Into<String>,
    {
        let path_to_repo = generate_path_to_repo(repo_name);
        fs::remove_dir_all(&path_to_repo).ok();
    }

    fn checkout_branch(repo: &Repository, branch_name: &str) {
        let obj = repo
            .revparse_single(&("refs/heads/".to_owned() + branch_name))
            .unwrap();

        repo.checkout_tree(&obj, None).unwrap();

        repo.set_head(&("refs/heads/".to_owned() + branch_name))
            .unwrap();
    }

    fn stage_everything(repo: &Repository) -> Oid {
        let mut index = repo.index().expect("cannot get the Index file");
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();

        let mut index = repo.index().unwrap();
        let root_tree_oid = index.write_tree().unwrap();
        root_tree_oid
    }

    #[test]
    fn deleted_branch_config_verification() {
        // This test verifies a git behaviour whereby deleting a branch will delete any and all configs whose keys begin with: branch.<name>
        // Reference: https://github.com/git/git/blob/f443b226ca681d87a3a31e245a70e6bc2769123c/builtin/branch.c#L184-L191

        let repo_name = "deleted_branch_config_verification";

        let repo = setup_git_repo(repo_name);

        let path_to_repo = generate_path_to_repo(repo_name);

        let root_tree_oid = {
            // create new file

            let mut file = File::create(path_to_repo.as_path().join("hello_world.txt")).unwrap();
            file.write_all(b"Hello, world!").unwrap();

            // stage all changes - git add -A *
            stage_everything(&repo)
        };

        // add first commit to master
        {
            let tree = repo.find_tree(root_tree_oid).unwrap();

            let author = &repo.signature().unwrap();
            let committer = &author;
            let message = "first commit";

            repo.commit(Some("HEAD"), author, committer, message, &tree, &[])
                .unwrap();
        };

        // create and checkout new branch named some_branch
        let branch_name = {
            let branch_name = "some_branch";

            // create branch
            let oid = repo.head().unwrap().target().unwrap();
            let commit = repo.find_commit(oid).unwrap();

            repo.branch("some_branch", &commit, false).unwrap();

            checkout_branch(&repo, branch_name);

            branch_name
        };

        let root_tree_oid = {
            // create new file

            let mut file = File::create(path_to_repo.as_path().join("file.txt")).unwrap();
            file.write_all(b"contents").unwrap();

            // stage all changes - git add -A *
            stage_everything(&repo)
        };

        // add commit to branch some_branch
        {
            let tree = repo.find_tree(root_tree_oid).unwrap();
            let head_id = repo.refname_to_id("HEAD").unwrap();
            let parent = repo.find_commit(head_id).unwrap();

            let author = &repo.signature().unwrap();
            let committer = &author;
            let message = "message";

            repo.commit(Some("HEAD"), author, committer, message, &tree, &[&parent])
                .unwrap();
        };

        // add custom config
        let repo_config = repo.config().unwrap();
        let mut local_config = repo_config.open_level(ConfigLevel::Local).unwrap();

        let config_key = format!("branch.{}.chain-name", branch_name);

        // verify config_key does not exist yet
        for entry in &local_config.entries(None).unwrap() {
            let entry = entry.unwrap();
            assert_ne!(entry.name().unwrap(), config_key);
        }

        local_config.set_str(&config_key, "chain_name").unwrap();

        let actual_value = local_config.get_string(&config_key).unwrap();
        assert_eq!(actual_value, "chain_name");

        // checkout master
        checkout_branch(&repo, "master");

        // delete branch some_branch
        let mut some_branch = repo.find_branch(branch_name, BranchType::Local).unwrap();
        assert!(!some_branch.is_head());
        some_branch.delete().unwrap();

        // verify if local custom config is deleted
        for entry in &local_config.entries(None).unwrap() {
            let entry = entry.unwrap();
            assert_ne!(entry.name().unwrap(), config_key);
        }

        teardown_git_repo(repo_name);
    }
}
