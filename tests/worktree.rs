#[path = "common/mod.rs"]
pub mod common;

use std::fs;

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, get_current_branch_name, run_git_command, run_test_bin_expect_err,
    run_test_bin_expect_ok, setup_git_repo, teardown_git_repo,
};

// Regression tests for https://github.com/dashed/git-chain/issues/48
//
// `git chain rebase` (and any command that switches branches) used to panic —
// and leave the working tree mutated with HEAD unmoved — when the target branch
// was checked out in another linked worktree. It should instead fail with an
// actionable error, leaving the repository untouched.

#[test]
fn rebase_stops_gracefully_when_chain_branch_in_other_worktree() {
    let repo_name = "rebase_worktree_occupied";
    let worktree_dir = generate_path_to_repo(format!("{}_worktree", repo_name));
    fs::remove_dir_all(&worktree_dir).ok();

    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    // create and checkout new branch named feature-1
    {
        let branch_name = "feature-1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);

        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "message");
    };

    // create and checkout new branch named feature-2
    {
        let branch_name = "feature-2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);

        create_new_file(&path_to_repo, "file_2.txt", "contents 2");
        commit_all(&repo, "message");
    };

    // run git chain setup
    let args: Vec<&str> = vec!["setup", "chain_name", "master", "feature-1", "feature-2"];
    run_test_bin_expect_ok(&path_to_repo, args);

    // Advance master so the rebase actually has work to do.
    checkout_branch(&repo, "master");
    create_new_file(&path_to_repo, "new_base.txt", "new base");
    commit_all(&repo, "new base");

    // Start the rebase from feature-1 so the main worktree does not hold feature-2.
    checkout_branch(&repo, "feature-1");

    // Occupy feature-2 in a separate linked worktree.
    let worktree_output = run_git_command(
        &path_to_repo,
        vec![
            "worktree",
            "add",
            &format!("../{}_worktree", repo_name),
            "feature-2",
        ],
    );
    assert!(
        worktree_output.status.success(),
        "git worktree add should succeed but got: {}",
        String::from_utf8_lossy(&worktree_output.stderr)
    );

    // feature-1 rebases before feature-2 in chain order, so if the rebase loop had started
    // at all it would have rewritten feature-1 before reaching the occupied branch.
    let feature_1_before = run_git_command(&path_to_repo, vec!["rev-parse", "feature-1"]);
    let feature_1_oid_before = String::from_utf8_lossy(&feature_1_before.stdout)
        .trim()
        .to_string();

    // git chain rebase
    let args: Vec<&str> = vec!["rebase"];
    let output = run_test_bin_expect_err(&path_to_repo, args);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let feature_1_after = run_git_command(&path_to_repo, vec!["rev-parse", "feature-1"]);
    let feature_1_oid_after = String::from_utf8_lossy(&feature_1_after.stdout)
        .trim()
        .to_string();

    let state_file = path_to_repo.join(".git/chain-rebase-state.json");

    let status_output = run_git_command(&path_to_repo, vec!["status", "--porcelain"]);
    let status_stdout = String::from_utf8_lossy(&status_output.stdout).to_string();

    // Diagnostic printing
    println!("=== TEST DIAGNOSTICS ===");
    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status.code().unwrap_or(0));
    println!("git status --porcelain: {}", status_stdout);
    println!("Current branch: {}", get_current_branch_name(&repo));
    println!(
        "feature-1: before={} after={} (rebased: {})",
        feature_1_oid_before,
        feature_1_oid_after,
        feature_1_oid_before != feature_1_oid_after
    );
    println!("STATE FILE EXISTS: {}", state_file.exists());
    println!(
        "error names the occupied branch: {}",
        stderr.contains("feature-2")
    );
    println!("EXPECTED: the pre-flight rejects the rebase before any branch is touched");
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: rebase with worktree-occupied branch");
    // assert!(false, "stdout: {}", stdout);
    // assert!(false, "stderr: {}", stderr);
    // assert!(false, "status code: {}", output.status.code().unwrap_or(0));

    assert!(
        !output.status.success(),
        "Rebase should fail when a chain branch is held by another worktree"
    );
    assert!(
        stderr.contains("feature-2"),
        "stderr should name the occupied branch but got: {}",
        stderr
    );
    assert!(
        stderr.contains("checked out in another worktree"),
        "stderr should explain the worktree conflict but got: {}",
        stderr
    );
    assert!(
        stderr.contains("rebase_worktree_occupied_worktree"),
        "stderr should name the offending worktree path but got: {}",
        stderr
    );
    assert!(
        !stderr.contains("panicked"),
        "git-chain should not panic but got: {}",
        stderr
    );
    // The pre-flight runs before any state is written, so there is nothing to recover from
    // and "then retry" is honest advice.
    assert!(
        !state_file.exists(),
        "no chain rebase state should be written when the pre-flight rejects the run, but {} \
         exists",
        state_file.display()
    );
    // Nothing was rebased: the failure happens before the loop starts.
    assert_eq!(
        feature_1_oid_after, feature_1_oid_before,
        "feature-1 should not have been rebased before the pre-flight rejected the run, but it \
         moved from {} to {}",
        feature_1_oid_before, feature_1_oid_after
    );
    assert!(
        status_stdout.is_empty(),
        "working tree should stay clean (no phantom changes) but git status shows: {}",
        status_stdout
    );
    assert_eq!(
        &get_current_branch_name(&repo),
        "feature-1",
        "HEAD should remain on the original branch"
    );

    fs::remove_dir_all(&worktree_dir).ok();
    teardown_git_repo(repo_name);
}

#[test]
fn navigation_fails_gracefully_when_branch_in_other_worktree() {
    let repo_name = "navigation_worktree_occupied";
    let worktree_dir = generate_path_to_repo(format!("{}_worktree", repo_name));
    fs::remove_dir_all(&worktree_dir).ok();

    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    // create and checkout new branch named feature-1
    {
        let branch_name = "feature-1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);

        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "message");
    };

    // create and checkout new branch named feature-2
    {
        let branch_name = "feature-2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);

        create_new_file(&path_to_repo, "file_2.txt", "contents 2");
        commit_all(&repo, "message");
    };

    // run git chain setup
    let args: Vec<&str> = vec!["setup", "chain_name", "master", "feature-1", "feature-2"];
    run_test_bin_expect_ok(&path_to_repo, args);

    // Occupy feature-1 in a separate linked worktree (the main worktree is on feature-2).
    let worktree_output = run_git_command(
        &path_to_repo,
        vec![
            "worktree",
            "add",
            &format!("../{}_worktree", repo_name),
            "feature-1",
        ],
    );
    assert!(
        worktree_output.status.success(),
        "git worktree add should succeed but got: {}",
        String::from_utf8_lossy(&worktree_output.stderr)
    );

    // git chain first — must check out feature-1, which is held by the worktree.
    let args: Vec<&str> = vec!["first"];
    let output = run_test_bin_expect_err(&path_to_repo, args);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let status_output = run_git_command(&path_to_repo, vec!["status", "--porcelain"]);
    let status_stdout = String::from_utf8_lossy(&status_output.stdout).to_string();

    // Diagnostic printing
    println!("=== TEST DIAGNOSTICS ===");
    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status.code().unwrap_or(0));
    println!("git status --porcelain: {}", status_stdout);
    println!("Current branch: {}", get_current_branch_name(&repo));
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: git chain first with worktree-occupied branch");
    // assert!(false, "stdout: {}", stdout);
    // assert!(false, "stderr: {}", stderr);
    // assert!(false, "status code: {}", output.status.code().unwrap_or(0));

    assert!(
        !output.status.success(),
        "git chain first should fail when the target branch is held by another worktree"
    );
    assert!(
        stderr.contains("Cannot check out branch 'feature-1'"),
        "stderr should name the occupied branch but got: {}",
        stderr
    );
    assert!(
        stderr.contains("checked out in another worktree"),
        "stderr should explain the worktree conflict but got: {}",
        stderr
    );
    assert!(
        stderr.contains("navigation_worktree_occupied_worktree"),
        "stderr should name the offending worktree path but got: {}",
        stderr
    );
    assert!(
        !stderr.contains("panicked"),
        "git-chain should not panic but got: {}",
        stderr
    );
    assert!(
        status_stdout.is_empty(),
        "working tree should stay clean (no phantom changes) but git status shows: {}",
        status_stdout
    );
    assert_eq!(
        &get_current_branch_name(&repo),
        "feature-2",
        "HEAD should remain on the original branch"
    );

    fs::remove_dir_all(&worktree_dir).ok();
    teardown_git_repo(repo_name);
}

#[test]
fn checkout_succeeds_when_worktree_holds_unrelated_branch() {
    // Guard against false positives: a linked worktree holding an unrelated
    // branch must not block checking out chain branches, and the current
    // worktree holding the target branch itself (self) must not be flagged.
    let repo_name = "worktree_unrelated_branch";
    let worktree_dir = generate_path_to_repo(format!("{}_worktree", repo_name));
    fs::remove_dir_all(&worktree_dir).ok();

    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    // create branch named unrelated (not part of the chain, left unchecked-out)
    create_branch(&repo, "unrelated");

    // create and checkout new branch named feature-1
    {
        let branch_name = "feature-1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);

        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "message");
    };

    // create and checkout new branch named feature-2
    {
        let branch_name = "feature-2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);

        create_new_file(&path_to_repo, "file_2.txt", "contents 2");
        commit_all(&repo, "message");
    };

    // run git chain setup
    let args: Vec<&str> = vec!["setup", "chain_name", "master", "feature-1", "feature-2"];
    run_test_bin_expect_ok(&path_to_repo, args);

    // A linked worktree holding a branch that is NOT the checkout target.
    let worktree_output = run_git_command(
        &path_to_repo,
        vec![
            "worktree",
            "add",
            &format!("../{}_worktree", repo_name),
            "unrelated",
        ],
    );
    assert!(
        worktree_output.status.success(),
        "git worktree add should succeed but got: {}",
        String::from_utf8_lossy(&worktree_output.stderr)
    );

    // git chain first — checking out feature-1 must succeed.
    let args: Vec<&str> = vec!["first"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    // Diagnostic printing
    println!("=== TEST DIAGNOSTICS (first run) ===");
    println!("STDOUT: {}", stdout);
    println!("EXIT STATUS: {}", output.status.code().unwrap_or(0));
    println!("Current branch: {}", get_current_branch_name(&repo));
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: git chain first with unrelated worktree");
    // assert!(false, "stdout: {}", stdout);

    assert!(
        output.status.success(),
        "git chain first should succeed when no worktree holds the target branch"
    );
    assert!(
        stdout.contains("Switched to branch: feature-1"),
        "stdout should confirm the branch switch but got: {}",
        stdout
    );
    assert_eq!(
        &get_current_branch_name(&repo),
        "feature-1",
        "HEAD should now be on feature-1"
    );

    // Running `first` again while already on feature-1 must stay a no-op
    // success with the linked worktree present.
    let args: Vec<&str> = vec!["first"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    // Diagnostic printing
    println!("=== TEST DIAGNOSTICS (second run) ===");
    println!("STDOUT: {}", stdout);
    println!("EXIT STATUS: {}", output.status.code().unwrap_or(0));
    println!("Current branch: {}", get_current_branch_name(&repo));
    println!("======");

    assert!(
        output.status.success(),
        "git chain first should succeed when already on the first branch"
    );
    assert!(
        stdout.contains("Already on the first branch of the chain"),
        "stdout should report already being on the first branch but got: {}",
        stdout
    );
    assert_eq!(
        &get_current_branch_name(&repo),
        "feature-1",
        "HEAD should remain on feature-1"
    );

    fs::remove_dir_all(&worktree_dir).ok();
    teardown_git_repo(repo_name);
}

/// A worktree part-way through a rebase still holds its branch (REBASE_AUDIT M8).
///
/// Mid-rebase the worktree's HEAD is detached, so a check that only reads HEAD sees nothing
/// and git-chain used to switch to the branch anyway — while git itself refuses, because it
/// also consults the rebase state (`is_worktree_being_rebased`, worktree.c). Both the
/// navigation commands and the chain-rebase pre-flight must now refuse.
#[test]
fn commands_refuse_a_branch_being_rebased_in_another_worktree() {
    let repo_name = "worktree_mid_rebase_holds_branch";
    let worktree_dir = generate_path_to_repo(format!("{}_worktree", repo_name));
    fs::remove_dir_all(&worktree_dir).ok();

    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");
    first_commit_all(&repo, "first commit");

    create_branch(&repo, "feature-1");
    checkout_branch(&repo, "feature-1");
    create_new_file(&path_to_repo, "file_1.txt", "contents 1");
    commit_all(&repo, "commit on feature-1");

    // feature-2 collides with master, so a rebase started inside the worktree stops there.
    create_branch(&repo, "feature-2");
    checkout_branch(&repo, "feature-2");
    create_new_file(&path_to_repo, "shared.txt", "feature 2 version");
    commit_all(&repo, "commit on feature-2");

    let args: Vec<&str> = vec!["setup", "chain_name", "master", "feature-1", "feature-2"];
    run_test_bin_expect_ok(&path_to_repo, args);

    checkout_branch(&repo, "master");
    create_new_file(&path_to_repo, "shared.txt", "master version");
    commit_all(&repo, "conflicting commit on master");

    let worktree_output = run_git_command(
        &path_to_repo,
        vec![
            "worktree",
            "add",
            &format!("../{}_worktree", repo_name),
            "feature-2",
        ],
    );
    assert!(
        worktree_output.status.success(),
        "git worktree add should succeed but got: {}",
        String::from_utf8_lossy(&worktree_output.stderr)
    );

    checkout_branch(&repo, "feature-1");

    // Start a conflicting rebase inside the linked worktree: its HEAD goes detached while
    // .git/worktrees/<name>/rebase-merge/head-name still names feature-2.
    let worktree_rebase = run_git_command(&worktree_dir, vec!["rebase", "master"]);
    assert!(
        !worktree_rebase.status.success(),
        "the rebase inside the worktree should stop on the conflict, got: {}",
        String::from_utf8_lossy(&worktree_rebase.stderr)
    );

    let worktree_head = run_git_command(&worktree_dir, vec!["rev-parse", "--abbrev-ref", "HEAD"]);
    let worktree_head_name = String::from_utf8_lossy(&worktree_head.stdout)
        .trim()
        .to_string();

    // Navigation to the mid-rebase branch.
    let nav_output = run_test_bin_expect_err(&path_to_repo, vec!["last"]);
    let nav_stderr = String::from_utf8_lossy(&nav_output.stderr).to_string();

    // And the chain-rebase pre-flight.
    let rebase_output = run_test_bin_expect_err(&path_to_repo, vec!["rebase"]);
    let rebase_stderr = String::from_utf8_lossy(&rebase_output.stderr).to_string();

    let state_file = path_to_repo.join(".git/chain-rebase-state.json");

    println!("=== TEST DIAGNOSTICS ===");
    println!("worktree HEAD while rebasing: {}", worktree_head_name);
    println!("NAV STDERR: {}", nav_stderr);
    println!("REBASE STDERR: {}", rebase_stderr);
    println!("Current branch: {}", get_current_branch_name(&repo));
    println!("STATE FILE EXISTS: {}", state_file.exists());
    println!("EXPECTED: both refuse, naming feature-2 and the worktree");
    println!("======");

    // Uncomment to stop test execution and debug this test case
    // assert!(false, "DEBUG STOP: mid-rebase worktree");
    // assert!(false, "nav stderr: {}", nav_stderr);
    // assert!(false, "rebase stderr: {}", rebase_stderr);

    // Precondition: the worktree's HEAD really is detached, so only the rebase state names
    // the branch.
    assert_eq!(
        worktree_head_name, "HEAD",
        "the worktree should have a detached HEAD while rebasing, got: {}",
        worktree_head_name
    );

    assert!(
        !nav_output.status.success(),
        "navigation should refuse a branch being rebased elsewhere"
    );
    assert!(
        nav_stderr.contains("feature-2"),
        "the navigation error should name the branch but got: {}",
        nav_stderr
    );
    assert!(
        nav_stderr.contains("checked out in another worktree"),
        "the navigation error should explain the worktree conflict but got: {}",
        nav_stderr
    );
    assert!(
        nav_stderr.contains("worktree_mid_rebase_holds_branch_worktree"),
        "the navigation error should name the worktree path but got: {}",
        nav_stderr
    );
    assert_eq!(
        &get_current_branch_name(&repo),
        "feature-1",
        "HEAD should remain on the original branch"
    );

    assert!(
        !rebase_output.status.success(),
        "the chain rebase pre-flight should refuse too"
    );
    assert!(
        rebase_stderr.contains("feature-2"),
        "the pre-flight error should name the branch but got: {}",
        rebase_stderr
    );
    assert!(
        rebase_stderr.contains("checked out in another worktree"),
        "the pre-flight error should explain the worktree conflict but got: {}",
        rebase_stderr
    );
    assert!(
        !state_file.exists(),
        "the pre-flight refuses before any state is written, but {} exists",
        state_file.display()
    );

    fs::remove_dir_all(&worktree_dir).ok();
    teardown_git_repo(repo_name);
}
