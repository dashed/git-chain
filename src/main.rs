use std::collections::HashSet;
use std::ffi::OsString;
use std::process;
use std::process::Command;

use clap::{App, Arg, ArgMatches, SubCommand};
use colored::*;
use git2::Error;

mod branch;
mod chain;
mod error;
mod git_chain;
mod types;

use branch::Branch;
use chain::Chain;
use git_chain::GitChain;
use types::*;

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
                BranchSearchResult::NotPartOfAnyChain => {
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

    let list_subcommand = SubCommand::with_name("list").about("List all chains.").arg(
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
        .version(env!("CARGO_PKG_VERSION"))
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
