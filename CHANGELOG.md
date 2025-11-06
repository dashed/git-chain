# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
