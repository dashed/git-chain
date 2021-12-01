use std::ffi::OsStr;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use git2::{BranchType, IndexAddOption, ObjectType, Oid, Repository};

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

fn generate_path_to_bare_repo<S>(repo_name: S) -> PathBuf
where
    S: Into<String>,
{
    let repo_name: String = repo_name.into();
    generate_path_to_repo(format!("bare_{}.git", repo_name))
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

pub fn setup_git_bare_repo<S>(repo_name: S) -> Repository
where
    S: Into<String>,
{
    let path_to_bare_repo = generate_path_to_bare_repo(repo_name);

    fs::remove_dir_all(&path_to_bare_repo).ok();
    fs::create_dir_all(&path_to_bare_repo).unwrap();

    let repo = match Repository::init_bare(path_to_bare_repo) {
        Ok(repo) => repo,
        Err(err) => panic!("failed to init bare repo: {}", err),
    };

    repo
}

pub fn teardown_git_repo<S>(repo_name: S)
where
    S: Into<String>,
{
    let path_to_repo = generate_path_to_repo(repo_name);
    fs::remove_dir_all(&path_to_repo).ok();
}

pub fn teardown_git_bare_repo<S>(repo_name: S)
where
    S: Into<String>,
{
    let path_to_repo = generate_path_to_bare_repo(repo_name);
    fs::remove_dir_all(&path_to_repo).ok();
}

pub fn create_branch(repo: &Repository, branch_name: &str) {
    // create branch from HEAD
    let oid = repo.head().unwrap().target().unwrap();
    let commit = repo.find_commit(oid).unwrap();

    repo.branch(branch_name, &commit, false).unwrap();
}

pub fn checkout_branch(repo: &Repository, branch_name: &str) {
    let obj = repo
        .revparse_single(&("refs/heads/".to_owned() + branch_name))
        .unwrap();

    repo.checkout_tree(&obj, None).unwrap();

    repo.set_head(&("refs/heads/".to_owned() + branch_name))
        .unwrap();
}

pub fn branch_exists(repo: &Repository, branch_name: &str) -> bool {
    repo.revparse_single(&("refs/heads/".to_owned() + branch_name))
        .is_ok()
}

pub fn branch_equal(repo: &Repository, branch_name: &str, other_branch: &str) -> bool {
    let obj = repo
        .revparse_single(&format!("{}^{{commit}}", branch_name))
        .unwrap();
    assert_eq!(obj.kind().unwrap(), ObjectType::Commit);

    let other_obj = repo
        .revparse_single(&format!("{}^{{commit}}", other_branch))
        .unwrap();
    assert_eq!(other_obj.kind().unwrap(), ObjectType::Commit);

    obj.id() == other_obj.id()
}

pub fn stage_everything(repo: &Repository) -> Oid {
    let mut index = repo.index().expect("cannot get the Index file");
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();

    let mut index = repo.index().unwrap();
    // root_tree_oid
    index.write_tree().unwrap()
}

pub fn create_first_commit(repo: &Repository, root_tree_oid: Oid, message: &str) {
    let tree = repo.find_tree(root_tree_oid).unwrap();

    let author = &repo.signature().unwrap();
    let committer = &author;

    repo.commit(Some("HEAD"), author, committer, message, &tree, &[])
        .unwrap();
}

pub fn create_commit(repo: &Repository, root_tree_oid: Oid, message: &str) {
    let tree = repo.find_tree(root_tree_oid).unwrap();
    let head_id = repo.refname_to_id("HEAD").unwrap();
    let parent = repo.find_commit(head_id).unwrap();

    let author = &repo.signature().unwrap();
    let committer = &author;

    repo.commit(Some("HEAD"), author, committer, message, &tree, &[&parent])
        .unwrap();
}

pub fn first_commit_all(repo: &Repository, message: &str) {
    // HEAD should not resolve to anything prior to creating the first commit
    assert!(repo.head().is_err());

    // stage all changes - git add -A *
    let root_tree_oid = stage_everything(repo);

    create_first_commit(repo, root_tree_oid, message);
}

pub fn commit_all(repo: &Repository, message: &str) {
    // stage all changes - git add -A *
    let root_tree_oid = stage_everything(repo);

    create_commit(repo, root_tree_oid, message);
}

pub fn delete_local_branch(repo: &Repository, branch_name: &str) {
    let mut some_branch = repo.find_branch(branch_name, BranchType::Local).unwrap();

    // Should not be able to delete branch_name if it is the current working tree
    assert!(!some_branch.is_head());

    some_branch.delete().unwrap();
}

pub fn get_current_branch_name(repo: &Repository) -> String {
    let head = repo.head().unwrap();
    head.shorthand().unwrap().to_string()
}

pub fn create_new_file(path_to_repo: &Path, file_name: &str, file_contents: &str) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path_to_repo.join(file_name))
        .unwrap();

    writeln!(file, "{}", file_contents).unwrap();
}

pub fn append_file(path_to_repo: &Path, file_name: &str, file_contents: &str) {
    let mut file = OpenOptions::new()
        .write(true)
        .append(true)
        .open(path_to_repo.join(file_name))
        .unwrap();

    writeln!(file, "{}", file_contents).unwrap();
}

pub fn run_test_bin<I, T, P: AsRef<Path>>(current_dir: P, arguments: I) -> Output
where
    I: IntoIterator<Item = T>,
    T: AsRef<OsStr>,
{
    let mut current_dir_buf: PathBuf = current_dir.as_ref().into();
    if current_dir_buf.is_relative() {
        current_dir_buf = current_dir_buf.canonicalize().unwrap();
    }

    assert_cmd::Command::cargo_bin(env!("CARGO_PKG_NAME"))
        .expect("Failed to get git-chain")
        .current_dir(current_dir_buf)
        .args(arguments)
        .output()
        .expect("Failed to run git-chain")
}

pub fn run_test_bin_expect_err<I, T, P: AsRef<Path>>(current_dir: P, arguments: I) -> Output
where
    I: IntoIterator<Item = T>,
    T: AsRef<OsStr>,
{
    let output = run_test_bin(current_dir, arguments);

    if output.status.success() {
        io::stdout().write_all(&output.stdout).unwrap();
        io::stderr().write_all(&output.stderr).unwrap();
    }

    assert!(!output.status.success(), "expect err");

    output
}

pub fn run_test_bin_expect_ok<I, T, P: AsRef<Path>>(current_dir: P, arguments: I) -> Output
where
    I: IntoIterator<Item = T>,
    T: AsRef<OsStr>,
{
    let output = run_test_bin(current_dir, arguments);

    if !output.status.success() {
        io::stdout().write_all(&output.stdout).unwrap();
        io::stderr().write_all(&output.stderr).unwrap();
    }

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    output
}

pub fn display_outputs(output: &Output) {
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
}

pub fn git_rebase_continue<P: AsRef<Path>>(current_dir: P) -> Output {
    let mut current_dir_buf: PathBuf = current_dir.as_ref().into();
    if current_dir_buf.is_relative() {
        current_dir_buf = current_dir_buf.canonicalize().unwrap();
    }

    // git rebase --continue
    let git_cmd = Command::new("git");

    let output = assert_cmd::Command::from_std(git_cmd)
        .current_dir(current_dir_buf)
        .arg("rebase")
        .arg("--continue")
        .output()
        .expect("Failed to run git-chain");

    assert!(output.status.success());

    output
}
