use git2::{IndexAddOption, Oid, Repository};
use std::fs;
use std::path::{Path, PathBuf};

pub fn generate_path_to_repo<S>(repo_name: S) -> PathBuf
where
    S: Into<String>,
{
    let repo_name: String = repo_name.into();
    let test_fixture_path = Path::new("./test_sandbox/");
    let path_to_repo = test_fixture_path.join(repo_name);
    assert!(path_to_repo.is_relative());
    path_to_repo
}

pub fn setup_git_repo<S>(repo_name: S) -> Repository
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

pub fn teardown_git_repo<S>(repo_name: S)
where
    S: Into<String>,
{
    let path_to_repo = generate_path_to_repo(repo_name);
    fs::remove_dir_all(&path_to_repo).ok();
}

pub fn checkout_branch(repo: &Repository, branch_name: &str) {
    let obj = repo
        .revparse_single(&("refs/heads/".to_owned() + branch_name))
        .unwrap();

    repo.checkout_tree(&obj, None).unwrap();

    repo.set_head(&("refs/heads/".to_owned() + branch_name))
        .unwrap();
}

pub fn stage_everything(repo: &Repository) -> Oid {
    let mut index = repo.index().expect("cannot get the Index file");
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();

    let mut index = repo.index().unwrap();
    let root_tree_oid = index.write_tree().unwrap();
    root_tree_oid
}
