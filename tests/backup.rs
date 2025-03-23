#[path = "common/mod.rs"]
pub mod common;

use common::{
    branch_equal, branch_exists, checkout_branch, commit_all, create_branch, create_new_file,
    first_commit_all, generate_path_to_repo, get_current_branch_name, run_test_bin_expect_ok,
    setup_git_repo, teardown_git_repo,
};

fn backup_name(chain_name: &str, branch_name: &str) -> String {
    format!("backup-{}/{}", chain_name, branch_name)
}

#[test]
fn backup_subcommand() {
    let repo_name = "backup_subcommand";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", "Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    assert_eq!(&get_current_branch_name(&repo), "master");

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

    // init subcommand with chain name, and use master as the root branch
    assert_eq!(&get_current_branch_name(&repo), "some_branch_3");

    let args: Vec<&str> = vec!["init", "chain_name_2"];
    run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        branch_exists(&repo, &backup_name("chain_name_2", "some_branch_2")),
        false
    );
    assert_eq!(
        branch_exists(&repo, &backup_name("chain_name_2", "some_branch_3")),
        false
    );

    let args: Vec<&str> = vec!["backup"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🎉 Successfully backed up chain: chain_name_2
"#
        .trim_start()
    );

    assert!(branch_exists(
        &repo,
        &backup_name("chain_name_2", "some_branch_2")
    ));
    assert!(branch_exists(
        &repo,
        &backup_name("chain_name_2", "some_branch_3")
    ));
    assert!(branch_equal(
        &repo,
        "some_branch_2",
        &backup_name("chain_name_2", "some_branch_2")
    ));
    assert!(branch_equal(
        &repo,
        "some_branch_3",
        &backup_name("chain_name_2", "some_branch_3")
    ));

    {
        assert_eq!(&get_current_branch_name(&repo), "some_branch_3");
        // create new file
        create_new_file(&path_to_repo, "file_3.5.txt", "contents 3.5");

        // add commit to branch some_branch_3
        commit_all(&repo, "message");
    };

    assert!(
        branch_equal(
            &repo,
            "some_branch_3",
            &backup_name("chain_name_2", "some_branch_3")
        ) == false
    );

    let args: Vec<&str> = vec!["backup"];
    let output = run_test_bin_expect_ok(&path_to_repo, args);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"
🎉 Successfully backed up chain: chain_name_2
"#
        .trim_start()
    );

    assert!(branch_exists(
        &repo,
        &backup_name("chain_name_2", "some_branch_2")
    ));
    assert!(branch_exists(
        &repo,
        &backup_name("chain_name_2", "some_branch_3")
    ));
    assert!(branch_equal(
        &repo,
        "some_branch_2",
        &backup_name("chain_name_2", "some_branch_2")
    ));
    assert!(branch_equal(
        &repo,
        "some_branch_3",
        &backup_name("chain_name_2", "some_branch_3")
    ));

    teardown_git_repo(repo_name);
}
