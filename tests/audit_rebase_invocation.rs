//! Characterization tests for the `git rebase` invocation that `git chain rebase` builds.
//!
//! # THESE TESTS NOW GUARD FIXED BEHAVIOR
//!
//! Each test here documents a HIGH severity finding from `REBASE_AUDIT.md` (repo root).
//! They began as *characterization* tests, pinning down what git-chain did while the bugs
//! were live; each was inverted when its fix landed, so it now fails if the behavior
//! regresses.
//!
//! Current status: **F1, F2, F3 and F4 are all FIXED**, and every test here guards a fix.
//!
//! ## F3 / F4 — the rest of the flag set is pinned
//!
//! `--empty=drop` replaced the vestigial `--keep-empty`, pinning the behavior chains rely on
//! (a commit that *becomes* empty because its change is already upstream is dropped) instead
//! of leaning on an unpinned default. Commits that *start* empty are governed by a different
//! knob and are still kept. `--no-rebase-merges` pins the flattening git-chain has always
//! relied on so `rebase.rebaseMerges=true` cannot change chain semantics mid-run.
//! `audit_f1_falls_back_to_the_frozen_fork_point` additionally guards the safety valve on
//! the F1 fix: when git's fork-point calculation is unavailable, the pre-computed SHA is
//! used and the replay window stays exactly what it always was.
//!
//! ## F1 — git's patch-id skipping is restored (FIXED)
//!
//! git-chain used to run `git rebase --onto <parent> <fork_point_sha> <branch>`.
//! Because the fork point is always an ancestor of `<branch>`, the left side of the `A...B`
//! symmetric difference that `sequencer_make_script` walks was empty, so `revs.cherry_mark`
//! could never mark a commit PATCHSAME. git itself never passes the fork point as
//! `<upstream>`: it layers it on as a negative ref (`^restrict_revision`) on top of the real
//! upstream (`builtin/rebase.c:296-311`).
//!
//! The invocation is now built by `chain_rebase_command`, which passes the parent *ref* as
//! `<upstream>` and adds `--fork-point` — restoring the skip while replaying the same window
//! — whenever git's fork-point calculation agrees with the SHA pre-computed before any branch
//! moved. When it disagrees or fails, the frozen SHA form is used unchanged.
//!
//! ## F2 — `rebase.updateRefs` is neutralised (FIXED)
//!
//! All three rebase invocations now run `git -c rebase.updateRefs=false rebase …`, so a user's
//! `rebase.updateRefs=true` no longer reaches them. (`-c` is used rather than
//! `--no-update-refs` because the flag errors on git < 2.38, while the config override is
//! honored by every version.) Before the fix, git dragged every other ref pointing into the
//! replayed range forward onto the rewritten commits — including the `backup-<chain>/<branch>`
//! refs that `git chain backup` had just created, which is precisely the safety net they exist
//! to provide.

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
    // (no "skipped previously applied commit" warning), and git-chain's own `--empty=drop`
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

/// F1 (FIXED): `git chain rebase` skips already-applied commits, exactly as plain git does.
///
/// Both halves of this test build a byte-identical scenario and rebase `b1` with the exact
/// command git-chain uses. The `<upstream>` argument of the `b2` rebase is what decides whether
/// the already-applied commit is silently skipped or blows up as a merge conflict: git-chain
/// used to pass the fork-point SHA, and now passes the parent branch ref with `--fork-point`,
/// which is git's own form. Half B remains the reference both halves are compared against.
#[test]
fn audit_f1_chain_rebase_skips_already_applied_commits() {
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
        "stdout shows the dedup form (parent ref as <upstream>): {}",
        chain_stdout.contains(
            "git -c rebase.updateRefs=false rebase --empty=drop --no-rebase-merges --fork-point \
             --onto b1 b1 b2"
        )
    );
    println!(
        "stderr reports skipping the already-applied commit: {}",
        chain_stderr.contains("skipped previously applied commit")
    );
    println!("git status --porcelain: {}", chain_status_porcelain);
    println!(
        "porcelain reports an unmerged path (UU): {}",
        chain_status_porcelain.contains("UU ")
    );
    println!(".git/rebase-merge exists: {}", chain_rebase_dir_exists);
    println!("EXPECTED (F1 fixed): chain rebase succeeds, skipping the duplicate commit");

    // Uncomment to stop test execution and debug half A
    // assert!(false, "DEBUG STOP: F1 half A (git-chain construction)");
    // assert!(false, "stdout: {}", chain_stdout);
    // assert!(false, "stderr: {}", chain_stderr);
    // assert!(false, "status code: {:?}", chain_output.status.code());
    // assert!(false, "porcelain: {}", chain_status_porcelain);

    assert!(
        chain_output.status.success(),
        "git chain rebase should succeed now that the duplicate commit is skipped.\nstdout: \
         {}\nstderr: {}",
        chain_stdout,
        chain_stderr
    );
    assert!(
        !chain_stderr.contains("Unable to completely rebase"),
        "stderr should not report a failed chain rebase but got: {}",
        chain_stderr
    );
    // The invocation itself: `<upstream>` is the `b1` branch ref, which is what lets git mark
    // the duplicate PATCHSAME, with `--fork-point` keeping the replay window unchanged.
    assert!(
        chain_stdout.contains(
            "git -c rebase.updateRefs=false rebase --empty=drop --no-rebase-merges --fork-point \
             --onto b1 b1 b2"
        ),
        "stdout should echo the dedup rebase invocation for b2 but got: {}",
        chain_stdout
    );
    // git's own report that the patch-id match was found and skipped.
    assert!(
        chain_stderr.contains("skipped previously applied commit"),
        "stderr should report skipping the already-applied commit but got: {}",
        chain_stderr
    );
    assert!(
        chain_stderr.contains("Successfully rebased"),
        "stderr should report a successful rebase but got: {}",
        chain_stderr
    );
    assert!(
        !chain_status_porcelain.contains("UU "),
        "b2 should not be left with an unmerged path, got porcelain: {}",
        chain_status_porcelain
    );
    assert!(
        chain_status_porcelain.is_empty(),
        "the working tree should be clean after the rebase, got porcelain: {}",
        chain_status_porcelain
    );
    assert!(
        !chain_rebase_dir_exists,
        "no rebase should be left in progress, but .git/rebase-merge exists. porcelain: {}",
        chain_status_porcelain
    );

    // The topology matches what plain git produces in half B: b2's own commit replanted on
    // top of the parent's refined fix.
    let chain_b2_log = git_stdout(
        &path_to_chain_repo,
        vec!["log", "--oneline", "--format=%s", "b2"],
    );
    println!("chain b2 log:\n{}", chain_b2_log);
    assert!(
        chain_b2_log.contains("C(b2) modifies L1"),
        "b2 should still carry its own commit after the rebase, got log:\n{}",
        chain_b2_log
    );
    assert!(
        chain_b2_log.contains("D(b1) improves L8"),
        "b2 should be replanted on top of b1's new tip, got log:\n{}",
        chain_b2_log
    );
    assert!(
        !chain_b2_log.contains("B(b2) fixes L8"),
        "b2's duplicate of the L8 fix should have been skipped, got log:\n{}",
        chain_b2_log
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
            "--empty=drop",
            "--no-rebase-merges",
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

/// F2 (FIXED): `rebase.updateRefs=true` must not drag the backup branches forward.
///
/// `git chain backup` exists to preserve pre-rebase history. The three rebase invocations run
/// `git -c rebase.updateRefs=false rebase …`, so git leaves the `backup-<chain>/<branch>` refs
/// on the pre-rebase commits even when the user has `rebase.updateRefs=true` configured. This
/// test fails if that isolation regresses and the backups are rewritten along with the branches
/// they exist to protect.
#[test]
fn audit_f2_update_refs_config_does_not_move_backup_branches() {
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
        "backup-audit_chain/b1 still records b1's pre-rebase tip: {}",
        backup_b1_after == b1_before
    );
    println!(
        "backup-audit_chain/b2 still records b2's pre-rebase tip: {}",
        backup_b2_after == b2_before
    );
    println!(
        "EXPECTED (F2 fixed): the backup refs stay on the pre-rebase commits and still preserve \
         pre-rebase history"
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
    // git prints this only when it rewrites extra refs, which the `-c rebase.updateRefs=false`
    // prefix prevents.
    assert!(
        !rebase_stderr.contains("Updated the following refs with --update-refs"),
        "git should not have rewritten any extra refs, but stderr reports that it did: {}",
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

    // The fix: the backups stay exactly where `git chain backup` put them.
    assert_eq!(
        backup_b1_after, backup_b1_before,
        "backup-audit_chain/b1 should still point at the pre-rebase commit {} but moved to {}",
        backup_b1_before, backup_b1_after
    );
    assert_eq!(
        backup_b2_after, backup_b2_before,
        "backup-audit_chain/b2 should still point at the pre-rebase commit {} but moved to {}",
        backup_b2_before, backup_b2_after
    );
    // Stated against the branches themselves: the backups still record the pre-rebase tips.
    assert_eq!(
        backup_b1_after, b1_before,
        "backup-audit_chain/b1 should still record b1's pre-rebase tip {}, got {}",
        b1_before, backup_b1_after
    );
    assert_eq!(
        backup_b2_after, b2_before,
        "backup-audit_chain/b2 should still record b2's pre-rebase tip {}, got {}",
        b2_before, backup_b2_after
    );
    // And so they are no longer interchangeable with the rebased branches.
    assert_ne!(
        backup_b1_after, b1_after,
        "backup-audit_chain/b1 should not have been dragged onto b1's new tip {}",
        b1_after
    );
    assert_ne!(
        backup_b2_after, b2_after,
        "backup-audit_chain/b2 should not have been dragged onto b2's new tip {}",
        b2_after
    );

    teardown_git_repo(repo_name);
}

/// Extract the `git rebase` invocation git-chain echoed for `branch`.
///
/// Returns the single line ending in that branch name, so each branch's form can be asserted
/// independently — a chain rebase echoes one command per branch and they need not agree.
fn echoed_rebase_command(stdout: &str, branch: &str) -> String {
    stdout
        .lines()
        .find(|line| {
            line.starts_with("git -c rebase.updateRefs=false rebase") && line.ends_with(branch)
        })
        .unwrap_or_default()
        .to_string()
}

/// The safety valve on the F1 fix: when git's fork-point calculation disagrees with the
/// pre-computed SHA — or cannot run at all — the frozen SHA form is used unchanged.
///
/// `--fork-point` reads the parent's reflog, which can be expired or rewritten. The
/// pre-computation in `rebase()` does not, and it stays the source of truth: the replayed
/// window must never change because a reflog happened to be missing.
///
/// The scenario rewrites `some_branch_1`'s commit so `some_branch_2`'s recorded base is no
/// longer reachable from it, then expires every reflog. `merge-base --fork-point
/// some_branch_1 some_branch_2` then fails outright. It also exercises the *other* side of
/// the guard in the same run: by the time the chain rebase starts, `master` has a fresh
/// reflog entry whose fork point does agree, so that branch takes the dedup form.
#[test]
fn audit_f1_falls_back_to_the_frozen_fork_point() {
    let repo_name = "audit_f1_frozen_fallback";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    create_new_file(&path_to_repo, "hello.txt", "hello");
    first_commit_all(&repo, "A");

    create_branch(&repo, "some_branch_1");
    checkout_branch(&repo, "some_branch_1");
    create_new_file(&path_to_repo, "b.txt", "b");
    commit_all(&repo, "B on some_branch_1");

    create_branch(&repo, "some_branch_2");
    checkout_branch(&repo, "some_branch_2");
    create_new_file(&path_to_repo, "c.txt", "c");
    commit_all(&repo, "C on some_branch_2");

    run_test_bin_expect_ok(
        &path_to_repo,
        vec![
            "setup",
            "audit_chain",
            "master",
            "some_branch_1",
            "some_branch_2",
        ],
    );

    // Rewrite some_branch_1's commit. The tree is unchanged (message-only amend), but the
    // commit some_branch_2 sits on is no longer reachable from its parent — which is exactly
    // the situation `--fork-point` exists to resolve, and the reflog is how it resolves it.
    checkout_branch(&repo, "some_branch_1");
    let amend_output = run_git_command(
        &path_to_repo,
        vec!["commit", "--amend", "-m", "B-prime on some_branch_1"],
    );
    assert!(
        amend_output.status.success(),
        "amending the commit should succeed, stderr: {}",
        String::from_utf8_lossy(&amend_output.stderr)
    );

    // Take the reflog away, so the fork-point calculation has nothing to work with.
    let expire_output = run_git_command(
        &path_to_repo,
        vec!["reflog", "expire", "--expire=all", "--all"],
    );
    assert!(
        expire_output.status.success(),
        "expiring the reflogs should succeed, stderr: {}",
        String::from_utf8_lossy(&expire_output.stderr)
    );

    checkout_branch(&repo, "master");
    create_new_file(&path_to_repo, "m.txt", "m");
    commit_all(&repo, "master advances");
    checkout_branch(&repo, "some_branch_1");

    // Precondition: git really cannot compute a fork point for the child branch here.
    let fork_point_probe = run_git_command(
        &path_to_repo,
        vec![
            "merge-base",
            "--fork-point",
            "some_branch_1",
            "some_branch_2",
        ],
    );

    let rebase_output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let rebase_stdout = String::from_utf8_lossy(&rebase_output.stdout).to_string();
    let rebase_stderr = String::from_utf8_lossy(&rebase_output.stderr).to_string();

    let branch_1_command = echoed_rebase_command(&rebase_stdout, "some_branch_1");
    let branch_2_command = echoed_rebase_command(&rebase_stdout, "some_branch_2");

    let porcelain = git_stdout(&path_to_repo, vec!["status", "--porcelain"]);
    let branch_2_log = git_stdout(
        &path_to_repo,
        vec!["log", "--oneline", "--format=%s", "some_branch_2"],
    );

    println!("=== F1 FALLBACK DIAGNOSTICS ===");
    println!(
        "merge-base --fork-point some_branch_1 some_branch_2 succeeds: {}",
        fork_point_probe.status.success()
    );
    println!("REBASE STDOUT: {}", rebase_stdout);
    println!("REBASE STDERR: {}", rebase_stderr);
    println!("EXIT SUCCESS: {}", rebase_output.status.success());
    println!("some_branch_1 command: {}", branch_1_command);
    println!("some_branch_2 command: {}", branch_2_command);
    println!(
        "some_branch_2 used the frozen form (no --fork-point): {}",
        !branch_2_command.contains("--fork-point")
    );
    println!(
        "some_branch_1 used the dedup form: {}",
        branch_1_command.contains("--fork-point")
    );
    println!("git status --porcelain: {}", porcelain);
    println!("some_branch_2 log:\n{}", branch_2_log);
    println!("EXPECTED: the child falls back to the frozen SHA and the rebase still succeeds");
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: F1 frozen fallback");
    // assert!(false, "stdout: {}", rebase_stdout);
    // assert!(false, "branch_2 command: {}", branch_2_command);

    assert!(
        !fork_point_probe.status.success(),
        "the scenario requires that git cannot compute a fork point for some_branch_2, but it \
         returned: {}",
        String::from_utf8_lossy(&fork_point_probe.stdout)
    );
    assert!(
        rebase_output.status.success(),
        "the chain rebase should still succeed via the frozen SHA.\nstdout: {}\nstderr: {}",
        rebase_stdout,
        rebase_stderr
    );

    // The child branch falls back: the parent ref is the `--onto` target, and the frozen SHA
    // is `<upstream>` — no `--fork-point` anywhere, because git could not supply one.
    assert!(
        !branch_2_command.is_empty(),
        "git-chain should echo a rebase command for some_branch_2, got stdout: {}",
        rebase_stdout
    );
    assert!(
        !branch_2_command.contains("--fork-point"),
        "some_branch_2 should use the frozen form, but its command was: {}",
        branch_2_command
    );
    assert!(
        branch_2_command.contains("--empty=drop --no-rebase-merges --onto some_branch_1 "),
        "some_branch_2 should be rebased onto its parent with the frozen SHA as <upstream>, but \
         its command was: {}",
        branch_2_command
    );

    // And the guard is per-branch, not global: master's reflog was rebuilt by the commit
    // above, so its fork point agrees and that branch takes the dedup form.
    assert!(
        branch_1_command.contains("--fork-point --onto master master some_branch_1"),
        "some_branch_1 should use the dedup form, but its command was: {}",
        branch_1_command
    );

    // Topology: the chain is consistent and the duplicate of the amended commit is gone.
    assert!(
        porcelain.is_empty(),
        "the working tree should be clean after the rebase, got porcelain: {}",
        porcelain
    );
    assert!(
        branch_2_log.contains("C on some_branch_2"),
        "some_branch_2 should still carry its own commit, got log:\n{}",
        branch_2_log
    );
    assert!(
        branch_2_log.contains("B-prime on some_branch_1"),
        "some_branch_2 should be replanted on the rewritten parent commit, got log:\n{}",
        branch_2_log
    );
    assert!(
        branch_2_log.contains("master advances"),
        "some_branch_2 should contain master's new commit, got log:\n{}",
        branch_2_log
    );
    assert!(
        !branch_2_log.contains("B on some_branch_1"),
        "the superseded copy of the parent's commit should have been dropped, got log:\n{}",
        branch_2_log
    );

    teardown_git_repo(repo_name);
}

/// F3: the two kinds of "empty" are governed by different flags, and the flag set encodes
/// both — a commit that BECOMES empty is dropped, one that STARTS empty is kept.
///
/// `--empty=drop` controls only the former. git-rebase.adoc is explicit: "commits which
/// start empty are kept (unless `--no-keep-empty` is specified)", and `keep_empty` defaults
/// to 1 (`builtin/rebase.c:143`). That is why replacing the vestigial `--keep-empty` with
/// `--empty=drop` preserves behavior rather than changing it — and this test pins both
/// halves of the distinction at once, so a future flag edit cannot quietly break either.
#[test]
fn audit_f3_keeps_start_empty_commits_and_drops_become_empty_ones() {
    let repo_name = "audit_f3_empty_commit_handling";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    create_new_file(&path_to_repo, "hello.txt", "hello");
    first_commit_all(&repo, "base");

    // b1 carries a change that master will later absorb, so b2's copy of it becomes empty.
    create_branch(&repo, "b1");
    checkout_branch(&repo, "b1");
    create_new_file(&path_to_repo, "shared.txt", "shared content");
    commit_all(&repo, "SHARED change on b1");

    create_branch(&repo, "b2");
    checkout_branch(&repo, "b2");
    create_new_file(&path_to_repo, "own.txt", "own");
    commit_all(&repo, "OWN change on b2");

    // A deliberately empty commit: it starts empty, so it must survive the rebase.
    let empty_output = run_git_command(
        &path_to_repo,
        vec!["commit", "--allow-empty", "-m", "DELIBERATELY EMPTY on b2"],
    );
    assert!(
        empty_output.status.success(),
        "creating the empty commit should succeed, stderr: {}",
        String::from_utf8_lossy(&empty_output.stderr)
    );

    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "audit_chain", "master", "b1", "b2"],
    );

    // Squash-merge b1 into master: b1's change is now upstream, so replaying b2's inherited
    // copy of it produces nothing.
    checkout_branch(&repo, "master");
    let merge_output = run_git_command(&path_to_repo, vec!["merge", "--squash", "b1"]);
    assert!(
        merge_output.status.success(),
        "the squash merge should succeed, stderr: {}",
        String::from_utf8_lossy(&merge_output.stderr)
    );
    commit_all(&repo, "squash merge of b1 into master");

    checkout_branch(&repo, "b1");

    let rebase_output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let rebase_stdout = String::from_utf8_lossy(&rebase_output.stdout).to_string();
    let rebase_stderr = String::from_utf8_lossy(&rebase_output.stderr).to_string();

    let b2_log = git_stdout(&path_to_repo, vec!["log", "--format=%s", "b2"]);

    println!("=== F3 DIAGNOSTICS ===");
    println!("REBASE STDOUT: {}", rebase_stdout);
    println!("REBASE STDERR: {}", rebase_stderr);
    println!("EXIT SUCCESS: {}", rebase_output.status.success());
    println!("b2 log:\n{}", b2_log);
    println!(
        "start-empty commit survived: {}",
        b2_log.contains("DELIBERATELY EMPTY on b2")
    );
    println!(
        "echoed command carries --empty=drop: {}",
        rebase_stdout.contains("--empty=drop")
    );
    println!(
        "echoed command carries --no-rebase-merges: {}",
        rebase_stdout.contains("--no-rebase-merges")
    );
    println!("EXPECTED: start-empty kept, become-empty dropped");
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: F3 empty handling");
    // assert!(false, "stdout: {}", rebase_stdout);
    // assert!(false, "b2 log: {}", b2_log);

    assert!(
        rebase_output.status.success(),
        "the chain rebase should succeed.\nstdout: {}\nstderr: {}",
        rebase_stdout,
        rebase_stderr
    );
    // The flag set itself, echoed by git-chain.
    assert!(
        rebase_stdout.contains("--empty=drop"),
        "the rebase invocation should pin --empty=drop, got: {}",
        rebase_stdout
    );
    assert!(
        rebase_stdout.contains("--no-rebase-merges"),
        "the rebase invocation should pin --no-rebase-merges, got: {}",
        rebase_stdout
    );
    // Starts empty -> kept.
    assert!(
        b2_log.contains("DELIBERATELY EMPTY on b2"),
        "a commit that starts empty must survive the rebase, got b2 log:\n{}",
        b2_log
    );
    // b2's own real work is untouched.
    assert!(
        b2_log.contains("OWN change on b2"),
        "b2 should still carry its own commit, got b2 log:\n{}",
        b2_log
    );
    // Becomes empty -> dropped. b2 inherited b1's change, which master now already has.
    assert_eq!(
        b2_log.matches("SHARED change on b1").count(),
        0,
        "the inherited change is already upstream and must be dropped, got b2 log:\n{}",
        b2_log
    );

    teardown_git_repo(repo_name);
}

/// F4: `rebase.rebaseMerges=true` must not change chain-rebase semantics.
///
/// Without `--no-rebase-merges`, that config makes git preserve merge topology inside a
/// chain branch — silently giving a different result than the same command run by a user
/// without the config. git-chain has always flattened; the flag pins that default so the
/// outcome is a property of git-chain rather than of the user's config, the same
/// isolation principle as `-c rebase.updateRefs=false` (REBASE_AUDIT.md F4).
#[test]
fn audit_f4_rebase_merges_config_cannot_change_chain_semantics() {
    let repo_name = "audit_f4_rebase_merges_pinned";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // The config a merge-preserving user is likely to have enabled (git 2.41+).
    let config_output =
        run_git_command(&path_to_repo, vec!["config", "rebase.rebaseMerges", "true"]);
    assert!(
        config_output.status.success(),
        "setting rebase.rebaseMerges should succeed, stderr: {}",
        String::from_utf8_lossy(&config_output.stderr)
    );

    create_new_file(&path_to_repo, "hello.txt", "hello");
    first_commit_all(&repo, "base");

    create_branch(&repo, "b1");
    checkout_branch(&repo, "b1");
    create_new_file(&path_to_repo, "f1.txt", "contents 1");
    commit_all(&repo, "c1 on b1");

    // Build a real merge commit inside b1, which is what the config would preserve.
    create_branch(&repo, "side");
    checkout_branch(&repo, "side");
    create_new_file(&path_to_repo, "side.txt", "side");
    commit_all(&repo, "commit on side");

    checkout_branch(&repo, "b1");
    let merge_output = run_git_command(
        &path_to_repo,
        vec!["merge", "--no-ff", "-m", "MERGE side into b1", "side"],
    );
    assert!(
        merge_output.status.success(),
        "creating the merge commit should succeed, stderr: {}",
        String::from_utf8_lossy(&merge_output.stderr)
    );

    create_branch(&repo, "b2");
    checkout_branch(&repo, "b2");
    create_new_file(&path_to_repo, "f2.txt", "contents 2");
    commit_all(&repo, "c2 on b2");

    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "audit_chain", "master", "b1", "b2"],
    );

    checkout_branch(&repo, "master");
    create_new_file(&path_to_repo, "m.txt", "m");
    commit_all(&repo, "master advances");
    checkout_branch(&repo, "b1");

    let merges_before = git_stdout(
        &path_to_repo,
        vec!["rev-list", "--merges", "--count", "master..b1"],
    );

    let rebase_output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let rebase_stdout = String::from_utf8_lossy(&rebase_output.stdout).to_string();
    let rebase_stderr = String::from_utf8_lossy(&rebase_output.stderr).to_string();

    let merges_after = git_stdout(
        &path_to_repo,
        vec!["rev-list", "--merges", "--count", "master..b1"],
    );
    let b1_log = git_stdout(&path_to_repo, vec!["log", "--format=%s", "b1"]);

    println!("=== F4 DIAGNOSTICS ===");
    println!("rebase.rebaseMerges is set to true in this repo");
    println!("REBASE STDOUT: {}", rebase_stdout);
    println!("REBASE STDERR: {}", rebase_stderr);
    println!("EXIT SUCCESS: {}", rebase_output.status.success());
    println!("merge commits in master..b1 before: {}", merges_before);
    println!("merge commits in master..b1 after:  {}", merges_after);
    println!(
        "echoed command carries --no-rebase-merges: {}",
        rebase_stdout.contains("--no-rebase-merges")
    );
    println!("b1 log:\n{}", b1_log);
    println!("EXPECTED: the merge is flattened despite the config");
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: F4 rebaseMerges pinned");
    // assert!(false, "stdout: {}", rebase_stdout);
    // assert!(false, "merges before={} after={}", merges_before, merges_after);

    // Precondition: there really was a merge to preserve.
    assert_eq!(
        merges_before, "1",
        "the scenario requires exactly one merge commit in b1, got: {}",
        merges_before
    );
    assert!(
        rebase_output.status.success(),
        "the chain rebase should succeed despite the config.\nstdout: {}\nstderr: {}",
        rebase_stdout,
        rebase_stderr
    );
    assert!(
        rebase_stdout.contains("--no-rebase-merges"),
        "the rebase invocation should pin --no-rebase-merges, got: {}",
        rebase_stdout
    );
    // The pinned default wins over the config: the topology is flattened.
    assert_eq!(
        merges_after, "0",
        "the merge commit should have been flattened, but master..b1 still has {} merge(s)",
        merges_after
    );
    // Flattening keeps the content: the side branch's commit is replayed inline.
    assert!(
        b1_log.contains("commit on side"),
        "the merged-in commit should be replayed onto b1, got b1 log:\n{}",
        b1_log
    );
    assert!(
        b1_log.contains("master advances"),
        "b1 should be replanted on the advanced master, got b1 log:\n{}",
        b1_log
    );

    teardown_git_repo(repo_name);
}
