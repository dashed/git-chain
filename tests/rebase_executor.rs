#![cfg(unix)]

#[path = "common/mod.rs"]
pub mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::{env, fs};

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, get_current_branch_name, run_test_bin, run_test_bin_expect_ok,
    run_test_bin_with_env, setup_git_repo,
};
use git2::{Oid, Repository};
use serde_json::{json, Value};

struct Fixture {
    repo: Repository,
    path: PathBuf,
    state: Value,
}

struct EntryCase {
    name: &'static str,
    args: &'static [&'static str],
    first_status: Option<&'static str>,
    target: usize,
    head: &'static str,
    rebase_error: &'static str,
}

const ENTRIES: [EntryCase; 3] = [
    EntryCase {
        name: "start",
        args: &["rebase", "--no-fork-point"],
        first_status: None,
        target: 0,
        head: "second",
        rebase_error: "Git command failed:",
    },
    EntryCase {
        name: "continue",
        args: &["rebase", "--continue"],
        first_status: Some("failed"),
        target: 0,
        head: "first",
        rebase_error: "Rebase failed for branch first onto master",
    },
    EntryCase {
        name: "skip",
        args: &["rebase", "--skip"],
        first_status: Some("in_progress"),
        target: 1,
        head: "second",
        rebase_error: "Rebase failed for branch second onto first",
    },
];

impl Fixture {
    fn new(name: &str) -> Self {
        let repo = setup_git_repo(name);
        let path = generate_path_to_repo(name);
        create_new_file(&path, "base.txt", "base");
        first_commit_all(&repo, "base");
        let base = repo.head().unwrap().target().unwrap().to_string();
        create_branch(&repo, "first");
        checkout_branch(&repo, "first");
        create_new_file(&path, "feature.txt", "first work");
        commit_all(&repo, "first work");
        let first = repo.head().unwrap().target().unwrap().to_string();
        // Independent branches ensure even the branch after --skip needs a real replay.
        checkout_branch(&repo, "master");
        create_branch(&repo, "second");
        checkout_branch(&repo, "second");
        create_new_file(&path, "second.txt", "second work");
        commit_all(&repo, "second work");
        let second = repo.head().unwrap().target().unwrap().to_string();
        run_test_bin_expect_ok(&path, ["setup", "executor", "master", "first", "second"]);
        checkout_branch(&repo, "master");
        create_new_file(&path, "upstream.txt", "upstream work");
        commit_all(&repo, "advance root");
        checkout_branch(&repo, "second");
        let state = json!({
            "version": 1,
            "chain_name": "executor",
            "original_branch": "second",
            "root_branch": "master",
            "options": { "step_rebase": false, "ignore_root": false, "squashed_merge_handling": "reset", "no_fork_point": true },
            "original_refs": { "first": first, "second": second },
            "merge_bases": [base, base],
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

    fn save(&self, first_status: &str) {
        let mut state = self.state.clone();
        state["branches"][0]["status"] = json!(first_status);
        fs::write(
            self.repo.commondir().join("chain-rebase-state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();
    }

    fn squash_both(&mut self) {
        // Give the independent second branch the same patch as first, but a distinct OID.
        checkout_branch(&self.repo, "master");
        let base = Oid::from_str(self.state["merge_bases"][0].as_str().unwrap()).unwrap();
        self.repo
            .find_reference("refs/heads/second")
            .unwrap()
            .set_target(base, "rebuild second with equivalent patch")
            .unwrap();
        checkout_branch(&self.repo, "second");
        create_new_file(&self.path, "feature.txt", "first work");
        commit_all(&self.repo, "equivalent work on second");
        self.state["original_refs"]["second"] = json!(self.oid("second").to_string());
        checkout_branch(&self.repo, "master");
        create_new_file(&self.path, "feature.txt", "first work");
        commit_all(&self.repo, "squash first into root");
        checkout_branch(&self.repo, "second");
    }

    fn oid(&self, branch: &str) -> Oid {
        self.repo
            .find_branch(branch, git2::BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap()
    }

    fn read_json(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.repo.commondir().join(name)).unwrap()).unwrap()
    }

    fn hook(&self, contents: &str) {
        let path = self.repo.path().join("hooks/pre-rebase");
        fs::write(&path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn all_rebase_entries_checkpoint_before_git_and_preserve_failure_policies() {
    for entry in &ENTRIES {
        let fixture = Fixture::new(&format!("executor_checkpoint_{}", entry.name));
        if let Some(status) = entry.first_status {
            fixture.save(status);
        }
        fixture.hook("#!/bin/sh\ncommon=$(git rev-parse --git-common-dir) || exit 2\ncp \"$common/chain-rebase-state.json\" \"$common/executor-observed.json\" || exit 3\nexit 9\n");
        let before = [fixture.oid("first"), fixture.oid("second")];
        let output = run_test_bin(&fixture.path, entry.args.iter().copied());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!(
            "ENTRY: {}\nSTDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
            entry.name, output.status
        );
        // assert!(false, "DEBUG STOP: entry={}, output={output:?}", entry.name);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            stdout.contains("rebase --empty=drop --no-rebase-merges"),
            "{stdout}"
        );
        assert!(stderr.contains(entry.rebase_error), "{stderr}");
        assert!(stderr.contains("rebase --abort"), "{stderr}");
        let observed = fixture.read_json("executor-observed.json");
        let saved = fixture.read_json("chain-rebase-state.json");
        assert_eq!(observed["branches"][entry.target]["status"], "in_progress");
        assert_eq!(saved["branches"][entry.target]["status"], "failed");
        assert_eq!(observed["original_refs"], fixture.state["original_refs"]);
        assert_eq!(observed["merge_bases"], fixture.state["merge_bases"]);
        assert_eq!(observed["branches"].as_array().unwrap().len(), 2);
        assert_eq!([fixture.oid("first"), fixture.oid("second")], before);
        assert_eq!(get_current_branch_name(&fixture.repo), entry.head);
    }
}

#[test]
fn all_rebase_entries_record_backup_before_reset_and_keep_state_on_reset_failure() {
    let original_path = env::var_os("PATH").unwrap();
    let real_git = env::split_paths(&original_path)
        .map(|directory| directory.join("git"))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    for entry in &ENTRIES {
        let mut fixture = Fixture::new(&format!("executor_backup_{}", entry.name));
        fixture.squash_both();
        if let Some(status) = entry.first_status {
            fixture.save(status);
        }
        let target = fixture.state["branches"][entry.target]["name"]
            .as_str()
            .unwrap();
        let backup = format!("backup-executor/{target}");
        let shim_dir = fixture.repo.path().join("executor-bin");
        fs::create_dir(&shim_dir).unwrap();
        let shim = shim_dir.join("git");
        fs::write(&shim, r#"#!/bin/sh
if [ "$1" = reset ] && [ "$2" = --hard ]; then
    common=$("$GIT_CHAIN_REAL_GIT" rev-parse --git-common-dir) || exit 2
    cp "$common/chain-rebase-state.json" "$common/executor-before-reset.json" || exit 3
    "$GIT_CHAIN_REAL_GIT" rev-parse --verify "$GIT_CHAIN_EXPECTED_BACKUP" > "$common/executor-backup.txt" || exit 4
    exit 77
fi
exec "$GIT_CHAIN_REAL_GIT" "$@"
"#).unwrap();
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
        let path =
            env::join_paths(std::iter::once(shim_dir).chain(env::split_paths(&original_path)))
                .unwrap();
        let before = [fixture.oid("first"), fixture.oid("second")];
        let output = run_test_bin_with_env(
            &fixture.path,
            entry.args.iter().copied(),
            [
                ("PATH", path),
                ("GIT_CHAIN_REAL_GIT", real_git.clone().into_os_string()),
                (
                    "GIT_CHAIN_EXPECTED_BACKUP",
                    format!("refs/heads/{backup}").into(),
                ),
            ],
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!(
            "ENTRY: {}\nSTDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
            entry.name, output.status
        );
        // assert!(false, "DEBUG STOP: entry={}, output={output:?}", entry.name);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            stdout.contains(&format!("Created backup branch: {backup}")),
            "{stdout}"
        );
        assert!(
            stderr.contains("Unable to run: git reset --hard"),
            "{stderr}"
        );
        let observed = fixture.read_json("executor-before-reset.json");
        let saved = fixture.read_json("chain-rebase-state.json");
        assert_eq!(observed["created_backups"], json!([backup]));
        assert_eq!(observed["branches"][entry.target]["status"], "pending");
        assert_eq!(saved, observed);
        assert_eq!(observed["original_refs"], fixture.state["original_refs"]);
        let backup_oid =
            fs::read_to_string(fixture.repo.commondir().join("executor-backup.txt")).unwrap();
        assert_eq!(backup_oid.trim(), before[entry.target].to_string());
        assert_eq!(fixture.oid(&backup), before[entry.target]);
        assert_eq!([fixture.oid("first"), fixture.oid("second")], before);
        assert_eq!(get_current_branch_name(&fixture.repo), entry.head);
    }
}

#[test]
fn step_mode_saves_no_session_and_stops_after_one_moved_rebase_not_a_squash_reset() {
    let fixture = Fixture::new("executor_step_reset_then_rebase");
    create_branch(&fixture.repo, "third");
    checkout_branch(&fixture.repo, "third");
    create_new_file(&fixture.path, "third.txt", "third work");
    commit_all(&fixture.repo, "third work");
    run_test_bin_expect_ok(&fixture.path, ["init", "executor", "master"]);
    let third_before = fixture.oid("third");
    let first_before = fixture.oid("first");
    let second_before = fixture.oid("second");
    checkout_branch(&fixture.repo, "master");
    create_new_file(&fixture.path, "feature.txt", "first work");
    commit_all(&fixture.repo, "squash first into root");
    let root = fixture.oid("master");
    checkout_branch(&fixture.repo, "first");
    fixture.hook(
        r#"#!/bin/sh
common=$(git rev-parse --git-common-dir) || exit 2
test ! -e "$common/chain-rebase-state.json" || exit 3
printf 'no saved session\n' > "$common/executor-step-hook.txt"
"#,
    );
    let output = run_test_bin(&fixture.path, ["rebase", "--step", "--no-fork-point"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "STDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
        output.status
    );
    // assert!(false, "DEBUG STOP: step reset then rebase, output={output:?}");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        stdout.contains("Resetting branch first to master"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Performed one rebase on branch: second"),
        "{stdout}"
    );
    assert!(
        stderr.contains("Successfully rebased and updated refs/heads/second."),
        "{stderr}"
    );
    assert_eq!(fixture.oid("first"), root);
    assert_eq!(fixture.oid("backup-executor/first"), first_before);
    assert_ne!(fixture.oid("second"), second_before);
    assert!(fixture
        .repo
        .graph_descendant_of(fixture.oid("second"), root)
        .unwrap());
    assert_eq!(fixture.oid("third"), third_before);
    assert_eq!(get_current_branch_name(&fixture.repo), "first");
    assert!(!fixture
        .repo
        .commondir()
        .join("chain-rebase-state.json")
        .exists());
    assert_eq!(
        fs::read_to_string(fixture.repo.commondir().join("executor-step-hook.txt")).unwrap(),
        "no saved session\n"
    );
}
