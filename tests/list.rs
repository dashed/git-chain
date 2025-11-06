#[path = "common/mod.rs"]
pub mod common;

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, get_current_branch_name, run_test_bin_expect_ok, setup_git_repo,
    teardown_git_repo,
};

#[test]
fn list_subcommand() {
    let repo_name = "list_subcommand";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    assert_eq!(&get_current_branch_name(&repo), "master");

    let args: Vec<&str> = vec!["list"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
No chains to list.
To initialize a chain for this branch, run git chain init <chain_name> <root_branch>
"#
        .trim_start()
    );

    // create and checkout new branch named not_part_of_any_chain
    {
        let branch_name = "not_part_of_any_chain";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        // create new file
        create_new_file(&path_to_repo, "not_part_of_any_chain.txt", "contents");

        // add commit to branch not_part_of_any_chain
        commit_all(&repo, "message");
    };
    assert_eq!(&get_current_branch_name(&repo), "not_part_of_any_chain");

    // create and checkout new branch named some_branch_1
    {
        checkout_branch(&repo, "master");
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
    run_test_bin_expect_ok(&path_to_repo, args);

    let args: Vec<&str> = vec!["list"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
chain_name
    ➜ some_branch_1 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // create and checkout new branch named some_branch_2
    {
        checkout_branch(&repo, "master");
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

    // init subcommand with chain name, and use master as the root branch
    assert_eq!(&get_current_branch_name(&repo), "some_branch_2");

    let args: Vec<&str> = vec!["init", "chain_name_2", "master"];
    run_test_bin_expect_ok(&path_to_repo, args);

    let args: Vec<&str> = vec!["list"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
chain_name
      some_branch_1 ⦁ 1 ahead
      master (root branch)

chain_name_2
    ➜ some_branch_2 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    teardown_git_repo(repo_name);
}
