#[path = "common/mod.rs"]
pub mod common;

use common::{
    checkout_branch, commit_all, create_branch, create_new_file, first_commit_all,
    generate_path_to_repo, run_git_command, run_test_bin, run_test_bin_expect_ok,
    run_test_bin_with_env, setup_git_repo, teardown_git_repo,
};
use git2::Repository;
use std::fs;
use std::path::{Path, PathBuf};

fn reporting_repo(name: &str, branches: &[&str]) -> (Repository, PathBuf) {
    let repo = setup_git_repo(name);
    let path = generate_path_to_repo(name);
    create_new_file(&path, "tracked.txt", "keep\nold\nremove");
    first_commit_all(&repo, "base");
    for branch in branches {
        create_branch(&repo, branch);
    }
    let mut arguments = vec!["setup", "reporting", "master"];
    arguments.extend_from_slice(branches);
    run_test_bin_expect_ok(&path, arguments);
    (repo, path)
}

fn checked_git(path: &Path, arguments: Vec<&str>) {
    let output = run_git_command(path, arguments);
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn detailed_merge(path: &Path, flags: &[&str]) -> String {
    let mut arguments = vec!["merge", "--simple", "--report-level=detailed"];
    arguments.extend_from_slice(flags);
    let output = run_test_bin(path, arguments);
    assert!(
        output.status.success(),
        "merge failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    console::strip_ansi_codes(&String::from_utf8_lossy(&output.stdout)).into_owned()
}

fn advance_both_branches(repo: &Repository, path: &Path) {
    checkout_branch(repo, "first");
    create_new_file(path, "child.txt", "child");
    commit_all(repo, "child work");
    checkout_branch(repo, "master");
    create_new_file(path, "tracked.txt", "keep\nnew\nadded");
    create_new_file(path, "upstream.txt", "one\ntwo");
    commit_all(repo, "upstream work");
    checkout_branch(repo, "first");
}

#[test]
fn detailed_merge_reports_fast_forward_without_reading_the_commit_message() {
    let name = "reporting_fast_forward";
    let (repo, path) = reporting_repo(name, &["first"]);
    create_new_file(&path, "upstream.txt", "upstream");
    commit_all(&repo, "Merge branch 'side' (ordinary one-parent commit)");
    let upstream = repo.head().unwrap().target().unwrap();
    checkout_branch(&repo, "first");

    let stdout = detailed_merge(&path, &[]);
    assert!(
        stdout.contains("master ➔ first: Fast-forwarded to master"),
        "{stdout}"
    );
    assert!(
        stdout.contains("1 insertions(+), 0 deletions(-) across 1 files"),
        "{stdout}"
    );
    assert!(!stdout.contains("Already up to date"), "{stdout}");
    assert!(!stdout.contains("Merge branch 'side'"), "{stdout}");
    assert_eq!(repo.head().unwrap().target().unwrap(), upstream);
    teardown_git_repo(name);
}

#[test]
fn detailed_merge_does_not_report_historical_or_unrelated_merges() {
    for parent in ["master", "side"] {
        let name = format!("reporting_unchanged_{parent}");
        let (repo, path) = reporting_repo(&name, &["first", "second"]);
        checkout_branch(&repo, "first");
        create_new_file(&path, "child.txt", "child");
        commit_all(&repo, "child work");
        checkout_branch(&repo, "master");
        if parent == "side" {
            create_branch(&repo, "side");
            checkout_branch(&repo, "side");
        }
        create_new_file(&path, "upstream.txt", "upstream");
        commit_all(&repo, "upstream work");
        checkout_branch(&repo, "first");
        checked_git(&path, vec!["merge", "--no-ff", parent]);
        let before = repo.head().unwrap().target().unwrap();

        let stdout = detailed_merge(&path, &[]);
        assert!(stdout.contains("Successful merges: 1"), "{stdout}");
        assert!(
            stdout.contains("master ➔ first: Already up to date"),
            "{stdout}"
        );
        // The new tip of second is itself a merge commit, but this run fast-forwarded it.
        assert!(
            stdout.contains("first ➔ second: Fast-forwarded to first"),
            "{stdout}"
        );
        assert!(!stdout.contains("Merge branch"), "{stdout}");
        assert_eq!(
            repo.find_branch("first", git2::BranchType::Local)
                .unwrap()
                .get()
                .target(),
            Some(before)
        );
        assert_eq!(
            repo.find_branch("second", git2::BranchType::Local)
                .unwrap()
                .get()
                .target(),
            Some(before)
        );
        teardown_git_repo(name);
    }
}

#[test]
fn detailed_merge_reports_custom_message_and_exact_tree_statistics() {
    let name = "reporting_custom_message";
    let (repo, path) = reporting_repo(name, &["first"]);
    advance_both_branches(&repo, &path);
    checked_git(
        &path,
        vec![
            "config",
            "branch.first.mergeOptions",
            "-m 'Bring in upstream changes'",
        ],
    );
    let before = repo.head().unwrap().target().unwrap();
    let upstream = repo
        .find_branch("master", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();

    let stdout = detailed_merge(&path, &[]);
    assert!(
        stdout.contains("master ➔ first: Bring in upstream changes"),
        "{stdout}"
    );
    assert!(
        stdout.contains("4 insertions(+), 2 deletions(-) across 2 files"),
        "{stdout}"
    );
    let merge = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(merge.parent_count(), 2);
    assert_eq!(merge.parent_id(0).unwrap(), before);
    assert_eq!(merge.parent_id(1).unwrap(), upstream);
    teardown_git_repo(name);
}

#[test]
fn detailed_merge_statistics_ignore_git_show_configuration() {
    let name = "reporting_show_config";
    let (repo, path) = reporting_repo(name, &["first"]);
    advance_both_branches(&repo, &path);
    checked_git(&path, vec!["config", "log.diffMerges", "off"]);

    let stdout = detailed_merge(&path, &[]);
    assert!(
        stdout.contains("master ➔ first: Merge branch 'master' into first"),
        "{stdout}"
    );
    assert!(
        stdout.contains("4 insertions(+), 2 deletions(-) across 2 files"),
        "{stdout}"
    );
    teardown_git_repo(name);
}

#[test]
fn detailed_merge_statistics_count_renames_and_binary_files() {
    let name = "reporting_rename_binary";
    let (repo, path) = reporting_repo(name, &["first"]);
    checkout_branch(&repo, "first");
    create_new_file(&path, "child.txt", "child");
    commit_all(&repo, "child work");
    checkout_branch(&repo, "master");
    fs::rename(path.join("tracked.txt"), path.join("renamed.txt")).unwrap();
    fs::write(path.join("binary.dat"), [0, 1, 2, 3]).unwrap();
    commit_all(&repo, "rename and add binary");
    checkout_branch(&repo, "first");

    let stdout = detailed_merge(&path, &[]);
    assert!(
        stdout.contains("0 insertions(+), 0 deletions(-) across 2 files"),
        "{stdout}"
    );
    assert!(path.join("renamed.txt").exists());
    assert_eq!(fs::read(path.join("binary.dat")).unwrap(), [0, 1, 2, 3]);
    teardown_git_repo(name);
}

#[test]
fn detailed_merge_does_not_call_an_uncommitted_squash_up_to_date() {
    let name = "reporting_uncommitted_squash";
    let (repo, path) = reporting_repo(name, &["first"]);
    create_new_file(&path, "upstream.txt", "upstream");
    commit_all(&repo, "upstream work");
    checkout_branch(&repo, "first");
    let before = repo.head().unwrap().target().unwrap();

    let stdout = detailed_merge(&path, &["--squash"]);
    assert!(
        stdout.contains("master ➔ first: Merge completed without creating a commit"),
        "{stdout}"
    );
    assert!(!stdout.contains("Already up to date"), "{stdout}");
    assert_eq!(repo.head().unwrap().target().unwrap(), before);
    assert!(!repo.statuses(None).unwrap().is_empty());
    teardown_git_repo(name);
}

#[test]
fn detailed_merge_counts_no_changes_with_non_english_git() {
    let name = "reporting_localized_noop";
    let (repo, path) = reporting_repo(name, &["first"]);
    checkout_branch(&repo, "first");
    let before = repo.head().unwrap().target().unwrap();
    let output = run_test_bin_with_env(
        &path,
        ["merge", "--simple", "--report-level=detailed"],
        [("LC_ALL", "de_DE.UTF-8")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("Successful merges: 0"), "{stdout}");
    assert!(
        stdout.contains("Chain reporting is already up-to-date"),
        "{stdout}"
    );
    assert_eq!(repo.head().unwrap().target().unwrap(), before);
    assert!(repo.statuses(None).unwrap().is_empty());
    teardown_git_repo(name);
}

#[test]
fn merge_no_report_alias_suppresses_the_summary() {
    let name = "reporting_no_report_alias";
    let (repo, path) = reporting_repo(name, &["first"]);
    advance_both_branches(&repo, &path);
    let before = repo.head().unwrap().target().unwrap();
    let root = repo
        .find_branch("master", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();

    let output = run_test_bin(&path, ["merge", "--simple", "--no-report"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "STDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
        output.status
    );
    // assert!(false, "DEBUG STOP: output={output:?}");
    assert!(output.status.success(), "{output:?}");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(
        stdout.contains("Successfully merged chain reporting"),
        "{stdout}"
    );
    assert!(!stdout.contains("Merge Summary"), "{stdout}");
    assert!(!stdout.contains("Detailed Merge Information"), "{stdout}");
    let after = repo.head().unwrap().target().unwrap();
    assert!(repo.graph_descendant_of(after, before).unwrap());
    assert!(repo.graph_descendant_of(after, root).unwrap());
    assert_eq!(repo.find_commit(after).unwrap().parent_count(), 2);
    teardown_git_repo(name);
}

#[test]
fn merge_detailed_report_alias_includes_operation_details() {
    let name = "reporting_detailed_report_alias";
    let (repo, path) = reporting_repo(name, &["first"]);
    advance_both_branches(&repo, &path);

    let output = run_test_bin(&path, ["merge", "--simple", "--detailed-report"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "STDOUT: {stdout}\nSTDERR: {stderr}\nSTATUS: {}",
        output.status
    );
    // assert!(false, "DEBUG STOP: output={output:?}");
    assert!(output.status.success(), "{output:?}");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(stdout.contains("Successful merges: 1"), "{stdout}");
    assert!(stdout.contains("Detailed Merge Information"), "{stdout}");
    assert!(stdout.contains("master ➔ first:"), "{stdout}");
    assert!(
        stdout.contains("4 insertions(+), 2 deletions(-) across 2 files"),
        "{stdout}"
    );
    assert_eq!(
        repo.head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .parent_count(),
        2
    );
    teardown_git_repo(name);
}
