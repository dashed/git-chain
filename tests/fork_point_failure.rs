#[path = "common/mod.rs"]
pub mod common;

use common::{
    create_new_file, first_commit_all, generate_path_to_repo, run_git_command, run_test_bin,
    run_test_bin_expect_ok, setup_git_repo, teardown_git_repo,
};

use git2::RepositoryState;
use std::path::Path;

/// Helper function to run git-chain and check for error messages in the output
/// This is useful when we want to verify error messages in stderr without expecting
/// a non-zero exit code (since git-chain may handle some errors gracefully)
fn run_and_check_for_error_messages<P: AsRef<Path>>(
    current_dir: P,
    args: Vec<&str>,
) -> (String, String) {
    let output = run_test_bin(current_dir, args);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);

    (stdout, stderr)
}

/// This test creates a scenario with completely unrelated Git branches.
///
/// This simulates what can happen in real-world scenarios when:
/// 1. Branch histories get completely rewritten (force pushed)
/// 2. Git's reflog entries expire
/// 3. Two branches that are supposed to be related end up having no common ancestor
#[test]
fn test_natural_forkpoint_loss() {
    let repo_name = "natural_forkpoint_loss_test";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // Create initial commit on master
    create_new_file(&path_to_repo, "init.txt", "Initial content");
    first_commit_all(&repo, "Initial commit");

    // Create an intentionally broken branch chain where branch1 and branch2 have no common ancestor

    // Branch1 - create normally from master
    run_git_command(&path_to_repo, vec!["checkout", "-b", "branch1"]);
    create_new_file(&path_to_repo, "branch1_file.txt", "Branch 1 content");
    run_git_command(&path_to_repo, vec!["add", "branch1_file.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 1 commit"]);

    // Branch2 - create as an orphan branch (with no relationship to master or branch1)
    run_git_command(&path_to_repo, vec!["checkout", "--orphan", "branch2"]);
    run_git_command(&path_to_repo, vec!["rm", "-rf", "."]); // Clear the working directory
    create_new_file(&path_to_repo, "branch2_file.txt", "Branch 2 content");
    run_git_command(&path_to_repo, vec!["add", "branch2_file.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 2 commit"]);

    // Branch3 - create from branch2
    run_git_command(&path_to_repo, vec!["checkout", "-b", "branch3"]);
    create_new_file(&path_to_repo, "branch3_file.txt", "Branch 3 content");
    run_git_command(&path_to_repo, vec!["add", "branch3_file.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 3 commit"]);

    // Set up a chain with these branches
    // Note: git-chain's setup command doesn't verify branch relationships at setup time
    run_test_bin_expect_ok(
        &path_to_repo,
        vec![
            "setup",
            "test_chain",
            "master",
            "branch1",
            "branch2",
            "branch3",
        ],
    );

    // Print the current branch structure
    println!("Branch Setup:");
    let chain_branches = run_git_command(&path_to_repo, vec!["branch", "-v"]);
    println!("{}", String::from_utf8_lossy(&chain_branches.stdout));

    // Confirm that branch1 and branch2 have no merge base
    println!("\nVerifying no merge base between branch1 and branch2:");
    let merge_base_cmd = run_git_command(&path_to_repo, vec!["merge-base", "branch1", "branch2"]);
    println!(
        "stdout: {}",
        String::from_utf8_lossy(&merge_base_cmd.stdout)
    );
    println!(
        "stderr: {}",
        String::from_utf8_lossy(&merge_base_cmd.stderr)
    );

    // Also check that fork-point detection fails
    println!("\nFork-point detection between branch1 and branch2:");
    let fork_point_cmd = run_git_command(
        &path_to_repo,
        vec!["merge-base", "--fork-point", "branch1", "branch2"],
    );
    println!(
        "stdout: {}",
        String::from_utf8_lossy(&fork_point_cmd.stdout)
    );
    println!(
        "stderr: {}",
        String::from_utf8_lossy(&fork_point_cmd.stderr)
    );

    // Now try to rebase the chain - this should produce errors during fork-point detection
    println!("\nRunning git-chain rebase:");
    let (stdout, stderr) = run_and_check_for_error_messages(&path_to_repo, vec!["rebase"]);

    // Check for error messages about missing fork points or merge bases
    let error_patterns = [
        "no merge base found",
        "Unable to get forkpoint",
        "common ancestor",
        "failed to find",
    ];

    let has_error_message = error_patterns
        .iter()
        .any(|pattern| stderr.contains(pattern) || stdout.contains(pattern));

    assert!(has_error_message,
            "Expected output to contain error about missing merge base or fork point.\nStdout: {}\nStderr: {}", 
            stdout, stderr);

    // Clean up any rebase in progress
    if repo.state() != RepositoryState::Clean {
        run_git_command(&path_to_repo, vec!["rebase", "--abort"]);
    }

    // Clean up test repository
    teardown_git_repo(repo_name);
}

/// This test creates a chain with completely unrelated branches to test edge cases.
#[test]
fn test_unable_to_get_forkpoint_error() {
    let repo_name = "forkpoint_error_test";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // Create initial commit on master
    create_new_file(&path_to_repo, "init.txt", "Initial content");
    first_commit_all(&repo, "Initial commit");

    // Create completely unrelated branches
    // Branch1 - create orphan branch with its own history
    run_git_command(&path_to_repo, vec!["checkout", "--orphan", "branch1"]);
    run_git_command(&path_to_repo, vec!["rm", "-rf", "."]);
    create_new_file(&path_to_repo, "branch1.txt", "Branch 1 content");
    run_git_command(&path_to_repo, vec!["add", "branch1.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 1 commit"]);

    // Branch2 - another orphan branch with different history
    run_git_command(&path_to_repo, vec!["checkout", "--orphan", "branch2"]);
    run_git_command(&path_to_repo, vec!["rm", "-rf", "."]);
    create_new_file(&path_to_repo, "branch2.txt", "Branch 2 content");
    run_git_command(&path_to_repo, vec!["add", "branch2.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 2 commit"]);

    // Set up a chain with these completely unrelated branches
    let args: Vec<&str> = vec!["setup", "unrelated_chain", "master", "branch1", "branch2"];

    // Setup should succeed - git-chain doesn't verify branch relationships at setup time
    run_test_bin_expect_ok(&path_to_repo, args);

    // Run rebase with our unrelated branches
    println!("Running git-chain rebase with unrelated branches:");
    let (stdout, stderr) = run_and_check_for_error_messages(&path_to_repo, vec!["rebase"]);

    // Check for error messages about missing fork points or merge bases
    let error_patterns = [
        "no merge base found",
        "Unable to get forkpoint",
        "common ancestor",
        "failed to find",
    ];

    let has_error_message = error_patterns
        .iter()
        .any(|pattern| stderr.contains(pattern) || stdout.contains(pattern));

    assert!(has_error_message,
            "Expected output to contain error about missing merge base or fork point.\nStdout: {}\nStderr: {}", 
            stdout, stderr);

    // Clean up test repo
    teardown_git_repo(repo_name);
}

/// Tests for a rebase conflict scenario.
///
/// This test creates a situation where branches have conflicts that
/// will cause the rebase to fail. This tests git-chain's ability to detect and
/// report conflicts during the rebase process.
#[test]
fn test_rebase_conflict_error() {
    let repo_name = "rebase_conflict_error";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // Create initial commit on master
    create_new_file(&path_to_repo, "init.txt", "Initial content");
    first_commit_all(&repo, "Initial commit");

    // Create branch1 from master
    run_git_command(&path_to_repo, vec!["branch", "branch1"]);
    run_git_command(&path_to_repo, vec!["checkout", "branch1"]);
    create_new_file(&path_to_repo, "branch1.txt", "Branch 1 content");
    run_git_command(&path_to_repo, vec!["add", "branch1.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 1 commit"]);

    // Create branch2 from branch1
    run_git_command(&path_to_repo, vec!["branch", "branch2"]);
    run_git_command(&path_to_repo, vec!["checkout", "branch2"]);
    create_new_file(&path_to_repo, "branch2.txt", "Branch 2 content");
    run_git_command(&path_to_repo, vec!["add", "branch2.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 2 commit"]);

    // Set up a chain
    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "test_chain", "master", "branch1", "branch2"],
    );

    // Create a scenario where rebasing would create a conflict:
    // Both master and branch1 modify the same file in different ways
    run_git_command(&path_to_repo, vec!["checkout", "master"]);
    create_new_file(&path_to_repo, "conflict.txt", "Master content");
    run_git_command(&path_to_repo, vec!["add", "conflict.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Add file on master"]);

    run_git_command(&path_to_repo, vec!["checkout", "branch1"]);
    create_new_file(&path_to_repo, "conflict.txt", "Branch1 content");
    run_git_command(&path_to_repo, vec!["add", "conflict.txt"]);
    run_git_command(
        &path_to_repo,
        vec!["commit", "-m", "Add conflicting file on branch1"],
    );

    // Try rebasing - this should fail due to conflict
    println!("Running git-chain rebase with conflicting changes:");
    let (stdout, stderr) = run_and_check_for_error_messages(&path_to_repo, vec!["rebase"]);

    // We expect to see a message about resolving rebase conflicts
    let has_conflict_message = stderr.contains("conflict")
        || stderr.contains("error")
        || stderr.contains("Unable to")
        || stdout.contains("conflict")
        || stdout.contains("CONFLICT");

    assert!(
        has_conflict_message,
        "Expected message about rebase conflict.\nStdout: {}\nStderr: {}",
        stdout, stderr
    );

    // Clean up any rebase in progress
    if repo.state() != RepositoryState::Clean {
        run_git_command(&path_to_repo, vec!["rebase", "--abort"]);
    }

    teardown_git_repo(repo_name);
}
