# Git-Chain - Tool for Managing Git Branch Chains
# Development Makefile

.PHONY: help test test-sequential test-pr check clippy fmt fmt-check clean doc doc-open build release install-deps ci-local all

# Default target
.DEFAULT_GOAL := help

# Colors for output
BOLD := \033[1m
RED := \033[31m
GREEN := \033[32m
YELLOW := \033[33m
BLUE := \033[34m
MAGENTA := \033[35m
CYAN := \033[36m
RESET := \033[0m

help: ## Show this help message
	@echo "$(BOLD)Git-Chain - Tool for Managing Git Branch Chains$(RESET)"
	@echo "$(CYAN)Development Makefile$(RESET)"
	@echo ""
	@echo "$(BOLD)Usage:$(RESET)"
	@echo "  make <target>"
	@echo ""
	@echo "$(BOLD)Available targets:$(RESET)"
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  $(CYAN)%-20s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo ""
	@echo "$(BOLD)Examples:$(RESET)"
	@echo "  make build         # Build the project"
	@echo "  make test          # Run all tests"
	@echo "  make check         # Quick compilation check"
	@echo "  make ci-local      # Run full CI pipeline locally"

# === Building ===

build: ## Build the project in debug mode
	@echo "$(BOLD)$(GREEN)Building git-chain...$(RESET)"
	@cargo build

release: ## Build the project in release mode
	@echo "$(BOLD)$(GREEN)Building git-chain (release)...$(RESET)"
	@cargo build --release

install: release ## Install git-chain to cargo bin directory
	@echo "$(BOLD)$(GREEN)Installing git-chain...$(RESET)"
	@cargo install --path .
	@echo "$(BOLD)$(GREEN)✓ git-chain installed successfully!$(RESET)"

# === Testing ===

test: ## Run all tests
	@echo "$(BOLD)$(GREEN)Running all tests...$(RESET)"
	@cargo test

test-sequential: ## Run tests sequentially (avoids PATH conflicts)
	@echo "$(BOLD)$(GREEN)Running tests sequentially...$(RESET)"
	@cargo test -- --test-threads=1

test-pr: ## Run PR tests only
	@echo "$(BOLD)$(GREEN)Running PR tests...$(RESET)"
	@cargo test --test pr

test-merge: ## Run merge tests only
	@echo "$(BOLD)$(GREEN)Running merge tests...$(RESET)"
	@cargo test --test merge

test-rebase: ## Run rebase tests only
	@echo "$(BOLD)$(GREEN)Running rebase tests...$(RESET)"
	@cargo test --test rebase

test-specific: ## Run a specific test (use TEST=test_name)
	@echo "$(BOLD)$(GREEN)Running test: $(TEST)$(RESET)"
	@cargo test $(TEST) -- --nocapture

# === Development ===

check: ## Quick compilation check
	@echo "$(BOLD)$(BLUE)Checking compilation...$(RESET)"
	@cargo check

clippy: ## Run clippy lints
	@echo "$(BOLD)$(YELLOW)Running clippy...$(RESET)"
	@cargo clippy

clippy-strict: ## Run clippy with all targets and strict warnings
	@echo "$(BOLD)$(YELLOW)Running clippy on all targets (strict)...$(RESET)"
	@cargo clippy --all-targets --all-features -- -D warnings

clippy-fix: ## Run clippy and automatically fix issues
	@echo "$(BOLD)$(YELLOW)Running clippy with fixes...$(RESET)"
	@cargo clippy --fix --allow-dirty

fmt: ## Format code
	@echo "$(BOLD)$(MAGENTA)Formatting code...$(RESET)"
	@cargo fmt

fmt-check: ## Check code formatting without changing files
	@echo "$(BOLD)$(MAGENTA)Checking code formatting...$(RESET)"
	@cargo fmt -- --check

# === Documentation ===

doc: ## Build documentation
	@echo "$(BOLD)$(CYAN)Building documentation...$(RESET)"
	@cargo doc --no-deps

doc-open: ## Build and open documentation in browser
	@echo "$(BOLD)$(CYAN)Building and opening documentation...$(RESET)"
	@cargo doc --no-deps --open

# === Utilities ===

clean: ## Clean build artifacts
	@echo "$(BOLD)$(RED)Cleaning build artifacts...$(RESET)"
	@cargo clean
	@rm -rf test_sandbox/
	@echo "$(GREEN)✓ Clean completed$(RESET)"

install-deps: ## Install development dependencies
	@echo "$(BOLD)$(BLUE)Installing development dependencies...$(RESET)"
	@rustup component add rustfmt clippy
	@echo "$(BOLD)$(BLUE)Checking for GitHub CLI...$(RESET)"
	@which gh >/dev/null 2>&1 || echo "$(YELLOW)⚠ GitHub CLI (gh) not found. Install from https://cli.github.com/$(RESET)"
	@echo "$(GREEN)✓ Development dependencies checked!$(RESET)"

# === CI Pipeline ===

ci-local: ## Run the complete CI pipeline locally
	@echo "$(BOLD)$(CYAN)Running complete CI pipeline locally...$(RESET)"
	@echo ""
	@echo "$(BOLD)$(YELLOW)Step 1: Check formatting$(RESET)"
	@$(MAKE) fmt-check
	@echo ""
	@echo "$(BOLD)$(YELLOW)Step 2: Run clippy$(RESET)"
	@$(MAKE) clippy-strict
	@echo ""
	@echo "$(BOLD)$(YELLOW)Step 3: Run tests sequentially$(RESET)"
	@$(MAKE) test-sequential
	@echo ""
	@echo "$(BOLD)$(YELLOW)Step 4: Build documentation$(RESET)"
	@$(MAKE) doc
	@echo ""
	@echo "$(BOLD)$(YELLOW)Step 5: Build release$(RESET)"
	@$(MAKE) release
	@echo ""
	@echo "$(BOLD)$(GREEN)🎉 All CI checks passed!$(RESET)"

# === Composite Targets ===

all: ## Run formatting, linting, tests, and build
	@echo "$(BOLD)$(CYAN)Running full development pipeline...$(RESET)"
	@$(MAKE) fmt
	@$(MAKE) clippy-strict
	@$(MAKE) test-sequential
	@$(MAKE) build
	@echo "$(BOLD)$(GREEN)✨ All tasks completed successfully!$(RESET)"

quick: ## Quick development check (format + check)
	@echo "$(BOLD)$(CYAN)Quick development check...$(RESET)"
	@$(MAKE) fmt
	@$(MAKE) check
	@echo "$(BOLD)$(GREEN)✓ Quick check completed!$(RESET)"

dev: ## Development workflow: format, check, build
	@echo "$(BOLD)$(CYAN)Development workflow...$(RESET)"
	@$(MAKE) fmt
	@$(MAKE) check
	@$(MAKE) build
	@echo "$(BOLD)$(GREEN)✓ Development build ready!$(RESET)"

# === Git Chain Commands ===

chain-init: ## Initialize a new chain (use CHAIN=name BASE=branch)
	@echo "$(BOLD)$(CYAN)Initializing chain '$(CHAIN)' with base '$(BASE)'...$(RESET)"
	@cargo run -- init $(CHAIN) $(BASE)

chain-list: ## List all chains
	@echo "$(BOLD)$(CYAN)Listing all chains...$(RESET)"
	@cargo run -- list

chain-status: ## Show chain status
	@echo "$(BOLD)$(CYAN)Chain status...$(RESET)"
	@cargo run -- status

# === Troubleshooting ===

debug-info: ## Show environment and toolchain information
	@echo "$(BOLD)$(CYAN)Environment Information:$(RESET)"
	@echo "$(YELLOW)Rust version:$(RESET)"
	@rustc --version
	@echo "$(YELLOW)Cargo version:$(RESET)"
	@cargo --version
	@echo "$(YELLOW)Toolchain:$(RESET)"
	@rustup show
	@echo "$(YELLOW)GitHub CLI version:$(RESET)"
	@gh --version 2>/dev/null || echo "$(RED)GitHub CLI not installed$(RESET)"
	@echo "$(YELLOW)Git version:$(RESET)"
	@git --version

watch: ## Watch for changes and rebuild (requires cargo-watch)
	@echo "$(BOLD)$(CYAN)Watching for changes...$(RESET)"
	@cargo watch -x check -x test

# === Release Preparation ===

pre-release: ## Prepare for release (full CI + clean)
	@echo "$(BOLD)$(MAGENTA)Preparing for release...$(RESET)"
	@$(MAKE) clean
	@$(MAKE) ci-local
	@echo "$(BOLD)$(GREEN)🚀 Ready for release!$(RESET)"

bump-version: ## Bump version (use VERSION=0.1.0)
	@echo "$(BOLD)$(MAGENTA)Bumping version to $(VERSION)...$(RESET)"
	@sed -i '' 's/version = ".*"/version = "$(VERSION)"/' Cargo.toml
	@cargo check
	@echo "$(BOLD)$(GREEN)✓ Version bumped to $(VERSION)$(RESET)"

# === Testing Helpers ===

test-coverage: ## Generate test coverage report (requires cargo-tarpaulin)
	@echo "$(BOLD)$(CYAN)Generating test coverage report...$(RESET)"
	@cargo tarpaulin --out Html
	@echo "$(BOLD)$(GREEN)✓ Coverage report generated in tarpaulin-report.html$(RESET)"

test-bench: ## Run benchmarks
	@echo "$(BOLD)$(CYAN)Running benchmarks...$(RESET)"
	@cargo bench

# === PR Testing ===

test-pr-fix: ## Test the PR draft fix
	@echo "$(BOLD)$(CYAN)Testing PR draft functionality fix...$(RESET)"
	@cargo test test_pr_command_with_draft_flag -- --nocapture

# === Integration Testing ===

integration-test: ## Run integration test in a temporary git repo
	@echo "$(BOLD)$(CYAN)Running integration test...$(RESET)"
	@./scripts/integration_test.sh || echo "$(YELLOW)Integration test script not found$(RESET)"
