#[path = "common/mod.rs"]
pub mod common;

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, get_current_branch_name, run_git_command, run_test_bin_expect_err,
    run_test_bin_expect_ok, run_test_bin_for_rebase, setup_git_repo, teardown_git_repo,
};

#[test]
fn prune_subcommand_squashed_merged_branch() {
    let repo_name = "prune_subcommand_squashed_merged_branch";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    assert_eq!(&get_current_branch_name(&repo), "master");

    // create and checkout new branch named some_branch_1
    {
        let branch_name = "some_branch_1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "message");

        create_new_file(&path_to_repo, "file_1.txt", "contents 2");
        commit_all(&repo, "message");

        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_2
    {
        let branch_name = "some_branch_2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_2");

        // create new file
        create_new_file(&path_to_repo, "file_2.txt", "contents 2");

        // add commit to branch some_branch_2
        commit_all(&repo, "message");
    };

    // run git chain setup
    let args: Vec<&str> = vec![
        "setup",
        "chain_name",
        "master",
        "some_branch_1",
        "some_branch_2",
    ];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up chain: chain_name

chain_name
    ➜ some_branch_2 ⦁ 1 ahead
      some_branch_1 ⦁ 3 ahead
      master (root branch)
"#
        .trim_start()
    );

    // squash and merge some_branch_1 onto master
    checkout_branch(&repo, "master");
    run_git_command(&path_to_repo, vec!["merge", "--squash", "some_branch_1"]);
    commit_all(&repo, "squash merge");

    // git chain rebase
    checkout_branch(&repo, "some_branch_1");
    let args: Vec<&str> = vec!["rebase"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("⚠️  Branch some_branch_1 is detected to be squashed and merged onto master."));
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Resetting branch some_branch_1 to master"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("git reset --hard master"));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_1")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("🎉 Successfully rebased chain chain_name"));

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_1

chain_name
      some_branch_2 ⦁ 1 ahead
    ➜ some_branch_1
      master (root branch)
"#
        .trim_start()
    );

    // git chain prune
    let args: Vec<&str> = vec!["prune"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
Removed the following branches from chain: chain_name

some_branch_1

Pruned 1 branches.
"#
        .trim_start()
    );

    // git chain
    checkout_branch(&repo, "some_branch_2");
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_2

chain_name
    ➜ some_branch_2 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    teardown_git_repo(repo_name);
}

#[test]
fn prune_nonexistent_chain() {
    let repo_name = "prune_nonexistent_chain";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");
        first_commit_all(&repo, "first commit");
    };

    assert_eq!(&get_current_branch_name(&repo), "master");

    // create a branch and init a chain
    {
        let branch_name = "some_branch_1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");
        commit_all(&repo, "message");
    };

    let args: Vec<&str> = vec!["init", "real_chain", "master"];
    run_test_bin_expect_ok(&path_to_repo, args);

    // Switch to master (not part of any chain) and try to prune — should return error
    checkout_branch(&repo, "master");

    let args: Vec<&str> = vec!["prune"];
    let output = run_test_bin_expect_err(&path_to_repo, args);

    let stderr = console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
        .trim()
        .to_string();

    // Diagnostic printing
    println!("STDERR: {}", stderr);
    println!("EXIT CODE: {}", output.status.code().unwrap_or(-1));

    assert!(
        stderr.contains("not part of any chain"),
        "stderr should indicate branch is not part of any chain, got: {}",
        stderr
    );

    teardown_git_repo(repo_name);
}
