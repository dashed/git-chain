use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

mod common;

use common::*;
use git2::Repository;

fn setup_mock_gh(test_name: &str) -> PathBuf {
    let mock_dir = PathBuf::from("./test_sandbox")
        .join(test_name)
        .join("mock_bin");
    fs::create_dir_all(&mock_dir).unwrap();

    let mock_gh_path = mock_dir.join("gh");

    // Create a mock gh script that responds to different commands
    let mock_script = r#"#!/bin/bash
# Mock gh CLI for testing

if [ "$1" = "--version" ]; then
    echo "gh version 2.40.0 (2024-01-01)"
    echo "https://github.com/cli/cli/releases/tag/v2.40.0"
    exit 0
fi

if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
    # Handle two different patterns of pr list
    
    # Pattern 1: gh pr list --head <branch> --json url
    if [ "$3" = "--head" ] && [ "$5" = "--json" ]; then
        branch="$4"
        case "$branch" in
            "feature-with-pr")
                echo '[{"url":"https://github.com/test/repo/pull/123"}]'
                ;;
            "feature-merged")
                echo '[{"url":"https://github.com/test/repo/pull/124"}]'
                ;;
            *)
                echo '[]'
                ;;
        esac
        exit 0
    fi
    
    # Pattern 2: gh pr list --state all --head <branch> --json url,state
    if [ "$3" = "--state" ] && [ "$4" = "all" ] && [ "$5" = "--head" ] && [ "$7" = "--json" ]; then
        branch="$6"
        case "$branch" in
            "feature-with-pr")
                echo '[{"url":"https://github.com/test/repo/pull/123","state":"OPEN"}]'
                ;;
            "feature-merged")
                echo '[{"url":"https://github.com/test/repo/pull/124","state":"MERGED"}]'
                ;;
            "feature-closed")
                echo '[{"url":"https://github.com/test/repo/pull/125","state":"CLOSED"}]'
                ;;
            *)
                echo '[]'
                ;;
        esac
        exit 0
    fi
fi

if [ "$1" = "pr" ] && [ "$2" = "create" ]; then
    # Check for the invalid combination of --draft and --web flags
    if [[ "$*" =~ --web ]] && [[ "$*" =~ --draft ]]; then
        echo "Error: the \`--draft\` flag is not supported with \`--web\`" >&2
        exit 1
    fi
    
    # Pattern: gh pr create --base <base> --head <head> --web
    if [ "$3" = "--base" ] && [ "$5" = "--head" ] && [ "$7" = "--web" ]; then
        base="$4"
        head="$6"
        echo "Opening https://github.com/test/repo/compare/$base...$head?expand=1 in your browser."
        exit 0
    fi
    
    # Pattern for draft PRs without --web: gh pr create --base <base> --head <head> --draft
    if [ "$3" = "--base" ] && [ "$5" = "--head" ] && [ "$7" = "--draft" ]; then
        base="$4"
        head="$6"
        # Draft PR creation outputs the URL to stdout
        echo "https://github.com/test/repo/pull/999"
        exit 0
    fi
fi

if [ "$1" = "browse" ]; then
    # gh browse <PR_NUMBER> - simulate opening PR in browser
    if [ -n "$2" ]; then
        echo "Opening https://github.com/test/repo/pull/$2 in your browser."
        exit 0
    fi
fi

# Default error response
echo "Error: unknown gh command" >&2
exit 1
"#;

    fs::write(&mock_gh_path, mock_script).unwrap();

    // Make the script executable
    let mut perms = fs::metadata(&mock_gh_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_gh_path, perms).unwrap();

    // Also create a mock git that handles push
    let mock_git_path = mock_dir.join("git");
    let mock_git_script = r#"#!/bin/bash
# Mock git for testing - only handles push, delegates everything else to real git

if [ "$1" = "push" ]; then
    echo "Successfully pushed to origin"
    exit 0
fi

# Delegate to real git
/usr/bin/git "$@"
"#;

    fs::write(&mock_git_path, mock_git_script).unwrap();
    let mut git_perms = fs::metadata(&mock_git_path).unwrap().permissions();
    git_perms.set_mode(0o755);
    fs::set_permissions(&mock_git_path, git_perms).unwrap();

    mock_dir
}

fn setup_git_repo_with_chain_and_mock(test_name: &str) -> (Repository, PathBuf) {
    let repo = setup_git_repo(test_name);
    let path_to_repo = generate_path_to_repo(test_name);

    // Set up mock gh
    let mock_dir = setup_mock_gh(test_name);

    // Create initial commit on main branch
    create_new_file(&path_to_repo, "README.md", "Initial commit");
    first_commit_all(&repo, "Initial commit");

    // Rename master to main
    {
        let mut master_branch = repo.find_branch("master", git2::BranchType::Local).unwrap();
        master_branch.rename("main", false).unwrap();
    }

    // Create a feature branch from main
    create_branch(&repo, "feature-1");
    checkout_branch(&repo, "feature-1");
    create_new_file(&path_to_repo, "feature1.txt", "Feature 1");
    commit_all(&repo, "Add feature 1");

    // Initialize chain for feature-1
    run_test_bin_expect_ok(&path_to_repo, ["init", "test-chain", "main"]);

    // Create another feature branch from feature-1
    create_branch(&repo, "feature-2");
    checkout_branch(&repo, "feature-2");
    create_new_file(&path_to_repo, "feature2.txt", "Feature 2");
    commit_all(&repo, "Add feature 2");

    // Initialize chain for feature-2
    run_test_bin_expect_ok(&path_to_repo, ["init", "test-chain", "feature-1"]);

    (repo, mock_dir)
}

#[test]
fn test_pr_command_creates_prs_for_chain() {
    let test_name = "test_pr_creates_prs";
    let (repo, mock_dir) = setup_git_repo_with_chain_and_mock(test_name);
    let path_to_repo = repo.workdir().unwrap();

    // Update PATH to include our mock directory (use absolute path)
    let original_path = env::var("PATH").unwrap_or_default();
    let absolute_mock_dir = mock_dir.canonicalize().unwrap();
    let new_path = format!("{}:{}", absolute_mock_dir.display(), original_path);

    env::set_var("PATH", new_path);

    // Run pr command
    let output = run_test_bin(path_to_repo, ["pr"]);

    // Restore original PATH
    env::set_var("PATH", original_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Debug output
    println!("=== TEST DIAGNOSTICS ===");
    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status);
    println!("======");

    // Assertions
    assert!(output.status.success(), "Command should succeed");
    assert!(
        stdout.contains("Pushed branch feature-1"),
        "Should push feature-1, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Pushed branch feature-2"),
        "Should push feature-2, got: {}",
        stdout
    );
    assert!(
        stdout.contains("✅ Created PR for feature-1 -> main"),
        "Should show success message for feature-1, got: {}",
        stdout
    );
    assert!(
        stdout.contains("✅ Created PR for feature-2 -> feature-1"),
        "Should show success message for feature-2, got: {}",
        stdout
    );

    teardown_git_repo(test_name);
}

#[test]
fn test_pr_command_skips_existing_prs() {
    let test_name = "test_pr_skips_existing";
    let repo = setup_git_repo(test_name);
    let path_to_repo = generate_path_to_repo(test_name);

    // Set up mock gh
    let mock_dir = setup_mock_gh(test_name);

    // Create initial commit on main branch
    create_new_file(&path_to_repo, "README.md", "Initial commit");
    first_commit_all(&repo, "Initial commit");

    // Rename master to main
    {
        let mut master_branch = repo.find_branch("master", git2::BranchType::Local).unwrap();
        master_branch.rename("main", false).unwrap();
    }

    // Create branches that will have existing PRs
    create_branch(&repo, "feature-with-pr");
    checkout_branch(&repo, "feature-with-pr");
    create_new_file(&path_to_repo, "feature.txt", "Feature");
    commit_all(&repo, "Add feature");

    // Initialize chain
    run_test_bin_expect_ok(&path_to_repo, ["init", "pr-chain", "main"]);

    create_branch(&repo, "feature-merged");
    checkout_branch(&repo, "feature-merged");
    create_new_file(&path_to_repo, "merged.txt", "Merged feature");
    commit_all(&repo, "Add merged feature");

    // Initialize chain for feature-merged
    run_test_bin_expect_ok(&path_to_repo, ["init", "pr-chain", "feature-with-pr"]);

    // Update PATH
    let original_path = env::var("PATH").unwrap_or_default();
    let absolute_mock_dir = mock_dir.canonicalize().unwrap();
    let new_path = format!("{}:{}", absolute_mock_dir.display(), original_path);
    env::set_var("PATH", new_path);

    // Run pr command
    let output = run_test_bin(path_to_repo, ["pr"]);

    // Restore original PATH
    env::set_var("PATH", original_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Debug output
    println!("=== TEST DIAGNOSTICS ===");
    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status);
    println!("======");

    // Assertions
    assert!(output.status.success(), "Command should succeed");
    assert!(
        stdout.contains("🔗 Open PR already exists for branch feature-with-pr"),
        "Should skip existing PR for feature-with-pr, got: {}",
        stdout
    );
    assert!(
        stdout.contains("https://github.com/test/repo/pull/123"),
        "Should show PR URL for feature-with-pr, got: {}",
        stdout
    );
    assert!(
        stdout.contains("🔗 Open PR already exists for branch feature-merged"),
        "Should skip existing PR for feature-merged, got: {}",
        stdout
    );
    assert!(
        stdout.contains("https://github.com/test/repo/pull/124"),
        "Should show PR URL for feature-merged, got: {}",
        stdout
    );

    teardown_git_repo(test_name);
}

#[test]
fn test_pr_command_with_draft_flag() {
    let test_name = "test_pr_draft";
    let (repo, mock_dir) = setup_git_repo_with_chain_and_mock(test_name);
    let path_to_repo = repo.workdir().unwrap();

    // Update PATH
    let original_path = env::var("PATH").unwrap_or_default();
    let absolute_mock_dir = mock_dir.canonicalize().unwrap();
    let new_path = format!("{}:{}", absolute_mock_dir.display(), original_path);
    env::set_var("PATH", new_path);

    // Run pr command with draft flag
    let output = run_test_bin(path_to_repo, ["pr", "--draft"]);

    // Restore original PATH
    env::set_var("PATH", original_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Debug output
    println!("=== TEST DIAGNOSTICS ===");
    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status);
    println!("======");

    // With the fix, draft PRs should now work successfully
    assert!(
        output.status.success(),
        "Command should succeed with draft flag"
    );
    assert!(
        stdout.contains("✅ Created PR for"),
        "Should show successful PR creation, got: {}",
        stdout
    );
    assert!(
        stdout.contains("🌐 Opened draft PR in browser")
            || stdout.contains("ℹ️  Draft PR created:"),
        "Should show browser opening or PR URL, got: {}",
        stdout
    );

    teardown_git_repo(test_name);
}

#[test]
fn test_gh_cli_not_installed() {
    let test_name = "test_gh_not_installed";
    let repo = setup_git_repo(test_name);
    let path_to_repo = generate_path_to_repo(test_name);

    // Create initial commit on main branch
    create_new_file(&path_to_repo, "README.md", "Initial commit");
    first_commit_all(&repo, "Initial commit");

    // Rename master to main
    {
        let mut master_branch = repo.find_branch("master", git2::BranchType::Local).unwrap();
        master_branch.rename("main", false).unwrap();
    }

    // Create a branch and initialize chain
    create_branch(&repo, "feature-1");
    checkout_branch(&repo, "feature-1");
    create_new_file(&path_to_repo, "feature.txt", "Feature");
    commit_all(&repo, "Add feature");
    run_test_bin_expect_ok(&path_to_repo, ["init", "test-chain", "main"]);

    // Create a directory without gh in PATH
    let empty_dir = path_to_repo.join("empty_bin");
    fs::create_dir_all(&empty_dir).unwrap();

    // Set PATH to only include the empty directory
    let original_path = env::var("PATH").unwrap_or_default();
    let absolute_empty_dir = empty_dir.canonicalize().unwrap();
    env::set_var("PATH", absolute_empty_dir.display().to_string());

    // Run pr command - should fail
    let output = run_test_bin(&path_to_repo, ["pr"]);

    // Restore original PATH
    env::set_var("PATH", original_path);

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Debug output
    println!("=== TEST DIAGNOSTICS ===");
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status);
    println!("======");

    // Assertions - the command should fail when gh is not installed
    assert!(
        !output.status.success(),
        "Command should fail when gh is not installed"
    );
    assert!(
        stderr.contains("GitHub CLI (gh) is not installed")
            || stderr.contains("not found in the PATH"),
        "Should show error about gh not being installed, got: {}",
        stderr
    );

    teardown_git_repo(test_name);
}

#[test]
fn test_list_command_with_pr_flag() {
    let test_name = "test_list_with_pr";
    let repo = setup_git_repo(test_name);
    let path_to_repo = generate_path_to_repo(test_name);

    // Set up mock gh
    let mock_dir = setup_mock_gh(test_name);

    // Create initial commit on main branch
    create_new_file(&path_to_repo, "README.md", "Initial commit");
    first_commit_all(&repo, "Initial commit");

    // Rename master to main
    {
        let mut master_branch = repo.find_branch("master", git2::BranchType::Local).unwrap();
        master_branch.rename("main", false).unwrap();
    }

    // Create a branch
    create_branch(&repo, "feature-with-pr");
    checkout_branch(&repo, "feature-with-pr");
    create_new_file(&path_to_repo, "feature.txt", "Feature");
    commit_all(&repo, "Add feature");

    // Initialize chain
    run_test_bin_expect_ok(&path_to_repo, ["init", "test-chain", "main"]);

    // Update PATH
    let original_path = env::var("PATH").unwrap_or_default();
    let absolute_mock_dir = mock_dir.canonicalize().unwrap();
    let new_path = format!("{}:{}", absolute_mock_dir.display(), original_path);
    env::set_var("PATH", new_path);

    // Run list command with --pr flag
    let output = run_test_bin(path_to_repo, ["list", "--pr"]);

    // Restore original PATH
    env::set_var("PATH", original_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Debug output
    println!("=== TEST DIAGNOSTICS ===");
    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status);
    println!("======");

    // Assertions
    assert!(output.status.success(), "Command should succeed");
    assert!(stdout.contains("test-chain"), "Should show chain name");
    assert!(
        stdout.contains("feature-with-pr"),
        "Should show branch name"
    );
    assert!(
        stdout.contains("https://github.com/test/repo/pull/123"),
        "Should show PR URL for feature-with-pr, got: {}",
        stdout
    );
    assert!(
        stdout.contains("[Open]") || stdout.contains("[OPEN]"),
        "Should show PR state as Open, got: {}",
        stdout
    );

    teardown_git_repo(test_name);
}

#[test]
fn test_status_command_with_pr_flag() {
    let test_name = "test_status_with_pr";
    let repo = setup_git_repo(test_name);
    let path_to_repo = generate_path_to_repo(test_name);

    // Set up mock gh
    let mock_dir = setup_mock_gh(test_name);

    // Create initial commit on main branch
    create_new_file(&path_to_repo, "README.md", "Initial commit");
    first_commit_all(&repo, "Initial commit");

    // Rename master to main
    {
        let mut master_branch = repo.find_branch("master", git2::BranchType::Local).unwrap();
        master_branch.rename("main", false).unwrap();
    }

    // Create a branch
    create_branch(&repo, "feature-with-pr");
    checkout_branch(&repo, "feature-with-pr");
    create_new_file(&path_to_repo, "feature.txt", "Feature");
    commit_all(&repo, "Add feature");

    // Initialize chain
    run_test_bin_expect_ok(&path_to_repo, ["init", "test-chain", "main"]);

    // Update PATH
    let original_path = env::var("PATH").unwrap_or_default();
    let absolute_mock_dir = mock_dir.canonicalize().unwrap();
    let new_path = format!("{}:{}", absolute_mock_dir.display(), original_path);
    env::set_var("PATH", new_path);

    // Run status command with --pr flag
    let output = run_test_bin(path_to_repo, ["status", "--pr"]);

    // Restore original PATH
    env::set_var("PATH", original_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Debug output
    println!("=== TEST DIAGNOSTICS ===");
    println!("STDOUT: {}", stdout);
    println!("STDERR: {}", stderr);
    println!("EXIT STATUS: {}", output.status);
    println!("======");

    // Assertions
    assert!(output.status.success(), "Command should succeed");
    assert!(stdout.contains("test-chain"), "Should show chain name");
    assert!(
        stdout.contains("feature-with-pr"),
        "Should show branch name"
    );
    assert!(
        stdout.contains("https://github.com/test/repo/pull/123"),
        "Should show PR URL for feature-with-pr, got: {}",
        stdout
    );

    teardown_git_repo(test_name);
}
