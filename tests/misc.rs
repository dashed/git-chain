#[path = "common/mod.rs"]
pub mod common;

use std::fs;

use common::{
    create_new_file, first_commit_all, generate_path_to_repo, get_current_branch_name,
    run_test_bin_expect_err, setup_git_repo, teardown_git_repo,
};

#[test]
fn no_subcommand() {
    let repo_name = "no_subcommand";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    assert_eq!(&get_current_branch_name(&repo), "master");

    let args: Vec<String> = vec![];
    let output = run_test_bin_expect_err(path_to_repo, args);
    assert!(String::from_utf8_lossy(&output.stdout).contains("On branch: master"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Branch is not part of any chain: master")
    );

    teardown_git_repo(repo_name);
}

#[test]
fn not_a_git_repo() {
    // Create a directory in the system temp location to avoid finding parent git repos
    let temp_dir = std::env::temp_dir();
    let path_to_non_git_dir = temp_dir.join("git_chain_test_not_a_repo");

    // Create a directory that is NOT a git repository
    fs::remove_dir_all(&path_to_non_git_dir).ok();
    fs::create_dir_all(&path_to_non_git_dir).unwrap();

    // Run git chain in the non-git directory
    let args: Vec<String> = vec![];
    let output = run_test_bin_expect_err(&path_to_non_git_dir, args);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Diagnostic printing
    println!("=== TEST DIAGNOSTICS ===");
    println!("Test directory: {:?}", path_to_non_git_dir);
    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status);
    println!("Is directory a git repo: false (intentional test condition)");
    println!("======");

    // Uncomment to stop test execution and inspect state with captured output
    // assert!(false, "DEBUG STOP: not_a_git_repo test section");
    // assert!(false, "stdout: {}", stdout);
    // assert!(false, "stderr: {}", stderr);
    // assert!(false, "status code: {}", output.status.code().unwrap_or(0));

    // Specific assertions based on expected behavior
    assert!(
        !output.status.success(),
        "Command should fail when run in non-git directory"
    );
    assert!(
        stderr.contains("Not a git repository"),
        "Error message should mention 'Not a git repository', got: {}",
        stderr
    );
    assert!(
        stderr.contains("This command must be run inside a git repository"),
        "Error message should provide helpful hint, got: {}",
        stderr
    );

    // Clean up
    fs::remove_dir_all(&path_to_non_git_dir).ok();
}
