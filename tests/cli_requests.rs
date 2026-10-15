use git_chain::cli::{try_parse_from, CommandRequest, MergeRequest, RebaseAction};
use git_chain::{
    MergeOptions, RebaseOptions, ReportLevel, SquashedMergeHandling, SquashedRebaseHandling,
};

fn merge_request(args: &[&str]) -> MergeRequest {
    let request = try_parse_from(
        ["git-chain", "merge"]
            .into_iter()
            .chain(args.iter().copied()),
    )
    .expect("merge arguments should parse without a repository");
    println!("ARGS: {args:?}\nREQUEST: {request:?}");
    // assert!(false, "DEBUG STOP: args={args:?}, request={request:?}");
    let CommandRequest::Merge(request) = request else {
        panic!("expected a merge request, got {request:?}");
    };
    request
}

fn rebase_action(args: &[&str]) -> RebaseAction {
    let request = try_parse_from(
        ["git-chain", "rebase"]
            .into_iter()
            .chain(args.iter().copied()),
    )
    .expect("rebase arguments should parse without a repository");
    println!("ARGS: {args:?}\nREQUEST: {request:?}");
    // assert!(false, "DEBUG STOP: args={args:?}, request={request:?}");
    let CommandRequest::Rebase(action) = request else {
        panic!("expected a rebase action, got {request:?}");
    };
    action
}

fn assert_conflict(args: &[&str]) {
    let error = try_parse_from(args.iter().copied()).expect_err("flags should conflict");
    println!("ARGS: {args:?}\nERROR: {error}");
    // assert!(false, "DEBUG STOP: args={args:?}, error={error:?}");
    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    assert_eq!(error.exit_code(), 2);
    assert!(error.use_stderr());
}

#[test]
fn merge_request_defaults() {
    let request = merge_request(&[]);
    assert_eq!(request.chain_name, None);
    assert_eq!(request.options, MergeOptions::default());
}

#[test]
fn merge_request_normalizes_all_options() {
    let request = merge_request(&[
        "--chain=another-chain",
        "--ignore-root",
        "--verbose",
        "--simple",
        "--stay",
        "--no-fork-point",
        "--squashed-merge=skip",
        "--no-ff",
        "--squash",
        "--strategy=recursive",
        "--strategy-option=ours",
        "--strategy-option=ignore-space-change",
        "--report-level=detailed",
    ]);
    assert_eq!(request.chain_name.as_deref(), Some("another-chain"));
    assert_eq!(
        request.options,
        MergeOptions {
            ignore_root: true,
            merge_flags: vec![
                "--no-ff".into(),
                "--squash".into(),
                "--strategy=recursive".into(),
                "--strategy-option=ours".into(),
                "--strategy-option=ignore-space-change".into(),
            ],
            use_fork_point: false,
            squashed_merge_handling: SquashedMergeHandling::Skip,
            verbose: true,
            return_to_original: false,
            simple_mode: true,
            report_level: ReportLevel::Detailed,
        }
    );
}

#[test]
fn merge_request_preserves_fast_forward_and_squash_policies() {
    let cases: &[(&[&str], &[&str], SquashedMergeHandling)] = &[
        (&["--ff"], &[], SquashedMergeHandling::Reset),
        (&["--ff-only"], &["--ff-only"], SquashedMergeHandling::Reset),
        (
            &["--ff", "--no-ff", "--ff-only", "--squashed-merge=merge"],
            &["--no-ff"],
            SquashedMergeHandling::Merge,
        ),
        (
            &["--fork-point", "--squashed-merge=skip"],
            &[],
            SquashedMergeHandling::Skip,
        ),
    ];
    for (args, flags, squash_policy) in cases {
        let options = merge_request(args).options;
        assert_eq!(options.merge_flags, *flags, "args: {args:?}");
        assert_eq!(
            options.squashed_merge_handling, *squash_policy,
            "args: {args:?}"
        );
        assert!(options.use_fork_point, "args: {args:?}");
    }
}

#[test]
fn rebase_start_options_are_typed() {
    let cases: &[(&[&str], RebaseOptions)] = &[
        (&[], RebaseOptions::default()),
        (
            &[
                "--ignore-root",
                "--no-fork-point",
                "--cleanup-backups",
                "--squashed-merge=rebase",
            ],
            RebaseOptions {
                step_rebase: false,
                ignore_root: true,
                squashed_merge_handling: SquashedRebaseHandling::Rebase,
                cleanup_backups: true,
                no_fork_point: true,
            },
        ),
        (
            &["--step", "--ignore-root", "--squashed-merge=skip"],
            RebaseOptions {
                step_rebase: true,
                ignore_root: true,
                squashed_merge_handling: SquashedRebaseHandling::Skip,
                cleanup_backups: false,
                no_fork_point: false,
            },
        ),
    ];
    for (args, expected) in cases {
        assert_eq!(
            rebase_action(args),
            RebaseAction::Start(*expected),
            "args: {args:?}"
        );
    }
}

#[test]
fn rebase_recovery_actions_keep_only_relevant_options() {
    let cases: &[(&[&str], RebaseAction)] = &[
        (&["--status"], RebaseAction::Status),
        (&["--abort"], RebaseAction::Abort),
        (&["--quit"], RebaseAction::Quit),
        (
            &["--continue"],
            RebaseAction::Continue {
                cleanup_backups: false,
            },
        ),
        (
            &["--skip"],
            RebaseAction::Skip {
                cleanup_backups: false,
            },
        ),
        (
            &["--continue", "--cleanup-backups"],
            RebaseAction::Continue {
                cleanup_backups: true,
            },
        ),
        (
            &["--skip", "--cleanup-backups"],
            RebaseAction::Skip {
                cleanup_backups: true,
            },
        ),
    ];
    for (args, expected) in cases {
        assert_eq!(&rebase_action(args), expected, "args: {args:?}");
    }
}

#[test]
fn operation_conflicts_are_rejected_before_repository_access() {
    let recovery = ["--continue", "--skip", "--abort", "--quit", "--status"];
    for (index, action) in recovery.iter().enumerate() {
        for other in recovery.iter().skip(index + 1) {
            assert_conflict(&["git-chain", "rebase", action, other]);
        }
        for start_option in [
            "--step",
            "--ignore-root",
            "--squashed-merge=reset",
            "--no-fork-point",
        ] {
            assert_conflict(&["git-chain", "rebase", action, start_option]);
        }
    }
    for action in ["--abort", "--quit", "--status", "--step"] {
        assert_conflict(&["git-chain", "rebase", action, "--cleanup-backups"]);
    }
    assert_conflict(&["git-chain", "merge", "--fork-point", "--no-fork-point"]);
    assert_conflict(&["git-chain", "merge", "--no-fork-point", "--fork-point"]);
}

#[test]
fn no_command_and_status_requests_are_equivalent() {
    let cases: &[(&[&str], bool)] = &[
        (&["git-chain"], false),
        (&["git-chain", "status"], false),
        (&["git-chain", "status", "--pr"], true),
        (&["git-chain", "status", "-p"], true),
    ];
    for (args, expected) in cases {
        let request = try_parse_from(args.iter().copied()).unwrap();
        println!("ARGS: {args:?}\nREQUEST: {request:?}");
        // assert!(false, "DEBUG STOP: args={args:?}, request={request:?}");
        let CommandRequest::Status { show_prs } = request else {
            panic!("expected a status request, got {request:?}");
        };
        assert_eq!(show_prs, *expected, "args: {args:?}");
    }
}

#[test]
fn help_version_and_invalid_values_return_clap_diagnostics() {
    use clap::error::ErrorKind;
    let cases: &[(&[&str], ErrorKind, i32, bool)] = &[
        (&["git-chain", "--help"], ErrorKind::DisplayHelp, 0, false),
        (
            &["git-chain", "merge", "--help"],
            ErrorKind::DisplayHelp,
            0,
            false,
        ),
        (
            &["git-chain", "rebase", "--help"],
            ErrorKind::DisplayHelp,
            0,
            false,
        ),
        (
            &["git-chain", "--version"],
            ErrorKind::DisplayVersion,
            0,
            false,
        ),
        (
            &["git-chain", "merge", "--squashed-merge=rebase"],
            ErrorKind::InvalidValue,
            2,
            true,
        ),
        (
            &["git-chain", "rebase", "--squashed-merge=merge"],
            ErrorKind::InvalidValue,
            2,
            true,
        ),
        (
            &["git-chain", "merge", "--report-level=unknown"],
            ErrorKind::InvalidValue,
            2,
            true,
        ),
        (
            &["git-chain", "unknown-command"],
            ErrorKind::InvalidSubcommand,
            2,
            true,
        ),
    ];
    for (args, kind, exit_code, stderr) in cases {
        let error = try_parse_from(args.iter().copied()).expect_err("expected a Clap diagnostic");
        println!("ARGS: {args:?}\nERROR: {error}");
        // assert!(false, "DEBUG STOP: args={args:?}, error={error:?}");
        assert_eq!(error.kind(), *kind, "args: {args:?}");
        assert_eq!(error.exit_code(), *exit_code, "args: {args:?}");
        assert_eq!(error.use_stderr(), *stderr, "args: {args:?}");
    }
}

#[test]
fn explicit_merge_report_level_takes_precedence_over_aliases() {
    let cases = [
        ("--report-level=minimal", ReportLevel::Minimal),
        ("--report-level=standard", ReportLevel::Standard),
        ("--report-level=detailed", ReportLevel::Detailed),
    ];
    for (flag, expected) in cases {
        for aliases in [
            &[][..],
            &["--no-report"][..],
            &["--detailed-report"][..],
            &["--no-report", "--detailed-report"][..],
        ] {
            let mut args = vec![flag];
            args.extend_from_slice(aliases);
            assert_eq!(
                merge_request(&args).options.report_level,
                expected,
                "args: {args:?}"
            );
            args.reverse();
            assert_eq!(
                merge_request(&args).options.report_level,
                expected,
                "args: {args:?}"
            );
        }
    }
}
