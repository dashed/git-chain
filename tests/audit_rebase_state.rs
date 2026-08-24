//! CHARACTERIZATION TESTS FOR KNOWN REBASE STATE-MACHINE DEFECTS
//!
//! ⚠️  SOME OF THESE TESTS ASSERT BUGGY BEHAVIOR ON PURPOSE. ⚠️
//!
//! Every test in this file documents a defect recorded in `REBASE_AUDIT.md`
//! (repository root), section 3 "State machine findings (failure recovery)".
//! A test that is still *characterizing* is written to PASS against the
//! defective implementation — passing there means "the bug is still present and
//! reproducible", not "the code is correct".
//!
//! WHEN A FIX LANDS, THE CORRESPONDING TEST MUST BE INVERTED, not deleted.
//! Each test carries an `AFTER THE FIX` block spelling out exactly which
//! assertions have to flip so the test becomes a regression guard for the
//! fixed behavior.
//!
//! Coverage (status: **C1, H1 and H2 are FIXED**, their tests now guard the
//! fixes; H3 still characterizes its defect):
//!
//! | Test                                                    | Audit finding | Status  |
//! |---------------------------------------------------------|---------------|---------|
//! | `audit_c1_rebase_failure_keeps_state_for_recovery`       | C1 · CRITICAL | FIXED   |
//! | `audit_c1_continue_retries_the_failed_branch`            | C1 · CRITICAL | FIXED   |
//! | `audit_h2_abort_keeps_work_on_untouched_branch`          | H2 · HIGH     | FIXED   |
//! | `audit_h2_abort_never_deletes_a_branch_on_zero_oid`      | H2 · HIGH     | FIXED   |
//! | `audit_h4_abort_deletes_state_before_final_checkout`     | H4 · HIGH     | FIXED   |
//! | `audit_h3_corrupt_state_wedges_all_recovery_commands`    | H3 · HIGH     | defect  |
//! | `audit_h1_cleanup_backups_keeps_user_created_backups`    | H1 · HIGH     | FIXED   |

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

/// The message of a branch's most recent reflog entry (`git reflog -1 --format=%gs`).
///
/// Note that git writes a reflog entry only when `update-ref` actually changes the ref;
/// restoring a branch to the value it already holds is a no-op and leaves the reflog
/// untouched.
fn last_reflog_message(path_to_repo: &Path, branch_name: &str) -> String {
    let output = run_git_command(
        path_to_repo,
        vec!["reflog", "-1", "--format=%gs", branch_name],
    );

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// True when `ancestor` is an ancestor of `descendant` (`git merge-base --is-ancestor`).
///
/// Used to prove that a chain is internally consistent: every branch must contain its
/// parent's tip.
fn is_ancestor(path_to_repo: &Path, ancestor: &str, descendant: &str) -> bool {
    run_git_command(
        path_to_repo,
        vec!["merge-base", "--is-ancestor", ancestor, descendant],
    )
    .status
    .success()
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
// C1 · CRITICAL (FIXED) — the failure path KEEPS the state file
// ---------------------------------------------------------------------------

/// Guards the fix for audit finding **C1**: when a `git rebase` subprocess fails
/// while leaving the repository clean, `rebase()` marks the branch `Failed` and
/// **keeps** the chain-rebase state, as git keeps its own rebase state on failure.
///
/// `original_refs` lives only in that file, and no backup branches are taken on the
/// normal (non-squash) path, so the file is the sole record of the pre-rebase
/// positions of the branches already rewritten. This test walks the whole recovery
/// path: the failure keeps the state and says so, `--status` reports the half-rebased
/// chain, and `--abort` puts every branch back.
///
/// It fails if the failure path ever deletes the state again.
#[cfg(unix)]
#[test]
fn audit_c1_rebase_failure_keeps_state_for_recovery() {
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
    println!(
        "output says the state was kept: {}",
        rebase_text.contains("Chain rebase state saved")
    );
    println!(
        "output points at rebase --abort: {}",
        rebase_text.contains("rebase --abort")
    );
    println!("EXPECTED (C1 fixed): the state survives and the output advises --abort");
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

    // The fix: the only record of the pre-rebase positions survives the failure.
    assert!(
        state_file.exists(),
        "the chain rebase state should survive a failed rebase, but {} does not exist",
        state_file.display()
    );
    assert!(
        rebase_text.contains("Chain rebase state saved"),
        "the failure output should tell the user the state was kept, got: {}",
        rebase_text
    );
    assert!(
        rebase_text.contains("rebase --abort"),
        "the failure output should point at rebase --abort for recovery, got: {}",
        rebase_text
    );

    // The normal rebase path still takes no backups, so the surviving state file really is
    // the only record of the pre-rebase positions — which is what makes keeping it critical.
    let backup_ref = rev_parse(&path_to_repo, "backup-chain_name/some_branch_1");
    println!("BACKUP REF FOR some_branch_1: {:?}", backup_ref);
    assert!(
        backup_ref.is_none(),
        "the normal rebase path takes no backups, so backup-chain_name/some_branch_1 \
         should not exist, but it resolved to {:?}",
        backup_ref
    );

    // Recovery step 1: --status reports the half-rebased chain instead of denying it.
    let args: Vec<&str> = vec!["rebase", "--status"];
    let status_output = run_test_bin_expect_ok(&path_to_repo, args);
    let status_text = combined_output(&status_output);

    println!("=== C1 DIAGNOSTICS: recovery ===");
    println!("STATUS EXIT SUCCESS: {}", status_output.status.success());
    println!("STATUS OUTPUT: {}", status_text);
    println!(
        "status names the failed branch: {}",
        status_text.contains("some_branch_2")
    );
    println!("status marks it Failed: {}", status_text.contains("Failed"));

    assert!(
        status_output.status.success(),
        "rebase --status should succeed, got output: {}",
        status_text
    );
    assert!(
        !status_text.contains("No chain rebase in progress"),
        "rebase --status should not deny the half-rebased chain, got: {}",
        status_text
    );
    assert!(
        status_text.contains("some_branch_1"),
        "rebase --status should report the rebased branch, got: {}",
        status_text
    );
    assert!(
        status_text.contains("some_branch_2"),
        "rebase --status should report the failed branch, got: {}",
        status_text
    );
    assert!(
        status_text.contains("Failed"),
        "rebase --status should mark some_branch_2 as Failed, got: {}",
        status_text
    );

    // Recovery step 2: --abort puts every branch back and consumes the state.
    let args: Vec<&str> = vec!["rebase", "--abort"];
    let abort_output = run_test_bin_expect_ok(&path_to_repo, args);
    let abort_text = combined_output(&abort_output);

    let branch_1_restored = rev_parse(&path_to_repo, "some_branch_1").unwrap();
    let branch_2_restored = rev_parse(&path_to_repo, "some_branch_2").unwrap();
    // some_branch_1 really was rewritten by the failed run, so restoring it moves the
    // ref and git records the reflog message the abort passes to `update-ref -m`.
    let branch_1_reflog = last_reflog_message(&path_to_repo, "some_branch_1");

    println!("ABORT EXIT SUCCESS: {}", abort_output.status.success());
    println!("ABORT OUTPUT: {}", abort_text);
    println!(
        "some_branch_1: original={} after abort={} (restored: {})",
        branch_1_original,
        branch_1_restored,
        branch_1_original == branch_1_restored
    );
    println!(
        "some_branch_2: original={} after abort={} (unchanged: {})",
        branch_2_original,
        branch_2_restored,
        branch_2_original == branch_2_restored
    );
    println!("STATE FILE EXISTS AFTER ABORT: {}", state_file.exists());
    println!("some_branch_1 last reflog message: {}", branch_1_reflog);
    println!(
        "reflog names the abort: {}",
        branch_1_reflog.contains("chain rebase (abort)")
    );
    println!("======");

    // Uncomment to stop test execution and debug the recovery path
    // assert!(false, "DEBUG STOP: C1 after --abort");
    // assert!(false, "status output: {}", status_text);
    // assert!(false, "abort output: {}", abort_text);
    // assert!(false, "branch_1: {} -> {}", branch_1_original, branch_1_restored);

    assert!(
        abort_output.status.success(),
        "rebase --abort should succeed now that the state survived, got output: {}",
        abort_text
    );
    assert!(
        abort_text.contains("Restoring branches"),
        "rebase --abort should report restoring the branches, got: {}",
        abort_text
    );
    assert_eq!(
        branch_1_restored, branch_1_original,
        "some_branch_1 should be restored to its pre-rebase commit {}, got {}",
        branch_1_original, branch_1_restored
    );
    assert_eq!(
        branch_2_restored, branch_2_original,
        "some_branch_2 was never rebased and should still be at {}, got {}",
        branch_2_original, branch_2_restored
    );
    assert!(
        !state_file.exists(),
        "the state file should be consumed by a successful --abort, but {} still exists",
        state_file.display()
    );
    // The restore is attributed in the reflog, so the rewind is traceable afterwards
    // (REBASE_AUDIT L3 — `update-ref` used to run without `-m`).
    assert!(
        branch_1_reflog.contains("chain rebase (abort)"),
        "some_branch_1's newest reflog entry should name the chain rebase abort, got: {}",
        branch_1_reflog
    );
    assert!(
        branch_1_reflog.contains("some_branch_1"),
        "the reflog message should name the restored branch, got: {}",
        branch_1_reflog
    );

    teardown_git_repo(repo_name);
}

/// The other half of the C1 recovery contract: `--continue` RETRIES the failed branch.
///
/// Keeping the state on failure (C1) makes `rebase --continue` reachable after a `Failed`
/// branch, so it has to do the right thing there. `Failed` means the branch was never moved —
/// the repository stayed clean and no git rebase is in progress — so `--continue` puts it back
/// in the queue and re-attempts it from its frozen merge base.
///
/// Resuming *past* it instead would replant the children onto the failed branch's stale tip,
/// report success for a chain that is no longer internally consistent, and delete the state —
/// walking the user straight back into C1. This test fails if that regresses.
#[cfg(unix)]
#[test]
fn audit_c1_continue_retries_the_failed_branch() {
    let repo_name = "audit_c1_continue_retries_failed";
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

    // Advance master so the chain actually has work to do.
    {
        checkout_branch(&repo, "master");
        create_new_file(&path_to_repo, "master_extra.txt", "master moved on");
        commit_all(&repo, "commit on master");
    };

    checkout_branch(&repo, "some_branch_1");
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

    let hook_path = path_to_repo.join(".git/hooks/pre-rebase");
    install_pre_rebase_hook_refusing(&path_to_repo, "some_branch_2");

    let branch_2_original = rev_parse(&path_to_repo, "some_branch_2").unwrap();
    let state_file = path_to_repo.join(".git/chain-rebase-state.json");

    // The rebase fails on some_branch_2 and keeps the state (the C1 fix).
    let rebase_output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let rebase_text = combined_output(&rebase_output);

    let branch_1_rebased = rev_parse(&path_to_repo, "some_branch_1").unwrap();
    let branch_2_after_failure = rev_parse(&path_to_repo, "some_branch_2").unwrap();

    println!("=== C1-continue DIAGNOSTICS: the failing rebase ===");
    println!("REBASE EXIT SUCCESS: {}", rebase_output.status.success());
    println!("REBASE OUTPUT: {}", rebase_text);
    println!("STATE FILE EXISTS AFTER FAILURE: {}", state_file.exists());
    println!(
        "some_branch_2 untouched by the failure: {}",
        branch_2_after_failure == branch_2_original
    );
    println!("======");

    assert!(
        !rebase_output.status.success(),
        "the hook should make the rebase fail, but it succeeded. Output: {}",
        rebase_text
    );
    assert!(
        state_file.exists(),
        "the state should survive the failure, but {} does not exist",
        state_file.display()
    );
    assert_eq!(
        branch_2_after_failure, branch_2_original,
        "some_branch_2 should be untouched by the refused rebase, but it moved from {} to {}",
        branch_2_original, branch_2_after_failure
    );

    // Remove the obstacle, exactly as a user would after reading the failure.
    std::fs::remove_file(&hook_path).unwrap();

    let continue_output = run_test_bin(&path_to_repo, vec!["rebase", "--continue"]);
    let continue_text = combined_output(&continue_output);

    let branch_1_final = rev_parse(&path_to_repo, "some_branch_1").unwrap();
    let branch_2_final = rev_parse(&path_to_repo, "some_branch_2").unwrap();

    let master_in_branch_1 = is_ancestor(&path_to_repo, "master", "some_branch_1");
    let branch_1_in_branch_2 = is_ancestor(&path_to_repo, "some_branch_1", "some_branch_2");

    println!("=== C1-continue DIAGNOSTICS: the retry ===");
    println!(
        "CONTINUE EXIT SUCCESS: {}",
        continue_output.status.success()
    );
    println!("CONTINUE OUTPUT: {}", continue_text);
    println!(
        "output announces the retry: {}",
        continue_text.contains("Retrying branch some_branch_2")
    );
    println!(
        "some_branch_2 moved: {} ({} -> {})",
        branch_2_final != branch_2_original,
        branch_2_original,
        branch_2_final
    );
    println!(
        "some_branch_1 untouched by the retry: {} ({} -> {})",
        branch_1_final == branch_1_rebased,
        branch_1_rebased,
        branch_1_final
    );
    println!(
        "master is an ancestor of some_branch_1: {}",
        master_in_branch_1
    );
    println!(
        "some_branch_1 is an ancestor of some_branch_2: {}",
        branch_1_in_branch_2
    );
    println!("STATE FILE EXISTS AFTER CONTINUE: {}", state_file.exists());
    println!("EXPECTED: the failed branch is retried and the chain ends up consistent");
    println!("======");

    // Uncomment to stop test execution and debug the retry
    // assert!(false, "DEBUG STOP: C1 --continue retry");
    // assert!(false, "rebase output: {}", rebase_text);
    // assert!(false, "continue output: {}", continue_text);
    // assert!(false, "branch_2: {} -> {}", branch_2_original, branch_2_final);

    assert!(
        continue_output.status.success(),
        "rebase --continue should succeed once the hook is gone, got: {}",
        continue_text
    );
    assert!(
        continue_text.contains("Retrying branch some_branch_2"),
        "rebase --continue should announce that it is retrying the failed branch, got: {}",
        continue_text
    );
    // The whole point: the previously failed branch was actually rebased this time.
    assert_ne!(
        branch_2_final, branch_2_original,
        "some_branch_2 should have been rebased by the retry, but it is still at {}",
        branch_2_original
    );
    assert_eq!(
        branch_1_final, branch_1_rebased,
        "some_branch_1 was already rebased and should not move again, but it went from {} to {}",
        branch_1_rebased, branch_1_final
    );
    // The chain is internally consistent: every branch contains its parent's tip.
    assert!(
        master_in_branch_1,
        "some_branch_1 should contain master's tip after the rebase"
    );
    assert!(
        branch_1_in_branch_2,
        "some_branch_2 should contain some_branch_1's new tip, otherwise the chain is broken"
    );
    // The summary counts the retried branch, and the run is finished.
    assert!(
        continue_text.contains("Rebased: 2"),
        "the summary should count both branches as rebased, got: {}",
        continue_text
    );
    assert!(
        continue_text.contains("Successfully rebased chain"),
        "the summary should report a completed chain rebase, got: {}",
        continue_text
    );
    assert!(
        !state_file.exists(),
        "a completed chain rebase should consume the state, but {} still exists",
        state_file.display()
    );

    teardown_git_repo(repo_name);
}

// ---------------------------------------------------------------------------
// H2 · HIGH (FIXED) — `--abort` checks each branch and reports honestly
// ---------------------------------------------------------------------------

/// Guards the fix for audit finding **H2**: `rebase_abort` no longer replays every
/// entry of `original_refs` blindly. It walks the branches in chain order, and for a
/// branch still marked `Pending` — one this rebase never reached — it compares the
/// current tip against the recorded original and leaves it alone if it has moved,
/// because the only thing that could have moved it is the user.
///
/// Here the chain rebase stops at a conflict on `some_branch_1` and never reaches
/// `some_branch_3`. The user aborts the git-level rebase, commits real work on
/// `some_branch_3`, and then aborts the chain rebase. That commit must survive, and
/// the summary must not claim every branch was restored.
///
/// This test fails if abort ever goes back to clobbering branches it did not rewrite.
#[test]
fn audit_h2_abort_keeps_work_on_untouched_branch() {
    let repo_name = "audit_h2_abort_keeps_untouched_work";
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
    println!(
        "output claims every branch was restored: {}",
        abort_text.contains("All branches restored")
    );
    println!(
        "output warns that some_branch_3 was left as-is: {}",
        abort_text.contains("Leaving some_branch_3 as-is")
    );
    println!(
        "output reports restoring some_branch_1: {}",
        abort_text.contains("Restored some_branch_1")
    );
    println!("EXPECTED (H2 fixed): the untouched branch keeps its commit");
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

    // The report is honest: it cannot claim everything was restored, because
    // some_branch_3 deliberately was not.
    assert!(
        !abort_text.contains("All branches restored"),
        "abort must not claim every branch was restored when one was left as-is, got: {}",
        abort_text
    );
    assert!(
        abort_text.contains("Leaving some_branch_3 as-is"),
        "abort should warn that some_branch_3 was left alone, got: {}",
        abort_text
    );

    // The fix: a branch the rebase never touched keeps the work committed on it.
    assert_eq!(
        branch_3_after_abort, branch_3_with_new_work,
        "some_branch_3 should still carry the commit made during the pause ({}), got {}",
        branch_3_with_new_work, branch_3_after_abort
    );
    assert_ne!(
        branch_3_after_abort, branch_3_original,
        "some_branch_3 must not be rewound to its pre-rebase position {}",
        branch_3_original
    );
    assert!(
        work_present_after_abort,
        "untouched_work.txt should still be reachable from some_branch_3 after the abort"
    );

    // The branch the rebase *did* manage is still restored.
    assert!(
        abort_text.contains("Restored some_branch_1"),
        "abort should still restore the branch the rebase was working on, got: {}",
        abort_text
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

/// Guards the data-loss half of **H2**: an all-zero OID in `original_refs` must never
/// reach `git update-ref`.
///
/// `git update-ref <ref> 0000…` is git's *delete-ref* syntax. Before the fix, a zeroed
/// entry made the abort print "Restored branch2 to 0000000" while deleting the branch it
/// claimed to restore — the exact opposite of what abort exists to do. `rebase_abort` now
/// rejects any recorded original that is not a usable object id, names the branch, and
/// leaves it alone.
///
/// The state file is hand-written (as in the H3 test) because git-chain never produces a
/// zeroed entry itself; the point is that a corrupted or truncated one cannot destroy work.
#[test]
fn audit_h2_abort_never_deletes_a_branch_on_zero_oid() {
    let repo_name = "audit_h2_abort_zero_oid";
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

    let master_oid = rev_parse(&path_to_repo, "master").unwrap();
    // The OID the crafted state will record as some_branch_1's original, so restoring it
    // is a real ref move and proves the healthy branches are still handled normally.
    let branch_1_target = rev_parse(&path_to_repo, "some_branch_1").unwrap();

    checkout_branch(&repo, "some_branch_1");
    create_new_file(&path_to_repo, "file_1_extra.txt", "more contents");
    commit_all(&repo, "second commit on some_branch_1");

    let branch_1_before_abort = rev_parse(&path_to_repo, "some_branch_1").unwrap();
    let branch_2_before_abort = rev_parse(&path_to_repo, "some_branch_2").unwrap();

    // Run the abort from master so the final checkout of some_branch_1 is a real one.
    checkout_branch(&repo, "master");

    // A schema-complete state file whose only anomaly is the zeroed original for
    // some_branch_2. Both branches are marked Completed so both are restore candidates.
    let state_file = path_to_repo.join(".git/chain-rebase-state.json");
    let zeroed_state = format!(
        r#"{{
  "version": 1,
  "chain_name": "chain_name",
  "original_branch": "some_branch_1",
  "root_branch": "master",
  "options": {{
    "step_rebase": false,
    "ignore_root": false,
    "squashed_merge_handling": "reset"
  }},
  "original_refs": {{
    "some_branch_1": "{}",
    "some_branch_2": "0000000000000000000000000000000000000000"
  }},
  "merge_bases": ["{}", "{}"],
  "branches": [
    {{ "name": "some_branch_1", "parent": "master", "status": "completed" }},
    {{ "name": "some_branch_2", "parent": "some_branch_1", "status": "completed" }}
  ],
  "current_index": 1,
  "completed_count": 2,
  "total_count": 2
}}
"#,
        branch_1_target, master_oid, branch_1_target
    );
    std::fs::write(&state_file, zeroed_state).unwrap();

    let abort_output = run_test_bin(&path_to_repo, vec!["rebase", "--abort"]);
    let abort_text = combined_output(&abort_output);

    let branch_2_after = rev_parse(&path_to_repo, "some_branch_2");
    let branch_1_after = rev_parse(&path_to_repo, "some_branch_1");

    println!("=== H2/zero-OID DIAGNOSTICS ===");
    println!("ABORT EXIT SUCCESS: {}", abort_output.status.success());
    println!("ABORT OUTPUT: {}", abort_text);
    println!(
        "some_branch_2 before abort: {} / after abort: {:?}",
        branch_2_before_abort, branch_2_after
    );
    println!("some_branch_2 still resolves: {}", branch_2_after.is_some());
    println!(
        "some_branch_1: before={} after={:?} (target was {})",
        branch_1_before_abort, branch_1_after, branch_1_target
    );
    println!(
        "output rejects the zeroed id: {}",
        abort_text.contains("not a usable object id")
    );
    println!(
        "output reports some_branch_2 as left as-is: {}",
        abort_text.contains("Left as-is: some_branch_2")
    );
    println!(
        "output claims every branch was restored: {}",
        abort_text.contains("All branches restored")
    );
    println!("EXPECTED: some_branch_2 survives untouched; some_branch_1 is restored");
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: zero-OID abort");
    // assert!(false, "abort output: {}", abort_text);
    // assert!(false, "branch_2 before={} after={:?}", branch_2_before_abort, branch_2_after);

    assert!(
        abort_output.status.success(),
        "abort should succeed while refusing the unusable entry, got: {}",
        abort_text
    );
    assert!(
        abort_text.contains("not a usable object id"),
        "abort should reject the zeroed original commit, got: {}",
        abort_text
    );
    assert!(
        abort_text.contains("some_branch_2"),
        "the rejection should name the branch it refused to restore, got: {}",
        abort_text
    );
    assert!(
        abort_text.contains("Left as-is: some_branch_2"),
        "the summary should report some_branch_2 as left as-is, got: {}",
        abort_text
    );
    assert!(
        !abort_text.contains("All branches restored"),
        "abort must not claim every branch was restored, got: {}",
        abort_text
    );

    // The invariant: abort never deletes a branch.
    assert!(
        branch_2_after.is_some(),
        "some_branch_2 must still exist after the abort, but it no longer resolves"
    );
    assert_eq!(
        branch_2_after.as_deref(),
        Some(branch_2_before_abort.as_str()),
        "some_branch_2 should be untouched at {}, got {:?}",
        branch_2_before_abort,
        branch_2_after
    );

    // The healthy entry is still restored normally.
    assert!(
        abort_text.contains("Restored some_branch_1"),
        "abort should still restore the branch with a usable original, got: {}",
        abort_text
    );
    assert_eq!(
        branch_1_after.as_deref(),
        Some(branch_1_target.as_str()),
        "some_branch_1 should be restored to {} (it was at {} before the abort), got {:?}",
        branch_1_target,
        branch_1_before_abort,
        branch_1_after
    );

    teardown_git_repo(repo_name);
}

// ---------------------------------------------------------------------------
// H4 · HIGH (FIXED) — `--abort` deletes state BEFORE its final checkout
// ---------------------------------------------------------------------------

/// Guards the fix for audit finding **H4**: `rebase_abort` used to checkout the
/// original branch before deleting the state, so a checkout failure left the state
/// behind. Every retry then re-ran the whole restore sweep and failed the same way.
///
/// Here the original branch is claimed by a linked worktree while the chain rebase is
/// paused on a conflict, so the final checkout cannot succeed. The abort itself is
/// still complete: the refs are restored and the state is gone, and the checkout
/// failure is reported as a warning rather than failing the command.
#[test]
fn audit_h4_abort_deletes_state_before_final_checkout() {
    let repo_name = "audit_h4_abort_state_before_checkout";
    let worktree_dir = generate_path_to_repo(format!("{}_worktree", repo_name));
    std::fs::remove_dir_all(&worktree_dir).ok();

    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");
        first_commit_all(&repo, "first commit");
    };

    // feature_1 rebases cleanly...
    {
        create_branch(&repo, "feature_1");
        checkout_branch(&repo, "feature_1");
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "commit on feature_1");
    };

    // ...while feature_2 touches the file master is about to change, so the chain
    // rebase pauses on feature_2 with feature_1 already rewritten.
    {
        create_branch(&repo, "feature_2");
        checkout_branch(&repo, "feature_2");
        create_new_file(&path_to_repo, "shared.txt", "feature 2 version");
        commit_all(&repo, "commit on feature_2");
    };

    let args: Vec<&str> = vec!["setup", "chain_name", "master", "feature_1", "feature_2"];
    run_test_bin_expect_ok(&path_to_repo, args);

    {
        checkout_branch(&repo, "master");
        create_new_file(&path_to_repo, "shared.txt", "master version");
        commit_all(&repo, "conflicting commit on master");
    };

    // Starting from feature_1 makes it the recorded original_branch.
    checkout_branch(&repo, "feature_1");
    let feature_1_original = rev_parse(&path_to_repo, "feature_1").unwrap();
    let feature_2_original = rev_parse(&path_to_repo, "feature_2").unwrap();

    let state_file = path_to_repo.join(".git/chain-rebase-state.json");

    let rebase_output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let rebase_text = combined_output(&rebase_output);

    let feature_1_rebased = rev_parse(&path_to_repo, "feature_1").unwrap();

    println!("=== H4 DIAGNOSTICS: the paused rebase ===");
    println!("REBASE EXIT SUCCESS: {}", rebase_output.status.success());
    println!("REBASE OUTPUT: {}", rebase_text);
    println!("STATE FILE EXISTS: {}", state_file.exists());
    println!(
        "feature_1 was rewritten before the pause: {}",
        feature_1_rebased != feature_1_original
    );
    println!("======");

    assert!(
        !rebase_output.status.success(),
        "the rebase should stop on the conflict in feature_2, got: {}",
        rebase_text
    );
    assert!(
        state_file.exists(),
        "the paused rebase should have left a state file at {}",
        state_file.display()
    );
    assert_ne!(
        feature_1_original, feature_1_rebased,
        "feature_1 should have been rewritten before the pause, but is still at {}",
        feature_1_original
    );

    // Claim the original branch in a linked worktree. The main worktree is mid-rebase
    // with a detached HEAD, so feature_1 is free to be taken.
    let worktree_output = run_git_command(
        &path_to_repo,
        vec![
            "worktree",
            "add",
            &format!("../{}_worktree", repo_name),
            "feature_1",
        ],
    );
    assert!(
        worktree_output.status.success(),
        "git worktree add should succeed but got: {}",
        String::from_utf8_lossy(&worktree_output.stderr)
    );

    let abort_output = run_test_bin(&path_to_repo, vec!["rebase", "--abort"]);
    let abort_text = combined_output(&abort_output);

    let feature_1_restored = rev_parse(&path_to_repo, "feature_1").unwrap();
    let feature_2_restored = rev_parse(&path_to_repo, "feature_2").unwrap();

    println!("=== H4 DIAGNOSTICS: abort with the original branch worktree-held ===");
    println!("ABORT EXIT SUCCESS: {}", abort_output.status.success());
    println!("ABORT OUTPUT: {}", abort_text);
    println!("STATE FILE EXISTS AFTER ABORT: {}", state_file.exists());
    println!(
        "feature_1 restored: {} ({} -> {})",
        feature_1_restored == feature_1_original,
        feature_1_rebased,
        feature_1_restored
    );
    println!(
        "feature_2 restored: {}",
        feature_2_restored == feature_2_original
    );
    println!(
        "checkout failure reported as a warning: {}",
        abort_text.contains("Could not switch back to")
    );
    println!("EXPECTED (H4 fixed): abort completes, state is gone, checkout is a warning");
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: H4 abort with worktree-held original branch");
    // assert!(false, "abort output: {}", abort_text);
    // assert!(false, "state exists: {}", state_file.exists());

    assert!(
        abort_output.status.success(),
        "abort should succeed even when the final checkout cannot run, got: {}",
        abort_text
    );
    assert!(
        abort_text.contains("Could not switch back to"),
        "the failed checkout should be reported as a warning, got: {}",
        abort_text
    );
    assert!(
        abort_text.contains("checked out in another worktree"),
        "the warning should explain why the checkout failed, got: {}",
        abort_text
    );
    // The point of the reorder: the abort is finished, so the state is gone and no
    // retry is needed.
    assert!(
        !state_file.exists(),
        "the state should be deleted before the final checkout, but {} still exists",
        state_file.display()
    );
    assert_eq!(
        feature_1_restored, feature_1_original,
        "feature_1 should be restored to {}, got {}",
        feature_1_original, feature_1_restored
    );
    assert_eq!(
        feature_2_restored, feature_2_original,
        "feature_2 should be restored to {}, got {}",
        feature_2_original, feature_2_restored
    );

    std::fs::remove_dir_all(&worktree_dir).ok();
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
// H1 · HIGH (FIXED) — `--cleanup-backups` deletes ONLY backups it created
// ---------------------------------------------------------------------------

/// Guards the fix for audit finding **H1**: `cleanup_backup_branches` deletes only the
/// refs listed in the run's `created_backups`, so a backup it did not make survives.
///
/// `git chain backup` writes refs in exactly the same namespace
/// (`src/branch.rs:217-226`). Before the fix, a deliberate pre-rebase safety net was
/// force-deleted by a later `rebase --cleanup-backups` — right after that rebase had
/// rewritten the very branches those backups pointed at, leaving the old commits
/// unreferenced.
///
/// This test fails if cleanup ever reaches outside its own run again.
#[test]
fn audit_h1_cleanup_backups_keeps_user_created_backups() {
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
    println!(
        "output announces backup cleanup: {}",
        rebase_text.contains("Cleaning up backup branches")
    );
    println!(
        "output reports deleting the user's backup of some_branch_1: {}",
        rebase_text.contains("Deleted backup-chain_name/some_branch_1")
    );
    println!(
        "output reports deleting the user's backup of some_branch_2: {}",
        rebase_text.contains("Deleted backup-chain_name/some_branch_2")
    );
    println!("EXPECTED (H1 fixed): both user backups survive, untouched");
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

    // The fix: this run created no backups of its own, so cleanup has nothing to do
    // and says nothing.
    assert!(
        !rebase_text.contains("Cleaning up backup branches"),
        "cleanup should not run when this rebase created no backups, got: {}",
        rebase_text
    );
    assert!(
        !rebase_text.contains("Deleted backup-chain_name/some_branch_1"),
        "the user's backup of some_branch_1 should not be deleted, got: {}",
        rebase_text
    );
    assert!(
        !rebase_text.contains("Deleted backup-chain_name/some_branch_2"),
        "the user's backup of some_branch_2 should not be deleted, got: {}",
        rebase_text
    );

    // Both backups still resolve, and still point exactly where they did before.
    assert_eq!(
        backup_1_after, backup_1_oid,
        "backup-chain_name/some_branch_1 should be untouched by the rebase, but went \
         from {:?} to {:?}",
        backup_1_oid, backup_1_after
    );
    assert_eq!(
        backup_2_after, backup_2_oid,
        "backup-chain_name/some_branch_2 should be untouched by the rebase, but went \
         from {:?} to {:?}",
        backup_2_oid, backup_2_after
    );
    // Restated as the property that matters: the pre-rebase commits are still reachable.
    assert_eq!(
        backup_1_after.as_deref(),
        Some(branch_1_before_rebase.as_str()),
        "backup-chain_name/some_branch_1 should still point at the pre-rebase tip of \
         some_branch_1 ({}), got {:?}",
        branch_1_before_rebase,
        backup_1_after
    );

    teardown_git_repo(repo_name);
}
