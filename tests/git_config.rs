use git2::{BranchType, ConfigLevel};
use std::fs::File;
use std::io::prelude::*;

mod common;
use common::{
    checkout_branch, commit_all, create_branch, first_commit_all, generate_path_to_repo,
    setup_git_repo, teardown_git_repo,
};

#[test]
fn deleted_branch_config_verification() {
    // This test verifies a git behaviour whereby deleting a branch will delete any and all configs whose keys begin with: branch.<name>
    // Reference: https://github.com/git/git/blob/f443b226ca681d87a3a31e245a70e6bc2769123c/builtin/branch.c#L184-L191

    let repo_name = "deleted_branch_config_verification";

    let repo = setup_git_repo(repo_name);

    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        let mut file = File::create(path_to_repo.as_path().join("hello_world.txt")).unwrap();
        file.write_all(b"Hello, world!").unwrap();

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    // create and checkout new branch named some_branch
    let branch_name = {
        let branch_name = "some_branch";
        create_branch(&repo, branch_name);
        checkout_branch(&repo, branch_name);
        branch_name
    };

    {
        // create new file

        let mut file = File::create(path_to_repo.as_path().join("file.txt")).unwrap();
        file.write_all(b"contents").unwrap();

        // add commit to branch some_branch
        commit_all(&repo, "message");
    };

    // add custom config
    let repo_config = repo.config().unwrap();
    let mut local_config = repo_config.open_level(ConfigLevel::Local).unwrap();

    let config_key = format!("branch.{}.chain-name", branch_name);

    // verify config_key does not exist yet
    for entry in &local_config.entries(None).unwrap() {
        let entry = entry.unwrap();
        assert_ne!(entry.name().unwrap(), config_key);
    }

    local_config.set_str(&config_key, "chain_name").unwrap();

    let actual_value = local_config.get_string(&config_key).unwrap();
    assert_eq!(actual_value, "chain_name");

    // checkout master
    checkout_branch(&repo, "master");

    // delete branch some_branch
    let mut some_branch = repo.find_branch(branch_name, BranchType::Local).unwrap();
    assert!(!some_branch.is_head());
    some_branch.delete().unwrap();

    // verify if local custom config is deleted
    for entry in &local_config.entries(None).unwrap() {
        let entry = entry.unwrap();
        assert_ne!(entry.name().unwrap(), config_key);
    }

    teardown_git_repo(repo_name);
}
