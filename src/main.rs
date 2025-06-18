use std::collections::HashSet;
use std::ffi::OsString;
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
use rand::Rng;
use regex::Regex;
use serde_json;

// Merge options types
#[derive(Debug, PartialEq, Clone, Copy)]
enum SquashedMergeHandling {
    // Reset the branch to the parent branch
    Reset,

    // Skip merging the branch
    Skip,

    // Force a merge despite the squashed merge detection
    Merge,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum ReportLevel {
    // Minimal reporting (just success/failure)
    Minimal,

    // Standard reporting (summary with counts)
    Standard,

    // Detailed reporting (all actions and their results)
    Detailed,
}

enum MergeResult {
    // Successfully merged with changes
    Success(String), // Contains the merge output message

    // Already up-to-date, no changes needed
    AlreadyUpToDate,

    // Merge conflict occurred
    Conflict(String), // Contains the conflict message
}

// For API consistency, we create our own Error variants
trait ErrorExt {
    #[allow(dead_code)]
    fn from_str(message: &str) -> Self;
    fn merge_conflict(branch: String, upstream: String, message: Option<String>) -> Self;
    fn git_command_failed(command: String, status: i32, stdout: String, stderr: String) -> Self;
}

impl ErrorExt for Error {
    fn from_str(message: &str) -> Self {
        Error::from_str(message)
    }

    fn merge_conflict(branch: String, upstream: String, message: Option<String>) -> Self {
        let mut error_msg = format!("Merge conflict between {} and {}", upstream, branch);
        if let Some(details) = message {
            error_msg.push('\n');
            error_msg.push_str(&details);
        }
        Error::from_str(&error_msg)
    }

    fn git_command_failed(command: String, status: i32, stdout: String, stderr: String) -> Self {
        let error_msg = format!(
            "Git command failed: {}\nStatus: {}\nStdout: {}\nStderr: {}",
            command, status, stdout, stderr
        );
        Error::from_str(&error_msg)
    }
}

struct MergeOptions {
    // Skip the merge of the root branch into the first branch
    ignore_root: bool,

    // Git merge options passed to all merge operations
    merge_flags: Vec<String>,

    // Whether to use fork point detection (more accurate but slower)
    use_fork_point: bool,

    // How to handle squashed merges (reset, skip, merge)
    squashed_merge_handling: SquashedMergeHandling,

    // Print verbose output
    verbose: bool,

    // Return to original branch after merging
    return_to_original: bool,

    // Use simple merge mode
    simple_mode: bool,

    // Level of detail in the final report
    report_level: ReportLevel,
}

impl Default for MergeOptions {
    fn default() -> Self {
        MergeOptions {
            ignore_root: false,
            merge_flags: vec![],
            use_fork_point: true,
            squashed_merge_handling: SquashedMergeHandling::Reset,
            verbose: false,
            return_to_original: true,
            simple_mode: false,
            report_level: ReportLevel::Standard,
        }
    }
}

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
    NotPartOfAnyChain,
    Branch(Branch),
}

enum SortBranch {
    First,
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

    fn display_status(&self, git_chain: &GitChain, show_prs: bool) -> Result<(), Error> {
        let chain = Chain::get_chain(git_chain, &self.chain_name)?;

        let current_branch = git_chain.get_current_branch_name()?;

        chain.display_list(git_chain, &current_branch, show_prs)?;

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

    fn display_list(&self, git_chain: &GitChain, current_branch: &str, show_prs: bool) -> Result<(), Error> {
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
                        let pr_objects: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap_or_default();
                        let pr_details: Vec<String> = pr_objects.iter().filter_map(|pr| {
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
                                },
                                _ => None,
                            }
                        }).collect();

                        if !pr_details.is_empty() {
                            let pr_list = pr_details.join("; ");
                            status_line.push_str(&format!(" ({})", pr_list));
                        }
                    }
                    _ => {
                        eprintln!("  Failed to retrieve PRs for branch {}.", branch.branch_name.bold());
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

// Structure to hold merge commit information
#[derive(Debug)]
struct MergeCommitInfo {
    message: Option<String>,
    stats: Option<MergeStats>,
}

#[derive(Debug)]
struct MergeStats {
    files_changed: usize,
    insertions: usize,
    deletions: usize,
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

        local_config.entries(None)?.for_each(|entry| {
            if let Some(key) = entry.name() {
                if regexp.is_match(key) && entry.has_value() {
                    let key = key.to_string();
                    let value = entry.value().unwrap().to_string();
                    entries.push((key, value));
                }
            }
        })?;

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
        Ok(self.git_local_branch_exists(branch_name)?
            || self.git_remote_branch_exists(branch_name)?)
    }

    fn git_local_branch_exists(&self, branch_name: &str) -> Result<bool, Error> {
        match self.repo.find_branch(branch_name, BranchType::Local) {
            Ok(_branch) => Ok(true),
            Err(ref e) if e.code() == ErrorCode::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn git_remote_branch_exists(&self, branch_name: &str) -> Result<bool, Error> {
        match self.repo.find_branch(branch_name, BranchType::Remote) {
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

    fn run_status(&self, show_prs: bool) -> Result<(), Error> {
        let branch_name = self.get_current_branch_name()?;
        println!("On branch: {}", branch_name.bold());
        println!();

        let results = Branch::get_branch_with_chain(self, &branch_name)?;

        match results {
            BranchSearchResult::NotPartOfAnyChain => {
                self.display_branch_not_part_of_chain_error(&branch_name);
                process::exit(1);
            }
            BranchSearchResult::Branch(branch) => {
                branch.display_status(self, show_prs)?;
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
            BranchSearchResult::NotPartOfAnyChain => {
                Branch::setup_branch(self, chain_name, root_branch, branch_name, &sort_option)?;

                match Branch::get_branch_with_chain(self, branch_name)? {
                    BranchSearchResult::NotPartOfAnyChain => {
                        eprintln!("Unable to set up chain for branch: {}", branch_name.bold());
                        process::exit(1);
                    }
                    BranchSearchResult::Branch(branch) => {
                        println!("🔗 Succesfully set up branch: {}", branch_name.bold());
                        println!();
                        branch.display_status(self, false)?;
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
            BranchSearchResult::NotPartOfAnyChain => {
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

    fn list_chains(&self, current_branch: &str, show_prs: bool) -> Result<(), Error> {
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
            chain.display_list(self, current_branch, show_prs)?;

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
            BranchSearchResult::NotPartOfAnyChain => {
                self.display_branch_not_part_of_chain_error(branch_name);
                process::exit(1);
            }
            BranchSearchResult::Branch(branch) => {
                branch.move_branch(self, chain_name, sort_option)?;

                match Branch::get_branch_with_chain(self, &branch.branch_name)? {
                    BranchSearchResult::NotPartOfAnyChain => {
                        eprintln!("Unable to move branch: {}", branch.branch_name.bold());
                        process::exit(1);
                    }
                    BranchSearchResult::Branch(branch) => {
                        println!("🔗 Succesfully moved branch: {}", branch.branch_name.bold());
                        println!();
                        branch.display_status(self, false)?;
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

    fn get_tree_id_from_branch_name(&self, branch_name: &str) -> Result<String, Error> {
        // tree_id = git rev-parse branch_name^{tree}
        // let output = Command::new("git")
        //     .arg("rev-parse")
        //     .arg(format!("{}^{{tree}}", branch_name))
        //     .output()
        //     .unwrap_or_else(|_| panic!("Unable to get tree id of branch {}", branch_name.bold())));

        // if output.status.success() {
        //     let raw_output = String::from_utf8(output.stdout).unwrap();
        //     let tree_id = raw_output.trim().to_string();
        //     return Ok(tree_id);
        // }

        // return Err(Error::from_str(&format!(
        //     "Unable to get tree id of branch {}",
        //     branch_name.bold()
        // )));

        match self
            .repo
            .revparse_single(&format!("{}^{{tree}}", branch_name))
        {
            Ok(tree_object) => {
                assert_eq!(tree_object.kind().unwrap(), ObjectType::Tree);
                Ok(tree_object.id().to_string())
            }
            Err(_err) => Err(Error::from_str(&format!(
                "Unable to get tree id of branch {}",
                branch_name.bold()
            ))),
        }
    }

    fn is_squashed_merged(
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

    fn rebase(&self, chain_name: &str, step_rebase: bool, ignore_root: bool) -> Result<(), Error> {
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

    fn smart_merge_base(
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

    fn merge_base(&self, ancestor_branch: &str, descendant_branch: &str) -> Result<String, Error> {
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

    fn merge_base_fork_point(
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

    fn is_ancestor(&self, ancestor_branch: &str, descendant_branch: &str) -> Result<bool, Error> {
        match self.merge_base(ancestor_branch, descendant_branch) {
            Ok(common_point) => {
                let (common_point_obj, _) = self.repo.revparse_ext(&common_point)?;
                let (ancestor_object, _reference) = self.repo.revparse_ext(ancestor_branch)?;
                Ok(common_point_obj.id() == ancestor_object.id())
            }
            Err(_) => Ok(false),
        }
    }

    fn preliminary_checks(&self, chain_name: &str) -> Result<(), Error> {
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

    fn get_previous_branch(&self, chain: &Chain, index: usize) -> String {
        if index == 0 {
            chain.root_branch.clone()
        } else {
            chain.branches[index - 1].branch_name.clone()
        }
    }

    fn calculate_basic_merge_bases(&self, chain: &Chain) -> Result<Vec<String>, Error> {
        let mut common_ancestors = vec![];

        for (index, branch) in chain.branches.iter().enumerate() {
            let prev_branch = self.get_previous_branch(chain, index);

            // Use regular merge-base without fork-point
            let common_point = self.merge_base(&prev_branch, &branch.branch_name)?;
            common_ancestors.push(common_point);
        }

        Ok(common_ancestors)
    }

    fn calculate_smart_merge_bases(&self, chain: &Chain) -> Result<Vec<String>, Error> {
        let mut common_ancestors = vec![];

        for (index, branch) in chain.branches.iter().enumerate() {
            let prev_branch = self.get_previous_branch(chain, index);

            // Use smart merge-base with potential fork-point
            let common_point = self.smart_merge_base(&prev_branch, &branch.branch_name)?;
            common_ancestors.push(common_point);
        }

        Ok(common_ancestors)
    }

    fn execute_merge(&self, upstream: &str, merge_flags: &[String]) -> Result<MergeResult, Error> {
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

    // Helper function to get merge commit information for detailed reporting
    fn get_merge_commit_info(
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

    fn report_merge_results(
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

    fn validate_chain_and_repository_state(&self, chain_name: &str) -> Result<(), Error> {
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

    fn reset_hard_to_branch(&self, branch_name: &str) -> Result<(), Error> {
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

    // Comprehensive merge with enhanced configuration
    // Provides capabilities like detailed reporting and flexible conflict handling.
    fn merge_chain_with_options(
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

    fn pr(&self, chain_name: &str, draft: bool) -> Result<(), Error> {
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
                        let pr_objects: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap_or_default();
                        if !pr_objects.is_empty() {
                            if let Some(pr_url) = pr_objects.get(0).and_then(|pr| pr.get("url")).and_then(|url| url.as_str()) {
                                println!("🔗 Open PR already exists for branch {}: {}", branch.branch_name.bold(), pr_url);
                            } else {
                                println!("🔗 Open PR already exists for branch {}", branch.branch_name.bold());
                            }
                            continue;
                        }
                    }
                    _ => {
                        eprintln!("  Failed to check existing PRs for branch {}.", branch.branch_name.bold());
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
                        eprintln!("Failed to push branch {}: {}", branch.branch_name.bold(), String::from_utf8_lossy(&unwrapped_push_output.stderr));
                        continue;
                    }
                } 

                println!("Pushed branch {}, creating PR...", branch.branch_name.bold());

                let mut gh_command = Command::new("gh");
                gh_command.arg("pr").arg("create").arg("--base").arg(base_branch).arg("--head").arg(&branch.branch_name);

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
                    println!("✅ Created PR for {} -> {}", branch.branch_name.bold(), base_branch.bold());
                    
                    // If draft mode, open the PR in browser separately
                    if draft {
                        let pr_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if let Some(pr_number) = pr_url.split('/').last() {
                            let browse_output = Command::new("gh")
                                .arg("browse")
                                .arg(pr_number)
                                .output();
                            
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

fn parse_sort_option(
    git_chain: &GitChain,
    chain_name: &str,
    before_branch: Option<&str>,
    after_branch: Option<&str>,
) -> Result<SortBranch, Error> {
    if let Some(before_branch) = before_branch {
        if !git_chain.git_local_branch_exists(before_branch)? {
            return Err(Error::from_str(&format!(
                "Branch does not exist: {}",
                before_branch.bold()
            )));
        }

        let before_branch = match Branch::get_branch_with_chain(git_chain, before_branch)? {
            BranchSearchResult::NotPartOfAnyChain => {
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
        if !git_chain.git_local_branch_exists(after_branch)? {
            return Err(Error::from_str(&format!(
                "Branch does not exist: {}",
                after_branch.bold()
            )));
        }

        let after_branch = match Branch::get_branch_with_chain(git_chain, after_branch)? {
            BranchSearchResult::NotPartOfAnyChain => {
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

            let sort_option = if sub_matches.is_present("first") {
                SortBranch::First
            } else {
                parse_sort_option(&git_chain, &chain_name, before_branch, after_branch)?
            };

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
        ("list", Some(sub_matches)) => {
            // List all chains.
            let current_branch = git_chain.get_current_branch_name()?;
            let show_prs = sub_matches.is_present("pr");
            git_chain.list_chains(&current_branch, show_prs)?;
        }
        ("move", Some(sub_matches)) => {
            // Move current branch or chain.

            let before_branch = sub_matches.value_of("before");
            let after_branch = sub_matches.value_of("after");
            let root_branch = sub_matches.value_of("root");
            let chain_name = sub_matches.value_of("chain_name");

            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain => {
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
                BranchSearchResult::NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            if Chain::chain_exists(&git_chain, &branch.chain_name)? {
                let step_rebase = sub_matches.is_present("step");
                let ignore_root = sub_matches.is_present("ignore_root");
                git_chain.rebase(&branch.chain_name, step_rebase, ignore_root)?;
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
                BranchSearchResult::NotPartOfAnyChain => {
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
                BranchSearchResult::NotPartOfAnyChain => {
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
                BranchSearchResult::NotPartOfAnyChain => {
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
                BranchSearchResult::NotPartOfAnyChain => {
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

                if !git_chain.git_local_branch_exists(branch_name)? {
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
                    BranchSearchResult::NotPartOfAnyChain => {}
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
            chain.display_list(&git_chain, &current_branch, false)?;
        }
        ("first", Some(_sub_matches)) => {
            // Switch to the first branch of the chain.

            let branch_name = git_chain.get_current_branch_name()?;

            let current_branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            if Chain::chain_exists(&git_chain, &current_branch.chain_name)? {
                let chain = Chain::get_chain(&git_chain, &current_branch.chain_name)?;
                let first_branch = chain.branches.first().unwrap();

                if current_branch.branch_name == first_branch.branch_name {
                    println!(
                        "Already on the first branch of the chain {}",
                        current_branch.chain_name.bold()
                    );
                    return Ok(());
                }

                git_chain.checkout_branch(&first_branch.branch_name)?;

                println!("Switched to branch: {}", first_branch.branch_name.bold());
            } else {
                eprintln!("Unable to find chain.");
                eprintln!("Chain does not exist: {}", current_branch.chain_name.bold());
                process::exit(1);
            }
        }
        ("last", Some(_sub_matches)) => {
            // Switch to the last branch of the chain.

            let branch_name = git_chain.get_current_branch_name()?;

            let current_branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            if Chain::chain_exists(&git_chain, &current_branch.chain_name)? {
                let chain = Chain::get_chain(&git_chain, &current_branch.chain_name)?;
                let last_branch = chain.branches.last().unwrap();

                if current_branch.branch_name == last_branch.branch_name {
                    println!(
                        "Already on the last branch of the chain {}",
                        current_branch.chain_name.bold()
                    );
                    return Ok(());
                }

                git_chain.checkout_branch(&last_branch.branch_name)?;

                println!("Switched to branch: {}", last_branch.branch_name.bold());
            } else {
                eprintln!("Unable to find chain.");
                eprintln!("Chain does not exist: {}", current_branch.chain_name.bold());
                process::exit(1);
            }
        }
        ("next", Some(_sub_matches)) => {
            // Switch to the next branch of the chain.

            let branch_name = git_chain.get_current_branch_name()?;

            let current_branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            if Chain::chain_exists(&git_chain, &current_branch.chain_name)? {
                let chain = Chain::get_chain(&git_chain, &current_branch.chain_name)?;
                let index_of_branch = chain
                    .branches
                    .iter()
                    .position(|b| b == &current_branch)
                    .unwrap();

                let index_of_next_branch = index_of_branch + 1;

                if index_of_next_branch == chain.branches.len() {
                    eprintln!("There is no next branch of the chain.");
                    process::exit(1);
                }

                let next_branch = &chain.branches[index_of_next_branch];

                if current_branch.branch_name == next_branch.branch_name {
                    println!(
                        "Already on the branch {}",
                        current_branch.branch_name.bold()
                    );
                    return Ok(());
                }

                git_chain.checkout_branch(&next_branch.branch_name)?;

                println!("Switched to branch: {}", next_branch.branch_name.bold());
            } else {
                eprintln!("Unable to find chain.");
                eprintln!("Chain does not exist: {}", current_branch.chain_name.bold());
                process::exit(1);
            }
        }
        ("prev", Some(_sub_matches)) => {
            // Switch to the previous branch of the chain.

            let branch_name = git_chain.get_current_branch_name()?;

            let current_branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            if Chain::chain_exists(&git_chain, &current_branch.chain_name)? {
                let chain = Chain::get_chain(&git_chain, &current_branch.chain_name)?;
                let index_of_branch = chain
                    .branches
                    .iter()
                    .position(|b| b == &current_branch)
                    .unwrap();

                if index_of_branch == 0 {
                    eprintln!("There is no previous branch of the chain.");
                    process::exit(1);
                }

                let index_of_prev_branch = index_of_branch - 1;
                let prev_branch = &chain.branches[index_of_prev_branch];

                if current_branch.branch_name == prev_branch.branch_name {
                    println!(
                        "Already on the branch {}",
                        current_branch.branch_name.bold()
                    );
                    return Ok(());
                }

                git_chain.checkout_branch(&prev_branch.branch_name)?;

                println!("Switched to branch: {}", prev_branch.branch_name.bold());
            } else {
                eprintln!("Unable to find chain.");
                eprintln!("Chain does not exist: {}", current_branch.chain_name.bold());
                process::exit(1);
            }
        }
        ("pr", Some(sub_matches)) => {
            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult:: NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            let draft = sub_matches.is_present("draft");
            git_chain.pr(&branch.chain_name, draft)?;
        }
        ("status", Some(sub_matches)) => {
            let show_prs = sub_matches.is_present("pr");
            git_chain.run_status(show_prs)?;
        }
        ("merge", Some(sub_matches)) => {
            // Comprehensive merge with enhanced configuration
            // Determine which chain to use
            let chain_name = match sub_matches.value_of("chain") {
                Some(name) => {
                    // User specified a chain explicitly
                    if !Chain::chain_exists(&git_chain, name)? {
                        eprintln!("Chain does not exist: {}", name.bold());
                        process::exit(1);
                    }
                    name.to_string()
                }
                None => {
                    // Use the chain of the current branch
                    let branch_name = git_chain.get_current_branch_name()?;
                    let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                        BranchSearchResult::NotPartOfAnyChain => {
                            git_chain.display_branch_not_part_of_chain_error(&branch_name);
                            process::exit(1);
                        }
                        BranchSearchResult::Branch(branch) => branch,
                    };

                    if !Chain::chain_exists(&git_chain, &branch.chain_name)? {
                        eprintln!("Unable to merge chain.");
                        eprintln!("Chain does not exist: {}", branch.chain_name.bold());
                        process::exit(1);
                    }

                    branch.chain_name
                }
            };

            // Build merge options based on command line flags
            let mut merge_flags = Vec::new();

            // Handle git merge flags
            if sub_matches.is_present("no_ff") {
                merge_flags.push("--no-ff".to_string());
            } else if sub_matches.is_present("ff_only") {
                merge_flags.push("--ff-only".to_string());
            }

            if sub_matches.is_present("squash") {
                merge_flags.push("--squash".to_string());
            }

            if let Some(strategy) = sub_matches.value_of("strategy") {
                merge_flags.push(format!("--strategy={}", strategy));
            }

            if let Some(strategy_options) = sub_matches.values_of("strategy_option") {
                for option in strategy_options {
                    merge_flags.push(format!("--strategy-option={}", option));
                }
            }

            // Determine squashed merge handling
            let squashed_merge_handling = match sub_matches.value_of("squashed_merge") {
                Some("reset") => SquashedMergeHandling::Reset,
                Some("skip") => SquashedMergeHandling::Skip,
                Some("merge") => SquashedMergeHandling::Merge,
                _ => SquashedMergeHandling::Reset, // Default
            };

            // Determine report level
            let report_level = match sub_matches.value_of("report_level") {
                Some("minimal") => ReportLevel::Minimal,
                Some("standard") => ReportLevel::Standard,
                Some("detailed") => ReportLevel::Detailed,
                _ => {
                    if sub_matches.is_present("no_report") {
                        ReportLevel::Minimal
                    } else if sub_matches.is_present("detailed_report") {
                        ReportLevel::Detailed
                    } else {
                        ReportLevel::Standard
                    }
                }
            };

            // Build the full options struct
            let options = MergeOptions {
                ignore_root: sub_matches.is_present("ignore_root"),
                merge_flags,
                use_fork_point: !sub_matches.is_present("no_fork_point"),
                squashed_merge_handling,
                verbose: sub_matches.is_present("verbose"),
                return_to_original: !sub_matches.is_present("stay"),
                simple_mode: sub_matches.is_present("simple"),
                report_level,
            };

            // Execute the merge with the configured options
            git_chain.merge_chain_with_options(&chain_name, options)?;
        }
        _ => {
            git_chain.run_status(false)?;
        }
    }

    Ok(())
}

fn parse_arg_matches<'a, I, T>(arguments: I) -> ArgMatches<'a>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let init_subcommand = SubCommand::with_name("init")
        .about("Initialize the current branch to a chain.")
        .arg(
            Arg::with_name("before")
                .short("b")
                .long("before")
                .value_name("branch_name")
                .help("Sort current branch before another branch.")
                .conflicts_with("after")
                .conflicts_with("first")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("after")
                .short("a")
                .long("after")
                .value_name("branch_name")
                .help("Sort current branch after another branch.")
                .conflicts_with("before")
                .conflicts_with("first")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("first")
                .short("f")
                .long("first")
                .help("Sort current branch as the first branch of the chain.")
                .conflicts_with("before")
                .conflicts_with("after")
                .takes_value(false),
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
        )
        .arg(
            Arg::with_name("ignore_root")
                .short("i")
                .long("ignore-root")
                .value_name("ignore_root")
                .help("Rebase each branch of the chain except for the first branch.")
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

    let pr_subcommand = SubCommand::with_name("pr")
        .about("Create a pull request for each branch in the current chain using the GitHub CLI.")
        .arg(
            Arg::with_name("draft")
                .short("d")
                .long("draft")
                .value_name("draft")
                .help("Create pull requests as drafts")
                .takes_value(false),
        );

    let status_subcommand = SubCommand::with_name("status")
        .about("Display the status of the current branch and its chain.")
        .arg(
            Arg::with_name("pr")
                .short("p")
                .long("pr")
                .help("Show open pull requests for the branch")
                .takes_value(false),
        );

    let list_subcommand = SubCommand::with_name("list")
        .about("List all chains.")
        .arg(
            Arg::with_name("pr")
                .short("p")
                .long("pr")
                .help("Show open pull requests for each branch in the chains")
                .takes_value(false),
        );

    // Merge with comprehensive options
    let merge_subcommand = SubCommand::with_name("merge")
        .about("Cascade merges through the branch chain by merging each parent branch into its child branch, preserving commit history.")
        .arg(
            Arg::with_name("ignore_root")
                .short("i")
                .long("ignore-root")
                .help("Don't merge the root branch into the first branch")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("Provides detailed output during merging process")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("simple")
                .short("s")
                .long("simple")
                .help("Use simple merge mode")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("no_report")
                .short("n")
                .long("no-report")
                .help("Suppress the merge summary report")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("detailed_report")
                .short("d")
                .long("detailed-report")
                .help("Show a more detailed merge report")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("fork_point")
                .short("f")
                .long("fork-point")
                .help("Use git merge-base --fork-point for finding common ancestors [default]")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("no_fork_point")
                .long("no-fork-point")
                .help("Don't use fork-point detection, use regular merge-base")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("stay")
                .long("stay")
                .help("Don't return to the original branch after merging")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("squashed_merge")
                .long("squashed-merge")
                .help("How to handle squashed merges [default: reset]")
                .possible_values(&["reset", "skip", "merge"])
                .default_value("reset")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("chain")
                .long("chain")
                .help("Specify a chain to merge other than the current one")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("report_level")
                .long("report-level")
                .help("Set the detail level for the merge report [default: standard]")
                .possible_values(&["minimal", "standard", "detailed"])
                .default_value("standard")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("ff")
                .long("ff")
                .help("Allow fast-forward merges [default]")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("no_ff")
                .long("no-ff")
                .help("Create a merge commit even when fast-forward is possible")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("ff_only")
                .long("ff-only")
                .help("Only allow fast-forward merges")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("squash")
                .long("squash")
                .help("Create a single commit instead of doing a merge")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("strategy")
                .long("strategy")
                .help("Use the specified merge strategy (passed directly to 'git merge' as --strategy=<STRATEGY>)")
                .long_help(
"Use the specified merge strategy. The value is passed directly to 'git merge' as '--strategy=<STRATEGY>'.
For the most up-to-date and complete information, refer to your Git version's
documentation with 'git merge --help' or 'man git-merge'.

Available strategies:

ort (default for single branch):
    The default strategy from Git 2.33.0. Performs a 3-way merge algorithm.
    Detects and handles renames. Creates a merged tree of common ancestors
    when multiple common ancestors exist.

recursive:
    Previous default strategy. Similar to 'ort' but with support for
    additional options like patience and diff-algorithm. Uses a 3-way
    merge algorithm and can detect and handle renames.

resolve:
    Only resolves two heads using a 3-way merge algorithm. Tries to
    detect criss-cross merge ambiguities but doesn't handle renames.

octopus:
    Default strategy when merging more than two branches. Refuses to do
    complex merges requiring manual resolution.

ours:
    Resolves any number of heads, but the resulting tree is always that
    of the current branch, ignoring all changes from other branches.

subtree:
    Modified 'ort' strategy. When merging trees A and B, if B corresponds
    to a subtree of A, B is adjusted to match A's tree structure.")
                .possible_values(&["ort", "recursive", "resolve", "octopus", "ours", "subtree"])
                .takes_value(true),
        )
        .arg(
            Arg::with_name("strategy_option")
                .long("strategy-option")
                .help("Pass merge strategy specific option (passed directly to 'git merge' as --strategy-option=<OPTION>)")
                .long_help(
"Pass merge strategy specific option. The value is passed directly to 'git merge' as '--strategy-option=<OPTION>'.
Can be specified multiple times for different options.
Available options depend on the selected merge strategy.

Note: These options are passed directly to 'git merge'. For the most
up-to-date and complete information, refer to your Git version's
documentation with 'git merge --help' or 'man git-merge'.

Common options for 'ort' and 'recursive' strategies:

ours:
    Forces conflicting hunks to be auto-resolved by favoring our side.
    Changes from other branches that don't conflict are preserved.
    Not to be confused with the 'ours' merge strategy.

theirs:
    Forces conflicting hunks to be auto-resolved by favoring their side.
    Opposite of 'ours' option.

ignore-space-change:
    Ignores whitespace changes when finding conflicts.

ignore-all-space:
    Ignores all whitespace when finding conflicts.

ignore-space-at-eol:
    Ignores only whitespace changes at the end of lines.

renormalize:
    Runs a virtual check-out and check-in of all three stages of a file
    when resolving a three-way merge, useful for merging branches with
    different line ending normalization rules.

find-renames[=<n>]:
    Detects renamed files. Optional value sets similarity threshold (0-100).

subtree[=<path>]:
    Instead of comparing trees at the same level, the specified path
    is prefixed to make the shape of two trees match.

Options specific to 'recursive' strategy:

patience:
    Uses the 'patience diff' algorithm for matching lines.

diff-algorithm=<algorithm>:
    Use a different diff algorithm, which can help avoid mismerges.
    Values: patience, minimal, histogram, myers

Examples:
    --strategy-option=ours
    --strategy-option=ignore-space-change
    --strategy-option=renormalize
    --strategy-option=patience
    --strategy-option=diff-algorithm=histogram
    --strategy-option=find-renames=70")
                .takes_value(true)
                .multiple(true),
        );

    let arg_matches = App::new("git-chain")
        .bin_name(executable_name())
        .version("0.0.9")
        .author("Alberto Leal <mailforalberto@gmail.com>")
        .about("Tool for rebasing a chain of local git branches.")
        .subcommand(init_subcommand)
        .subcommand(remove_subcommand)
        .subcommand(move_subcommand)
        .subcommand(rebase_subcommand)
        .subcommand(push_subcommand)
        .subcommand(prune_subcommand)
        .subcommand(setup_subcommand)
        .subcommand(rename_subcommand)
        .subcommand(pr_subcommand)
        .subcommand(status_subcommand)
        .subcommand(merge_subcommand)
        .subcommand(list_subcommand)
        .subcommand(
            SubCommand::with_name("backup").about("Back up all branches of the current chain."),
        )
        .subcommand(
            SubCommand::with_name("first").about("Switch to the first branch of the chain."),
        )
        .subcommand(SubCommand::with_name("last").about("Switch to the last branch of the chain."))
        .subcommand(SubCommand::with_name("next").about("Switch to the next branch of the chain."))
        .subcommand(
            SubCommand::with_name("prev").about("Switch to the previous branch of the chain."),
        )
        .get_matches_from(arguments);

    arg_matches
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

fn check_gh_cli_installed() -> Result<(), Error> {
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