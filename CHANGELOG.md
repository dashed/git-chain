# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Rebase now shows progress reporting during chain rebase: `📌 [2/5] Rebasing feature-auth onto main...`
- Rebase completion now shows a summary report with counts by category (rebased, skipped, squash-reset)
- `rebase --continue` and `rebase --skip` now show progress reporting and summary report
- Rebase conflict error message now shows numbered recovery steps with `--continue` and `--abort` instructions
- Replaced `process::exit(1)` with proper error propagation in core operations
  - `rebase`, `backup`, `push`, `prune`, and `pr` operations now return `Result<(), Error>` instead of calling `process::exit(1)`
  - Errors propagate to the top-level handler in `main.rs` for consistent formatting
  - `check_gh_cli_installed()` returns `Result` instead of exiting directly
- Updated CLAUDE.md to reference Makefile targets instead of raw cargo commands

### Added
- Added `--skip` flag to `rebase` command for skipping a conflicted branch and continuing the chain rebase
  - Aborts any in-progress git rebase
  - Restores the conflicted branch to its original position using saved refs
  - Marks the branch as Skipped and resumes rebasing from the next pending branch
- Added external `git rebase --abort` detection in `rebase --continue`
  - Detects when the user has aborted a git rebase directly (bypassing git-chain)
  - Compares branch's current OID with saved original ref to identify external aborts
  - Provides clear error message suggesting `--skip` or `--abort`
- Added chain modification validation in `rebase --continue` and `rebase --skip`
  - Validates that pending branches still exist before attempting to rebase them
  - Warns and automatically skips branches that were deleted externally during a chain rebase
- Added atomic state file writes using write-to-temp-then-rename pattern
  - Prevents state file corruption if the process is killed mid-write
- Added integration tests for rebase --skip:
  - `rebase_skip_conflicted_branch`, `rebase_skip_no_state`, `rebase_skip_then_verify_chain_status`
- Added integration tests for robustness features:
  - `rebase_continue_after_external_abort`, `rebase_continue_with_deleted_branch`
- Added `--continue` flag to `rebase` command for resuming a chain rebase after resolving conflicts
  - Loads saved state from `.git/chain-rebase-state.json`
  - Marks the conflicted branch as completed and resumes from the next pending branch
  - Uses pre-computed merge bases for correct rebasing after partial chain rebase
- Added `--abort` flag to `rebase` command for rolling back a chain rebase
  - Aborts any in-progress git rebase
  - Restores all branches to their original positions using saved refs
  - Returns to the original branch and cleans up state file
- Added chain rebase state tracking via `.git/chain-rebase-state.json`
  - Persists original branch refs, merge bases, and per-branch rebase status
  - Enables recovery from conflicts without re-computing merge bases
  - Blocks new rebase when prior state exists (directs user to --continue or --abort)
  - Skipped for `--step` mode which re-runs from scratch each time
- Added `ChainRebaseState`, `BranchState`, and `BranchRebaseStatus` types for state serialization
- Added `rebase_state` module for state file I/O (read, write, check, delete)
- Added `get_branch_commit_oid()` helper for capturing branch refs before rebase
- Added integration tests for rebase state tracking:
  - `rebase_continue_with_remaining_branches`, `rebase_abort_after_conflict`
  - `rebase_continue_no_state`, `rebase_abort_no_state`, `rebase_blocked_when_state_exists`
- Added `serde` dependency for JSON serialization of rebase state
- Added `--squashed-merge` flag to `rebase` command with three modes:
  - `reset` (default): auto-creates a backup branch before destructive `git reset --hard`
  - `skip`: skips squash-merged branches entirely during rebase
  - `rebase`: forces normal rebase despite squash-merge detection
- Added `SquashedRebaseHandling` enum in `types.rs` for rebase-specific squash handling
- Added integration tests for squash-merge handling in rebase:
  - `rebase_squashed_merge_skip`, `rebase_squashed_merge_force_rebase`
- Added `lint` Makefile target (combines `fmt-check` + `clippy-strict`)
- Added `test-file` Makefile target for running all tests in a specific file
- Added `--status` flag to `rebase` command to show current chain rebase state
  - Displays per-branch status with emoji indicators (Completed, Skipped, Pending, Conflict, etc.)
  - Shows progress through the chain and original branch information
  - Reports "No chain rebase in progress" when no state file exists
- Added `--cleanup-backups` flag to `rebase` command for deleting backup branches after successful rebase
  - Works with `rebase`, `rebase --continue`, and `rebase --skip`
  - Deletes `backup-<chain>/<branch>` branches created during squash-merge reset
  - Reports count of cleaned up backup branches
- Added integration tests for Phase 3 polish and UX features:
  - `rebase_progress_reporting`, `rebase_status_no_state`, `rebase_status_during_conflict`
  - `rebase_cleanup_backups`, `rebase_no_cleanup_without_flag`, `rebase_summary_report_with_skipped_branches`
- Added integration tests for error propagation:
  - `rebase_nonexistent_chain`, `rebase_dirty_working_directory`, `rebase_missing_branch_in_chain`
  - `backup_nonexistent_chain`, `push_nonexistent_chain`, `prune_nonexistent_chain`

### Fixed
- Squash-merge reset in `rebase` now auto-creates a backup branch before destructive `git reset --hard`
- Updated `.PHONY` declaration in Makefile to include all targets

### Removed
- Removed dead Makefile targets: `test-bench`, `test-pr-fix`, `integration-test`

## [0.0.13] - 2025-11-05

### Improved
- Enhanced error message when running git-chain outside a git repository
  - Replaced technical git2 error with clear, actionable message
  - Added helpful hints directing users to run 'git init'
  - Styled error output consistently with colored formatting (error: in red, hint: in yellow)
  - Mirrored Git's own error message style for better user familiarity

### Added
- Added integration test for non-git repository edge case

## [0.0.12] - 2025-11-05

### Fixed
- Fixed test failures when users have custom `init.defaultBranch` git configuration ([#47](https://github.com/dashed/git-chain/pull/47))

## [0.0.11] - 2025-11-05

### Changed
- Upgraded git2 dependency from 0.19.0 to 0.20.2
- Updated libgit2-sys to 0.18.2+1.9.1

## [0.0.10] - 2025-11-05

### Fixed
- Fixed help message to show correct argument order: `init <chain_name> <root_branch>` ([#46](https://github.com/dashed/git-chain/pull/46))
- Fixed test assertion to match corrected help message
- Fixed PR command --draft and --web flag interoperability issue with GitHub CLI
- Fixed PR tests in GitHub Actions
- Fixed rebase_no_forkpoint test
- Fixed various merge test cases

### Added
- Added `pr` subcommand for creating GitHub pull requests ([#40](https://github.com/dashed/git-chain/pull/40))
- Added support for `--pr` flag on `list` and `status` commands to show PR information
- Added support for `--draft` flag when creating PRs
- Added tests for PR functionality

### Changed
- Improved merge commit information retrieval
- Updated GitHub Actions workflow
- Updated gitignore

## [0.0.9] - (Previous version)

[unreleased]: https://github.com/dashed/git-chain/compare/v0.0.13...HEAD
[0.0.13]: https://github.com/dashed/git-chain/compare/v0.0.12...v0.0.13
[0.0.12]: https://github.com/dashed/git-chain/compare/v0.0.11...v0.0.12
[0.0.11]: https://github.com/dashed/git-chain/compare/v0.0.10...v0.0.11
[0.0.10]: https://github.com/dashed/git-chain/compare/v0.0.9...v0.0.10
