use std::path::PathBuf;

pub mod common;
use common::{
    checkout_branch, commit_all, create_branch, create_new_file, display_outputs, first_commit_all,
    generate_path_to_bare_repo, generate_path_to_repo, get_current_branch_name, run_git_command,
    run_test_bin_expect_ok, setup_git_bare_repo, setup_git_repo, teardown_git_repo,
};

#[test]
fn push_subcommand() {
    let repo_name = "push_subcommand";
    let repo = setup_git_repo(repo_name);
    let _bare_repo = setup_git_bare_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    let path_to_bare_repo = {
        let mut path_to_bare_repo_buf: PathBuf = generate_path_to_bare_repo(repo_name);
        if path_to_bare_repo_buf.is_relative() {
            path_to_bare_repo_buf = path_to_bare_repo_buf.canonicalize().unwrap();
        }

        path_to_bare_repo_buf.to_str().unwrap().to_string()
    };

    run_git_command(
        path_to_repo.clone(),
        vec!["remote", "add", "origin", &path_to_bare_repo],
    );

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
      some_branch_1 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // git chain push
    let args: Vec<&str> = vec!["push"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🛑 Cannot push. Branch has no upstream: some_branch_1
🛑 Cannot push. Branch has no upstream: some_branch_2
Pushed 0 branches.
"#
        .trim_start()
    );

    run_git_command(
        &path_to_repo,
        vec!["push", "--all", "--set-upstream", "origin"],
    );

    // git chain push
    let args: Vec<&str> = vec!["push"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
✅ Pushed some_branch_1
✅ Pushed some_branch_2
Pushed 2 branches.
"#
        .trim_start()
    );

    teardown_git_repo(repo_name);
}
