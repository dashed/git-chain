# git-chain

A powerful tool for managing and rebasing chains of dependent Git branches (stacked branches).

## What Problem Does Git Chain Solve?

When working on complex features, developers often create a series of branches where each branch builds upon the previous one. For example:

```
                            I---J---K  feature-2
                           /
                  E---F---G  feature-1
                 /
    A---B---C---D  master
```

When new changes are added to the `master` branch, updating all branches in the chain becomes tedious and error-prone:

1. You need to rebase `feature-1` onto the updated `master`
2. Then rebase `feature-2` onto the updated `feature-1`
3. Repeat for any additional branches in the chain

Git Chain automates this entire process. It keeps track of relationships between branches and handles the rebasing for you.

## Key Concepts

- **Chain**: A sequence of branches that build upon each other, with a designated root branch.
- **Root Branch**: The foundation branch (typically `main` or `master`) that the chain ultimately merges into.
- **Branch Order**: The sequence in which branches depend on each other in the chain.

**Note**:
- A branch can belong to at most one chain.
- The root branch is not part of the chain, but serves as its foundation.

## How Git Chain Works

Git Chain stores branch relationships in your repository's Git config, tracking:
- Which chain a branch belongs to
- The order of branches within a chain
- Each branch's root branch

Git Chain offers two strategies for updating branches:
1. **Rebase**: Rewrites branch history by replaying commits on top of the updated parent branch
2. **Merge**: Preserves branch history by creating merge commits that incorporate changes from the parent branch

When operating on chains, Git Chain:
1. Determines the correct fork-point for each branch using `git merge-base --fork-point`
2. Updates each branch in sequence, preserving the dependency order
3. Handles edge cases like squash merges and chain reorganization

## Rebase Strategy: How git-chain Updates Your Branches

### Basic Concept

When you run `git chain rebase`, git-chain intelligently updates each branch in your chain to incorporate changes from its parent branch. Think of it like moving your work to sit on top of the latest version of your parent branch. This rewrites commit history, giving a cleaner, linear history but generating new commit hashes.

### How It Works

1. **Order Matters**: Branches are updated in the order they appear in the chain, starting from the one closest to the root branch. This ensures each branch builds upon an already-updated parent.

2. **Finding the Right Starting Point**: For each branch, git-chain determines where your branch originally split from its parent. This point (called a "fork-point") is crucial for keeping only your changes when rebasing.

   > **What is a fork-point?** A fork-point is the specific commit where you originally created your branch from its parent. It's more intelligent than just finding a common ancestor - Git uses its reflog (a history of where branch tips have been) to determine the exact point where your branch's history forked from the parent branch. This is especially useful when the parent branch has been rebased or reorganized since you created your branch. When rebasing, Git needs to know this point to correctly identify which commits belong to your branch (and should be moved) versus which commits were already in the parent branch (and should be left alone).

3. **Smart Detection**: git-chain uses Git's sophisticated "fork-point" detection, which is smarter than simple ancestry checking. It:
   - First checks if your branch can be simply fast-forwarded
   - If not, uses Git's history records (reflog) to find the original branching point
   - Falls back to a regular merge-base if fork-point detection fails

   > **Note on the Fallback Mechanism**: Sometimes Git can't determine the fork-point, particularly in these situations:
   > - When older reflog entries have been cleaned up by `git gc`
   > - If you created your branch from an older commit (not the tip) of the parent branch
   > - After certain operations that affect repository history
   >
   > When Git's fork-point detection fails, git-chain automatically falls back to using `git merge-base`, which finds the most recent common ancestor between two branches. While this ensures rebasing can proceed, it might be less precise than using the true fork-point.

4. **Handling Squash Merges**: If you've squash-merged a branch into its parent (combining all commits into one), git-chain detects this and automatically creates a backup branch before resetting the branch to its parent. You can control this behavior with the `--squashed-merge` flag (see Command Options below).

5. **The Actual Rebasing**: For each branch, git-chain runs a command similar to:
   ```
   git rebase --keep-empty --onto <parent_branch> <fork_point> <branch>
   ```
   This moves your changes to sit on top of the updated parent branch.

To read more about `fork-point`, see: https://git-scm.com/docs/git-merge-base#_discussion_on_fork_point_mode

### Command Options and Flags

Git Chain's rebase command offers customization through its flags:

- **`--step, -s`**: Rebase one branch at a time, requiring manual confirmation between steps
  ```
  git chain rebase --step
  ```
  Perfect for carefully managing complex rebases where conflicts might occur.

- **`--ignore-root, -i`**: Skip rebasing the first branch onto the root branch
  ```
  git chain rebase --ignore-root
  ```
  Useful when you want to update relationships between chain branches without incorporating root branch changes.

- **`--continue`**: Resume a chain rebase after resolving conflicts
  ```
  git chain rebase --continue
  ```
  After resolving a rebase conflict and completing the git-level rebase (`git rebase --continue`), use this to continue rebasing the remaining branches in the chain. Uses saved merge bases from the original run.

- **`--skip`**: Skip the current conflicted branch and continue with the rest of the chain
  ```
  git chain rebase --skip
  ```
  When a rebase conflict occurs and you don't want to resolve it, use `--skip` to restore the conflicted branch to its original position and continue rebasing the remaining branches. This aborts any in-progress git rebase automatically.

- **`--abort`**: Abort a chain rebase and restore all branches
  ```
  git chain rebase --abort
  ```
  Rolls back the entire chain rebase by restoring all branches to their original positions before the rebase started. Aborts any in-progress git rebase and cleans up the state file.

- **`--status`**: Show the current chain rebase state
  ```
  git chain rebase --status
  ```
  Displays the status of each branch in an ongoing chain rebase, including which branches have been completed, skipped, or are still pending. Reports "No chain rebase in progress" when no state file exists.

- **`--cleanup-backups`**: Delete backup branches after successful rebase
  ```
  git chain rebase --cleanup-backups
  ```
  After a successful rebase, automatically deletes any `backup-<chain>/<branch>` branches that were created during squash-merge reset. Can also be combined with `--continue` and `--skip`.

- **`--squashed-merge=<mode>`**: How to handle branches detected as squash-merged
  ```
  git chain rebase --squashed-merge=reset   # Default: auto-backup + reset to parent
  git chain rebase --squashed-merge=skip    # Skip the squash-merged branch
  git chain rebase --squashed-merge=rebase  # Force normal rebase despite detection
  ```
  When git-chain detects that a branch has been squash-merged into its parent:
  - **`reset`** (default): Creates a backup branch (`backup-<chain>/<branch>`) before resetting the branch to match its parent with `git reset --hard`. This is the safest option — your original commits are preserved in the backup branch.
  - **`skip`**: Leaves the branch untouched and continues with the next branch in the chain.
  - **`rebase`**: Ignores the squash-merge detection and performs a normal rebase. This may cause conflicts if the branch has multiple commits.

### Examples (`git chain rebase`)

Here are some common scenarios and how to handle them with git-chain rebase:

#### 1. Standard chain update

**Scenario**: You want to update all branches in the chain to incorporate changes from their parent branches.

**Solution**:
```
git chain rebase
```
This rebases all branches in the chain sequentially, starting from the one closest to the root branch.

#### 2. Updating just the relationship between chain branches

**Scenario**: You want to update only relationships between branches in a chain, not incorporating new root branch changes.

**Solution**:
```
git chain rebase --ignore-root
```
This skips rebasing the first branch onto the root branch.

#### 3. Handling squash-merged branches

**Scenario**: A branch in your chain was squash-merged into its parent, and you want to skip it during rebase.

**Solution**:
```
git chain rebase --squashed-merge=skip
```
This skips any branches detected as squash-merged, leaving them untouched while rebasing the rest of the chain.

#### 4. Recovering from rebase conflicts

**Scenario**: A rebase conflict occurred and you need to resolve it and continue.

**Solution**:
```
# After resolving the conflict in the git-level rebase:
git add <resolved-files>
git rebase --continue

# Then resume the chain rebase for remaining branches:
git chain rebase --continue
```

Or if you want to skip the conflicted branch and continue with the rest:
```
git chain rebase --skip
```

Or if you want to abort the entire chain rebase and restore all branches:
```
git chain rebase --abort
```

#### 5. Careful rebasing with potential conflicts

**Scenario**: You anticipate conflicts and want to handle each branch separately.

**Solution**:
```
git chain rebase --step
```
This rebases one branch at a time, waiting for your confirmation between steps.

### Handling Rebase Conflicts

When rebasing branches in a chain, conflicts can sometimes occur. Git Chain saves the chain rebase state to `.git/chain-rebase-state.json`, enabling proper recovery via `--continue` and `--abort`.

1. **Conflict Detection**: When a rebase conflict occurs, git-chain:
   - Pauses the rebasing process at the conflicted commit
   - Saves the chain rebase state (original branch refs, merge bases, per-branch status)
   - Leaves the repository in a conflicted state for you to resolve
   - Provides numbered recovery steps with `--continue`, `--skip`, and `--abort` instructions

2. **Resolution Process**:
   - The conflicted files will be marked with conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`)
   - Resolve conflicts manually by editing the conflicted files
   - Add the resolved files with `git add <file>`
   - Continue the git-level rebase with `git rebase --continue`

3. **Continuing After Resolution**:
   - After resolving the conflicts and completing the git-level rebase, resume the chain rebase:
   ```
   git chain rebase --continue
   ```
   - Git Chain loads the saved state and continues rebasing the remaining branches using pre-computed merge bases
   - After all branches are rebased, the state file is cleaned up and you are returned to your original branch

4. **Skipping a Problematic Branch**:
   - If you don't want to resolve the conflict for a particular branch, skip it and continue:
   ```
   git chain rebase --skip
   ```
   - This aborts the in-progress git rebase, restores the conflicted branch to its original position, and continues rebasing the remaining branches

5. **Aborting the Entire Chain Rebase**:
   - If you decide to cancel the entire chain rebase and restore all branches:
   ```
   git chain rebase --abort
   ```
   - This restores all branches to their original positions, aborts any in-progress git rebase, and cleans up the state file

6. **External Abort Detection**:
   - If you run `git rebase --abort` directly (bypassing git-chain), git-chain detects this when you run `--continue`
   - It compares the branch's current commit with the saved original ref to determine if the rebase was aborted externally
   - You'll be prompted to use `--skip` to skip the branch or `--abort` to cancel the entire chain rebase

**Example Conflict Workflow**:
```
$ git chain rebase
Rebasing branch feature/auth onto master...
Auto-merging src/auth.js
CONFLICT (content): Merge conflict in src/auth.js
error: could not apply 1a2b3c4... Add authentication feature

# Resolve the conflict
$ vim src/auth.js
$ git add src/auth.js
$ git rebase --continue

# Resume the chain rebase for remaining branches
$ git chain rebase --continue
Continuing chain rebase...
Rebasing branch feature/profiles onto feature/auth...
# Continues with remaining branches
```

### Progress Reporting and Summary

When rebasing a chain, git-chain provides real-time progress and a summary report:

**During rebase**:
```
📌 [1/3] Rebasing feature-auth onto master...
📌 [2/3] Rebasing feature-profiles onto feature-auth...
📌 [3/3] Rebasing feature-settings onto feature-profiles...
```

**After completion**:
```
📊 Rebase Summary for Chain: my-feature
  ✅ Rebased: 3

🎉 Successfully rebased chain my-feature
```

**Checking rebase status** during a conflict:
```
$ git chain rebase --status

📊 Chain Rebase Status: my-feature
   Root: master

   ✅ feature-auth (1/3) onto master — Completed
   ❌ feature-profiles (2/3) onto feature-auth — Conflict  ← current
   ⏳ feature-settings (3/3) onto feature-profiles — Pending

   Progress: 1/3 branches completed
   Original branch: feature-settings
```

### Recovery Options

If a rebase goes wrong, Git Chain provides several recovery options:

1. **Skip Conflicted Branch**: Skip the branch that has conflicts and continue with the rest of the chain:
   ```
   git chain rebase --skip
   ```

2. **Abort Chain Rebase**: If a chain rebase is in progress (state file exists), abort and restore all branches:
   ```
   git chain rebase --abort
   ```

3. **Backup Branches**: Backup branches are automatically created when squash-merged branches are reset (via `--squashed-merge=reset`). You can also create backups manually with `git chain backup`. To restore:
   ```
   git checkout branch-name
   git reset --hard backup-chain-name/branch-name
   ```

4. **Reflog**: Even without backups, you can recover using Git's reflog:
   ```
   git checkout branch-name
   git reflog
   git reset --hard branch-name@{1}  # Reset to previous state
   ```

5. **Abort Git-Level Rebase**: If only the git-level rebase needs aborting (before using `--abort`):
   ```
   git rebase --abort
   ```

## Merge Strategy: Preserving Branch History

### Basic Concept

When you run `git chain merge`, git-chain cascades merges through your branch chain by merging each parent branch into its child branch. Unlike rebasing, merging preserves the original commit history by creating merge commits that link branches together.

### How It Works

1. **Order Matters**: Branches are updated in the order they appear in the chain, starting from the one closest to the root branch. Each branch incorporates changes from its parent through a merge.

2. **Finding the Right Starting Point**: Git Chain uses the same intelligent fork-point detection as in rebasing to identify the best common ancestor for each merge.

3. **Smart Detection**: Git Chain checks for special cases:
   - If branches can be fast-forwarded (no merge needed)
   - If a branch has been squash-merged (to avoid duplicate changes)
   - If there are merge conflicts that need manual resolution

4. **The Actual Merging**: For each branch, git-chain runs a command similar to:
   ```
   git checkout <branch>
   git merge <parent_branch>
   ```
   This incorporates all changes from the parent branch while preserving the branch's original commit history.

### Command Options and Flags

Git Chain's merge command offers extensive customization through various flags and options:

#### Basic Options

- **`--verbose, -v`**: Provides detailed output during the merging process
  ```
  git chain merge --verbose
  ```
  Shows exactly what's happening with each branch, including Git's merge output.

- **`--ignore-root, -i`**: Skips merging the root branch into the first branch
  ```
  git chain merge --ignore-root
  ```
  Useful when you want to update relationships between branches in the chain without incorporating root branch changes.

- **`--stay`**: Don't return to the original branch after merging
  ```
  git chain merge --stay
  ```
  By default, git-chain returns you to your starting branch. Use this flag to remain on the last merged branch.

- **`--chain=<name>`**: Operate on a specific chain other than the current one
  ```
  git chain merge --chain=feature-x
  ```
  Allows you to merge a chain even when you're not on a branch that belongs to it.

#### Merge Behavior Controls

- **`--simple, -s`**: Use simple merge mode without advanced detection
  ```
  git chain merge --simple
  ```
  Disables fork-point detection and squashed merge handling for a faster, simpler merge process.

- **`--fork-point, -f`**: Use Git's fork-point detection (default behavior)
  ```
  git chain merge --fork-point
  ```
  Explicitly enables fork-point detection for finding better merge bases.

- **`--no-fork-point`**: Disable fork-point detection, use regular merge-base
  ```
  git chain merge --no-fork-point
  ```
  Can be faster but potentially less accurate. Useful for repositories with limited reflog history.

- **`--squashed-merge=<mode>`**: How to handle branches that appear squash-merged
  ```
  git chain merge --squashed-merge=reset  # Default: reset to match parent branch
  git chain merge --squashed-merge=skip   # Skip branches that appear squashed
  git chain merge --squashed-merge=merge  # Force merge despite the detection
  ```
  Controls behavior when Git Chain detects that a branch appears to have been squash-merged into its parent.

#### Git Merge Options

- **Fast-forward behavior**:
  ```
  git chain merge --ff        # Allow fast-forward if possible (default)
  git chain merge --no-ff     # Always create a merge commit
  git chain merge --ff-only   # Only allow fast-forward merges
  ```
  Controls how Git handles cases where a branch can be fast-forwarded.

- **`--squash`**: Create a single commit instead of a merge commit
  ```
  git chain merge --squash
  ```
  Combines all changes from the source branch into a single commit.

- **`--strategy=<strategy>`**: Use a specific Git merge strategy
  ```
  git chain merge --strategy=recursive
  git chain merge --strategy=ours
  ```
  Specifies which Git merge strategy to use (e.g., recursive, resolve, octopus).

- **`--strategy-option=<option>`**: Pass strategy-specific options
  ```
  git chain merge --strategy=recursive --strategy-option=ignore-space-change
  git chain merge --strategy=recursive --strategy-option=patience
  ```
  Customizes the behavior of the selected merge strategy.

#### Reporting Options

- **Adjusting report detail**:
  ```
  git chain merge --report-level=minimal    # Basic success/failure messages
  git chain merge --report-level=standard   # Summary with counts (default)
  git chain merge --report-level=detailed   # Comprehensive per-branch details
  git chain merge --no-report               # Suppress merge summary report
  git chain merge --detailed-report         # Same as --report-level=detailed
  ```
  Controls how much information is displayed after the merge completes.

### Examples (`git chain merge`)

Here are some common scenarios and how to handle them with git-chain merge:

#### 1. Updating PRs without breaking review comments

**Scenario**: You have multiple PRs open, and the main branch has received changes.

**Solution**:
```
git chain merge
```
This preserves all original commits while incorporating upstream changes via merge commits.

#### 2. Custom merge handling for a specific chain

**Scenario**: You want to update a feature chain while on an unrelated branch.

**Solution**:
```
git chain merge --chain=feature-login --verbose
```
This updates the specified chain with detailed output and returns you to your original branch when complete.

#### 3. Clean merge history with no extra merge commits

**Scenario**: You want to update the chain while maintaining a cleaner history where possible.

**Solution**:
```
git chain merge --ff-only
```
This only updates branches that can be fast-forwarded, failing if a real merge would be required.

#### 4. Simplified merge for branches with squashed history

**Scenario**: Your workflow includes squash-merging branches, and you need to handle this intelligently.

**Solution**:
```
git chain merge --squashed-merge=skip
```
This skips branches that appear to have been squash-merged, avoiding duplicate changes.

#### 5. Focused merge excluding root changes

**Scenario**: You want to merge changes between branches in the chain but not from the root branch.

**Solution**:
```
git chain merge --ignore-root
```
This skips merging the root branch into the first chain branch.

#### 6. Complex conflict resolution with detailed reporting

**Scenario**: You anticipate merge conflicts and want clear information to resolve them.

**Solution**:
```
git chain merge --verbose --detailed-report
```
This provides maximum information during the merge process and in the final report.

### Handling Merge Conflicts

When merging branches in a chain, conflicts can sometimes occur. Git Chain handles conflicts as follows:

1. **Conflict Detection**: When a merge conflict occurs, git-chain:
   - Stops the merging process at the conflicted branch
   - Leaves the repository in a conflicted state for you to resolve
   - Provides information about which branches conflicted
   - Shows which files have conflicts

2. **Resolution Process**:
   - The conflicted files will be marked with conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`)
   - Resolve conflicts manually by editing the conflicted files
   - Add the resolved files with `git add <file>`
   - Complete the merge with `git commit`

3. **Continuing After Resolution**:
   - After resolving the conflicts and committing the merge, you can continue updating the chain:
   ```
   git chain merge
   ```
   - Git Chain will pick up where it left off, continuing with the remaining branches

**Example Conflict Output**:
```
Processing branch: feature/auth
Auto-merging src/config.js
Merge made by the 'recursive' strategy.
 src/config.js | 10 ++++++++++
 1 file changed, 10 insertions(+)

Processing branch: feature/profiles
🛑 Merge conflict between feature/auth and feature/profiles:
Auto-merging src/models/user.js
CONFLICT (content): Merge conflict in src/models/user.js
Automatic merge failed; fix conflicts and then commit the result.

error: Merge conflict between feature/auth and feature/profiles

📊 Merge Summary for Chain: feature
  ✅ Successful merges: 1
  ⚠️  Merge conflicts: 1
     - feature/auth into feature/profiles

⚠️  Chain feature was partially merged with conflicts.
   Run `git status` to see conflicted files.
   After resolving conflicts, continue with regular git commands:
     git add <resolved-files>
     git commit -m "Merge conflict resolution"
```

### When to Use Merge vs. Rebase

- **Use Merge When**:
  - You're working with branches that already have open pull/merge requests
  - You want to preserve the complete history of branch development
  - You need to maintain the context of review comments on specific commits
  - You're collaborating with others who are also working on the branches

- **Use Rebase When**:
  - You're working on private branches that haven't been shared
  - You prefer a linear, cleaner history
  - You want each branch's changes to appear as if they were developed on top of the latest version of their parent

## Installation

1. Install Rust and Cargo: https://rustup.rs
2. Get the Git Chain code:
   ```
   git clone https://github.com/dashed/git-chain.git
   cd git-chain
   ```
3. Install the tool:
   ```
   make install
   ```

This allows you to use the tool with:
```
git chain <command>
```

Alternatively, you can create a Git alias:
```
git config --global alias.chain "!/path/to/target/release/git-chain"
```

## Development

This project uses a Makefile for all development tasks. Run `make help` to see every available target.

```bash
make build                        # Build in debug mode
make release                      # Build in release mode
make test                         # Run all tests
make test-sequential              # Run tests single-threaded
make test-specific TEST=test_name # Run a specific test with output
make test-file FILE=backup        # Run all tests in a file
make lint                         # Format check + strict clippy
make fmt                          # Auto-format code
make clippy                       # Run clippy lints
make ci-local                     # Run the full CI pipeline locally
make clean                        # Clean build artifacts
make debug-info                   # Show toolchain and environment info
```

## Getting Started: A Simple Example

Let's see how to use Git Chain with a simple example:

### 1. Set up a chain

Assuming you have branches `feature-1` and `feature-2` that are stacked:

```
git chain setup my-feature master feature-1 feature-2
```

This creates a chain named "my-feature" with `master` as the root branch and the branches arranged in order.

### 2. Rebase the entire chain

When you need to update the chain (after new commits on `master` or any branch in the chain):

```
git checkout feature-2  # You can be on any branch in the chain
git chain rebase
```

Git Chain will:
- Find all the branches in the chain
- Determine the correct rebase order
- Rebase each branch on top of its parent

### 3. View your current chain

To see information about the current chain:

```
git chain
```

This displays the chain structure and shows the relationship between branches.

## Core Commands

### Creating and Managing Chains

```
# Set up a new chain with multiple branches
git chain setup <chain_name> <root_branch> <branch_1> <branch_2> ... <branch_N>

# Add the current branch to a chain (in the last position)
git chain init <chain_name> <root_branch>

# Add the current branch with specific positioning
git chain init <chain_name> <root_branch> --before=<other_branch>
git chain init <chain_name> <root_branch> --after=<other_branch>
git chain init <chain_name> <root_branch> --first

# Move a branch within its chain
git chain move --before=<other_branch>
git chain move --after=<other_branch>
git chain move --chain=<chain_name>

# Rename a chain
git chain rename <new_chain_name>
```

### Viewing Chain Information

```
# Display the current chain (if the current branch is part of one)
git chain

# List all chains in the repository
git chain list
```

### Working with Chains

```
# Rebase all branches in the current chain (rewrites history)
git chain rebase

# Continue chain rebase after resolving conflicts
git chain rebase --continue

# Abort chain rebase and restore all branches
git chain rebase --abort

# Rebase one branch at a time
git chain rebase --step

# Skip rebasing the first branch onto the root branch
git chain rebase --ignore-root

# Show the status of an in-progress chain rebase
git chain rebase --status

# Delete backup branches after successful rebase
git chain rebase --cleanup-backups

# Specify how to handle squash-merged branches during rebase
git chain rebase --squashed-merge=reset   # Auto-backup + reset (default)
git chain rebase --squashed-merge=skip    # Skip squash-merged branches
git chain rebase --squashed-merge=rebase  # Force normal rebase

# Merge all branches in the current chain (preserves history)
git chain merge

# Merge with detailed output
git chain merge --verbose

# Skip merging the root branch into the first branch
git chain merge --ignore-root

# Create merge commits even for fast-forward merges
git chain merge --no-ff

# Only allow fast-forward merges
git chain merge --ff-only

# Use simple merge mode (without advanced detection)
git chain merge --simple

# Specify how to handle squashed merges
git chain merge --squashed-merge=reset  # Reset branch to parent (default)
git chain merge --squashed-merge=skip   # Skip branches that appear squashed
git chain merge --squashed-merge=merge  # Merge despite squashed detection

# Set the level of detail in the merge report
git chain merge --report-level=minimal    # Basic success/failure only
git chain merge --report-level=standard   # Summary with counts (default)
git chain merge --report-level=detailed   # Comprehensive per-branch details
git chain merge --no-report               # Suppress report entirely
git chain merge --detailed-report         # Same as --report-level=detailed

# Don't return to original branch after merging
git chain merge --stay

# Merge a different chain than the current one
git chain merge --chain=feature-login

# Use specific Git merge strategy and options
git chain merge --strategy=recursive --strategy-option=ignore-space-change

# Create backup branches for all branches in the chain
git chain backup

# Push all branches in the chain to their upstreams
git chain push
git chain push --force  # Use --force-with-lease

# Navigate between branches in the chain
git chain first  # Switch to the first branch in the chain
git chain last   # Switch to the last branch in the chain
git chain next   # Switch to the next branch in the chain
git chain prev   # Switch to the previous branch in the chain

# Prune branches that have been merged to the root branch
git chain prune
```

### Removing Branches from Chains

```
# Remove the current branch from its chain
git chain remove

# Remove the entire chain
git chain remove --chain

# Remove a specific chain
git chain remove --chain=<chain_name>
```

## Smart Branch Management Features

Git Chain has several advanced features:

- **Multiple update strategies**: Choose between rebasing (rewriting history) or merging (preserving history)
- **Fork-point detection**: Uses Git's fork-point detection to find the correct base for rebases and merges
- **Squash-merge detection**: Can detect when a branch has been squash-merged into its parent
- **Detailed reporting**: Provides clear summaries of operations performed on your branches
- **Backup branches**: Creates backup branches before rebasing to safeguard your work
- **Branch navigation**: Easily move between branches in your chain
- **Chain reorganization**: Move branches around within the chain or between chains

## ⚠️ Important Limitations ⚠️

Git Chain does not:

1. Create or delete branches for you. You still need to use standard Git commands for these operations.

2. Make assumptions about your branching intent. It only works with the chain structure you explicitly define.

## Similar Tools

This tool was inspired by [Shopify/git-chain](https://github.com/Shopify/git-chain).

If you need more features, check out these alternatives:
- [git-stack](https://github.com/epage/git-stack)
- [gh-stack](https://github.com/timothyandrew/gh-stack)

## License

MIT
