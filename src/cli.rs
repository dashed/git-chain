use std::ffi::OsString;

use clap::{App, Arg, ArgMatches, SubCommand};

use crate::executable_name;

pub fn parse_arg_matches<'a, I, T>(arguments: I) -> ArgMatches<'a>
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
        )
        .arg(
            Arg::with_name("squashed_merge")
                .long("squashed-merge")
                .help("How to handle squashed merges [default: reset]")
                .possible_values(&["reset", "skip", "rebase"])
                .default_value("reset")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("continue_rebase")
                .long("continue")
                .help("Continue the chain rebase after resolving conflicts")
                .conflicts_with_all(&[
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "abort_rebase",
                    "skip_rebase",
                    "status_rebase",
                ])
                .takes_value(false),
        )
        .arg(
            Arg::with_name("abort_rebase")
                .long("abort")
                .help("Abort the chain rebase and restore all branches to their original state")
                .conflicts_with_all(&[
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "continue_rebase",
                    "skip_rebase",
                    "status_rebase",
                ])
                .takes_value(false),
        )
        .arg(
            Arg::with_name("skip_rebase")
                .long("skip")
                .help("Skip the current conflicted branch and continue with the rest of the chain")
                .conflicts_with_all(&[
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "continue_rebase",
                    "abort_rebase",
                    "status_rebase",
                ])
                .takes_value(false),
        )
        .arg(
            Arg::with_name("status_rebase")
                .long("status")
                .help("Show the current chain rebase state")
                .conflicts_with_all(&[
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "continue_rebase",
                    "abort_rebase",
                    "skip_rebase",
                ])
                .takes_value(false),
        )
        .arg(
            Arg::with_name("cleanup_backups")
                .long("cleanup-backups")
                .help("Delete backup branches after successful rebase")
                .conflicts_with_all(&["abort_rebase", "status_rebase"])
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
