use std::process;

use colored::*;
use git2::{BranchType, Config, ConfigLevel, Error, ErrorClass, ErrorCode, ObjectType, Repository};
use regex::Regex;

use super::GitChain;
use crate::types::*;
use crate::{executable_name, Branch, Chain};

impl GitChain {
    pub fn init() -> Result<Self, Error> {
        let name_of_current_executable = executable_name();

        let repo = match Repository::discover(".") {
            Ok(repo) => repo,
            Err(ref e)
                if e.class() == ErrorClass::Repository && e.code() == ErrorCode::NotFound =>
            {
                eprintln!(
                    "{} Not a git repository (or any of the parent directories)",
                    "error:".red().bold()
                );
                eprintln!(
                    "\n{} This command must be run inside a git repository.",
                    "hint:".yellow().bold()
                );
                eprintln!(
                    "{} Run {} to create a new git repository.",
                    "hint:".yellow().bold(),
                    "git init".bold()
                );
                process::exit(1);
            }
            Err(e) => return Err(e),
        };

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

    pub fn get_current_branch_name(&self) -> Result<String, Error> {
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

    pub fn get_local_git_config(&self) -> Result<Config, Error> {
        self.repo.config()?.open_level(ConfigLevel::Local)
    }

    pub fn get_git_config(&self, key: &str) -> Result<Option<String>, Error> {
        let local_config = self.get_local_git_config()?;
        match local_config.get_string(key) {
            Ok(value) => Ok(Some(value)),
            Err(ref e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn get_git_configs_matching_key(
        &self,
        regexp: &Regex,
    ) -> Result<Vec<(String, String)>, Error> {
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

    pub fn set_git_config(&self, key: &str, value: &str) -> Result<(), Error> {
        let mut local_config = self.get_local_git_config()?;
        local_config.set_str(key, value)?;
        Ok(())
    }

    pub fn delete_git_config(&self, key: &str) -> Result<(), Error> {
        let mut local_config = self.get_local_git_config()?;
        match local_config.remove(key) {
            Ok(()) => Ok(()),
            Err(ref e) if e.code() == ErrorCode::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn checkout_branch(&self, branch_name: &str) -> Result<(), Error> {
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

    pub fn git_branch_exists(&self, branch_name: &str) -> Result<bool, Error> {
        Ok(self.git_local_branch_exists(branch_name)?
            || self.git_remote_branch_exists(branch_name)?)
    }

    pub fn git_local_branch_exists(&self, branch_name: &str) -> Result<bool, Error> {
        match self.repo.find_branch(branch_name, BranchType::Local) {
            Ok(_branch) => Ok(true),
            Err(ref e) if e.code() == ErrorCode::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn git_remote_branch_exists(&self, branch_name: &str) -> Result<bool, Error> {
        match self.repo.find_branch(branch_name, BranchType::Remote) {
            Ok(_branch) => Ok(true),
            Err(ref e) if e.code() == ErrorCode::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn display_branch_not_part_of_chain_error(&self, branch_name: &str) {
        eprintln!("❌ Branch is not part of any chain: {}", branch_name.bold());
        eprintln!(
            "To initialize a chain for this branch, run {} init <chain_name> <root_branch>",
            self.executable_name
        );
    }

    pub fn run_status(&self, show_prs: bool) -> Result<(), Error> {
        let branch_name = self.get_current_branch_name()?;
        println!("On branch: {}", branch_name.bold());
        println!();

        let results = Branch::get_branch_with_chain(self, &branch_name)?;

        match results {
            BranchSearchResult::NotPartOfAnyChain => {
                return Err(Error::from_str(&format!(
                    "Branch is not part of any chain: {}\nTo initialize a chain for this branch, run {} init <chain_name> <root_branch>",
                    &branch_name, self.executable_name
                )));
            }
            BranchSearchResult::Branch(branch) => {
                branch.display_status(self, show_prs)?;
            }
        }

        Ok(())
    }

    pub fn init_chain(
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
                        return Err(Error::from_str(&format!(
                            "Unable to set up chain for branch: {}",
                            branch_name
                        )));
                    }
                    BranchSearchResult::Branch(branch) => {
                        println!("🔗 Succesfully set up branch: {}", branch_name.bold());
                        println!();
                        branch.display_status(self, false)?;
                    }
                };
            }
            BranchSearchResult::Branch(branch) => {
                return Err(Error::from_str(&format!(
                    "Unable to initialize branch to a chain.\nBranch already part of a chain: {}\nIt is part of the chain: {}\nWith root branch: {}",
                    branch_name, branch.chain_name, branch.root_branch
                )));
            }
        };

        Ok(())
    }

    pub fn remove_branch_from_chain(&self, branch_name: String) -> Result<(), Error> {
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

    pub fn list_chains(&self, current_branch: &str, show_prs: bool) -> Result<(), Error> {
        let list = Chain::get_all_chains(self)?;

        if list.is_empty() {
            println!("No chains to list.");
            println!(
                "To initialize a chain for this branch, run {} init <chain_name> <root_branch>",
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

    pub fn move_branch(
        &self,
        chain_name: &str,
        branch_name: &str,
        sort_option: &SortBranch,
    ) -> Result<(), Error> {
        match Branch::get_branch_with_chain(self, branch_name)? {
            BranchSearchResult::NotPartOfAnyChain => {
                return Err(Error::from_str(&format!(
                    "Branch is not part of any chain: {}\nTo initialize a chain for this branch, run {} init <chain_name> <root_branch>",
                    branch_name, self.executable_name
                )));
            }
            BranchSearchResult::Branch(branch) => {
                branch.move_branch(self, chain_name, sort_option)?;

                match Branch::get_branch_with_chain(self, &branch.branch_name)? {
                    BranchSearchResult::NotPartOfAnyChain => {
                        return Err(Error::from_str(&format!(
                            "Unable to move branch: {}",
                            branch.branch_name
                        )));
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

    pub fn get_commit_hash_of_head(&self) -> Result<String, Error> {
        let head = self.repo.head()?;
        let oid = head.target().unwrap();
        let commit = self.repo.find_commit(oid).unwrap();
        Ok(commit.id().to_string())
    }

    pub fn get_tree_id_from_branch_name(&self, branch_name: &str) -> Result<String, Error> {
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

    pub fn dirty_working_directory(&self) -> Result<bool, Error> {
        // perform equivalent to git diff-index HEAD
        let obj = self.repo.revparse_single("HEAD")?;
        let tree = obj.peel(ObjectType::Tree)?;

        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(tree.as_tree(), None)?;

        let diff_stats = diff.stats()?;
        let has_changes = diff_stats.files_changed() > 0
            || diff_stats.insertions() > 0
            || diff_stats.deletions() > 0;

        Ok(has_changes)
    }
}
