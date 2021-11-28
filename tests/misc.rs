pub mod common;
use common::run_test_bin_expect_err;

#[test]
fn no_subcommand() {
    let args: Vec<String> = vec![];
    let output = run_test_bin_expect_err(args);
    assert!(String::from_utf8_lossy(&output.stdout).contains("On branch: master"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Branch is not part of any chain: master")
    );
}
