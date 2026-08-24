//! Characterization tests for the `git rebase` invocation that `git chain rebase` builds.
//!
//! # THESE TESTS ASSERT DEFECTIVE BEHAVIOR ON PURPOSE
//!
//! Each test here documents a HIGH severity finding from `REBASE_AUDIT.md` (repo root).
//! They are *characterization* tests: they pin down what git-chain does **today**, which is
//! wrong, so they PASS while the bug exists and FAIL the moment it is fixed.
//!
//! **When a fix lands, the corresponding test MUST BE INVERTED**, not deleted. The inverted
//! assertions are spelled out inline next to each defect assertion, marked `AFTER THE FIX:`.
//!
//! ## F1 — merge-base-as-upstream disables git's patch-id skipping
//!
//! `src/git_chain/operations.rs:253-266` runs
//! `git rebase --keep-empty --onto <parent> <fork_point_sha> <branch>`. Because the fork point
//! is always an ancestor of `<branch>`, the left side of the `A...B` symmetric difference that
//! `sequencer_make_script` walks is empty, so `revs.cherry_mark` can never mark a commit
//! PATCHSAME. git itself never passes the fork point as `<upstream>`: it layers it on as a
//! negative ref (`^restrict_revision`) on top of the real upstream (`builtin/rebase.c:296-311`).
//! The result is that git-chain replays commits whose patch is already on the parent branch,
//! which conflicts whenever the parent has since touched the same lines.
//!
//! ## F2 — `rebase.updateRefs` is never neutralised
//!
//! None of the three rebase invocations pass `--no-update-refs`, so a user's
//! `rebase.updateRefs=true` applies. git drags every other ref that points into the replayed
//! range forward onto the rewritten commits — including the `backup-<chain>/<branch>` refs that
//! `git chain backup` just created, which is precisely the safety net they exist to provide.

#[path = "common/mod.rs"]
pub mod common;

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, run_git_command, run_test_bin, run_test_bin_expect_ok,
    run_test_bin_for_rebase, setup_git_repo, teardown_git_repo,
};

use git2::Repository;
use std::path::Path;

/// Build the ten-line file used by the F1 scenario.
///
/// The three arguments are the suffixes applied to lines 1, 5 and 8, which is where every
/// commit in the scenario makes its edit. Rewriting the whole file keeps the fixture readable
/// and guarantees the two "fix L8" commits produce a byte-identical diff, and therefore an
/// identical patch-id.
fn f1_file_contents(l1_suffix: &str, l5_suffix: &str, l8_suffix: &str) -> String {
    format!(
        "L1{}\nL2\nL3\nL4\nL5{}\nL6\nL7\nL8{}\nL9\nL10",
        l1_suffix, l5_suffix, l8_suffix
    )
}

/// Run a git command that is expected to succeed and return its trimmed stdout.
fn git_stdout(path_to_repo: &Path, arguments: Vec<&str>) -> String {
    let printable = arguments.join(" ");
    let output = run_git_command(path_to_repo, arguments);

    assert!(
        output.status.success(),
        "git {} should succeed but exited with {:?}. stderr: {}",
        printable,
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Materialize the F1 scenario in `repo`.
///
/// Resulting history:
///
/// ```text
/// master: base -> "master advances"
/// b1:     base -> A (L5)        -> B2 (L8 fix, same patch as B) -> D (L8 improved)
/// b2:     base -> A (L5)        -> B  (L8 fix)                  -> C (L1)
/// ```
///
/// `B` (on b2) and `B2` (on b1) make the identical edit, so they share a patch-id. This models
/// the routine stacked-branch move of landing a fix on the parent branch while the child still
/// carries it. `D` then edits the same lines again, so replaying `B` on top of b1 cannot merge
/// cleanly — it can only be skipped, which is exactly what git's patch-id detection does.
fn build_f1_scenario(repo: &Repository, path_to_repo: &Path) {
    // Pin the two rebase knobs that would otherwise let ambient user config decide the outcome.
    // `rebase.backend` matters because the apply backend skips already-applied commits silently
    // (no "skipped previously applied commit" warning), and git-chain's own `--keep-empty`
    // forces the merge backend anyway — so pinning `merge` keeps both halves comparable.
    // `rebase.updateRefs` is pinned off so this test isolates F1 from F2.
    for (key, value) in [("rebase.backend", "merge"), ("rebase.updateRefs", "false")] {
        let output = run_git_command(path_to_repo, vec!["config", key, value]);
        assert!(
            output.status.success(),
            "setting {}={} should succeed, stderr: {}",
            key,
            value,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    create_new_file(path_to_repo, "f.txt", &f1_file_contents("", "", ""));
    first_commit_all(repo, "base");

    create_branch(repo, "b1");
    checkout_branch(repo, "b1");
    create_new_file(path_to_repo, "f.txt", &f1_file_contents("", "-b1", ""));
    commit_all(repo, "A(b1) modifies L5");

    create_branch(repo, "b2");
    checkout_branch(repo, "b2");
    create_new_file(path_to_repo, "f.txt", &f1_file_contents("", "-b1", "-fix"));
    commit_all(repo, "B(b2) fixes L8");
    create_new_file(
        path_to_repo,
        "f.txt",
        &f1_file_contents("-b2", "-b1", "-fix"),
    );
    commit_all(repo, "C(b2) modifies L1");

    checkout_branch(repo, "b1");
    create_new_file(path_to_repo, "f.txt", &f1_file_contents("", "-b1", "-fix"));
    commit_all(repo, "B2(b1) lands the same L8 fix on the parent");
    create_new_file(
        path_to_repo,
        "f.txt",
        &f1_file_contents("", "-b1", "-fix-improved"),
    );
    commit_all(repo, "D(b1) improves L8 further");

    checkout_branch(repo, "master");
    create_new_file(path_to_repo, "extra.txt", "extra");
    commit_all(repo, "master advances");
}

/// F1: `git chain rebase` conflicts on input that plain `git rebase` replays cleanly.
///
/// Both halves of this test build a byte-identical scenario and rebase `b1` with the exact
/// command git-chain uses. The only thing that differs is the `<upstream>` argument of the `b2`
/// rebase: git-chain passes the fork-point SHA, git's own form passes the parent branch ref.
/// That single difference decides whether the already-applied commit is silently skipped or
/// blows up as a merge conflict.
#[test]
fn audit_f1_chain_rebase_conflicts_where_plain_git_succeeds() {
    // ---------------------------------------------------------------------------------
    // Half A — git-chain's construction: expected (defectively) to conflict.
    // ---------------------------------------------------------------------------------
    let chain_repo_name = "audit_f1_chain_rebase";
    let chain_repo = setup_git_repo(chain_repo_name);
    let path_to_chain_repo = generate_path_to_repo(chain_repo_name);

    build_f1_scenario(&chain_repo, &path_to_chain_repo);

    // Confirm the premise of the whole test: the two "fix L8" commits really do share a
    // patch-id. Without this, git would have nothing to skip and the comparison below would
    // prove nothing.
    let b2_fix_diff = git_stdout(&path_to_chain_repo, vec!["show", "b2~1"]);
    let b1_fix_diff = git_stdout(&path_to_chain_repo, vec!["show", "b1~1"]);
    let b2_fix_subject = git_stdout(
        &path_to_chain_repo,
        vec!["log", "-1", "--format=%s", "b2~1"],
    );
    let b1_fix_subject = git_stdout(
        &path_to_chain_repo,
        vec!["log", "-1", "--format=%s", "b1~1"],
    );

    println!("=== F1 SCENARIO (git-chain repo) ===");
    println!("b2~1 subject: {}", b2_fix_subject);
    println!("b1~1 subject: {}", b1_fix_subject);
    println!(
        "the two commits have different subjects (so they are distinct commits): {}",
        b2_fix_subject != b1_fix_subject
    );

    // Compare the two diffs directly: identical diff bodies imply identical patch-ids, which is
    // the property git's cherry-mark relies on.
    let b2_fix_body = b2_fix_diff
        .split("\ndiff --git ")
        .nth(1)
        .unwrap_or("")
        .to_string();
    let b1_fix_body = b1_fix_diff
        .split("\ndiff --git ")
        .nth(1)
        .unwrap_or("")
        .to_string();

    println!("b2 fix diff body is non-empty: {}", !b2_fix_body.is_empty());
    println!("b1 fix diff body is non-empty: {}", !b1_fix_body.is_empty());
    println!("diff bodies are identical: {}", b2_fix_body == b1_fix_body);

    assert_ne!(
        b2_fix_subject, b1_fix_subject,
        "the two L8 fixes must be distinct commits, but both are subject {:?}",
        b2_fix_subject
    );
    assert!(
        !b2_fix_body.is_empty(),
        "b2~1 should carry a diff, got: {}",
        b2_fix_diff
    );
    assert!(
        !b1_fix_body.is_empty(),
        "b1~1 should carry a diff, got: {}",
        b1_fix_diff
    );
    assert_eq!(
        b2_fix_body, b1_fix_body,
        "the L8 fix on b2 and the one on b1 must be patch-identical for this test to mean \
         anything.\nb2~1 body:\n{}\nb1~1 body:\n{}",
        b2_fix_body, b1_fix_body
    );

    run_test_bin_expect_ok(
        &path_to_chain_repo,
        vec!["setup", "audit_chain", "master", "b1", "b2"],
    );

    checkout_branch(&chain_repo, "b2");

    let chain_output = run_test_bin(&path_to_chain_repo, vec!["rebase"]);
    let chain_stdout = String::from_utf8_lossy(&chain_output.stdout).to_string();
    let chain_stderr = String::from_utf8_lossy(&chain_output.stderr).to_string();

    let chain_status_porcelain = git_stdout(&path_to_chain_repo, vec!["status", "--porcelain"]);
    let chain_rebase_dir_exists = path_to_chain_repo
        .join(".git")
        .join("rebase-merge")
        .is_dir();

    println!("=== F1 HALF A: git chain rebase ===");
    println!("STDOUT: {}", chain_stdout);
    println!("STDERR: {}", chain_stderr);
    println!("EXIT STATUS success: {}", chain_output.status.success());
    println!("EXIT CODE: {:?}", chain_output.status.code());
    println!(
        "stderr mentions 'Unable to completely rebase': {}",
        chain_stderr.contains("Unable to completely rebase")
    );
    println!(
        "stdout shows the merge-base SHA as <upstream>: {}",
        chain_stdout.contains("git rebase --keep-empty --onto b1 ")
    );
    println!("git status --porcelain: {}", chain_status_porcelain);
    println!(
        "porcelain reports an unmerged path (UU): {}",
        chain_status_porcelain.contains("UU ")
    );
    println!(".git/rebase-merge exists: {}", chain_rebase_dir_exists);
    println!("EXPECTED (defect F1): chain rebase fails and leaves b2 conflicted mid-rebase");

    // Uncomment to stop test execution and debug half A
    // assert!(false, "DEBUG STOP: F1 half A (git-chain construction)");
    // assert!(false, "stdout: {}", chain_stdout);
    // assert!(false, "stderr: {}", chain_stderr);
    // assert!(false, "status code: {:?}", chain_output.status.code());
    // assert!(false, "porcelain: {}", chain_status_porcelain);

    // AFTER THE FIX: assert!(chain_output.status.success(), ...)
    assert!(
        !chain_output.status.success(),
        "DEFECT F1 no longer reproduces: git chain rebase succeeded. If the fix landed, invert \
         this test.\nstdout: {}\nstderr: {}",
        chain_stdout,
        chain_stderr
    );
    assert_eq!(
        chain_output.status.code(),
        Some(1),
        "git chain rebase should exit 1 on conflict, got {:?}. stderr: {}",
        chain_output.status.code(),
        chain_stderr
    );
    assert!(
        chain_stderr.contains("Unable to completely rebase"),
        "stderr should report the failed chain rebase but got: {}",
        chain_stderr
    );
    // This is the defective invocation itself, echoed by git-chain: `<upstream>` is a raw SHA
    // (the fork point) rather than the `b1` branch ref.
    assert!(
        chain_stdout.contains("git rebase --keep-empty --onto b1 "),
        "stdout should echo the rebase invocation for b2 but got: {}",
        chain_stdout
    );
    // AFTER THE FIX: assert!(!chain_status_porcelain.contains("UU "), ...)
    assert!(
        chain_status_porcelain.contains("UU "),
        "b2 should be left with an unmerged path after the conflict, got porcelain: {}",
        chain_status_porcelain
    );
    // AFTER THE FIX: assert!(!chain_rebase_dir_exists, ...)
    assert!(
        chain_rebase_dir_exists,
        "a git rebase should be left in progress (.git/rebase-merge), but the directory is \
         missing. porcelain: {}",
        chain_status_porcelain
    );

    // ---------------------------------------------------------------------------------
    // Half B — git's own construction on the identical scenario: succeeds.
    // ---------------------------------------------------------------------------------
    let git_repo_name = "audit_f1_plain_git";
    let git_repo = setup_git_repo(git_repo_name);
    let path_to_git_repo = generate_path_to_repo(git_repo_name);

    build_f1_scenario(&git_repo, &path_to_git_repo);

    // Rebase b1 with the exact command git-chain used, so both halves reach the same state
    // before the invocation under comparison.
    let merge_base_master_b1 = git_stdout(&path_to_git_repo, vec!["merge-base", "master", "b1"]);
    let b1_output = run_git_command(
        &path_to_git_repo,
        vec![
            "rebase",
            "--keep-empty",
            "--onto",
            "master",
            &merge_base_master_b1,
            "b1",
        ],
    );
    let b1_stderr = String::from_utf8_lossy(&b1_output.stderr).to_string();

    println!("=== F1 HALF B: preparing b1 exactly as git-chain does ===");
    println!("merge-base(master, b1): {}", merge_base_master_b1);
    println!("b1 rebase STDERR: {}", b1_stderr);
    println!("b1 rebase success: {}", b1_output.status.success());

    assert!(
        b1_output.status.success(),
        "rebasing b1 onto master should succeed, stderr: {}",
        b1_stderr
    );

    // The one and only difference from half A: `<upstream>` is the `b1` branch ref, so the
    // left side of `b1...b2` is non-empty and cherry-mark can do its job.
    let git_output = run_git_command(
        &path_to_git_repo,
        vec!["rebase", "--onto", "b1", "b1", "b2"],
    );
    let git_stdout_text = String::from_utf8_lossy(&git_output.stdout).to_string();
    let git_stderr_text = String::from_utf8_lossy(&git_output.stderr).to_string();

    let git_status_porcelain = git_stdout(&path_to_git_repo, vec!["status", "--porcelain"]);
    let git_rebase_dir_exists = path_to_git_repo.join(".git").join("rebase-merge").is_dir();
    let git_b2_log = git_stdout(&path_to_git_repo, vec!["log", "--oneline", "b2"]);

    println!("=== F1 HALF B: git rebase --onto b1 b1 b2 ===");
    println!("STDOUT: {}", git_stdout_text);
    println!("STDERR: {}", git_stderr_text);
    println!("EXIT STATUS success: {}", git_output.status.success());
    println!(
        "stderr mentions 'skipped previously applied commit': {}",
        git_stderr_text.contains("skipped previously applied commit")
    );
    println!(
        "stderr mentions 'Successfully rebased': {}",
        git_stderr_text.contains("Successfully rebased")
    );
    println!("git status --porcelain: {:?}", git_status_porcelain);
    println!(".git/rebase-merge exists: {}", git_rebase_dir_exists);
    println!("b2 log:\n{}", git_b2_log);
    println!(
        "OBSERVED: git skipped the already-applied commit and finished cleanly, on the same \
         input that made git chain rebase conflict above"
    );

    // Uncomment to stop test execution and debug half B
    // assert!(false, "DEBUG STOP: F1 half B (git's construction)");
    // assert!(false, "stdout: {}", git_stdout_text);
    // assert!(false, "stderr: {}", git_stderr_text);
    // assert!(false, "status code: {:?}", git_output.status.code());
    // assert!(false, "b2 log: {}", git_b2_log);

    assert!(
        git_output.status.success(),
        "plain git rebase should succeed on this input but exited {:?}.\nstdout: {}\nstderr: {}",
        git_output.status.code(),
        git_stdout_text,
        git_stderr_text
    );
    assert!(
        git_stderr_text.contains("skipped previously applied commit"),
        "git should report skipping the already-applied commit but stderr was: {}",
        git_stderr_text
    );
    assert!(
        git_stderr_text.contains("Successfully rebased"),
        "git should report a successful rebase but stderr was: {}",
        git_stderr_text
    );
    assert!(
        git_status_porcelain.is_empty(),
        "the working tree should be clean after git's rebase, got porcelain: {}",
        git_status_porcelain
    );
    assert!(
        !git_rebase_dir_exists,
        "no rebase should be left in progress after git's rebase, but .git/rebase-merge exists"
    );
    assert!(
        git_b2_log.contains("C(b2) modifies L1"),
        "b2 should still carry its own commit after the rebase, got log:\n{}",
        git_b2_log
    );
    assert!(
        git_b2_log.contains("D(b1) improves L8 further"),
        "b2 should be replanted on top of b1's new tip, got log:\n{}",
        git_b2_log
    );

    teardown_git_repo(chain_repo_name);
    teardown_git_repo(git_repo_name);
}

/// F2: `rebase.updateRefs=true` makes `git chain rebase` drag the backup branches forward.
///
/// `git chain backup` exists to preserve pre-rebase history. Because the rebase invocation never
/// passes `--no-update-refs`, git rewrites the `backup-<chain>/<branch>` refs along with the
/// branches they were meant to protect, so after the rebase they point at the *new* commits.
#[test]
fn audit_f2_update_refs_config_moves_backup_branches() {
    let repo_name = "audit_f2_update_refs";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // The config a stacked-branch user is likely to have enabled (git 2.38+).
    let config_output = run_git_command(&path_to_repo, vec!["config", "rebase.updateRefs", "true"]);
    assert!(
        config_output.status.success(),
        "setting rebase.updateRefs should succeed, stderr: {}",
        String::from_utf8_lossy(&config_output.stderr)
    );

    create_new_file(&path_to_repo, "base.txt", "base");
    first_commit_all(&repo, "base");

    create_branch(&repo, "b1");
    checkout_branch(&repo, "b1");
    create_new_file(&path_to_repo, "a.txt", "a");
    commit_all(&repo, "a(b1)");

    create_branch(&repo, "b2");
    checkout_branch(&repo, "b2");
    create_new_file(&path_to_repo, "b.txt", "b");
    commit_all(&repo, "b(b2)");

    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "audit_chain", "master", "b1", "b2"],
    );

    checkout_branch(&repo, "b1");
    let backup_output = run_test_bin_expect_ok(&path_to_repo, vec!["backup"]);
    let backup_stdout = String::from_utf8_lossy(&backup_output.stdout).to_string();

    let backup_b1_before = git_stdout(&path_to_repo, vec!["rev-parse", "backup-audit_chain/b1"]);
    let backup_b2_before = git_stdout(&path_to_repo, vec!["rev-parse", "backup-audit_chain/b2"]);
    let b1_before = git_stdout(&path_to_repo, vec!["rev-parse", "b1"]);
    let b2_before = git_stdout(&path_to_repo, vec!["rev-parse", "b2"]);

    println!("=== F2 SETUP ===");
    println!("backup stdout: {}", backup_stdout);
    println!(
        "backup stdout confirms the backup: {}",
        backup_stdout.contains("Successfully backed up chain")
    );
    println!("backup-audit_chain/b1 before: {}", backup_b1_before);
    println!("backup-audit_chain/b2 before: {}", backup_b2_before);
    println!("b1 before: {}", b1_before);
    println!("b2 before: {}", b2_before);

    assert!(
        backup_stdout.contains("Successfully backed up chain"),
        "git chain backup should confirm the backup but printed: {}",
        backup_stdout
    );
    // The backups start out where the branches are; that is what makes them backups.
    assert_eq!(
        backup_b1_before, b1_before,
        "backup-audit_chain/b1 should start at b1's tip"
    );
    assert_eq!(
        backup_b2_before, b2_before,
        "backup-audit_chain/b2 should start at b2's tip"
    );

    // Give the chain something to rebase onto.
    checkout_branch(&repo, "master");
    create_new_file(&path_to_repo, "m.txt", "m");
    commit_all(&repo, "master advances");
    checkout_branch(&repo, "b1");

    let rebase_output = run_test_bin_for_rebase(&path_to_repo, vec!["rebase"]);
    let rebase_stdout = String::from_utf8_lossy(&rebase_output.stdout).to_string();
    let rebase_stderr = String::from_utf8_lossy(&rebase_output.stderr).to_string();

    let backup_b1_after = git_stdout(&path_to_repo, vec!["rev-parse", "backup-audit_chain/b1"]);
    let backup_b2_after = git_stdout(&path_to_repo, vec!["rev-parse", "backup-audit_chain/b2"]);
    let b1_after = git_stdout(&path_to_repo, vec!["rev-parse", "b1"]);
    let b2_after = git_stdout(&path_to_repo, vec!["rev-parse", "b2"]);

    println!("=== F2 AFTER git chain rebase ===");
    println!("STDOUT: {}", rebase_stdout);
    println!("STDERR: {}", rebase_stderr);
    println!("EXIT STATUS success: {}", rebase_output.status.success());
    println!(
        "stderr mentions '--update-refs' rewriting: {}",
        rebase_stderr.contains("Updated the following refs with --update-refs")
    );
    println!(
        "b1 moved: {} ({} -> {})",
        b1_before != b1_after,
        b1_before,
        b1_after
    );
    println!(
        "b2 moved: {} ({} -> {})",
        b2_before != b2_after,
        b2_before,
        b2_after
    );
    println!(
        "backup-audit_chain/b1 moved: {} ({} -> {})",
        backup_b1_before != backup_b1_after,
        backup_b1_before,
        backup_b1_after
    );
    println!(
        "backup-audit_chain/b2 moved: {} ({} -> {})",
        backup_b2_before != backup_b2_after,
        backup_b2_before,
        backup_b2_after
    );
    println!(
        "backup-audit_chain/b1 now equals b1's new tip: {}",
        backup_b1_after == b1_after
    );
    println!(
        "backup-audit_chain/b2 now equals b2's new tip: {}",
        backup_b2_after == b2_after
    );
    println!(
        "EXPECTED (defect F2): the backup refs are dragged onto the rebased commits and no \
         longer preserve pre-rebase history"
    );

    // Uncomment to stop test execution and debug F2
    // assert!(false, "DEBUG STOP: F2 backup refs after rebase");
    // assert!(false, "stdout: {}", rebase_stdout);
    // assert!(false, "stderr: {}", rebase_stderr);
    // assert!(false, "backup b1: {} -> {}", backup_b1_before, backup_b1_after);
    // assert!(false, "backup b2: {} -> {}", backup_b2_before, backup_b2_after);

    assert!(
        rebase_output.status.success(),
        "git chain rebase should succeed here.\nstdout: {}\nstderr: {}",
        rebase_stdout,
        rebase_stderr
    );
    // git only prints this because git-chain let `rebase.updateRefs` through.
    // AFTER THE FIX (with --no-update-refs): assert!(!rebase_stderr.contains("--update-refs"), ...)
    assert!(
        rebase_stderr.contains("Updated the following refs with --update-refs"),
        "git should report rewriting extra refs but stderr was: {}",
        rebase_stderr
    );

    // Sanity: the chain branches themselves were rebased, so there was history to protect.
    assert_ne!(
        b1_before, b1_after,
        "b1 should have been rebased onto the advanced master"
    );
    assert_ne!(
        b2_before, b2_after,
        "b2 should have been rebased onto the new b1"
    );

    // The defect. AFTER THE FIX: assert_eq!(backup_b1_after, backup_b1_before, ...)
    assert_ne!(
        backup_b1_after, backup_b1_before,
        "DEFECT F2 no longer reproduces: backup-audit_chain/b1 still points at the pre-rebase \
         commit {}. If the fix landed, invert this test.",
        backup_b1_before
    );
    // AFTER THE FIX: assert_eq!(backup_b2_after, backup_b2_before, ...)
    assert_ne!(
        backup_b2_after, backup_b2_before,
        "DEFECT F2 no longer reproduces: backup-audit_chain/b2 still points at the pre-rebase \
         commit {}. If the fix landed, invert this test.",
        backup_b2_before
    );
    // AFTER THE FIX: assert_ne!(backup_b1_after, b1_after, ...)
    assert_eq!(
        backup_b1_after, b1_after,
        "backup-audit_chain/b1 should have been dragged onto b1's new tip, making it useless as \
         a backup. backup: {}, b1: {}",
        backup_b1_after, b1_after
    );
    // AFTER THE FIX: assert_ne!(backup_b2_after, b2_after, ...)
    assert_eq!(
        backup_b2_after, b2_after,
        "backup-audit_chain/b2 should have been dragged onto b2's new tip, making it useless as \
         a backup. backup: {}, b2: {}",
        backup_b2_after, b2_after
    );

    teardown_git_repo(repo_name);
}
