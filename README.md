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

When rebasing, Git Chain:
1. Determines the correct fork-point for each branch using `git merge-base --fork-point`
2. Rebases each branch in sequence, preserving the dependency order
3. Handles edge cases like squash merges and chain reorganization

## Installation

1. Install Rust and Cargo: https://rustup.rs
2. Get the Git Chain code:
   ```
   git clone https://github.com/evansst/git-chain.git
   cd git-chain
   ```
3. Build the tool:
   ```
   make build
   ```
4. Make it available on your system:
   ```
   cp target/release/git-chain /usr/local/bin/
   ```

This allows you to use the tool with:
```
git chain <command>
```

Alternatively, you can create a Git alias:
```
git config --global alias.chain "!/path/to/target/release/git-chain"
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
# Rebase all branches in the current chain
git chain rebase

# Rebase step-by-step (one branch at a time)
git chain rebase --step

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

## Smart Rebasing Features

Git Chain has several advanced features:

- **Fork-point detection**: Uses Git's fork-point detection to find the correct base for rebases
- **Squash-merge detection**: Can detect when a branch has been squash-merged into its parent
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