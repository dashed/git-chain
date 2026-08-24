use std::ffi::OsString;

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::executable_name;

pub fn parse_arg_matches<I, T>(arguments: I) -> ArgMatches
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let init_subcommand = Command::new("init")
        .about("Initialize the current branch to a chain.")
        .arg(
            Arg::new("before")
                .short('b')
                .long("before")
                .value_name("branch_name")
                .help("Sort current branch before another branch.")
                .conflicts_with("after")
                .conflicts_with("first")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("after")
                .short('a')
                .long("after")
                .value_name("branch_name")
                .help("Sort current branch after another branch.")
                .conflicts_with("before")
                .conflicts_with("first")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("first")
                .short('f')
                .long("first")
                .help("Sort current branch as the first branch of the chain.")
                .conflicts_with("before")
                .conflicts_with("after")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("chain_name")
                .help("The name of the chain.")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("root_branch")
                .help("The root branch which the chain of branches will merge into.")
                .required(false)
                .index(2),
        );

    let remove_subcommand = Command::new("remove")
        .about("Remove current branch from its chain.")
        .arg(
            Arg::new("chain_name")
                .short('c')
                .long("chain")
                .value_name("chain_name")
                .help("Delete chain by removing all of its branches.")
                .action(ArgAction::Set),
        );

    let move_subcommand = Command::new("move")
        .about("Move current branch or chain.")
        .arg(
            Arg::new("before")
                .short('b')
                .long("before")
                .value_name("branch_name")
                .help("Sort current branch before another branch.")
                .conflicts_with("after")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("after")
                .short('a')
                .long("after")
                .value_name("branch_name")
                .help("Sort current branch after another branch.")
                .conflicts_with("before")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("root")
                .short('r')
                .long("root")
                .value_name("root_branch")
                .help("Set root branch of current branch and the chain it is a part of.")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("chain_name")
                .short('c')
                .long("chain")
                .value_name("chain_name")
                .help("Move current branch to another chain.")
                .conflicts_with("root")
                .action(ArgAction::Set),
        );

    let rebase_subcommand = Command::new("rebase")
        .about("Rebase all branches for the current chain.")
        .arg(
            Arg::new("step")
                .short('s')
                .long("step")
                .help("Stop at the first rebase.")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("ignore_root")
                .short('i')
                .long("ignore-root")
                .help("Rebase each branch of the chain except for the first branch.")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("squashed_merge")
                .long("squashed-merge")
                .help("How to handle squashed merges [default: reset]")
                .value_parser(["reset", "skip", "rebase"])
                .default_value("reset")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("continue_rebase")
                .long("continue")
                .help("Continue the chain rebase after resolving conflicts")
                .conflicts_with_all([
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "abort_rebase",
                    "skip_rebase",
                    "status_rebase",
                    "quit_rebase",
                ])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("abort_rebase")
                .long("abort")
                .help("Abort the chain rebase and restore all branches to their original state")
                .conflicts_with_all([
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "continue_rebase",
                    "skip_rebase",
                    "status_rebase",
                    "quit_rebase",
                ])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("skip_rebase")
                .long("skip")
                .help("Skip the current conflicted branch and continue with the rest of the chain")
                .conflicts_with_all([
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "continue_rebase",
                    "abort_rebase",
                    "status_rebase",
                    "quit_rebase",
                ])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("status_rebase")
                .long("status")
                .help("Show the current chain rebase state")
                .conflicts_with_all([
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "continue_rebase",
                    "abort_rebase",
                    "skip_rebase",
                    "quit_rebase",
                ])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("quit_rebase")
                .long("quit")
                .help("Discard the chain rebase state without touching any branch")
                .long_help(
                    "Discard the chain rebase state without touching any branch.\n\n\
                     This is the escape hatch for a state file the other recovery commands \
                     cannot read. Branches are left exactly where they are — nothing is \
                     restored, so use '--abort' instead if you want the chain rewound.",
                )
                .conflicts_with_all([
                    "step",
                    "ignore_root",
                    "squashed_merge",
                    "continue_rebase",
                    "abort_rebase",
                    "skip_rebase",
                    "status_rebase",
                ])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("cleanup_backups")
                .long("cleanup-backups")
                .help("Delete backup branches after successful rebase")
                .long_help(
                    "Delete backup branches after a successful rebase.\n\n\
                     Only the backups this rebase run created are removed. Backups made by \
                     'git chain backup' or by an earlier run are left alone.",
                )
                .conflicts_with_all(["abort_rebase", "status_rebase", "quit_rebase"])
                .action(ArgAction::SetTrue),
        );

    let push_subcommand = Command::new("push")
        .about("Push all branches of the current chain to their upstreams.")
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .help("Push branches with --force-with-lease")
                .action(ArgAction::SetTrue),
        );

    let prune_subcommand = Command::new("prune")
        .about("Prune any branches of the current chain that are ancestors of the root branch.")
        .arg(
            Arg::new("dry_run")
                .short('d')
                .long("dry-run")
                .help("Output branches that will be pruned.")
                .action(ArgAction::SetTrue),
        );

    let rename_subcommand = Command::new("rename").about("Rename current chain.").arg(
        Arg::new("chain_name")
            .help("The new name of the chain.")
            .required(true)
            .index(1),
    );

    let setup_subcommand = Command::new("setup")
        .about("Set up a chain.")
        .arg(
            Arg::new("chain_name")
                .help("The new name of the chain.")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("root_branch")
                .help("The root branch which the chain of branches will merge into.")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::new("branch")
                .help("A branch to add to the chain")
                .required(true)
                .num_args(1..)
                .index(3),
        );

    let pr_subcommand = Command::new("pr")
        .about("Create a pull request for each branch in the current chain using the GitHub CLI.")
        .arg(
            Arg::new("draft")
                .short('d')
                .long("draft")
                .help("Create pull requests as drafts")
                .action(ArgAction::SetTrue),
        );

    let status_subcommand = Command::new("status")
        .about("Display the status of the current branch and its chain.")
        .arg(
            Arg::new("pr")
                .short('p')
                .long("pr")
                .help("Show open pull requests for the branch")
                .action(ArgAction::SetTrue),
        );

    let list_subcommand = Command::new("list").about("List all chains.").arg(
        Arg::new("pr")
            .short('p')
            .long("pr")
            .help("Show open pull requests for each branch in the chains")
            .action(ArgAction::SetTrue),
    );

    // Merge with comprehensive options
    let merge_subcommand = Command::new("merge")
        .about("Cascade merges through the branch chain by merging each parent branch into its child branch, preserving commit history.")
        .arg(
            Arg::new("ignore_root")
                .short('i')
                .long("ignore-root")
                .help("Don't merge the root branch into the first branch")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Provides detailed output during merging process")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("simple")
                .short('s')
                .long("simple")
                .help("Use simple merge mode")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no_report")
                .short('n')
                .long("no-report")
                .help("Suppress the merge summary report")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("detailed_report")
                .short('d')
                .long("detailed-report")
                .help("Show a more detailed merge report")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("fork_point")
                .short('f')
                .long("fork-point")
                .help("Use git merge-base --fork-point for finding common ancestors [default]")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no_fork_point")
                .long("no-fork-point")
                .help("Don't use fork-point detection, use regular merge-base")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("stay")
                .long("stay")
                .help("Don't return to the original branch after merging")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("squashed_merge")
                .long("squashed-merge")
                .help("How to handle squashed merges [default: reset]")
                .value_parser(["reset", "skip", "merge"])
                .default_value("reset")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("chain")
                .long("chain")
                .help("Specify a chain to merge other than the current one")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("report_level")
                .long("report-level")
                .help("Set the detail level for the merge report [default: standard]")
                .value_parser(["minimal", "standard", "detailed"])
                .default_value("standard")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("ff")
                .long("ff")
                .help("Allow fast-forward merges [default]")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no_ff")
                .long("no-ff")
                .help("Create a merge commit even when fast-forward is possible")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("ff_only")
                .long("ff-only")
                .help("Only allow fast-forward merges")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("squash")
                .long("squash")
                .help("Create a single commit instead of doing a merge")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("strategy")
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
                .value_parser(["ort", "recursive", "resolve", "octopus", "ours", "subtree"])
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("strategy_option")
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
                .action(ArgAction::Append)
                .num_args(1..),
        );

    Command::new("git-chain")
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
        .subcommand(Command::new("backup").about("Back up all branches of the current chain."))
        .subcommand(Command::new("first").about("Switch to the first branch of the chain."))
        .subcommand(Command::new("last").about("Switch to the last branch of the chain."))
        .subcommand(Command::new("next").about("Switch to the next branch of the chain."))
        .subcommand(Command::new("prev").about("Switch to the previous branch of the chain."))
        .get_matches_from(arguments)
}
