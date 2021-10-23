# git-chain

> Tool for rebasing a chain of local git branches.

## Usage

```sh
# set up a chain
git chain setup <chain_name> <root_branch> <branch_1> <branch_2> ... <branch_N>

# add current branch to a chain
git chain init <root_branch> <chain_name>
git chain init master super_big_feature

git chain init <root_branch> <chain_name> --before=<other_branch>
git chain init <root_branch> <chain_name> --after=<other_branch>

# list all chains
git chain list

# display current chain
git chain

# rebase all branches on the chain
git chain rebase
# run at most one rebase will perform a history rewrite
git chain rebase --step

# back up all branches of the current chain.
git chain backup

# push all branches on the current chain to their upstreams.
git chain push
# push branches with --force-with-lease
git chain push --force

# prune any branches of the current chain that are ancestors of the root branch.
git chain prune

# remove current branch from any chain
git chain remove

# remove current branch and the chain it is a part of
git chain remove --chain

# remove chain by name
git chain remove --chain=<chain_name>

# move the current branch
git chain move --before=<other_branch>
git chain move --after=<other_branch>
git chain move --chain=<chain_name>
git chain move --chain=<chain_name> --before=<other_branch>
git chain move --chain=<chain_name> --after=<other_branch>

# update the root branch of the chain the current branch is a part of
git chain move --root=<root_branch>

# rename current chain
git chain rename <chain_name>
```

# Motivation

Suppose you have branches, each depending on a parent branch:

```
                            I---J---K  feature-2
                           /
                  E---F---G  feature-1
                 /
    A---B---C---D  master
```

After pulling in new changes on the `master` branch, then rebasing `feature-1` and `feature-2` on top of `master` can be tedious to do.

With `git-chain`, you can automate the rebasing steps by setting up the chain `feature-1` and `feature-2` with `master` as the root branch.

`git-chain` can also rebase if you add commits in any branch in the chain:

```
                            J---K---L  feature-2
                           /
                  E---F---G---H---I  feature-1
                 /
    A---B---C---D  master
```

This tool was built to solve: https://stackoverflow.com/q/20834648/412627

# Installation

1. Install `cargo` and `rust`: https://rustup.rs
2. Checkout this repository and run `cargo build --release`
3. Copy `target/release/git-chain` to your path. (e.g. `cp target/release/git-chain /usr/local/bin/`)

# Prior art

This tool is largely inspired by https://github.com/Shopify/git-chain

# License

MIT.
