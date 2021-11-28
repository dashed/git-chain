pub mod common;
use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, get_current_branch_name, run_test_bin_expect_err,
    run_test_bin_expect_ok, setup_git_repo, teardown_git_repo,
};

#[test]
fn init_subcommand() {
    let repo_name = "init_subcommand";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    assert_eq!(&get_current_branch_name(&repo), "master");

    // init subcommand with no arguments
    let args: Vec<&str> = vec!["init"];
    let output = run_test_bin_expect_err(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("The following required arguments were not provided"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("<chain_name>"));

    // init subcommand with chain name, but no root branch
    let args: Vec<&str> = vec!["init", "chain_name"];
    let output = run_test_bin_expect_err(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Please provide the root branch."));

    // init subcommand with chain name, and use current branch as the root branch
    assert_eq!(&get_current_branch_name(&repo), "master");

    let args: Vec<&str> = vec!["init", "chain_name", "master"];
    let output = run_test_bin_expect_err(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Current branch cannot be the root branch: master"));

    // create and checkout new branch named some_branch
    {
        let branch_name = "some_branch";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        // create new file
        create_new_file(&path_to_repo, "file.txt", "contents");

        // add commit to branch some_branch
        commit_all(&repo, "message");
    };

    // init subcommand with chain name, and use master as the root branch
    assert_eq!(&get_current_branch_name(&repo), "some_branch");

    let args: Vec<&str> = vec!["init", "chain_name", "master"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up branch: some_branch

chain_name
    ➜ some_branch ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    teardown_git_repo(repo_name);
}
