# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[unreleased]: https://github.com/dashed/git-chain/compare/v0.0.9...HEAD
[0.0.10]: https://github.com/dashed/git-chain/compare/v0.0.9...v0.0.10
