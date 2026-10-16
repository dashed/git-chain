#![cfg(unix)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use git_chain::process::{CommandSpec, ProcessRunner, SystemProcessRunner};
use git_chain::repository::{rebase::RebaseCommands, RepoContext};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "git-chain-process-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ScriptedRunner {
    calls: RefCell<Vec<CommandSpec>>,
    results: RefCell<VecDeque<io::Result<Output>>>,
}

impl ScriptedRunner {
    fn new(results: impl IntoIterator<Item = io::Result<Output>>) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            results: RefCell::new(results.into_iter().collect()),
        }
    }
}

impl ProcessRunner for &ScriptedRunner {
    fn output(&self, command: &CommandSpec) -> io::Result<Output> {
        self.calls.borrow_mut().push(command.clone());
        self.results
            .borrow_mut()
            .pop_front()
            .expect("unexpected native command")
    }
}

fn native_output(code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: stdout.to_vec(),
        stderr: stderr.to_vec(),
    }
}

fn repo_context(directory: &TestDirectory) -> RepoContext {
    let repo = git2::Repository::init(&directory.0).unwrap();
    RepoContext::from_repository(&repo, directory.0.clone()).unwrap()
}

#[test]
fn system_runner_preserves_arguments_cwd_environment_and_raw_output() {
    let directory = TestDirectory::new();
    let cwd = directory.0.join("directory with spaces");
    fs::create_dir(&cwd).unwrap();
    let parent_cwd = std::env::current_dir().unwrap();
    let env_key = "GIT_CHAIN_PROCESS_RUNNER_TEST_VALUE";
    let parent_value = std::env::var_os(env_key);
    let argument = OsString::from_vec(b"literal $HOME `uname` spaces \xff".to_vec());
    let env_value = OsString::from_vec(b"child value \xfe".to_vec());
    let command = CommandSpec {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "printf '%s\\n' \"$PWD\"; printf '%s\\n' \"$GIT_CHAIN_PROCESS_RUNNER_TEST_VALUE\"; printf '%s' \"$1\"; printf '\\377' >&2; exit 7".into(),
            "runner-probe".into(),
            argument.clone(),
        ],
        cwd: cwd.clone(),
        env: vec![(env_key.into(), env_value.clone())],
    };

    let output = SystemProcessRunner.output(&command).unwrap();
    println!("COMMAND: {command:?}\nOUTPUT: {output:?}");
    // assert!(false, "DEBUG STOP: command={command:?}, output={output:?}");
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stderr, b"\xff");
    assert_eq!(
        output.stdout,
        [
            cwd.as_os_str().as_bytes(),
            b"\n",
            env_value.as_bytes(),
            b"\n",
            argument.as_bytes()
        ]
        .concat(),
    );
    assert_eq!(std::env::current_dir().unwrap(), parent_cwd);
    assert_eq!(std::env::var_os(env_key), parent_value);
}

#[test]
fn system_runner_preserves_spawn_errors() {
    let directory = TestDirectory::new();
    let command = CommandSpec {
        program: directory.0.join("missing-executable").into_os_string(),
        args: Vec::new(),
        cwd: directory.0.clone(),
        env: Vec::new(),
    };
    let error = SystemProcessRunner
        .output(&command)
        .expect_err("executable does not exist");
    println!("COMMAND: {command:?}\nERROR: {error:?}");
    // assert!(false, "DEBUG STOP: command={command:?}, error={error:?}");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn rebase_commands_use_matching_fork_point_and_injected_execution() {
    let directory = TestDirectory::new();
    let nested = directory.0.join("nested directory");
    fs::create_dir(&nested).unwrap();
    let repo = git2::Repository::init(&directory.0).unwrap();
    let context = RepoContext::from_repository(&repo, nested.clone()).unwrap();
    let runner = ScriptedRunner::new([
        Ok(native_output(0, b"frozen\n", b"")),
        Ok(native_output(1, b"rebase output\xff", b"rebase error\xfe")),
    ]);
    let commands = RebaseCommands::new(context, &runner);
    let (printable, command) = commands.build("parent", "frozen", "feature", true);
    println!("DISPLAY: {printable}\nCOMMAND: {command:?}");
    // assert!(false, "DEBUG STOP: command={command:?}, calls={:?}", runner.calls);
    assert_eq!(
        printable,
        "git -c rebase.updateRefs=false rebase --empty=drop --no-rebase-merges --fork-point --onto parent parent feature",
    );
    assert_eq!(
        command,
        CommandSpec {
            program: "git".into(),
            args: [
                "-c",
                "rebase.updateRefs=false",
                "rebase",
                "--empty=drop",
                "--no-rebase-merges",
                "--fork-point",
                "--onto",
                "parent",
                "parent",
                "feature"
            ]
            .map(OsString::from)
            .to_vec(),
            cwd: nested.clone(),
            env: Vec::new(),
        }
    );
    let output = commands.execute(&command).unwrap();
    println!("OUTPUT: {output:?}");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"rebase output\xff");
    assert_eq!(output.stderr, b"rebase error\xfe");
    assert_eq!(
        *runner.calls.borrow(),
        vec![
            CommandSpec {
                program: "git".into(),
                args: ["merge-base", "--fork-point", "parent", "feature"]
                    .map(OsString::from)
                    .to_vec(),
                cwd: nested,
                env: Vec::new(),
            },
            command,
        ]
    );
    assert!(runner.results.borrow().is_empty());
}

#[test]
fn rebase_commands_fall_back_to_frozen_base_when_probe_is_unusable() {
    let cases = [
        Ok(native_output(0, b"different\n", b"")),
        Ok(native_output(1, b"frozen\n", b"query failed")),
        Ok(native_output(0, b"", b"")),
        Ok(native_output(0, b"frozen\nextra", b"")),
        Ok(native_output(0, b"\xff", b"")),
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "git is unavailable",
        )),
    ];
    for result in cases {
        let directory = TestDirectory::new();
        println!("PROBE RESULT: {result:?}");
        let runner = ScriptedRunner::new([result]);
        let commands = RebaseCommands::new(repo_context(&directory), &runner);
        let (printable, command) = commands.build("parent", "frozen", "feature", true);
        println!("DISPLAY: {printable}\nCOMMAND: {command:?}");
        // assert!(false, "DEBUG STOP: command={command:?}, calls={:?}", runner.calls);
        assert_eq!(
            printable,
            "git -c rebase.updateRefs=false rebase --empty=drop --no-rebase-merges --onto parent frozen feature",
        );
        assert_eq!(
            command,
            CommandSpec {
                program: "git".into(),
                args: [
                    "-c",
                    "rebase.updateRefs=false",
                    "rebase",
                    "--empty=drop",
                    "--no-rebase-merges",
                    "--onto",
                    "parent",
                    "frozen",
                    "feature"
                ]
                .map(OsString::from)
                .to_vec(),
                cwd: directory.0.clone(),
                env: Vec::new(),
            }
        );
        assert_eq!(
            *runner.calls.borrow(),
            vec![CommandSpec {
                program: "git".into(),
                args: ["merge-base", "--fork-point", "parent", "feature"]
                    .map(OsString::from)
                    .to_vec(),
                cwd: directory.0.clone(),
                env: Vec::new(),
            }]
        );
        assert!(runner.results.borrow().is_empty());
    }
}

#[test]
fn rebase_commands_skip_the_probe_when_fork_point_is_disabled() {
    let directory = TestDirectory::new();
    let runner = ScriptedRunner::new([]);
    let commands = RebaseCommands::new(repo_context(&directory), &runner);
    let (printable, command) = commands.build("parent", "frozen", "feature", false);
    println!("DISPLAY: {printable}\nCOMMAND: {command:?}");
    // assert!(false, "DEBUG STOP: command={command:?}, calls={:?}", runner.calls);
    assert_eq!(
        command.args,
        [
            "-c",
            "rebase.updateRefs=false",
            "rebase",
            "--empty=drop",
            "--no-rebase-merges",
            "--onto",
            "parent",
            "frozen",
            "feature"
        ]
        .map(OsString::from)
    );
    assert_eq!(printable, "git -c rebase.updateRefs=false rebase --empty=drop --no-rebase-merges --onto parent frozen feature");
    assert!(runner.calls.borrow().is_empty());
}

#[test]
fn rebase_commands_preserve_execution_spawn_errors() {
    let directory = TestDirectory::new();
    let runner = ScriptedRunner::new([Err(io::Error::from_raw_os_error(13))]);
    let commands = RebaseCommands::new(repo_context(&directory), &runner);
    let (_, command) = commands.build("parent", "frozen", "feature", false);
    let error = commands
        .execute(&command)
        .expect_err("injected spawn failure");
    println!("COMMAND: {command:?}\nERROR: {error:?}");
    // assert!(false, "DEBUG STOP: command={command:?}, error={error:?}");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.raw_os_error(), Some(13));
    assert_eq!(*runner.calls.borrow(), vec![command]);
    assert!(runner.results.borrow().is_empty());
}

#[test]
fn repository_context_keeps_linked_worktree_paths_distinct() {
    let directory = TestDirectory::new();
    let repo = git2::Repository::init(&directory.0).unwrap();
    let tree_oid = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let signature = git2::Signature::now("Process runner test", "runner@example.invalid").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
        .unwrap();
    let linked_path = directory.0.join("linked");
    let worktree = repo.worktree("linked", &linked_path, None).unwrap();
    let linked_repo = git2::Repository::open(worktree.path()).unwrap();
    let nested = linked_path.join("nested");
    fs::create_dir(&nested).unwrap();
    let context = RepoContext::from_repository(&linked_repo, nested.clone()).unwrap();
    let command = CommandSpec {
        program: "git".into(),
        args: ["rev-parse", "--absolute-git-dir"]
            .map(OsString::from)
            .to_vec(),
        cwd: context.command_dir().to_path_buf(),
        env: Vec::new(),
    };

    let output = SystemProcessRunner.output(&command).unwrap();
    println!("CONTEXT: {context:?}\nOUTPUT: {output:?}");
    // assert!(false, "DEBUG STOP: context={context:?}, output={output:?}");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let native_git_dir = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    assert_eq!(
        native_git_dir.canonicalize().unwrap(),
        context.git_dir().canonicalize().unwrap()
    );
    assert_eq!(context.command_dir(), nested);
    assert_eq!(
        context.worktree_root().unwrap().canonicalize().unwrap(),
        linked_path
    );
    assert_eq!(
        context.common_dir().canonicalize().unwrap(),
        repo.path().canonicalize().unwrap()
    );
    assert_ne!(
        context.git_dir().canonicalize().unwrap(),
        context.common_dir().canonicalize().unwrap()
    );
}
