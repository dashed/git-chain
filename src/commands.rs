use std::collections::HashSet;
use std::process;

use clap::ArgMatches;
use colored::*;
use git2::Error;

use crate::git_chain::GitChain;
use crate::types::*;
use crate::{Branch, Chain};

pub fn parse_sort_option(
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

pub fn run(arg_matches: ArgMatches) -> Result<(), Error> {
    let git_chain = GitChain::init()?;

    match arg_matches.subcommand() {
        Some(("init", sub_matches)) => {
            // Initialize the current branch to a chain.

            let chain_name = sub_matches
                .get_one::<String>("chain_name")
                .map(|s| s.as_str())
                .unwrap()
                .to_string();
            let root_branch = sub_matches
                .get_one::<String>("root_branch")
                .map(|s| s.as_str());

            let before_branch = sub_matches.get_one::<String>("before").map(|s| s.as_str());
            let after_branch = sub_matches.get_one::<String>("after").map(|s| s.as_str());

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

            let sort_option = if sub_matches.get_flag("first") {
                SortBranch::First
            } else {
                parse_sort_option(&git_chain, &chain_name, before_branch, after_branch)?
            };

            git_chain.init_chain(&chain_name, &root_branch, &branch_name, sort_option)?
        }
        Some(("remove", sub_matches)) => {
            // Remove current branch from its chain.

            let chain_name = sub_matches
                .get_one::<String>("chain_name")
                .map(|s| s.as_str());

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
        Some(("list", sub_matches)) => {
            // List all chains.
            let current_branch = git_chain.get_current_branch_name()?;
            let show_prs = sub_matches.get_flag("pr");
            git_chain.list_chains(&current_branch, show_prs)?;
        }
        Some(("move", sub_matches)) => {
            // Move current branch or chain.

            let before_branch = sub_matches.get_one::<String>("before").map(|s| s.as_str());
            let after_branch = sub_matches.get_one::<String>("after").map(|s| s.as_str());
            let root_branch = sub_matches.get_one::<String>("root").map(|s| s.as_str());
            let chain_name = sub_matches
                .get_one::<String>("chain_name")
                .map(|s| s.as_str());

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
        Some(("rebase", sub_matches)) => {
            let cleanup_backups = sub_matches.get_flag("cleanup_backups");

            if sub_matches.get_flag("status_rebase") {
                git_chain.rebase_status()?;
            } else if sub_matches.get_flag("continue_rebase") {
                git_chain.rebase_continue(cleanup_backups)?;
            } else if sub_matches.get_flag("skip_rebase") {
                git_chain.rebase_skip(cleanup_backups)?;
            } else if sub_matches.get_flag("abort_rebase") {
                git_chain.rebase_abort()?;
            } else {
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
                    let step_rebase = sub_matches.get_flag("step");
                    let ignore_root = sub_matches.get_flag("ignore_root");
                    let squashed_merge_handling = match sub_matches
                        .get_one::<String>("squashed_merge")
                        .map(|s| s.as_str())
                    {
                        Some("skip") => SquashedRebaseHandling::Skip,
                        Some("rebase") => SquashedRebaseHandling::Rebase,
                        _ => SquashedRebaseHandling::Reset,
                    };
                    git_chain.rebase(
                        &branch.chain_name,
                        step_rebase,
                        ignore_root,
                        squashed_merge_handling,
                        cleanup_backups,
                    )?;
                } else {
                    eprintln!("Unable to rebase chain.");
                    eprintln!("Chain does not exist: {}", branch.chain_name.bold());
                    process::exit(1);
                }
            }
        }
        Some(("backup", _sub_matches)) => {
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
        Some(("push", sub_matches)) => {
            // Push all branches of the current chain to their upstreams.

            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            let force_push = sub_matches.get_flag("force");
            git_chain.push(&branch.chain_name, force_push)?;
        }
        Some(("prune", sub_matches)) => {
            // Prune any branches of the current chain.

            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            let dry_run = sub_matches.get_flag("dry_run");

            git_chain.prune(&branch.chain_name, dry_run)?;
        }
        Some(("rename", sub_matches)) => {
            // Rename current chain.

            let new_chain_name = sub_matches
                .get_one::<String>("chain_name")
                .map(|s| s.as_str())
                .unwrap()
                .to_string();

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
        Some(("setup", sub_matches)) => {
            // Set up a chain.

            let chain_name = sub_matches
                .get_one::<String>("chain_name")
                .map(|s| s.as_str())
                .unwrap()
                .to_string();
            let root_branch = sub_matches
                .get_one::<String>("root_branch")
                .map(|s| s.as_str())
                .unwrap()
                .to_string();

            let branches: Vec<String> = sub_matches
                .get_many::<String>("branch")
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
        Some(("first", _sub_matches)) => {
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
        Some(("last", _sub_matches)) => {
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
        Some(("next", _sub_matches)) => {
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
        Some(("prev", _sub_matches)) => {
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
        Some(("pr", sub_matches)) => {
            let branch_name = git_chain.get_current_branch_name()?;

            let branch = match Branch::get_branch_with_chain(&git_chain, &branch_name)? {
                BranchSearchResult::NotPartOfAnyChain => {
                    git_chain.display_branch_not_part_of_chain_error(&branch_name);
                    process::exit(1);
                }
                BranchSearchResult::Branch(branch) => branch,
            };

            let draft = sub_matches.get_flag("draft");
            git_chain.pr(&branch.chain_name, draft)?;
        }
        Some(("status", sub_matches)) => {
            let show_prs = sub_matches.get_flag("pr");
            git_chain.run_status(show_prs)?;
        }
        Some(("merge", sub_matches)) => {
            // Comprehensive merge with enhanced configuration
            // Determine which chain to use
            let chain_name = match sub_matches.get_one::<String>("chain").map(|s| s.as_str()) {
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
            if sub_matches.get_flag("no_ff") {
                merge_flags.push("--no-ff".to_string());
            } else if sub_matches.get_flag("ff_only") {
                merge_flags.push("--ff-only".to_string());
            }

            if sub_matches.get_flag("squash") {
                merge_flags.push("--squash".to_string());
            }

            if let Some(strategy) = sub_matches
                .get_one::<String>("strategy")
                .map(|s| s.as_str())
            {
                merge_flags.push(format!("--strategy={}", strategy));
            }

            if let Some(strategy_options) = sub_matches.get_many::<String>("strategy_option") {
                for option in strategy_options {
                    merge_flags.push(format!("--strategy-option={}", option));
                }
            }

            // Determine squashed merge handling
            let squashed_merge_handling = match sub_matches
                .get_one::<String>("squashed_merge")
                .map(|s| s.as_str())
            {
                Some("reset") => SquashedMergeHandling::Reset,
                Some("skip") => SquashedMergeHandling::Skip,
                Some("merge") => SquashedMergeHandling::Merge,
                _ => SquashedMergeHandling::Reset, // Default
            };

            // Determine report level
            let report_level = match sub_matches
                .get_one::<String>("report_level")
                .map(|s| s.as_str())
            {
                Some("minimal") => ReportLevel::Minimal,
                Some("standard") => ReportLevel::Standard,
                Some("detailed") => ReportLevel::Detailed,
                _ => {
                    if sub_matches.get_flag("no_report") {
                        ReportLevel::Minimal
                    } else if sub_matches.get_flag("detailed_report") {
                        ReportLevel::Detailed
                    } else {
                        ReportLevel::Standard
                    }
                }
            };

            // Build the full options struct
            let options = MergeOptions {
                ignore_root: sub_matches.get_flag("ignore_root"),
                merge_flags,
                use_fork_point: !sub_matches.get_flag("no_fork_point"),
                squashed_merge_handling,
                verbose: sub_matches.get_flag("verbose"),
                return_to_original: !sub_matches.get_flag("stay"),
                simple_mode: sub_matches.get_flag("simple"),
                report_level,
            };

            // Execute the merge with the configured options
            git_chain.merge_chain_with_options(&chain_name, options)?;
        }
        // No subcommand: display the status of the current branch and its chain.
        // clap rejects unknown subcommands before this point, so the only value
        // reaching this arm is None.
        _ => {
            git_chain.run_status(false)?;
        }
    }

    Ok(())
}
