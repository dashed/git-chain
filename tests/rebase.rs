use console;

use git2::RepositoryState;

#[path = "common/mod.rs"]
pub mod common;

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, get_current_branch_name, run_git_command, run_test_bin_expect_err,
    run_test_bin_expect_ok, run_test_bin_for_rebase, setup_git_repo, teardown_git_repo,
};

#[test]
fn rebase_subcommand_simple() {
    let repo_name = "rebase_subcommand_simple";
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

        // create new file
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");

        // add commit to branch some_branch_1
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

    // create and checkout new branch named some_branch_3
    {
        let branch_name = "some_branch_3";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_3");

        // create new file
        create_new_file(&path_to_repo, "file_3.txt", "contents 3");

        // add commit to branch some_branch_3
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_2.5
    {
        checkout_branch(&repo, "some_branch_2");
        let branch_name = "some_branch_2.5";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_2.5");

        // create new file
        create_new_file(&path_to_repo, "file_2.5.txt", "contents 2.5");

        // add commit to branch some_branch_2.5
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_1.5
    {
        checkout_branch(&repo, "some_branch_1");
        let branch_name = "some_branch_1.5";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_1.5");

        // create new file
        create_new_file(&path_to_repo, "file_1.5.txt", "contents 1.5");

        // add commit to branch some_branch_1.5
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_0
    {
        checkout_branch(&repo, "master");
        let branch_name = "some_branch_0";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_0");

        // create new file
        create_new_file(&path_to_repo, "file_0.txt", "contents 0");

        // add commit to branch some_branch_0
        commit_all(&repo, "message");
    };

    assert_eq!(&get_current_branch_name(&repo), "some_branch_0");

    // run git chain setup
    let args: Vec<&str> = vec![
        "setup",
        "chain_name",
        "master",
        "some_branch_0",
        "some_branch_1",
        "some_branch_1.5",
        "some_branch_2",
        "some_branch_2.5",
        "some_branch_3",
    ];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up chain: chain_name

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

    // git chain rebase
    let args: Vec<&str> = vec!["rebase"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Current branch some_branch_0 is up to date."));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("🎉 Successfully rebased chain chain_name"));

    let actual = console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
        .trim()
        .replace("\r", "\n");
    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_1."));
    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_1.5."));
    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_2."));
    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_2.5."));

    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_3."));

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_0

chain_name
      some_branch_3 ⦁ 1 ahead
      some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
    ➜ some_branch_0 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // git chain rebase
    let args: Vec<&str> = vec!["rebase"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Chain chain_name is already up-to-date.")
    );

    teardown_git_repo(repo_name);
}

#[test]
fn rebase_subcommand_conflict() {
    let repo_name = "rebase_subcommand_conflict";
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

        // create new file
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");

        // add commit to branch some_branch_1
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

    {
        // create a conflict
        checkout_branch(&repo, "some_branch_1");
        create_new_file(&path_to_repo, "file_2.txt", "conflict");
        commit_all(&repo, "add conflict");
    };

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_1

chain_name
      some_branch_2 ⦁ 1 ahead ⦁ 1 behind
    ➜ some_branch_1 ⦁ 2 ahead
      master (root branch)
"#
        .trim_start()
    );

    // git chain rebase
    assert_eq!(&get_current_branch_name(&repo), "some_branch_1");

    let args: Vec<&str> = vec!["rebase"];
    let output = run_test_bin_expect_err(&path_to_repo, args);

    assert_eq!(&get_current_branch_name(&repo), "HEAD");

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Current branch some_branch_1 is up to date"));

    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        r#"
🛑 Unable to completely rebase some_branch_2 to some_branch_1
⚠️  Resolve any rebase merge conflicts, and then run git chain rebase
"#
        .trim_start()
    );

    assert_eq!(repo.state(), RepositoryState::RebaseInteractive);

    commit_all(&repo, "add conflict");
    run_git_command(&path_to_repo, vec!["rebase", "--continue"]);

    assert_eq!(repo.state(), RepositoryState::Clean);
    assert_eq!(&get_current_branch_name(&repo), "some_branch_2");

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_2

chain_name
    ➜ some_branch_2 ⦁ 1 ahead
      some_branch_1 ⦁ 2 ahead
      master (root branch)
"#
        .trim_start()
    );

    teardown_git_repo(repo_name);
}

#[test]
fn rebase_subcommand_step() {
    let repo_name = "rebase_subcommand_step";
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

        // create new file
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");

        // add commit to branch some_branch_1
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

    // create and checkout new branch named some_branch_3
    {
        let branch_name = "some_branch_3";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_3");

        // create new file
        create_new_file(&path_to_repo, "file_3.txt", "contents 3");

        // add commit to branch some_branch_3
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_2.5
    {
        checkout_branch(&repo, "some_branch_2");
        let branch_name = "some_branch_2.5";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_2.5");

        // create new file
        create_new_file(&path_to_repo, "file_2.5.txt", "contents 2.5");

        // add commit to branch some_branch_2.5
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_1.5
    {
        checkout_branch(&repo, "some_branch_1");
        let branch_name = "some_branch_1.5";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_1.5");

        // create new file
        create_new_file(&path_to_repo, "file_1.5.txt", "contents 1.5");

        // add commit to branch some_branch_1.5
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_0
    {
        checkout_branch(&repo, "master");
        let branch_name = "some_branch_0";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_0");

        // create new file
        create_new_file(&path_to_repo, "file_0.txt", "contents 0");

        // add commit to branch some_branch_0
        commit_all(&repo, "message");
    };

    assert_eq!(&get_current_branch_name(&repo), "some_branch_0");

    // run git chain setup
    let args: Vec<&str> = vec![
        "setup",
        "chain_name",
        "master",
        "some_branch_0",
        "some_branch_1",
        "some_branch_1.5",
        "some_branch_2",
        "some_branch_2.5",
        "some_branch_3",
    ];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up chain: chain_name

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

    // git chain rebase --step
    let args: Vec<&str> = vec!["rebase", "--step"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Current branch some_branch_0 is up to date."));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Performed one rebase on branch: some_branch_1"));

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("To continue rebasing, run git chain rebase --step"));

    assert!(
        console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
            .trim()
            .replace("\r", "\n")
            .contains("Successfully rebased and updated refs/heads/some_branch_1."),
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
      some_branch_1.5 ⦁ 2 ahead ⦁ 2 behind
      some_branch_1 ⦁ 1 ahead
    ➜ some_branch_0 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // git chain rebase --step
    let args: Vec<&str> = vec!["rebase", "--step"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Performed one rebase on branch: some_branch_1.5"));

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("To continue rebasing, run git chain rebase --step"));

    assert!(
        console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
            .trim()
            .replace("\r", "\n")
            .contains("Successfully rebased and updated refs/heads/some_branch_1.5."),
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
      some_branch_2 ⦁ 2 ahead ⦁ 3 behind
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
    ➜ some_branch_0 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // git chain rebase --step
    let args: Vec<&str> = vec!["rebase", "--step"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Performed one rebase on branch: some_branch_2"));

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("To continue rebasing, run git chain rebase --step"));

    assert!(
        console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
            .trim()
            .replace("\r", "\n")
            .contains("Successfully rebased and updated refs/heads/some_branch_2."),
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
      some_branch_2.5 ⦁ 3 ahead ⦁ 4 behind
      some_branch_2 ⦁ 1 ahead
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
    ➜ some_branch_0 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // git chain rebase --step
    let args: Vec<&str> = vec!["rebase", "--step"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Performed one rebase on branch: some_branch_2.5"));

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("To continue rebasing, run git chain rebase --step"));

    assert!(
        console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
            .trim()
            .replace("\r", "\n")
            .contains("Successfully rebased and updated refs/heads/some_branch_2.5."),
    );

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_0

chain_name
      some_branch_3 ⦁ 3 ahead ⦁ 5 behind
      some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
    ➜ some_branch_0 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    // git chain rebase --step
    let args: Vec<&str> = vec!["rebase", "--step"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("🎉 Successfully rebased chain chain_name"));

    assert!(
        console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
            .trim()
            .replace("\r", "\n")
            .contains("Successfully rebased and updated refs/heads/some_branch_3."),
    );

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_0

chain_name
      some_branch_3 ⦁ 1 ahead
      some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
    ➜ some_branch_0 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    teardown_git_repo(repo_name);
}

#[test]
fn rebase_subcommand_squashed_merged_branch() {
    let repo_name = "rebase_subcommand_squashed_merged_branch";
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

    teardown_git_repo(repo_name);
}

#[test]
fn rebase_subcommand_ignore_root() {
    let repo_name = "rebase_subcommand_ignore_root";
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

        // create new file
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");

        // add commit to branch some_branch_1
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

    // create and checkout new branch named some_branch_3
    {
        let branch_name = "some_branch_3";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_3");

        // create new file
        create_new_file(&path_to_repo, "file_3.txt", "contents 3");

        // add commit to branch some_branch_3
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_2.5
    {
        checkout_branch(&repo, "some_branch_2");
        let branch_name = "some_branch_2.5";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_2.5");

        // create new file
        create_new_file(&path_to_repo, "file_2.5.txt", "contents 2.5");

        // add commit to branch some_branch_2.5
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_1.5
    {
        checkout_branch(&repo, "some_branch_1");
        let branch_name = "some_branch_1.5";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_1.5");

        // create new file
        create_new_file(&path_to_repo, "file_1.5.txt", "contents 1.5");

        // add commit to branch some_branch_1.5
        commit_all(&repo, "message");
    };

    // create and checkout new branch named some_branch_0
    {
        checkout_branch(&repo, "master");
        let branch_name = "some_branch_0";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_0");

        // create new file
        create_new_file(&path_to_repo, "file_0.txt", "contents 0");

        // add commit to branch some_branch_0
        commit_all(&repo, "message");
    };

    // checkout master branch and create a new file
    {
        checkout_branch(&repo, "master");
        assert_eq!(&get_current_branch_name(&repo), "master");

        // create new file
        create_new_file(&path_to_repo, "canada.txt", "canada");

        // add commit to branch master
        commit_all(&repo, "message");
    };

    checkout_branch(&repo, "some_branch_0");
    assert_eq!(&get_current_branch_name(&repo), "some_branch_0");

    // run git chain setup
    let args: Vec<&str> = vec![
        "setup",
        "chain_name",
        "master",
        "some_branch_0",
        "some_branch_1",
        "some_branch_1.5",
        "some_branch_2",
        "some_branch_2.5",
        "some_branch_3",
    ];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up chain: chain_name

chain_name
      some_branch_3 ⦁ 1 ahead ⦁ 1 behind
      some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead ⦁ 1 behind
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead ⦁ 1 behind
    ➜ some_branch_0 ⦁ 1 ahead ⦁ 1 behind
      master (root branch)
"#
        .trim_start()
    );

    // git chain rebase --ignore-root
    let args: Vec<&str> = vec!["rebase", "--ignore-root"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("⚠️  Not rebasing branch some_branch_0 against root branch master. Skipping."));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("⚠️ Did not rebase chain against root branch: master"));
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("🎉 Successfully rebased chain chain_name"));

    let actual = console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
        .trim()
        .replace("\r", "\n");
    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_1."));
    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_1.5."));
    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_2."));
    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_2.5."));

    assert!(actual.contains("Successfully rebased and updated refs/heads/some_branch_3."));

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: some_branch_0

chain_name
      some_branch_3 ⦁ 1 ahead
      some_branch_2.5 ⦁ 1 ahead
      some_branch_2 ⦁ 1 ahead
      some_branch_1.5 ⦁ 1 ahead
      some_branch_1 ⦁ 1 ahead
    ➜ some_branch_0 ⦁ 1 ahead ⦁ 1 behind
      master (root branch)
"#
        .trim_start()
    );

    // git chain rebase --ignore-root
    let args: Vec<&str> = vec!["rebase", "--ignore-root"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("⚠️  Not rebasing branch some_branch_0 against root branch master. Skipping."));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Switching back to branch: some_branch_0")
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("⚠️ Did not rebase chain against root branch: master"));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Chain chain_name is already up-to-date.")
    );

    teardown_git_repo(repo_name);
}

#[test]
fn rebase_no_forkpoint() {
    std::env::set_var("LANG", "C");
    let repo_name = "rebase_no_forkpoint";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");

        assert_eq!(&get_current_branch_name(&repo), "master");
    };

    // create and checkout new branch named feature_1
    {
        let branch_name = "feature_1";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "feature_1");

        // create new file
        create_new_file(&path_to_repo, "file_1.txt", "contents 1");

        // add commit to branch feature_1
        commit_all(&repo, "message");
    };

    // go back to master branch and add a few commits
    {
        checkout_branch(&repo, "master");

        // create new file
        create_new_file(&path_to_repo, "hello_world2.txt", "Hello, world!");

        // add commit to master
        commit_all(&repo, "hello_world");

        // create new file
        create_new_file(&path_to_repo, "hello_world3.txt", "Hello, world!");

        // add commit to master
        commit_all(&repo, "hello_world");

        assert_eq!(&get_current_branch_name(&repo), "master");
    };

    // // check out HEAD~2
    // {
    //     let commit = repo
    //         .find_commit(repo.head().unwrap().target().unwrap())
    //         .unwrap();
    //     let parent = commit.parent(0).unwrap();
    //     let parent = parent.parent(0).unwrap();
    //     repo.set_head_detached(parent.id()).unwrap();
    // };

    // create and checkout new branch named feature_2
    {
        let branch_name = "feature_2";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
    };

    {
        assert_eq!(&get_current_branch_name(&repo), "feature_2");

        // create new file
        create_new_file(&path_to_repo, "file_2.txt", "contents 1");

        // add commit to branch feature_1
        commit_all(&repo, "message");
    };

    // run git chain setup
    let args: Vec<&str> = vec!["setup", "chain_name", "master", "feature_1", "feature_2"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🔗 Succesfully set up chain: chain_name

chain_name
    ➜ feature_2 ⦁ 3 ahead ⦁ 1 behind
      feature_1 ⦁ 1 ahead ⦁ 2 behind
      master (root branch)
"#
        .trim_start(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    // This prevents a fork point between feature_1 and master from being found.
    let output = run_git_command(
        &path_to_repo,
        vec!["reflog", "expire", "--all", "--expire=all"],
    );
    assert!(output.status.success());

    // assert that there is no fork point between feature_1 and master
    let output = run_git_command(
        &path_to_repo,
        vec!["merge-base", "--fork-point", "master", "feature_1"],
    );
    assert!(!output.status.success());
    assert_eq!(output.status.code().unwrap(), 1);

    // git chain rebase
    let args: Vec<&str> = vec!["rebase"];
    let output = run_test_bin_for_rebase(&path_to_repo, args);

    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("🎉 Successfully rebased chain chain_name"));

    let actual = console::strip_ansi_codes(&String::from_utf8_lossy(&output.stderr))
        .trim()
        .replace("\r", "\n");

    // Successfully rebased and updated refs/heads/feature_1.
    // dropping 408c36d18367659844a8d55411831e32c452b217 hello_world -- patch contents already upstream
    // dropping 7e78446b248d162cdc7de3c1aaec9455c642adda hello_world -- patch contents already upstream
    // Successfully rebased and updated refs/heads/feature_2.
    assert!(actual.contains("Successfully rebased and updated refs/heads/feature_1."));
    assert!(actual.contains("Successfully rebased and updated refs/heads/feature_2."));
    assert!(actual.contains("hello_world -- patch contents already upstream"));
    assert!(
        actual
            .matches("hello_world -- patch contents already upstream")
            .count()
            == 2
    );
    assert!(actual.matches("dropping").count() == 2);

    // git chain
    let args: Vec<&str> = vec![];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
On branch: feature_2

chain_name
    ➜ feature_2 ⦁ 1 ahead
      feature_1 ⦁ 1 ahead
      master (root branch)
"#
        .trim_start()
    );

    teardown_git_repo(repo_name);
}
