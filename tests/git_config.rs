use git2::{BranchType, ConfigLevel, IndexAddOption, Oid, Repository};
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

fn generate_path_to_repo<S>(repo_name: S) -> PathBuf
where
    S: Into<String>,
{
    let repo_name: String = repo_name.into();
    let test_fixture_path = Path::new("./test_sandbox/");
    let path_to_repo = test_fixture_path.join(repo_name);
    assert!(path_to_repo.is_relative());
    path_to_repo
}

fn setup_git_repo<S>(repo_name: S) -> Repository
where
    S: Into<String>,
{
    let path_to_repo = generate_path_to_repo(repo_name);

    fs::remove_dir_all(&path_to_repo).ok();
    fs::create_dir_all(&path_to_repo).unwrap();

    let repo = match Repository::init(path_to_repo) {
        Ok(repo) => repo,
        Err(err) => panic!("failed to init repo: {}", err),
    };

    let mut config = repo.config().unwrap();
    config.set_str("user.name", "name").unwrap();
    config.set_str("user.email", "email").unwrap();

    repo
}

fn teardown_git_repo<S>(repo_name: S)
where
    S: Into<String>,
{
    let path_to_repo = generate_path_to_repo(repo_name);
    fs::remove_dir_all(&path_to_repo).ok();
}

fn checkout_branch(repo: &Repository, branch_name: &str) {
    let obj = repo
        .revparse_single(&("refs/heads/".to_owned() + branch_name))
        .unwrap();

    repo.checkout_tree(&obj, None).unwrap();

    repo.set_head(&("refs/heads/".to_owned() + branch_name))
        .unwrap();
}

fn stage_everything(repo: &Repository) -> Oid {
    let mut index = repo.index().expect("cannot get the Index file");
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();

    let mut index = repo.index().unwrap();
    let root_tree_oid = index.write_tree().unwrap();
    root_tree_oid
}

#[test]
fn deleted_branch_config_verification() {
    // This test verifies a git behaviour whereby deleting a branch will delete any and all configs whose keys begin with: branch.<name>
    // Reference: https://github.com/git/git/blob/f443b226ca681d87a3a31e245a70e6bc2769123c/builtin/branch.c#L184-L191

    let repo_name = "deleted_branch_config_verification";

    let repo = setup_git_repo(repo_name);

    let path_to_repo = generate_path_to_repo(repo_name);

    let root_tree_oid = {
        // create new file

        let mut file = File::create(path_to_repo.as_path().join("hello_world.txt")).unwrap();
        file.write_all(b"Hello, world!").unwrap();

        // stage all changes - git add -A *
        stage_everything(&repo)
    };

    // add first commit to master
    {
        let tree = repo.find_tree(root_tree_oid).unwrap();

        let author = &repo.signature().unwrap();
        let committer = &author;
        let message = "first commit";

        repo.commit(Some("HEAD"), author, committer, message, &tree, &[])
            .unwrap();
    };

    // create and checkout new branch named some_branch
    let branch_name = {
        let branch_name = "some_branch";

        // create branch
        let oid = repo.head().unwrap().target().unwrap();
        let commit = repo.find_commit(oid).unwrap();

        repo.branch("some_branch", &commit, false).unwrap();

        checkout_branch(&repo, branch_name);

        branch_name
    };

    let root_tree_oid = {
        // create new file

        let mut file = File::create(path_to_repo.as_path().join("file.txt")).unwrap();
        file.write_all(b"contents").unwrap();

        // stage all changes - git add -A *
        stage_everything(&repo)
    };

    // add commit to branch some_branch
    {
        let tree = repo.find_tree(root_tree_oid).unwrap();
        let head_id = repo.refname_to_id("HEAD").unwrap();
        let parent = repo.find_commit(head_id).unwrap();

        let author = &repo.signature().unwrap();
        let committer = &author;
        let message = "message";

        repo.commit(Some("HEAD"), author, committer, message, &tree, &[&parent])
            .unwrap();
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
