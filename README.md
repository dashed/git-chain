# git-chain

> Tool for rebasing a chain of local git branches.

# Motivation

Suppose you have branches, each depending on a parent branch (usually called "stacked branches"):

```
                            I---J---K  feature-2
                           /
                  E---F---G  feature-1
                 /
    A---B---C---D  master
```

Pulling in new changes on the `master` branch, and then rebasing `feature-1` and `feature-2` on top of `master` can be tedious to do.

With `git-chain`, you can automate the rebasing steps by setting up the chain `feature-1` and `feature-2` with `master` as the root branch:

1. `git-chain setup big-feature master feature-1 feature-2`
2. `git checkout feature-2` (switch into any branch of the `big-feature` chain)
3. `git-chain rebase`

`git-chain` can also rebase all the branches of the chain if you add commits in any branch in the chain:

```
                            J---K---L  feature-2
                           /
                  E---F---G---H---I  feature-1
                 /
    A---B---C---D  master
```

This tool was built to solve the following Stack Overflow question: https://stackoverflow.com/q/20834648/412627

# Concepts

A **chain** (or a "git-chain") consists of the **root branch**, and **branches** that branch off of other branches containing incremental changes of a large feature.

The **root branch** is the branch of which the chain of branches will merge into. Typically the **root branch** is `master` or `main`.

The "chain" as defined can also be called "stacked branches" in other tools. See below.

**Note:**

- A branch can be part of at most one chain.
- The root branch is not part of the chain.

## Rebase strategy

`git-chain` will rebase branches of the chain in the order that they are defined. For each branch, a _fork-point_ is generated with `git merge-base --fork-point` between the branch and the branch's parent (its dependency). The parent of the first branch of the chain is the root branch.

The rebase is applied in the following way for each branch:

```
fork_point=$(git merge-base --fork-point $parent_branch $branch)
git rebase --onto $parent_branch $fork_point $branch
```

The fork-points are generated for each branch before rebasing.

To read more about `fork-point`, see: https://git-scm.com/docs/git-merge-base#_discussion_on_fork_point_mode

# ⚠️⚠️⚠️ What this tool does not do

1. This tool will not create, nor destroy branches for you. You should use `git branch` (or other commands) for that.

2. This tool doesn't attempt to be smart, or make assumptions of what your branching intent looks like. It only understands the chain that you have set up.

# Installation

1. Install `cargo` and `rust`: https://rustup.rs
2. Checkout this repository and run `make build`
3. Copy `target/release/git-chain` to your path. (e.g. `cp target/release/git-chain /usr/local/bin/`)
4. Create a git alias for `git-chain`: `git config --global alias.chain "!git-chain"` (or you can avoid copying the binary to /usr/local/bin and alias directly to the build product by running `git config --global alias.chain "!/path/to/.../target/release/git-chain"`)

## Usage

```sh
# Set up a new chain.
git chain setup <chain_name> <root_branch> <branch_1> <branch_2> ... <branch_N>

# Add current branch to a chain into the last position.
git chain init <chain_name> <root_branch>
# Example:
git chain init super_big_feature master

git chain init <chain_name> <root_branch> --before=<other_branch>
git chain init <chain_name> <root_branch> --after=<other_branch>

git chain init <chain_name> <root_branch> --first

# Display current chain.
git chain

# List all chains.
git chain list

# Back up all branches of the current chain.
# For each branch in the current chain, create new branch with the name: backup-<chain_name>/<branch>
# If the backup branch already exists, then it is replaced.
git chain backup

# Rebase all branches on the chain.
git chain rebase
# Run at most one rebase that will perform a history rewrite.
git chain rebase --step

# Push all branches on the current chain to their upstreams.
# Note: this is not a force push!
git chain push
# Push branches with --force-with-lease
git chain push --force

# Prune any branches of the current chain that are ancestors of the root branch.
git chain prune

# Remove current branch from any chain.
git chain remove

# Remove current branch and the chain it is a part of.
git chain remove --chain

# Remove chain by name.
git chain remove --chain=<chain_name>

# Move the current branch.
git chain move --before=<other_branch>
git chain move --after=<other_branch>
git chain move --chain=<chain_name>
git chain move --chain=<chain_name> --before=<other_branch>
git chain move --chain=<chain_name> --after=<other_branch>

# Update the root branch of the chain the current branch is a part of.
git chain move --root=<root_branch>

# Rename current chain.
git chain rename <chain_name>

# Switching between branches on the current chain.
git chain first
git chain last
git chain next
git chain prev
```

# Other tools

This tool is largely inspired by [Shopify/git-chain](https://github.com/Shopify/git-chain). In fact, I initially used this tool first, before writing my own version.

You may be interested in exploring these tools that have a richer feature set than `git-chain`:

- https://github.com/epage/git-stack

- https://github.com/timothyandrew/gh-stack

# License

MIT.