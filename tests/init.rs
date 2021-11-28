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

    // create and checkout new branch named some_branch_1
    {
        let branch_name = "some_branch_1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        // create new file
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");

        // add commit to branch some_branch_1
        commit_all(&repo, "message");
    };

    // init subcommand with chain name, and use master as the root branch
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

    let args: Vec<&str> = vec!["init", "chain_name", "master"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up branch: some_branch_1

chain_name
    ➜ some_branch_1 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // create and checkout new branch named some_branch_2
    {
        let branch_name = "some_branch_2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        // create new file
        create_new_file(&path_to_repo, "file_2.txt", "contents 2");

        // add commit to branch some_branch_2
        commit_all(&repo, "message");
    };

    // init subcommand with existing chain name, and use some_branch_1 as the root branch
    assert_eq!(&get_current_branch_name(&repo), "some_branch_2");

    let args: Vec<&str> = vec!["init", "chain_name", "some_branch_1"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
Using root branch master of chain chain_name instead of some_branch_1
🔗 Succesfully set up branch: some_branch_2

chain_name
    ➜ some_branch_2 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // create and checkout new branch named some_branch_3
    {
        let branch_name = "some_branch_3";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        // create new file
        create_new_file(&path_to_repo, "file_3.txt", "contents 3");

        // add commit to branch some_branch_3
        commit_all(&repo, "message");
    };

    // init subcommand with existing chain name without any explicit root branch
    assert_eq!(&get_current_branch_name(&repo), "some_branch_3");

    let args: Vec<&str> = vec!["init", "chain_name"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up branch: some_branch_3

chain_name
    ➜ some_branch_3 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // create and checkout new branch named some_branch_2.5
    {
        checkout_branch(&repo, "some_branch_2");
        let branch_name = "some_branch_2.5";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        // create new file
        create_new_file(&path_to_repo, "file_2.5.txt", "contents 2.5");

        // add commit to branch some_branch_2.5
        commit_all(&repo, "message");
    };

    // Test option: --before=branch
    assert_eq!(&get_current_branch_name(&repo), "some_branch_2.5");

    let args: Vec<&str> = vec!["init", "chain_name", "--before=some_branch_3"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up branch: some_branch_2.5

chain_name
      some_branch_3 ⦁ 1 ahead ⦁ 1 behind
    ➜ some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // create and checkout new branch named some_branch_1.5
    {
        checkout_branch(&repo, "some_branch_1");
        let branch_name = "some_branch_1.5";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        // create new file
        create_new_file(&path_to_repo, "file_1.5.txt", "contents 1.5");

        // add commit to branch some_branch_1.5
        commit_all(&repo, "message");
    };

    // Test option: --after=branch
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1.5");

    let args: Vec<&str> = vec!["init", "chain_name", "--after=some_branch_1"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up branch: some_branch_1.5

chain_name
      some_branch_3 ⦁ 1 ahead ⦁ 1 behind
      some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead ⦁ 1 behind
    ➜ some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // create and checkout new branch named some_branch_0
    {
        checkout_branch(&repo, "master");
        let branch_name = "some_branch_0";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        // create new file
        create_new_file(&path_to_repo, "file_0.txt", "contents 0");

        // add commit to branch some_branch_0
        commit_all(&repo, "message");
    };

    // Test option: --first
    assert_eq!(&get_current_branch_name(&repo), "some_branch_0");

    let args: Vec<&str> = vec!["init", "chain_name", "--first"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up branch: some_branch_0

chain_name
      some_branch_3 ⦁ 1 ahead ⦁ 1 behind
      some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead ⦁ 1 behind
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead ⦁ 1 behind
    ➜ some_branch_0 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_0

chain_name
      some_branch_3 ⦁ 1 ahead ⦁ 1 behind
      some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead ⦁ 1 behind
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead ⦁ 1 behind
    ➜ some_branch_0 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    teardown_git_repo(repo_name);
}
