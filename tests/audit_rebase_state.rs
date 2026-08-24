//! CHARACTERIZATION TESTS FOR KNOWN REBASE STATE-MACHINE DEFECTS
//!
//! ⚠️  THESE TESTS ASSERT BUGGY BEHAVIOR ON PURPOSE. ⚠️
//!
//! Every test in this file documents a defect recorded in `REBASE_AUDIT.md`
//! (repository root), section 3 "State machine findings (failure recovery)".
//! They are written to PASS against the current, defective implementation —
//! passing here means "the bug is still present and reproducible", not "the
//! code is correct".
//!
//! WHEN A FIX LANDS, THE CORRESPONDING TEST MUST BE INVERTED, not deleted.
//! Each test carries an `AFTER THE FIX` block spelling out exactly which
//! assertions have to flip so the test becomes a regression guard for the
//! fixed behavior.
//!
//! Coverage:
//!
//! | Test                                                  | Audit finding |
//! |-------------------------------------------------------|---------------|
//! | `audit_c1_rebase_failure_deletes_state_no_recovery`     | C1 · CRITICAL |
//! | `audit_h2_abort_discards_work_on_untouched_branch`      | H2 · HIGH     |
//! | `audit_h3_corrupt_state_wedges_all_recovery_commands`   | H3 · HIGH     |
//! | `audit_h1_cleanup_backups_deletes_user_created_backups` | H1 · HIGH     |

#[path = "common/mod.rs"]
pub mod common;

use std::path::Path;

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, get_current_branch_name, run_git_command, run_test_bin,
    run_test_bin_expect_err, run_test_bin_expect_ok, run_test_bin_for_rebase, setup_git_repo,
    teardown_git_repo,
};

/// Resolve a revision to a full OID, or `None` when git cannot resolve it.
///
/// Used to prove both that a branch moved and that a ref stopped existing.
fn rev_parse(path_to_repo: &Path, revision: &str) -> Option<String> {
    let output = run_git_command(path_to_repo, vec!["rev-parse", "--verify", revision]);

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// True when `<revision>` names an object that exists (e.g. `branch:path/to/file`).
fn object_exists(path_to_repo: &Path, revision: &str) -> bool {
    run_git_command(path_to_repo, vec!["cat-file", "-e", revision])
        .status
        .success()
}

/// Combined stdout + stderr with ANSI styling removed, for substring assertions.
fn combined_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    console::strip_ansi_codes(&format!("{}{}", stdout, stderr)).to_string()
}

/// Install an executable `pre-rebase` hook that refuses to rebase `branch_name`.
///
/// git runs `pre-rebase` with `$1` = upstream and `$2` = the branch being
/// rebased. A non-zero exit makes `git rebase` fail *without* leaving the
/// repository in a rebase state — exactly the "clean failure" shape that
/// drives git-chain down the `Failed` path in `operations.rs:278-279`.
#[cfg(unix)]
fn install_pre_rebase_hook_refusing(path_to_repo: &Path, branch_name: &str) {
    use std::os::unix::fs::PermissionsExt;

    let hooks_dir = path_to_repo.join(".git/hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();

    let hook_path = hooks_dir.join("pre-rebase");
    let script = format!(
        "#!/bin/sh\nif [ \"$2\" = \"{}\" ]; then\n  echo \"pre-rebase hook: refusing {}\" >&2\n  exit 1\nfi\nexit 0\n",
        branch_name, branch_name
    );
    std::fs::write(&hook_path, script).unwrap();
    std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

// ---------------------------------------------------------------------------
// C1 · CRITICAL — the failure path DELETES the state file
// ---------------------------------------------------------------------------

/// Documents audit finding **C1**: when a `git rebase` subprocess fails while
/// leaving the repository clean, `rebase()` marks the branch `Failed` and then
/// calls `delete_state()` (`src/git_chain/operations.rs:278-279`) — *after*
/// earlier branches have already been rewritten.
///
/// `original_refs` lives only in that file, and no backup branches are taken on
/// the normal (non-squash) path, so deleting it destroys the sole record of the
/// pre-rebase positions. The chain is left half-rebased with no recovery path.
///
/// AFTER THE FIX (keep state on `Failed`, per REBASE_AUDIT.md §4):
///   - `state_file.exists()`         must flip to `true`
///   - `--abort` must SUCCEED, print "Restoring branches", and move
///     `some_branch_1` back to `branch_1_original`
///   - `--status` must report the chain with `some_branch_2` marked Failed
///     instead of "No chain rebase in progress."
#[cfg(unix)]
#[test]
fn audit_c1_rebase_failure_deletes_state_no_recovery() {
    let repo_name = "audit_c1_rebase_failure_deletes_state";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");
        first_commit_all(&repo, "first commit");
    };

    {
        let branch_name = "some_branch_1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "commit on some_branch_1");
    };

    {
        let branch_name = "some_branch_2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "file_2.txt", "contents 2");
        commit_all(&repo, "commit on some_branch_2");
    };

    let args: Vec<&str> = vec![
        "setup",
        "chain_name",
        "master",
        "some_branch_1",
        "some_branch_2",
    ];
    run_test_bin_expect_ok(&path_to_repo, args);

    // Advance master so that rebasing some_branch_1 actually rewrites it.
    {
        checkout_branch(&repo, "master");
        create_new_file(&path_to_repo, "master_extra.txt", "master moved on");
        commit_all(&repo, "commit on master");
    };

    checkout_branch(&repo, "some_branch_1");
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

    // The second branch of the chain will fail cleanly (repository state stays
    // Clean), which is the precondition for the defective `Failed` path.
    install_pre_rebase_hook_refusing(&path_to_repo, "some_branch_2");

    let branch_1_original = rev_parse(&path_to_repo, "some_branch_1").unwrap();
    let branch_2_original = rev_parse(&path_to_repo, "some_branch_2").unwrap();

    let state_file = path_to_repo.join(".git/chain-rebase-state.json");

    // git chain rebase — expected to fail on some_branch_2
    let args: Vec<&str> = vec!["rebase"];
    let rebase_output = run_test_bin(&path_to_repo, args);
    let rebase_text = combined_output(&rebase_output);

    let branch_1_after = rev_parse(&path_to_repo, "some_branch_1").unwrap();
    let branch_2_after = rev_parse(&path_to_repo, "some_branch_2").unwrap();

    // Diagnostics
    println!("=== C1 DIAGNOSTICS: the failing rebase ===");
    println!("REBASE EXIT SUCCESS: {}", rebase_output.status.success());
    println!("REBASE OUTPUT: {}", rebase_text);
    println!(
        "some_branch_1: original={} after={} (changed: {})",
        branch_1_original,
        branch_1_after,
        branch_1_original != branch_1_after
    );
    println!(
        "some_branch_2: original={} after={} (changed: {})",
        branch_2_original,
        branch_2_after,
        branch_2_original != branch_2_after
    );
    println!("STATE FILE EXISTS AFTER FAILURE: {}", state_file.exists());
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: C1 after failing rebase");
    // assert!(false, "rebase output: {}", rebase_text);
    // assert!(false, "status code: {}", rebase_output.status.code().unwrap_or(0));
    // assert!(false, "state file exists: {}", state_file.exists());

    assert!(
        !rebase_output.status.success(),
        "git chain rebase should fail when the pre-rebase hook refuses some_branch_2, \
         but it exited successfully. Output: {}",
        rebase_text
    );
    assert!(
        rebase_text.contains("pre-rebase hook: refusing some_branch_2"),
        "output should surface the hook refusal, got: {}",
        rebase_text
    );

    // The chain is left HALF-rebased: branch 1 rewritten, branch 2 untouched.
    assert_ne!(
        branch_1_original, branch_1_after,
        "some_branch_1 should have been rewritten before the failure, but it is \
         still at {}",
        branch_1_original
    );
    assert_eq!(
        branch_2_original, branch_2_after,
        "some_branch_2 should be untouched because its rebase was refused, but it \
         moved from {} to {}",
        branch_2_original, branch_2_after
    );

    // DEFECT: the only record of the pre-rebase positions is now gone.
    assert!(
        !state_file.exists(),
        "DEFECT C1 no longer reproduces: the state file survived the failure at {}. \
         If the fix landed, invert this assertion (see AFTER THE FIX above).",
        state_file.display()
    );

    // Consequence 1: --abort cannot restore anything.
    let args: Vec<&str> = vec!["rebase", "--abort"];
    let abort_output = run_test_bin_expect_err(&path_to_repo, args);
    let abort_text = combined_output(&abort_output);

    println!("=== C1 DIAGNOSTICS: recovery attempts ===");
    println!("ABORT EXIT SUCCESS: {}", abort_output.status.success());
    println!("ABORT OUTPUT: {}", abort_text);

    assert!(
        !abort_output.status.success(),
        "rebase --abort should fail because the state file was deleted, got output: {}",
        abort_text
    );
    assert!(
        abort_text.contains("No chain rebase in progress"),
        "rebase --abort should report that there is nothing to abort, got: {}",
        abort_text
    );

    // Consequence 2: --status offers no recovery affordance either. Note that
    // `rebase --status` exits 0 by design when no state exists
    // (`operations.rs:1040-1043`); the defect is the *content* of the report,
    // not the exit code.
    let args: Vec<&str> = vec!["rebase", "--status"];
    let status_output = run_test_bin_expect_ok(&path_to_repo, args);
    let status_text = combined_output(&status_output);

    println!("STATUS EXIT SUCCESS: {}", status_output.status.success());
    println!("STATUS OUTPUT: {}", status_text);
    println!("======");

    assert!(
        status_output.status.success(),
        "rebase --status exits 0 when no state file exists, got output: {}",
        status_text
    );
    assert!(
        status_text.contains("No chain rebase in progress"),
        "rebase --status should claim no rebase is in progress even though the chain \
         is half-rebased, got: {}",
        status_text
    );
    assert!(
        !status_text.contains("some_branch_1"),
        "rebase --status should not mention the half-rebased chain at all, got: {}",
        status_text
    );

    // The pre-rebase original of some_branch_1 is now recorded nowhere that
    // git-chain can reach: no state file, and no backup branch was ever made.
    let backup_ref = rev_parse(&path_to_repo, "backup-chain_name/some_branch_1");
    println!("BACKUP REF FOR some_branch_1: {:?}", backup_ref);
    assert!(
        backup_ref.is_none(),
        "the normal rebase path takes no backups, so backup-chain_name/some_branch_1 \
         should not exist, but it resolved to {:?}",
        backup_ref
    );

    teardown_git_repo(repo_name);
}

// ---------------------------------------------------------------------------
// H2 · HIGH — `--abort` is an unverified clobber that always reports success
// ---------------------------------------------------------------------------

/// Documents audit finding **H2**: `rebase_abort` replays every entry of
/// `original_refs` through `git update-ref` (`operations.rs:1276-1300`) with no
/// old-value argument, no backup, and no check of what the branch points at
/// now. Per-branch failures only `eprintln!`, and step 7 unconditionally prints
/// "All branches restored to their original state."
///
/// Here the chain rebase stops at a conflict on `some_branch_1` and never
/// reaches `some_branch_3`. The user aborts the git-level rebase, commits real
/// work on `some_branch_3`, and then aborts the chain rebase — which silently
/// throws that commit away while reporting success.
///
/// AFTER THE FIX (compare-and-swap or refuse/warn on externally moved branches):
///   - `branch_3_after_abort` must equal `branch_3_with_new_work`, not
///     `branch_3_original`
///   - `object_exists(... "some_branch_3:untouched_work.txt")` must flip to `true`
///   - the unconditional "All branches restored" claim must become conditional
#[test]
fn audit_h2_abort_discards_work_on_untouched_branch() {
    let repo_name = "audit_h2_abort_discards_untouched_work";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");
        first_commit_all(&repo, "first commit");
    };

    {
        let branch_name = "some_branch_1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "shared.txt", "branch 1 version");
        commit_all(&repo, "commit on some_branch_1");
    };

    {
        let branch_name = "some_branch_2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "file_2.txt", "contents 2");
        commit_all(&repo, "commit on some_branch_2");
    };

    {
        let branch_name = "some_branch_3";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "file_3.txt", "contents 3");
        commit_all(&repo, "commit on some_branch_3");
    };

    let args: Vec<&str> = vec![
        "setup",
        "chain_name",
        "master",
        "some_branch_1",
        "some_branch_2",
        "some_branch_3",
    ];
    run_test_bin_expect_ok(&path_to_repo, args);

    // master edits the same file as some_branch_1 -> conflict on the first branch.
    {
        checkout_branch(&repo, "master");
        create_new_file(&path_to_repo, "shared.txt", "master version");
        commit_all(&repo, "conflicting commit on master");
    };

    checkout_branch(&repo, "some_branch_1");
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

    let branch_3_original = rev_parse(&path_to_repo, "some_branch_3").unwrap();

    // git chain rebase -> conflict on some_branch_1, state file kept.
    let args: Vec<&str> = vec!["rebase"];
    let rebase_output = run_test_bin_expect_err(&path_to_repo, args);
    let rebase_text = combined_output(&rebase_output);

    let state_file = path_to_repo.join(".git/chain-rebase-state.json");

    println!("=== H2 DIAGNOSTICS: conflict reached ===");
    println!("REBASE EXIT SUCCESS: {}", rebase_output.status.success());
    println!("REBASE OUTPUT: {}", rebase_text);
    println!("STATE FILE EXISTS: {}", state_file.exists());
    println!("======");

    assert!(
        !rebase_output.status.success(),
        "git chain rebase should stop with a conflict, got output: {}",
        rebase_text
    );
    assert!(
        rebase_text.contains("Unable to completely rebase some_branch_1"),
        "output should report the conflict on some_branch_1, got: {}",
        rebase_text
    );
    assert!(
        state_file.exists(),
        "the state file must survive a conflict so that --abort can restore refs"
    );

    // The user steps away from the conflict at the git level ...
    let git_abort = run_git_command(&path_to_repo, vec!["rebase", "--abort"]);
    println!("git rebase --abort SUCCESS: {}", git_abort.status.success());
    assert!(
        git_abort.status.success(),
        "git rebase --abort should succeed, got: {}",
        combined_output(&git_abort)
    );

    // ... and commits genuine work on a branch the chain rebase never reached.
    let git_checkout = run_git_command(&path_to_repo, vec!["checkout", "some_branch_3"]);
    assert!(
        git_checkout.status.success(),
        "checking out some_branch_3 should succeed, got: {}",
        combined_output(&git_checkout)
    );

    create_new_file(
        &path_to_repo,
        "untouched_work.txt",
        "important unpushed work",
    );
    commit_all(&repo, "IMPORTANT unpushed work on some_branch_3");

    let branch_3_with_new_work = rev_parse(&path_to_repo, "some_branch_3").unwrap();
    let work_present_before_abort =
        object_exists(&path_to_repo, "some_branch_3:untouched_work.txt");

    println!("=== H2 DIAGNOSTICS: new work committed on some_branch_3 ===");
    println!("some_branch_3 original:      {}", branch_3_original);
    println!("some_branch_3 with new work: {}", branch_3_with_new_work);
    println!(
        "untouched_work.txt reachable from some_branch_3 BEFORE abort: {}",
        work_present_before_abort
    );
    println!("======");

    assert_ne!(
        branch_3_original, branch_3_with_new_work,
        "the new commit should have moved some_branch_3 off {}",
        branch_3_original
    );
    assert!(
        work_present_before_abort,
        "untouched_work.txt should be reachable from some_branch_3 before the abort"
    );

    // git chain rebase --abort
    let args: Vec<&str> = vec!["rebase", "--abort"];
    let abort_output = run_test_bin(&path_to_repo, args);
    let abort_text = combined_output(&abort_output);

    let branch_3_after_abort = rev_parse(&path_to_repo, "some_branch_3").unwrap();
    let work_present_after_abort = object_exists(&path_to_repo, "some_branch_3:untouched_work.txt");

    println!("=== H2 DIAGNOSTICS: after git chain rebase --abort ===");
    println!("ABORT EXIT SUCCESS: {}", abort_output.status.success());
    println!("ABORT OUTPUT: {}", abort_text);
    println!("some_branch_3 after abort: {}", branch_3_after_abort);
    println!(
        "reverted to pre-rebase original: {}",
        branch_3_after_abort == branch_3_original
    );
    println!(
        "untouched_work.txt reachable from some_branch_3 AFTER abort: {}",
        work_present_after_abort
    );
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: H2 after chain abort");
    // assert!(false, "abort output: {}", abort_text);
    // assert!(false, "branch_3 original={} new_work={} after={}",
    //         branch_3_original, branch_3_with_new_work, branch_3_after_abort);

    assert!(
        abort_output.status.success(),
        "git chain rebase --abort should exit successfully, got: {}",
        abort_text
    );

    // DEFECT: success is claimed unconditionally.
    assert!(
        abort_text.contains("All branches restored"),
        "abort should claim every branch was restored, got: {}",
        abort_text
    );

    // DEFECT: a branch the rebase never touched was clobbered.
    assert_eq!(
        branch_3_after_abort, branch_3_original,
        "DEFECT H2 no longer reproduces: some_branch_3 was expected to be clobbered \
         back to its pre-rebase position {}, but it is at {}. If the fix landed, \
         invert this assertion (see AFTER THE FIX above).",
        branch_3_original, branch_3_after_abort
    );
    assert_ne!(
        branch_3_after_abort, branch_3_with_new_work,
        "DEFECT H2 no longer reproduces: the commit made during the pause survived \
         at {}. If the fix landed, invert this assertion.",
        branch_3_with_new_work
    );
    assert!(
        !work_present_after_abort,
        "DEFECT H2 no longer reproduces: untouched_work.txt is still reachable from \
         some_branch_3 after the abort. If the fix landed, invert this assertion."
    );

    // The state file is gone, so the discarded commit cannot be recovered
    // through git-chain — only through git's reflog.
    println!("STATE FILE EXISTS AFTER ABORT: {}", state_file.exists());
    assert!(
        !state_file.exists(),
        "the state file should be deleted by a successful abort"
    );

    teardown_git_repo(repo_name);
}

// ---------------------------------------------------------------------------
// H3 · HIGH — a corrupt state file wedges every command; no `--quit`
// ---------------------------------------------------------------------------

/// Documents audit finding **H3**: `read_state` (`src/rebase_state.rs:19-30`)
/// hard-fails on any schema mismatch, and all four recovery entry points call
/// it. Meanwhile the guard in `rebase()` (`operations.rs:26-37`) only checks
/// `state_exists` and swallows the read error, so a bare `rebase` is refused
/// with advice pointing at the very commands that cannot run.
///
/// The result is a repository that cannot be recovered through git-chain at
/// all. git's equivalent escape hatch is `git rebase --quit`
/// (`builtin/rebase.c:1177-1178`), which git-chain has no counterpart for.
///
/// The state file written here is well-formed JSON that is missing exactly one
/// required field (`merge_bases`), modelling a partial write or a schema change
/// between versions.
///
/// AFTER THE FIX (add `rebase --quit`, or make recovery tolerate unreadable state):
///   - at least one of `--status` / `--abort` / `--quit` must SUCCEED and clear
///     the state file
///   - the parse error must name the state file path, so
///     `mentions_state_file_path` flips to `true`
#[test]
fn audit_h3_corrupt_state_wedges_all_recovery_commands() {
    let repo_name = "audit_h3_corrupt_state_wedges_repo";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");
        first_commit_all(&repo, "first commit");
    };

    {
        let branch_name = "some_branch_1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "commit on some_branch_1");
    };

    {
        let branch_name = "some_branch_2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "file_2.txt", "contents 2");
        commit_all(&repo, "commit on some_branch_2");
    };

    let args: Vec<&str> = vec![
        "setup",
        "chain_name",
        "master",
        "some_branch_1",
        "some_branch_2",
    ];
    run_test_bin_expect_ok(&path_to_repo, args);

    checkout_branch(&repo, "some_branch_1");
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

    // Write a state file that is valid JSON but missing the required
    // `merge_bases` field (see `ChainRebaseState` in src/types.rs).
    let state_file = path_to_repo.join(".git/chain-rebase-state.json");
    let corrupt_state = r#"{
  "version": 1,
  "chain_name": "chain_name",
  "original_branch": "some_branch_1",
  "root_branch": "master",
  "options": {
    "step_rebase": false,
    "ignore_root": false,
    "squashed_merge_handling": "reset"
  },
  "original_refs": {},
  "branches": [],
  "current_index": 0,
  "completed_count": 0,
  "total_count": 0
}
"#;
    std::fs::write(&state_file, corrupt_state).unwrap();

    assert!(
        state_file.exists(),
        "the corrupt state file should have been written to {}",
        state_file.display()
    );

    // Every recovery entry point reads the state and therefore dies.
    let status_output = run_test_bin(&path_to_repo, vec!["rebase", "--status"]);
    let status_text = combined_output(&status_output);

    let continue_output = run_test_bin(&path_to_repo, vec!["rebase", "--continue"]);
    let continue_text = combined_output(&continue_output);

    let skip_output = run_test_bin(&path_to_repo, vec!["rebase", "--skip"]);
    let skip_text = combined_output(&skip_output);

    let abort_output = run_test_bin(&path_to_repo, vec!["rebase", "--abort"]);
    let abort_text = combined_output(&abort_output);

    let plain_output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let plain_text = combined_output(&plain_output);

    let parse_error = "Failed to parse chain rebase state file";
    let mentions_state_file_path = status_text.contains("chain-rebase-state.json")
        || continue_text.contains("chain-rebase-state.json")
        || skip_text.contains("chain-rebase-state.json")
        || abort_text.contains("chain-rebase-state.json")
        || plain_text.contains("chain-rebase-state.json");

    println!("=== H3 DIAGNOSTICS: every entry point against a corrupt state ===");
    println!(
        "--status  SUCCESS: {} | {}",
        status_output.status.success(),
        status_text.trim()
    );
    println!(
        "--continue SUCCESS: {} | {}",
        continue_output.status.success(),
        continue_text.trim()
    );
    println!(
        "--skip    SUCCESS: {} | {}",
        skip_output.status.success(),
        skip_text.trim()
    );
    println!(
        "--abort   SUCCESS: {} | {}",
        abort_output.status.success(),
        abort_text.trim()
    );
    println!(
        "rebase    SUCCESS: {} | {}",
        plain_output.status.success(),
        plain_text.trim()
    );
    println!(
        "any output names the state file path: {}",
        mentions_state_file_path
    );
    println!(
        "bare rebase names the affected chain: {}",
        plain_text.contains("chain_name")
    );
    println!("state file still present: {}", state_file.exists());
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: H3 corrupt state");
    // assert!(false, "status: {}", status_text);
    // assert!(false, "continue: {}", continue_text);
    // assert!(false, "abort: {}", abort_text);
    // assert!(false, "plain rebase: {}", plain_text);

    assert!(
        !status_output.status.success(),
        "rebase --status should fail on a corrupt state file, got: {}",
        status_text
    );
    assert!(
        status_text.contains(parse_error),
        "rebase --status should report a parse failure, got: {}",
        status_text
    );
    assert!(
        status_text.contains("merge_bases"),
        "rebase --status should name the missing field, got: {}",
        status_text
    );

    assert!(
        !continue_output.status.success(),
        "rebase --continue should fail on a corrupt state file, got: {}",
        continue_text
    );
    assert!(
        continue_text.contains(parse_error),
        "rebase --continue should report a parse failure, got: {}",
        continue_text
    );

    assert!(
        !skip_output.status.success(),
        "rebase --skip should fail on a corrupt state file, got: {}",
        skip_text
    );
    assert!(
        skip_text.contains(parse_error),
        "rebase --skip should report a parse failure, got: {}",
        skip_text
    );

    assert!(
        !abort_output.status.success(),
        "rebase --abort should fail on a corrupt state file, got: {}",
        abort_text
    );
    assert!(
        abort_text.contains(parse_error),
        "rebase --abort should report a parse failure, got: {}",
        abort_text
    );

    // The bare command refuses too, and its advice points at the four commands
    // that were just shown to be unusable.
    assert!(
        !plain_output.status.success(),
        "bare git chain rebase should be refused while a state file exists, got: {}",
        plain_text
    );
    assert!(
        plain_text.contains("A chain rebase is already in progress"),
        "bare git chain rebase should report an in-progress rebase, got: {}",
        plain_text
    );
    assert!(
        plain_text.contains("rebase --abort"),
        "bare git chain rebase should advise --abort even though it cannot run, got: {}",
        plain_text
    );

    // DEFECT: the guard swallows the read error (`operations.rs:27-31`), so the
    // message cannot even name the chain and never hints that the state is
    // corrupt rather than merely in progress.
    assert!(
        !plain_text.contains("chain_name"),
        "DEFECT H3 no longer reproduces: bare git chain rebase now names the chain, \
         which means the state was read successfully. Got: {}",
        plain_text
    );

    // DEFECT: the user is never told where the file lives, so there is no way
    // out short of knowing git-chain internals.
    assert!(
        !mentions_state_file_path,
        "DEFECT H3 no longer reproduces: some output now names chain-rebase-state.json. \
         If the fix landed, invert this assertion (see AFTER THE FIX above)."
    );

    // DEFECT: nothing cleared the state, so the repository stays wedged.
    assert!(
        state_file.exists(),
        "DEFECT H3 no longer reproduces: the state file was cleared by one of the \
         recovery commands. If the fix landed, invert this assertion."
    );

    teardown_git_repo(repo_name);
}

// ---------------------------------------------------------------------------
// H1 · HIGH — `--cleanup-backups` deletes backups it did not create
// ---------------------------------------------------------------------------

/// Documents audit finding **H1**: `cleanup_backup_branches`
/// (`operations.rs:1192-1229`) force-deletes `backup-<chain>/<branch>` for every
/// branch in the state, with no record of which backups this run created.
///
/// `git chain backup` writes refs in exactly the same namespace
/// (`src/branch.rs:217-226`), so a deliberate pre-rebase safety net is destroyed
/// by a later `rebase --cleanup-backups` — and the successful rebase has just
/// rewritten the branches those backups pointed at.
///
/// AFTER THE FIX (only delete backups this run created):
///   - each `rev_parse(... "backup-chain_name/some_branch_N")` must still
///     resolve, and equal the OID recorded before the rebase
///   - the "Deleted backup-..." lines must disappear from the output
#[test]
fn audit_h1_cleanup_backups_deletes_user_created_backups() {
    let repo_name = "audit_h1_cleanup_backups_deletes_user_backups";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");
        first_commit_all(&repo, "first commit");
    };

    {
        let branch_name = "some_branch_1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "commit on some_branch_1");
    };

    {
        let branch_name = "some_branch_2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        create_new_file(&path_to_repo, "file_2.txt", "contents 2");
        commit_all(&repo, "commit on some_branch_2");
    };

    let args: Vec<&str> = vec![
        "setup",
        "chain_name",
        "master",
        "some_branch_1",
        "some_branch_2",
    ];
    run_test_bin_expect_ok(&path_to_repo, args);

    checkout_branch(&repo, "some_branch_1");
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

    // The user deliberately takes a safety net before rewriting history.
    let args: Vec<&str> = vec!["backup"];
    let backup_output = run_test_bin_expect_ok(&path_to_repo, args);
    let backup_text = combined_output(&backup_output);

    let backup_1_oid = rev_parse(&path_to_repo, "backup-chain_name/some_branch_1");
    let backup_2_oid = rev_parse(&path_to_repo, "backup-chain_name/some_branch_2");

    println!("=== H1 DIAGNOSTICS: user-created backups ===");
    println!("BACKUP OUTPUT: {}", backup_text.trim());
    println!("backup-chain_name/some_branch_1: {:?}", backup_1_oid);
    println!("backup-chain_name/some_branch_2: {:?}", backup_2_oid);
    println!("======");

    assert!(
        backup_text.contains("Successfully backed up chain"),
        "git chain backup should report success, got: {}",
        backup_text
    );
    assert!(
        backup_1_oid.is_some(),
        "backup-chain_name/some_branch_1 should exist after git chain backup"
    );
    assert!(
        backup_2_oid.is_some(),
        "backup-chain_name/some_branch_2 should exist after git chain backup"
    );

    // Advance master so the rebase does real work and rewrites both branches.
    {
        checkout_branch(&repo, "master");
        create_new_file(&path_to_repo, "master_extra.txt", "master moved on");
        commit_all(&repo, "commit on master");
    };

    checkout_branch(&repo, "some_branch_1");
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

    let branch_1_before_rebase = rev_parse(&path_to_repo, "some_branch_1").unwrap();

    // git chain rebase --cleanup-backups
    // `git rebase` writes to stderr even on success, hence run_test_bin_for_rebase.
    let args: Vec<&str> = vec!["rebase", "--cleanup-backups"];
    let rebase_output = run_test_bin_for_rebase(&path_to_repo, args);
    let rebase_text = combined_output(&rebase_output);

    let branch_1_after_rebase = rev_parse(&path_to_repo, "some_branch_1").unwrap();
    let backup_1_after = rev_parse(&path_to_repo, "backup-chain_name/some_branch_1");
    let backup_2_after = rev_parse(&path_to_repo, "backup-chain_name/some_branch_2");

    println!("=== H1 DIAGNOSTICS: after rebase --cleanup-backups ===");
    println!("REBASE EXIT SUCCESS: {}", rebase_output.status.success());
    println!("REBASE OUTPUT: {}", rebase_text);
    println!(
        "some_branch_1: before={} after={} (rewritten: {})",
        branch_1_before_rebase,
        branch_1_after_rebase,
        branch_1_before_rebase != branch_1_after_rebase
    );
    println!(
        "backup-chain_name/some_branch_1 after: {:?}",
        backup_1_after
    );
    println!(
        "backup-chain_name/some_branch_2 after: {:?}",
        backup_2_after
    );
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: H1 after cleanup-backups");
    // assert!(false, "rebase output: {}", rebase_text);
    // assert!(false, "backup_1 before={:?} after={:?}", backup_1_oid, backup_1_after);

    assert!(
        rebase_output.status.success(),
        "git chain rebase --cleanup-backups should succeed, got: {}",
        rebase_text
    );
    assert_ne!(
        branch_1_before_rebase, branch_1_after_rebase,
        "the rebase should have rewritten some_branch_1 away from {}, making the \
         user's backup the only remaining pointer to the old commits",
        branch_1_before_rebase
    );

    // The deleted backup was the sole surviving ref for the pre-rebase commits:
    // it pointed exactly where some_branch_1 stood before the rewrite.
    println!(
        "user backup pointed at the pre-rebase tip of some_branch_1: {}",
        backup_1_oid.as_deref() == Some(branch_1_before_rebase.as_str())
    );
    assert_eq!(
        backup_1_oid.as_deref(),
        Some(branch_1_before_rebase.as_str()),
        "the user's backup should point at the pre-rebase tip of some_branch_1 ({}), \
         which the rebase then rewrote — so deleting the backup drops the last ref \
         to those commits",
        branch_1_before_rebase
    );

    // DEFECT: backups created by an earlier, separate `git chain backup` are
    // reported as cleanup targets and force-deleted.
    assert!(
        rebase_text.contains("Cleaning up backup branches"),
        "output should announce backup cleanup, got: {}",
        rebase_text
    );
    assert!(
        rebase_text.contains("Deleted backup-chain_name/some_branch_1"),
        "output should report deleting the user's backup of some_branch_1, got: {}",
        rebase_text
    );
    assert!(
        rebase_text.contains("Deleted backup-chain_name/some_branch_2"),
        "output should report deleting the user's backup of some_branch_2, got: {}",
        rebase_text
    );

    assert!(
        backup_1_after.is_none(),
        "DEFECT H1 no longer reproduces: backup-chain_name/some_branch_1 survived at \
         {:?}. If the fix landed, invert this assertion (see AFTER THE FIX above).",
        backup_1_after
    );
    assert!(
        backup_2_after.is_none(),
        "DEFECT H1 no longer reproduces: backup-chain_name/some_branch_2 survived at \
         {:?}. If the fix landed, invert this assertion.",
        backup_2_after
    );

    teardown_git_repo(repo_name);
}
