#[path = "common/mod.rs"]
pub mod common;

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, get_current_branch_name, run_test_bin_expect_err,
    run_test_bin_expect_ok, setup_git_repo, teardown_git_repo,
};

#[test]
fn backup_fails_with_untracked_files() {
    let repo_name = "backup_fails_with_untracked";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // initial commit on master
    create_new_file(&path_to_repo, "initial.txt", "initial");
    first_commit_all(&repo, "initial commit");

    // create feature branch
    create_branch(&repo, "feature");
    checkout_branch(&repo, "feature");
    create_new_file(&path_to_repo, "feature.txt", "feature");
    commit_all(&repo, "feature commit");

    // initialize chain with root master
    let args = vec!["init", "chain", "master"];
    run_test_bin_expect_ok(&path_to_repo, args);

    // add untracked file
    create_new_file(&path_to_repo, "untracked.txt", "dirty");

    // attempt backup and expect failure mentioning branch name
    let args = vec!["backup"];
    let output = run_test_bin_expect_err(&path_to_repo, args);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("uncommitted"));
    assert!(stderr.contains(&get_current_branch_name(&repo)));

    teardown_git_repo(repo_name);
}

#[test]
fn merge_fails_with_untracked_files() {
    let repo_name = "merge_fails_with_untracked";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // initial commit on master
    create_new_file(&path_to_repo, "initial.txt", "initial");
    first_commit_all(&repo, "initial commit");

    // create feature branch and commit
    create_branch(&repo, "feature");
    checkout_branch(&repo, "feature");
    create_new_file(&path_to_repo, "feature.txt", "feature");
    commit_all(&repo, "feature commit");

    // initialize chain with root master
    let args = vec!["init", "chain", "master"];
    run_test_bin_expect_ok(&path_to_repo, args);

    // add untracked file
    create_new_file(&path_to_repo, "untracked.txt", "dirty");

    // attempt merge and expect failure mentioning branch name
    let args = vec!["merge"];
    let output = run_test_bin_expect_err(&path_to_repo, args);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("uncommitted"));
    assert!(stderr.contains(&get_current_branch_name(&repo)));

    teardown_git_repo(repo_name);
}

#[test]
fn rebase_fails_with_untracked_files() {
    let repo_name = "rebase_fails_with_untracked";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    // initial commit on master
    create_new_file(&path_to_repo, "initial.txt", "initial");
    first_commit_all(&repo, "initial commit");

    // create feature branch and commit
    create_branch(&repo, "feature");
    checkout_branch(&repo, "feature");
    create_new_file(&path_to_repo, "feature.txt", "feature");
    commit_all(&repo, "feature commit");

    // initialize chain with root master
    let args = vec!["init", "chain", "master"];
    run_test_bin_expect_ok(&path_to_repo, args);

    // add untracked file
    create_new_file(&path_to_repo, "untracked.txt", "dirty");

    // attempt rebase and expect failure mentioning branch name
    let args = vec!["rebase"];
    let output = run_test_bin_expect_err(&path_to_repo, args);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("uncommitted"));
    assert!(stderr.contains(&get_current_branch_name(&repo)));

    teardown_git_repo(repo_name);
}
