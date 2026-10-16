#[path = "common/mod.rs"]
pub mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, run_git_command, run_test_bin, run_test_bin_expect_ok, setup_git_repo,
};
use git2::Repository;
use serde_json::{json, Value};

struct StateFixture {
    repo: Repository,
    path: PathBuf,
    state: Value,
}

impl StateFixture {
    fn new(name: &str) -> Self {
        let repo = setup_git_repo(name);
        let path = generate_path_to_repo(name);
        create_new_file(&path, "base.txt", "base");
        first_commit_all(&repo, "base");
        let base = repo.head().unwrap().target().unwrap().to_string();
        create_branch(&repo, "first");
        checkout_branch(&repo, "first");
        create_new_file(&path, "first.txt", "first work");
        commit_all(&repo, "first work");
        let first = repo.head().unwrap().target().unwrap().to_string();
        create_branch(&repo, "second");
        checkout_branch(&repo, "second");
        create_new_file(&path, "second.txt", "second work");
        commit_all(&repo, "second work");
        let second = repo.head().unwrap().target().unwrap().to_string();
        run_test_bin_expect_ok(&path, ["setup", "validation", "master", "first", "second"]);
        checkout_branch(&repo, "master");
        create_new_file(&path, "upstream.txt", "upstream work");
        commit_all(&repo, "advance root");
        checkout_branch(&repo, "first");
        let state = json!({
            "version": 1,
            "chain_name": "validation",
            "original_branch": "first",
            "root_branch": "master",
            "options": { "step_rebase": false, "ignore_root": false, "squashed_merge_handling": "reset", "no_fork_point": false },
            "original_refs": { "first": first, "second": second },
            "merge_bases": [base, first],
            "branches": [
                { "name": "first", "parent": "master", "status": "pending" },
                { "name": "second", "parent": "first", "status": "pending" },
            ],
            "completed_count": 0,
            "total_count": 2,
            "created_backups": [],
        });
        Self { repo, path, state }
    }

    fn state_path(&self) -> PathBuf {
        self.repo.commondir().join("chain-rebase-state.json")
    }

    fn save(&self, state: &Value) {
        fs::write(self.state_path(), serde_json::to_vec_pretty(state).unwrap()).unwrap();
    }
}

impl Drop for StateFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// Compare contents, including refs, reflogs, index, worktree files, config and recovery files.
fn snapshot(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(path, path, &mut files);
    files
}

fn assert_resume_rejected(fixture: &StateFixture, action: &str, reason: &str) {
    let before = snapshot(&fixture.path);
    let output = run_test_bin(&fixture.path, ["rebase", action]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "ACTION: {action}\nSTDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
        output.status
    );
    // assert!(false, "DEBUG STOP: action={action}, output={output:?}");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("Invalid chain rebase state"), "{stderr}");
    assert!(stderr.contains("chain-rebase-state.json"), "{stderr}");
    assert!(stderr.contains(reason), "{stderr}");
    assert!(stderr.contains("rebase --quit"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_eq!(
        snapshot(&fixture.path),
        before,
        "rejection must leave all repository contents unchanged"
    );
}

#[test]
fn continue_rejects_misaligned_recovery_arrays_before_execution() {
    let fixture = StateFixture::new("validation_misaligned_recovery_arrays");
    let mut state = fixture.state.clone();
    state["merge_bases"] = json!([]);
    fixture.save(&state);
    assert_resume_rejected(&fixture, "--continue", "2 branches but 0 merge bases");
}

#[test]
fn resume_rejects_invalid_plan_records_without_mutating_the_repository() {
    let fixture = StateFixture::new("validation_invalid_plan_records");
    let cases = [
        (
            "/merge_bases",
            json!([
                fixture.state["merge_bases"][0],
                fixture.state["merge_bases"][1],
                fixture.state["merge_bases"][1]
            ]),
            "2 branches but 3 merge bases",
        ),
        ("/branches", json!([]), "no branches were recorded"),
        (
            "/branches/1/name",
            json!("first"),
            "duplicate branch 'first'",
        ),
        (
            "/branches/0/name",
            json!(""),
            "a recorded branch name is empty",
        ),
        (
            "/branches/0/parent",
            json!("second"),
            "parent of branch 'first' is 'second', expected 'master'",
        ),
        (
            "/branches/1/parent",
            json!("master"),
            "parent of branch 'second' is 'master', expected 'first'",
        ),
        (
            "/root_branch",
            json!("first"),
            "root branch 'first' is also a chain member",
        ),
        (
            "/chain_name",
            json!(""),
            "chain name, root branch and original branch must not be empty",
        ),
        (
            "/options/squashed_merge_handling",
            json!("unexpected"),
            "unknown squash policy 'unexpected'",
        ),
        (
            "/merge_bases/1",
            json!("not-an-oid"),
            "invalid merge base for branch 'second'",
        ),
        (
            "/merge_bases/1",
            json!("0".repeat(40)),
            "invalid merge base for branch 'second'",
        ),
        (
            "/original_refs/first",
            json!("0".repeat(40)),
            "invalid original commit for branch 'first'",
        ),
        (
            "/original_refs/second",
            json!("0".repeat(64)),
            "invalid original commit for branch 'second'",
        ),
        (
            "/original_refs/second",
            json!("not-an-oid"),
            "invalid original commit for branch 'second'",
        ),
        (
            "/original_refs",
            json!({"first": fixture.state["original_refs"]["first"]}),
            "missing original commit for branch 'second'",
        ),
    ];
    for (pointer, replacement, reason) in cases {
        let mut state = fixture.state.clone();
        state["branches"][0]["status"] = json!("in_progress");
        *state.pointer_mut(pointer).unwrap() = replacement;
        fixture.save(&state);
        assert_resume_rejected(&fixture, "--continue", reason);
        assert_resume_rejected(&fixture, "--skip", reason);
    }
}

#[test]
fn invalid_resume_preserves_live_git_rebase_and_quit_remains_available() {
    let fixture = StateFixture::new("validation_live_git_rebase");
    let base = fixture.state["merge_bases"][0].as_str().unwrap();
    let paused = run_git_command(
        &fixture.path,
        [
            "rebase", "--exec", "false", "--onto", "master", base, "first",
        ],
    );
    assert!(!paused.status.success(), "{paused:?}");
    let git_state = fixture.repo.state();
    assert_ne!(git_state, git2::RepositoryState::Clean);
    let mut state = fixture.state.clone();
    state["branches"][0]["status"] = json!("conflict");
    state["merge_bases"][1] = json!("invalid-base");
    fixture.save(&state);
    assert_resume_rejected(&fixture, "--skip", "invalid merge base for branch 'second'");
    assert_eq!(fixture.repo.state(), git_state);

    let mut expected = snapshot(&fixture.path);
    assert!(expected
        .remove(&PathBuf::from(".git/chain-rebase-state.json"))
        .is_some());
    let quit = run_test_bin(&fixture.path, ["rebase", "--quit"]);
    let stdout = String::from_utf8_lossy(&quit.stdout);
    let stderr = String::from_utf8_lossy(&quit.stderr);
    println!(
        "QUIT STDOUT: {stdout}\nQUIT STDERR: {stderr}\nQUIT STATUS: {}",
        quit.status
    );
    // assert!(false, "DEBUG STOP: quit={quit:?}");
    assert!(quit.status.success(), "{quit:?}");
    assert!(
        stdout.contains("Discarded the chain rebase state"),
        "{stdout}"
    );
    assert!(stderr.is_empty(), "{stderr}");
    assert!(!fixture.state_path().exists());
    assert_eq!(fixture.repo.state(), git_state);
    assert_eq!(
        snapshot(&fixture.path),
        expected,
        "quit must only delete chain state"
    );
}

#[test]
fn abort_restores_usable_refs_without_requiring_a_valid_resume_plan() {
    let fixture = StateFixture::new("validation_tolerant_abort");
    let original_first =
        git2::Oid::from_str(fixture.state["original_refs"]["first"].as_str().unwrap()).unwrap();
    let moved_tip = fixture
        .repo
        .find_branch("master", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    fixture
        .repo
        .reference(
            "refs/heads/first",
            moved_tip,
            true,
            "simulate visited rebase branch",
        )
        .unwrap();
    fixture
        .repo
        .reference("refs/heads/second", moved_tip, true, "simulate user work")
        .unwrap();
    let mut state = fixture.state.clone();
    state["branches"][0]["status"] = json!("completed");
    state["merge_bases"] = json!([]);
    state["options"]["squashed_merge_handling"] = json!("unknown");
    state["original_refs"]
        .as_object_mut()
        .unwrap()
        .remove("second");
    fixture.save(&state);

    let output = run_test_bin(&fixture.path, ["rebase", "--abort"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "STDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
        output.status
    );
    // assert!(false, "DEBUG STOP: output={output:?}");
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("Restored first"), "{stdout}");
    assert!(
        stderr.contains("No original commit recorded for second"),
        "{stderr}"
    );
    assert_eq!(fixture.repo.head().unwrap().target(), Some(original_first));
    assert_eq!(
        fixture
            .repo
            .find_branch("second", git2::BranchType::Local)
            .unwrap()
            .get()
            .target(),
        Some(moved_tip)
    );
    assert!(fixture.repo.statuses(None).unwrap().is_empty());
    assert!(!fixture.state_path().exists());
}

#[test]
fn continue_accepts_legacy_v1_defaults_and_stale_counters() {
    let fixture = StateFixture::new("validation_legacy_v1_defaults");
    let mut state = fixture.state.clone();
    state.as_object_mut().unwrap().remove("created_backups");
    state["options"]
        .as_object_mut()
        .unwrap()
        .remove("no_fork_point");
    state["completed_count"] = json!(999);
    state["total_count"] = json!(0);
    fixture.save(&state);

    let output = run_test_bin(&fixture.path, ["rebase", "--continue"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "STDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
        output.status
    );
    // assert!(false, "DEBUG STOP: output={output:?}");
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("Rebased: 2"), "{stdout}");
    assert!(!stderr.contains("error:"), "{stderr}");
    let first = fixture
        .repo
        .find_branch("first", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let second = fixture
        .repo
        .find_branch("second", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let root = fixture
        .repo
        .find_branch("master", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    assert!(fixture.repo.graph_descendant_of(first, root).unwrap());
    assert!(fixture.repo.graph_descendant_of(second, first).unwrap());
    assert!(!fixture.state_path().exists());
    assert!(fixture.repo.statuses(None).unwrap().is_empty());
}

#[test]
fn continue_still_skips_a_deleted_pending_member() {
    let fixture = StateFixture::new("validation_deleted_pending_member");
    fixture.save(&fixture.state);
    let deletion = run_git_command(&fixture.path, ["branch", "-D", "second"]);
    assert!(deletion.status.success(), "{deletion:?}");

    let output = run_test_bin(&fixture.path, ["rebase", "--continue"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "STDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
        output.status
    );
    // assert!(false, "DEBUG STOP: output={output:?}");
    assert!(output.status.success(), "{output:?}");
    assert!(
        stdout.contains("Branch 'second' no longer exists, skipping"),
        "{stdout}"
    );
    assert!(stdout.contains("Rebased: 1"), "{stdout}");
    assert!(stdout.contains("Skipped: 1"), "{stdout}");
    assert!(!stderr.contains("error:"), "{stderr}");
    assert!(fixture
        .repo
        .find_branch("second", git2::BranchType::Local)
        .is_err());
    let first = fixture
        .repo
        .find_branch("first", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let root = fixture
        .repo
        .find_branch("master", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    assert!(fixture.repo.graph_descendant_of(first, root).unwrap());
    assert!(!fixture.state_path().exists());
    assert!(fixture.repo.statuses(None).unwrap().is_empty());
}
