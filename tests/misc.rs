pub mod common;
use common::{
    create_new_file, first_commit_all, generate_path_to_repo, run_test_bin_expect_err,
    setup_git_repo, teardown_git_repo,
};

#[test]
fn no_subcommand() {
    let repo_name = "no_subcommand";
    let repo = setup_git_repo(repo_name);
    let path_to_repo = generate_path_to_repo(repo_name);

    {
        // create new file
        create_new_file(&path_to_repo, "hello_world.txt", b"Hello, world!");

        // add first commit to master
        first_commit_all(&repo, "first commit");
    };

    let args: Vec<String> = vec![];
    let output = run_test_bin_expect_err(path_to_repo, args);
    assert!(String::from_utf8_lossy(&output.stdout).contains("On branch: master"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Branch is not part of any chain: master")
    );

    teardown_git_repo(repo_name);
}
