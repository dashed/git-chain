# Git-Chain Development Guidelines

## Build, Test, Lint Commands

Use the Makefile for all development tasks. Run `make help` to see all available targets.

- Build: `make build` (or `make release` for release mode)
- Run all tests: `make test` (or `make test-sequential` for single-threaded)
- Run a specific test: `make test-specific TEST=test_name`
- Run tests in a specific file: `make test-file FILE=backup`
- Check for errors without building: `make check`
- Format code: `make fmt`
- Check formatting: `make fmt-check`
- Lint (format check + strict clippy): `make lint`
- Run clippy: `make clippy` (or `make clippy-strict` for CI-level strictness)
- Full CI pipeline locally: `make ci-local`
- Quick dev check (format + check): `make quick`
- Clean build artifacts: `make clean`

## Code Style Guidelines
- **Formatting**: Follow standard Rust style with 4-space indentation
- **Imports**: Group imports by std, external crates, then local modules
- **Naming**: Use snake_case for variables/functions, CamelCase for types/structs
- **Error Handling**: Use Result types with descriptive error messages
- **Tests**: Create integration tests in the tests/ directory
  - Write separate assertions instead of combining with OR conditions
  - For example, use:
    ```rust
    assert!(output.status.success());
    assert!(stdout.contains("Expected message"));
    ```
    Instead of:
    ```rust
    assert!(output.status.success() || stdout.contains("Expected message"));
    ```
- **Documentation**: Document all public functions with doc comments
- **Git Workflow**: Create focused commits with descriptive messages
- **Comments**: Explain complex operations, not obvious functionality


## Test Writing Guidelines

### Important Rules for Writing Tests

1. **Avoid OR conditions in assertions**

   Please avoid using the OR operator (`||`) in assertions, as it creates test conditions that may evaluate differently depending on the order of execution.

   ❌ **Avoid this pattern**:
   ```rust
   assert!(!output.status.success() || stdout.contains("Merge conflicts:"), 
          "Expected either a non-zero exit code or conflict message in output");
   ```

   ✅ **Use this pattern instead**:
   ```rust
   assert!(!output.status.success(),
          "Expected command to fail but it succeeded");
   assert!(stdout.contains("Merge conflicts:"), 
          "Expected output to contain conflict message");
   ```

2. **Avoid conditional assertions**

   Never use `if/else` blocks to conditionally execute different assertions. This makes test logic difficult to follow and can hide issues.

   ❌ **Avoid this pattern**:
   ```rust
   if !output.status.success() {
       assert!(true, "Merge failed as expected due to conflicts");
   } else {
       assert!(stdout.contains("Merge conflicts:"), "Expected output to contain conflict message");
   }
   ```

   ✅ **Use this pattern instead**:
   ```rust
   assert!(!output.status.success(), "Merge failed as expected due to conflicts");
   assert!(stdout.contains("Merge conflicts:"), "Expected output to contain conflict message");
   ```

3. **Always check stdout, stderr, and status separately**

   When testing command output, always check stdout, stderr, and exit status with separate assertions. This makes failures more specific and easier to debug.

   ✅ **Recommended pattern**:
   ```rust
   // Print debug information
   println!("STDOUT: {}", stdout);
   println!("STDERR: {}", stderr);
   println!("STATUS: {}", output.status.success());

   // Separate assertions with detailed error messages
   assert!(output.status.success(), "Command failed unexpectedly");
   assert!(stdout.contains("Expected text"), "stdout should contain expected text but got: {}", stdout);
   assert!(stderr.is_empty(), "stderr should be empty but got: {}", stderr);
   ```

4. **Include detailed error messages**

   Always include descriptive error messages in assertions, and where relevant, show the actual values that failed the assertion.

   ✅ **Example**:
   ```rust
   assert!(
       stdout.contains("Successfully merged"),
       "stdout should indicate successful merge but got: {}", 
       stdout
   );
   ```

5. **Use diagnostic printing with corresponding assertions**

   For complex tests, use diagnostic printing to show exactly what's being tested, but always accompany diagnostics with corresponding assertions. Never print diagnostic information without also asserting on the conditions being diagnosed.

   ❌ **Avoid this pattern** (diagnostics without assertions):
   ```rust
   // Only printing diagnostics without asserting
   println!("Contains 'expected term' in stdout: {}", stdout.contains("expected term"));
   println!("Command succeeded: {}", output.status.success());
   ```

   ✅ **Recommended pattern**:
   ```rust
   // Print key test conditions clearly
   println!("Contains 'expected term' in stdout: {}", stdout.contains("expected term"));
   println!("Contains 'expected term' in stderr: {}", stderr.contains("expected term"));
   
   // Print expected vs. observed behavior
   println!("EXPECTED BEHAVIOR: Command should fail with an error message");
   println!("OBSERVED: Command {} with message: {}", 
           if output.status.success() { "succeeded" } else { "failed" },
           if !stderr.is_empty() { &stderr } else { "none" });
   
   // Always assert on the conditions you're diagnosing
   assert!(!output.status.success(), "Command should have failed");
   assert!(stdout.contains("expected term"), "Expected term should be in stdout");
   assert!(!stderr.is_empty(), "Error message should be present in stderr");
   ```

   For every diagnostic print, there should be a corresponding assertion. This includes:
   - Exit status (output.status.success())
   - Standard output content (stdout)
   - Standard error content (stderr)
   - Any other conditions that are critical to the test

6. **Include commented debug assertions with captured output**

   Add commented-out assertions that print variable values when failing. This technique captures and displays the exact content of variables when test execution stops, making debugging much easier.

   ✅ **Example**:
   ```rust
   // Uncomment to stop test execution and debug this test case
   // assert!(false, "DEBUG STOP: Test section name");
   // assert!(false, "stdout: {}", stdout);
   // assert!(false, "stderr: {}", stderr);
   // assert!(false, "status code: {}", output.status.code().unwrap_or(0));
   // assert!(false, "git branch output: {}", git_branch_output);
   
   // Regular assertions follow
   assert!(output.status.success(), "Command should succeed");
   ```
   
   This technique is especially useful because:
   - The output is formatted directly in the error message
   - Multi-line outputs are preserved in the test failure message
   - You can capture the exact state at failure time
   - Variables are evaluated at exactly that point in execution
   - It works better than println!() when output is interleaved

The goal is to create tests that are:
- Clear about what they're testing
- Provide specific feedback when they fail
- Evaluate all conditions regardless of short-circuit evaluation
- Are easy to debug when something goes wrong
- Include enough diagnostic information to understand behavior

## Test, Debug, Edit Loop

When developing or updating tests, follow this systematic approach to ensure all conditions are properly tested. VERY IMPORTANT: You must complete this entire loop for one test before moving to the next test.

### Step-by-Step Process

1. **Analyze the test** - Understand what behavior or condition the test should verify
   ```rust
   // First review the test to understand its purpose
   // Example test to verify merge fails with uncommitted changes
   ```

2. **Add diagnostic printing** - Insert detailed diagnostics that reveal current state
   ```rust
   // Print relevant state and conditions
   println!("=== TEST DIAGNOSTICS ===");
   println!("STDOUT: {}", stdout);
   println!("STDERR: {}", stderr);
   println!("EXIT STATUS: {}", output.status);
   println!("Has uncommitted changes: {}", has_uncommitted_changes);
   println!("Current branch: {}", current_branch);
   println!("======");
   ```

3. **Insert debug breaks with captured output** - Add assertions that stop execution and display output
   ```rust
   // Uncomment to stop test execution and inspect state with captured output
   // assert!(false, "DEBUG STOP: uncommitted_changes test section");
   // assert!(false, "stdout: {}", stdout);
   // assert!(false, "stderr: {}", stderr);
   // assert!(false, "status code: {}", output.status.code().unwrap_or(0));
   ```

4. **Run the specific test** - Execute only the test you're working on
   ```
   cargo test test_merge_with_uncommitted_changes -- --nocapture
   ```
   Note: The `--nocapture` flag ensures println! output is displayed

5. **Review diagnostics** - Analyze the output to determine correct assertions

6. **Add appropriate assertions** - Create specific assertions based on diagnostics
   ```rust
   // Add assertions that precisely test expected conditions
   assert!(!output.status.success(), "Command should fail with uncommitted changes");
   assert!(stderr.contains("uncommitted changes"), 
           "Error message should mention uncommitted changes, got: {}", stderr);
   ```

7. **Comment out the debug break** - Remove or comment out the debug assertion

8. **Run the test again** - Verify assertions work as expected
   ```
   cargo test test_merge_with_uncommitted_changes
   ```

9. **Refine as needed** - Adjust the assertions for better specificity and clarity

10. **VERIFY TEST PASSES** - Make sure the test passes before moving to the next test
    ```
    cargo test test_merge_with_uncommitted_changes
    ```

### Important: Always Follow Complete Loop

You MUST follow this complete "Test, Debug, Edit" Loop for EACH test before moving on to the next test:

1. Start with one specific test
2. Add diagnostics and assertions following the guidelines
3. Run the test to verify it works correctly
4. Debug and fix any issues until the test passes
5. Only after the test passes, move on to the next test

This ensures that each test is thoroughly improved and validated before proceeding to the next one.

### Practical Examples

**Example 1: Testing error conditions**
```rust
// Test that merge fails with uncommitted changes
#[test]
fn test_merge_with_uncommitted_changes() {
    let repo = setup_test_repo();
    // Create uncommitted change
    write_to_file(&repo, "file.txt", "modified content");
    
    let output = run_command(&repo, "chain", &["merge", "feature"]);
    
    // Diagnostic printing
    println!("Has uncommitted changes: true (intentional test condition)");
    println!("STDOUT: {}", output.stdout);
    println!("STDERR: {}", output.stderr);
    println!("EXIT STATUS: {}", output.status.code().unwrap_or(0));
    
    // Debug breaks with captured output (uncomment for debugging)
    // assert!(false, "DEBUG STOP: Checking uncommitted changes behavior");
    // assert!(false, "stdout: {}", output.stdout);
    // assert!(false, "stderr: {}", output.stderr);
    // assert!(false, "status code: {}", output.status.code().unwrap_or(0));
    
    // Specific assertions based on diagnostics
    assert!(!output.status.success(), 
            "Command should fail with uncommitted changes");
    assert!(output.stderr.contains("uncommitted changes"), 
            "Error message should mention uncommitted changes, got: {}", 
            output.stderr);
}
```

**Example 2: Testing success conditions**
```rust
// Test that merge succeeds with the right conditions
#[test]
fn test_successful_merge() {
    let repo = setup_test_repo();
    // Setup branches for merge
    
    let output = run_command(&repo, "chain", &["merge", "feature"]);
    
    // Diagnostic printing
    println!("STDOUT: {}", output.stdout);
    println!("STDERR: {}", output.stderr);
    println!("EXIT STATUS: {}", output.status.code().unwrap_or(0));
    
    // Specific assertions
    assert!(output.status.success(), 
            "Merge command should succeed, got exit code: {}", 
            output.status.code().unwrap_or(0));
    assert!(output.stdout.contains("Successfully merged"), 
            "Output should indicate successful merge, got: {}", 
            output.stdout);
}
```

### Benefits

This approach helps you:
- Understand exactly what the code is doing under test conditions
- See all relevant output before deciding on appropriate assertions
- Create precise assertions that check specific conditions
- Provide detailed diagnostics that make test failures more informative
- Systematically avoid conditional logic or OR operators in tests
- Build up comprehensive test coverage iteratively
- Easily debug tests when they fail

### Pro Tips

1. **Use assert!(false, ...) for superior output capture**
   ```rust
   // This technique displays output better than println!
   assert!(false, "stdout: {}", stdout);
   assert!(false, "stderr: {}", stderr);
   ```
   - Preserves all whitespace and formatting in the output
   - Works better for multi-line output than println!
   - Can capture multiple variables in a single failure point
   - Displays the output directly in test failure messages

2. **Keep diagnostic assertions in the code but commented out**
   ```rust
   // Keep these for future debugging (commented out)
   // assert!(false, "branch name: {}", branch_name);
   // assert!(false, "commit message: {}", commit_message);
   ```
   
3. **Add context variables to capture key test state**
   ```rust
   // Capture state in variables for both printing and assertions
   let has_conflict = stdout.contains("CONFLICT");
   let is_on_branch = !current_branch.is_empty();
   
   // Use in both diagnostics and assertions
   println!("Has conflict: {}", has_conflict);
   assert!(!has_conflict, "Merge should not have conflicts");
   ```

Remember to leave diagnostic printing and commented debug assertions in place even after the test is working. This makes future debugging much easier when tests start failing after code changes.

## External Resources

The project includes the following external repositories in the `external/` directory for reference:

1. **git-scm.com** - The official Git website source code, containing documentation and examples of Git usage
2. **git** - The actual Git source code repository, which includes:
   - Official Git implementation in C
   - Git documentation in AsciiDoc format
   - Command definitions and implementations
   - Core Git functionality code
   - Test suites
3. **git2-rs** - The Rust bindings for libgit2, which includes:
   - A safe Rust API for libgit2 functionality
   - Direct access to Git repositories, objects, and operations
   - Support for Git worktrees and other advanced features
   - Examples demonstrating Git operations in Rust
4. **clap** - A Rust command line argument parsing library used in git-chain:
   - Declarative interface for defining command-line arguments
   - Robust error handling and help message generation
   - Support for subcommands, flags, options, and positional arguments
   - Type conversion and validation capabilities

These repositories are useful for understanding Git internals and implementing Git functionality in Rust. Use git2-rs when working with Git internals from Rust code, especially for operations related to worktrees, repositories, and references. The git source code is valuable for understanding the original C implementation of Git commands. The clap library is essential for understanding the command-line interface implementation in git-chain.