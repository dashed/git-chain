#[path = "common/mod.rs"]
pub mod common;

use common::{
    create_new_file, first_commit_all, generate_path_to_repo, run_git_command, run_test_bin,
    run_test_bin_expect_ok, setup_git_repo, teardown_git_repo,
};

use git2::RepositoryState;

/// Tests a scenario where a branch has been rebased multiple times, creating complex reflog history
/// that can confuse fork-point detection.
#[test]
fn test_complex_rebased_remote_history() {
    std::env::set_var("LANG", "C");
    let repo_name = "complex_rebased_remote";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // Create initial commit on master
    create_new_file(&path_to_repo, "init.txt", "Initial content");
    first_commit_all(&repo, "Initial commit");

    // Create a "remote" branch
    run_git_command(&path_to_repo, vec!["checkout", "-b", "origin/master"]);

    // Create several commits and rebase them multiple times to create complex reflog
    for i in 1..4 {
        create_new_file(
            &path_to_repo,
            &format!("remote_file_{}.txt", i),
            &format!("Remote content {}", i),
        );
        run_git_command(
            &path_to_repo,
            vec!["add", &format!("remote_file_{}.txt", i)],
        );
        run_git_command(
            &path_to_repo,
            vec!["commit", "-m", &format!("Remote commit {}", i)],
        );

        // Create a temporary branch, make a commit, then rebase origin/master onto it
        run_git_command(
            &path_to_repo,
            vec!["checkout", "-b", &format!("temp_branch_{}", i), "master"],
        );
        create_new_file(
            &path_to_repo,
            &format!("temp_file_{}.txt", i),
            &format!("Temp content {}", i),
        );
        run_git_command(&path_to_repo, vec!["add", &format!("temp_file_{}.txt", i)]);
        run_git_command(
            &path_to_repo,
            vec!["commit", "-m", &format!("Temp commit {}", i)],
        );

        // Rebase origin/master onto the temp branch
        run_git_command(&path_to_repo, vec!["checkout", "origin/master"]);
        run_git_command(&path_to_repo, vec!["rebase", &format!("temp_branch_{}", i)]);
    }

    // Create a feature branch from an older version of origin/master
    run_git_command(&path_to_repo, vec!["checkout", "-b", "feature", "master"]);
    create_new_file(&path_to_repo, "feature_file.txt", "Feature content");
    run_git_command(&path_to_repo, vec!["add", "feature_file.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Feature commit"]);

    // Setup chain
    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "test_chain", "master", "origin/master", "feature"],
    );

    // Test the fork-point detection
    println!("Testing fork-point detection with complex rebased history:");
    let output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(output.status.success());

    // Check for specific success and error messages in the output
    // Note: Git rebase backends handle output differently:
    // - apply backend writes errors to stdout
    // - merge backend writes errors to stderr
    // Both backends write progress/info messages slightly differently

    assert!(
        stdout.contains("🎉 Successfully rebased chain test_chain"),
        "Expected success message in stdout, but got:\n{}",
        stdout
    );
    assert!(
        stderr.contains("Successfully rebased and updated refs/heads/feature."),
        "Expected rebase success message for branch feature in stderr, but got:\n{}",
        stderr
    );

    // Clean up any rebase in progress
    if repo.state() != RepositoryState::Clean {
        run_git_command(&path_to_repo, vec!["rebase", "--abort"]);
    }

    teardown_git_repo(repo_name);
}

/// Tests a scenario with criss-cross merges that create multiple "best" merge bases
#[test]
fn test_criss_cross_merge_bases() {
    let repo_name = "criss_cross_merge_test";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // Create initial commit
    create_new_file(&path_to_repo, "init.txt", "Initial content");
    first_commit_all(&repo, "Initial commit");

    // Create branch A and B from the initial commit
    run_git_command(&path_to_repo, vec!["branch", "A"]);
    run_git_command(&path_to_repo, vec!["branch", "B"]);

    // Make a commit on branch A
    run_git_command(&path_to_repo, vec!["checkout", "A"]);
    create_new_file(&path_to_repo, "A1.txt", "A1 content");
    run_git_command(&path_to_repo, vec!["add", "A1.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "A1 commit"]);

    // Make a commit on branch B
    run_git_command(&path_to_repo, vec!["checkout", "B"]);
    create_new_file(&path_to_repo, "B1.txt", "B1 content");
    run_git_command(&path_to_repo, vec!["add", "B1.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "B1 commit"]);

    // Merge B into A
    run_git_command(&path_to_repo, vec!["checkout", "A"]);
    run_git_command(&path_to_repo, vec!["merge", "B", "-m", "Merge B into A"]);

    // Make another commit on branch B
    run_git_command(&path_to_repo, vec!["checkout", "B"]);
    create_new_file(&path_to_repo, "B2.txt", "B2 content");
    run_git_command(&path_to_repo, vec!["add", "B2.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "B2 commit"]);

    // Merge A into B - creating a criss-cross pattern
    run_git_command(&path_to_repo, vec!["merge", "A", "-m", "Merge A into B"]);

    // Create branch C from A and branch D from B
    run_git_command(&path_to_repo, vec!["checkout", "A"]);
    run_git_command(&path_to_repo, vec!["checkout", "-b", "C"]);
    create_new_file(&path_to_repo, "C.txt", "C content");
    run_git_command(&path_to_repo, vec!["add", "C.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "C commit"]);

    run_git_command(&path_to_repo, vec!["checkout", "B"]);
    run_git_command(&path_to_repo, vec!["checkout", "-b", "D"]);
    create_new_file(&path_to_repo, "D.txt", "D content");
    run_git_command(&path_to_repo, vec!["add", "D.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "D commit"]);

    // Setup a chain with these branches
    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "test_chain", "master", "C", "D"],
    );

    // Our criss-cross pattern doesn't always result in multiple merge bases due to
    // how git handles the merges, but it still tests the complex ancestry scenario

    // Verify we have a merge base (though it might not always be multiple)
    let merge_bases = run_git_command(&path_to_repo, vec!["merge-base", "--all", "C", "D"]);
    let merge_base_count = String::from_utf8_lossy(&merge_bases.stdout).lines().count();
    println!("Found {} merge bases between C and D", merge_base_count);

    // Test git-chain behavior with complex merge history
    println!("Testing git-chain with criss-cross merge history:");
    // let (stdout, stderr) = run_and_check_for_error_messages(&path_to_repo, vec!["rebase"]);
    let output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(output.status.success());

    // Check for specific success and error messages in the output

    assert!(
        stdout.contains("🎉 Successfully rebased chain test_chain"),
        "Expected success message in stdout, but got:\n{}",
        stdout
    );
    assert!(
        stderr.contains("Successfully rebased and updated refs/heads/C."),
        "Expected rebase success message for branch C in stderr, but got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Successfully rebased and updated refs/heads/D."),
        "Expected rebase success message for branch D in stderr, but got:\n{}",
        stderr
    );

    // Clean up any rebase in progress
    if repo.state() != RepositoryState::Clean {
        run_git_command(&path_to_repo, vec!["rebase", "--abort"]);
    }

    teardown_git_repo(repo_name);
}

/// Tests a scenario with a shallow clone where merge-base can fail due to truncated history
#[test]
fn test_shallow_clone_merge_base() {
    // For this test, we'll use a simpler approach since we can't easily use clone
    // in the test environment. Instead, we'll simulate a shallow history by
    // creating branches with minimal shared history.
    let repo_name = "shallow_clone_simulated";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // Create initial commit on master
    create_new_file(&path_to_repo, "init.txt", "Initial content");
    first_commit_all(&repo, "Initial commit");

    // Create branch1 from master
    run_git_command(&path_to_repo, vec!["checkout", "master"]);
    run_git_command(&path_to_repo, vec!["checkout", "-b", "branch1"]);
    create_new_file(&path_to_repo, "branch1.txt", "Branch 1 content");
    run_git_command(&path_to_repo, vec!["add", "branch1.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 1 commit"]);

    // Create branch2 from master
    run_git_command(&path_to_repo, vec!["checkout", "master"]);
    run_git_command(&path_to_repo, vec!["checkout", "-b", "branch2"]);
    create_new_file(&path_to_repo, "branch2.txt", "Branch 2 content");
    run_git_command(&path_to_repo, vec!["add", "branch2.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 2 commit"]);

    // Manually delete the reflog to simulate a shallow clone with limited history
    run_git_command(
        &path_to_repo,
        vec!["reflog", "expire", "--expire=all", "--all"],
    );
    run_git_command(&path_to_repo, vec!["gc", "--prune=now"]);

    // Setup the chain
    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "test_chain", "master", "branch1", "branch2"],
    );

    // Now try to run rebase - which will require merge-base
    println!("Testing merge-base behavior with simulated shallow history:");
    let output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(output.status.success());

    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);

    // Check for specific success and error messages in the output

    assert!(
        stdout.contains("Current branch branch1 is up to date."),
        "Expected success message in stdout, but got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("🎉 Successfully rebased chain test_chain"),
        "Expected success message in stdout, but got:\n{}",
        stdout
    );

    assert!(
        stderr.contains("Successfully rebased and updated refs/heads/branch2."),
        "Expected rebase success message for branch branch2 in stderr, but got:\n{}",
        stderr
    );

    // Clean up any rebase in progress
    if repo.state() != RepositoryState::Clean {
        run_git_command(&path_to_repo, vec!["rebase", "--abort"]);
    }

    teardown_git_repo(repo_name);
}

/// Tests a scenario where git garbage collection has removed objects needed for merge-base
#[test]
fn test_unreachable_objects_after_gc() {
    let repo_name = "unreachable_objects_test";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // Create initial commit on master
    create_new_file(&path_to_repo, "init.txt", "Initial content");
    first_commit_all(&repo, "Initial commit");

    // Create branch1 from master with several commits
    run_git_command(&path_to_repo, vec!["checkout", "-b", "branch1"]);
    for i in 1..4 {
        create_new_file(
            &path_to_repo,
            &format!("branch1_file{}.txt", i),
            &format!("Branch 1 content {}", i),
        );
        run_git_command(
            &path_to_repo,
            vec!["add", &format!("branch1_file{}.txt", i)],
        );
        run_git_command(
            &path_to_repo,
            vec!["commit", "-m", &format!("Branch 1 commit {}", i)],
        );
    }

    // Create branch2 from branch1
    run_git_command(&path_to_repo, vec!["checkout", "-b", "branch2"]);
    create_new_file(&path_to_repo, "branch2.txt", "Branch 2 content");
    run_git_command(&path_to_repo, vec!["add", "branch2.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Branch 2 commit"]);

    // Setup the chain
    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "test_chain", "master", "branch1", "branch2"],
    );

    // Rewrite branch1 history, making previous commits unreachable
    run_git_command(&path_to_repo, vec!["checkout", "branch1"]);
    run_git_command(&path_to_repo, vec!["reset", "--hard", "master"]);
    create_new_file(&path_to_repo, "new_branch1.txt", "New Branch 1 content");
    run_git_command(&path_to_repo, vec!["add", "new_branch1.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "New Branch 1 commit"]);
    run_git_command(&path_to_repo, vec!["push", "--force", "..", "branch1"]);

    // Run aggressive GC to remove unreachable objects
    run_git_command(
        &path_to_repo,
        vec!["reflog", "expire", "--expire=now", "--all"],
    );
    run_git_command(&path_to_repo, vec!["gc", "--prune=now", "--aggressive"]);

    // Try rebasing the chain
    println!("Testing git-chain with unreachable objects after GC:");
    let output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(output.status.success());

    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);

    // Check for specific success and error messages in the output

    assert!(
        stdout.contains("Current branch branch1 is up to date."),
        "Expected success message in stdout, but got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("🎉 Successfully rebased chain test_chain"),
        "Expected success message in stdout, but got:\n{}",
        stdout
    );

    assert!(
        stderr.contains("Successfully rebased and updated refs/heads/branch2."),
        "Expected rebase success message for branch branch2 in stderr, but got:\n{}",
        stderr
    );

    // Clean up any rebase in progress
    if repo.state() != RepositoryState::Clean {
        run_git_command(&path_to_repo, vec!["rebase", "--abort"]);
    }

    teardown_git_repo(repo_name);
}

/// Tests a scenario with octopus merges (merges with more than two parents)
#[test]
fn test_octopus_merge_ancestry() {
    let repo_name = "octopus_merge_test";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // Create initial commit on master
    create_new_file(&path_to_repo, "init.txt", "Initial content");
    first_commit_all(&repo, "Initial commit");

    // Create three feature branches
    for i in 1..4 {
        run_git_command(&path_to_repo, vec!["checkout", "master"]);
        run_git_command(
            &path_to_repo,
            vec!["checkout", "-b", &format!("feature{}", i)],
        );
        create_new_file(
            &path_to_repo,
            &format!("feature{}.txt", i),
            &format!("Feature {} content", i),
        );
        run_git_command(&path_to_repo, vec!["add", &format!("feature{}.txt", i)]);
        run_git_command(
            &path_to_repo,
            vec!["commit", "-m", &format!("Feature {} commit", i)],
        );
    }

    // Go back to master and create an octopus merge
    run_git_command(&path_to_repo, vec!["checkout", "master"]);
    run_git_command(
        &path_to_repo,
        vec![
            "merge",
            "feature1",
            "feature2",
            "feature3",
            "-m",
            "Octopus merge",
        ],
    );

    // Create a new branch from master after the octopus merge
    run_git_command(&path_to_repo, vec!["checkout", "-b", "post_octopus"]);
    create_new_file(&path_to_repo, "post_octopus.txt", "Post octopus content");
    run_git_command(&path_to_repo, vec!["add", "post_octopus.txt"]);
    run_git_command(&path_to_repo, vec!["commit", "-m", "Post octopus commit"]);

    // Setup chain with branches that have octopus merge ancestry
    run_test_bin_expect_ok(
        &path_to_repo,
        vec!["setup", "test_chain", "master", "feature1", "post_octopus"],
    );

    // Test fork-point and merge-base behavior with octopus merge ancestry
    println!("Testing merge-base behavior with octopus merge ancestry:");
    let output = run_test_bin(&path_to_repo, vec!["rebase"]);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(output.status.success());

    // Check for specific success and error messages in the output

    assert!(stdout.contains("🎉 Successfully rebased chain test_chain"));

    assert!(stderr.contains("Successfully rebased and updated refs/heads/feature1."));
    assert!(
        stderr.contains("dropping")
            && stderr.contains("Feature 2 commit -- patch contents already upstream")
    );
    assert!(
        stderr.contains("dropping")
            && stderr.contains("Feature 3 commit -- patch contents already upstream")
    );
    assert!(stderr.contains("Successfully rebased and updated refs/heads/post_octopus."));

    // Clean up any rebase in progress
    if repo.state() != RepositoryState::Clean {
        run_git_command(&path_to_repo, vec!["rebase", "--abort"]);
    }

    teardown_git_repo(repo_name);
}
