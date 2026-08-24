# Release 0.0.14 — Implementation Review & Audit

**Date:** 2026-08-24 · **Tag:** `v0.0.14` (`35b9d1f`) · **Range:** `v0.0.13..v0.0.14`
**Status:** shipped; CI green on every commit pushed this session; 0 open Dependabot alerts.

The tag range contains two bodies of work: the pre-existing rebase Phase-3 stream
(`e40eab0..54eb4f1`, landed Feb–Jun 2026: `--continue`/`--abort`/`--skip`/`--status`,
state tracking, squash-merge handling, progress/summary reporting) and the ten commits
produced in this session (`a0a2ef8..35b9d1f`). This document analyzes the session's
work in detail and records the independent cumulative audit that covered the full range.

---

## 1. Session commits at a glance

| Commit | Type | Change |
|---|---|---|
| `a0a2ef8` | fix | Worktree-aware `checkout_branch` — error instead of panic (issue #48) |
| `3b3fb06` | feat | Post-rebase `prune` suggestion (issue #3) |
| `cde2f0c` | improve | Branch name in uncommitted-changes errors (salvaged from PR #43) |
| `3609989` | fix | 23 `useless_borrows_in_formatting` lint fixes (CI toolchain drift) |
| `a1d3809` | chore | GitHub Actions: checkout v4→v7, cache v3→v6 |
| `136e043` | chore | git2 0.20.2 → 0.21.0 (libgit2 1.9.7) |
| `c198997` | chore | colored 3.1.1, regex 1.13.1, serde_json 1.0.151, assert_cmd 2.2.2, console 0.16.4 |
| `60a9b8f` | chore | rand 0.8.5 → 0.10.2 (fixes RUSTSEC-2026-0097) |
| `c1e1860` | chore | clap 2.33.3 → 4.6.6 migration (removes atty/ansi_term) |
| `35b9d1f` | chore | Version bump 0.0.13 → 0.0.14 + CHANGELOG sectioning |

Also done outside the commit stream: closed stale PRs #42 and #43 with explanations,
filed issue #49 (targeted merge-reporting fix), deleted 4 stale Dependabot branches.

---

## 2. Worktree checkout fix (`a0a2ef8`, issue #48)

### Problem
`Git::checkout_branch` (`src/git_chain/core.rs`) had two defects when the target
branch was checked out in another linked worktree:

1. `set_head` failure was converted to `panic!` via `unwrap_or_else`, despite the
   function returning `Result` and every caller using `?`. libgit2 legitimately
   refuses to point HEAD at a branch held by another worktree, so this was a
   reachable, user-triggerable crash.
2. `checkout_tree()` ran **before** the HEAD move was validated, so the failed
   checkout left the working tree mutated to the target branch's tree with HEAD
   unmoved — presenting as a large pile of phantom staged/unstaged changes
   (~46 files in the original report).

### Design decisions
- **Pre-validation over reordering.** Considered `set_head`-first with rollback,
  but rejected: `checkout_tree` uses HEAD as its default baseline, so moving HEAD
  first would make the tree update a silent no-op (baseline == target). Instead,
  a pre-check refuses before anything is touched — mirroring `git checkout`'s own
  `die_if_checked_out` ordering.
- **HEAD-file inspection over `open_from_worktree`.** git2-rs 0.20/0.21 does not
  bind `git_branch_is_checked_out`, so the helper
  `branch_checked_out_in_other_worktree` replicates it: reads
  `<gitdir>/HEAD` for the main worktree (`commondir`) and each linked worktree
  (`commondir/worktrees/<name>`), comparing against `ref: refs/heads/<branch>`.
  Reading files directly (exactly what libgit2 does internally) also covers
  registered-but-missing worktree directories, where `Repository::open_from_worktree`
  would fail yet `set_head` would still refuse.
- **Self-exclusion by gitdir identity**, not workdir paths: the candidate whose
  canonicalized gitdir equals `self.repo.path()` is the current worktree and is
  skipped — correct whether git-chain runs from the main or a linked worktree.
- **No automatic tree restore** on residual `set_head` failure: a forced restore
  could clobber legitimate local modifications that a SAFE checkout had tolerated
  (files identical between HEAD and target). Data-loss risk outweighed cosmetics;
  the pre-check makes the residual path very unlikely.

### Error message
```
Cannot check out branch 'X': it is checked out in another worktree at '<path>'.
Remove that worktree (git worktree remove <path>), or prune stale worktrees (git worktree prune), then retry.
```
Surfaced by the existing top-level handler (`error: …`, exit 1).

### Tests (`tests/worktree.rs`, new)
1. `rebase_stops_gracefully_when_chain_branch_in_other_worktree` — the issue's
   exact repro; asserts clean failure, no `panicked`, **empty `git status
   --porcelain`** (the phantom-changes regression guard), HEAD unchanged. Also
   implicitly exercises the self-skip (checkout of the current branch with the
   helper active).
2. `navigation_fails_gracefully_when_branch_in_other_worktree` — `git chain first`
   path through `src/commands.rs`.
3. `checkout_succeeds_when_worktree_holds_unrelated_branch` — false-positive guard.

### Accepted limitations
- `git chain rebase` stops at the occupied branch rather than skipping it
  (deliberate: a skipped parent would leave children rebasing onto a stale base;
  matches git's own refusal semantics). Documented as a possible follow-up.
- The `git rebase` subprocess would also refuse for such branches; its failure
  path was already graceful. The fix makes git-chain's own checkout equally graceful
  and adds the worktree path to the message, which git's error lacks in this flow.

---

## 3. Post-rebase prune suggestion (`3b3fb06`, issue #3)

### Behavior
After a chain rebase completes, chain branches whose tips are contained in the
root branch (typical after the squash-merge `Reset` flow) trigger:

```
💡 The following branches are ancestors of the root branch master and can be removed from the chain:

feat-1

To remove them from the chain, run git chain prune
```

### Design decisions
- **Reuses `is_ancestor` verbatim** — the exact predicate `Chain::prune` uses
  (`src/chain.rs:337`), so the suggestion can never disagree with what `prune`
  would do. `prune` only removes chain metadata (git config), never deletes git
  branches, so suggesting it is safe.
- **Single choke point**: detection + printing live in one function
  (`print_prune_suggestion`, `src/git_chain/operations.rs`) called from
  `print_rebase_summary` — covering full rebase, `--continue`, and `--skip` —
  plus the `--step` whole-chain-completed tail.
- **Best-effort by contract**: `is_ancestor(...).unwrap_or(false)` — a detection
  error must never fail a successful rebase.

### Review findings and dispositions (reviewer pass)
| Finding | Severity | Disposition |
|---|---|---|
| Vacuous test assertion (branch name matched progress lines, not the suggestion) | Medium | **Fixed** — test extracts the text between the two suggestion markers; also asserts the *other* branch is absent (over-listing guard) |
| "now ancestors" wording misleading for the reflexive case (zero-commit branch tip == root tip triggers accurately but was never "merged") | Medium | **Fixed** — "now" dropped |
| 🧹 emoji collision with adjacent backup-cleanup block | Low | **Fixed** — 💡 |
| Colon after "run" broke the four existing hint-format precedents | Low | **Fixed** |
| Bold branch list didn't match `prune`'s plain list output | Nit | **Fixed** — plain |
| Two-helper redundancy + `&&&str` deref in filter | Simplification | **Fixed** — single `print_prune_suggestion(impl IntoIterator<Item = &str>)` |
| Suggestion hidden until final `--step` (squash-reset doesn't count as a rebase op, so intermediate steps take the "Performed one rebase" arm) | Low | **Accepted** — self-corrects at completion; tail-only call site was the agreed design |
| Stale-state divergence: `--continue` after manually removing a branch from the chain mid-rebase still suggests it | Low | **Accepted** — contrived, output-only; fixing requires reloading the chain inside a currently-infallible printer |
| No dedicated `--continue`/`--skip`/`--step` suggestion tests | Low | **Accepted** — paths share `print_rebase_summary`; reviewer hand-verified all three |

### Tests (`tests/prune.rs`)
- Positive: squash-merge scenario; marker-delimited section contains exactly the
  merged branch; a follow-up `prune` run proves the suggestion accurate
  ("Pruned 1 branches.").
- Negative: normal rebase produces no suggestion.

---

## 4. Error-message improvement (`cde2f0c`)

All four `dirty_working_directory` preflight errors (rebase, `rebase
--continue`/`--skip`, merge, backup) now name the current branch:
`You have uncommitted changes on branch feat-1.` Salvaged from closed PR #43;
existing tests assert on the phrase `"uncommitted changes"`, which survives.
The rest of PR #43 (blocking chain operations on **untracked** files) was
rejected: stricter than git itself with no safety gain — collisions are already
refused by git subprocesses and SAFE checkouts.

---

## 5. CI repair and hygiene (`3609989`, `a1d3809`)

- **Root cause of the master CI failures:** toolchain drift, not the session's
  commits. The runner's clippy reached 1.98.0, whose new
  `useless_borrows_in_formatting` lint fails `-D warnings` on 23 pre-existing
  `&arg` uses in `format!`/`panic!` args. Reproduced exactly by installing
  1.98.0 side-by-side; fixed via `cargo clippy --fix` (mechanical, no behavior
  change). Verified locally on 1.98 before pushing; CI confirmed green.
- **Actions bumps:** `actions/checkout@v4→v7`, `actions/cache@v3→v6` (Node 20
  deprecation warnings eliminated; verified in the subsequent run's logs).

---

## 6. Dependency modernization (`136e043`, `c198997`, `60a9b8f`, `c1e1860`)

### Process
Three parallel audits preceded any change:
- **Security audit:** cargo-audit (RustSec) + GitHub Dependabot; pre-simulated the
  full bump set in a scratch manifest → zero advisories, before any code changed.
- **clap audit:** built a working clap-4 prototype of this exact CLI and diffed
  its runtime behavior against the compiled clap-2 binary (not changelog reading).
- **Misc-deps audit:** rsync'd repo copy with all bumps applied; full build, CI-gate
  clippy, and 106-test run before recommending.

### Per-dependency outcomes

| Dep | From → To | Code changes | Notes |
|---|---|---|---|
| git2 | 0.20.2 → 0.21.0 | 4 sites in `core.rs` | `shorthand`/`ConfigEntry::name` now `Result` (errors treated as missing — behavior preserved); `StringArray::iter` yields `Result<Option<&str>>` (errored entries skipped in worktree enumeration). Dropped ssh/https/cred default features — git-chain never uses libgit2 transports (all network ops shell out) — removing the whole `url`/`idna` tree |
| colored | 2.1.0 → 3.1.1 | none | Color-detection logic verified byte-for-byte equivalent (CLICOLOR_FORCE > NO_COLOR > CLICOLOR && tty); piped output confirmed ANSI-free, so test assertions unaffected. Drops `lazy_static` |
| rand | 0.8.5 → 0.10.2 | 3 lines in `branch.rs` | `thread_rng()`→`rng()`, `gen_range`→`random_range`, and the **trap**: `use rand::Rng` still compiles in 0.10 (it's now the renamed `RngCore`) while silently losing the range methods — the import must become `RngExt`. Fixes RUSTSEC-2026-0097 (unreachable in our build — `log` feature off — but bumped regardless) |
| regex | 1.11.1 → 1.13.1 | none | Bug-fix releases only |
| serde_json / serde | 1.0.140 → 1.0.151 / 1.0.229 | none | Ryū→Żmij float formatter change irrelevant (no floats in serialized state); "reject non-string enum keys" irrelevant (only `HashMap<String,String>`) |
| assert_cmd | 2.0.16 → 2.2.2 | none | The 2.2.0 "cargo_bin panics" breaking change applies to the free function, not the inherent method we call (still `Result`) |
| console | 0.15.8 → 0.16.4 | none | Only `strip_ansi_codes`, unchanged signature; 0.16 exists mainly because 0.15.12 was yanked |
| between | 0.1.0 (latest) | none | Sole published version; unmaintained but repo-owned by the same author |

### clap 2.33.3 → 4.6.6 (the substantial migration)
- Purely mechanical builder-API port: 17 subcommands, 44 args, 42 accessor call
  sites; ~180 lines in `src/cli.rs`, ~45 in `src/commands.rs`, **one** test-line
  edit (`tests/init.rs:33`, error text lowercased by clap 4).
- Load-bearing constraints, both verified empirically before implementation:
  1. **Zero `Command` settings** — bare `git-chain` must keep running `status`
     via the dispatch fallthrough (asserted by `tests/misc.rs` and ~25 other
     bare-invocation tests). `subcommand_required`/`arg_required_else_help`
     would have shattered the suite.
  2. **`--strategy-option` multi-value acceptance** — clap 2 accepted both
     repetition and multiple values per occurrence; `ArgAction::Append` alone
     would reject `--strategy-option ours patience`. `Append + num_args(1..)`
     preserves parsing exactly (proven by a dual-clap comparison crate over 11
     argvs, and at runtime by observing both values forwarded as `-X` options).
- Empirically disproven risk: `rebase --continue` conflicting with
  `squashed_merge`'s `default_value("reset")` — clap 4 does not count defaults
  as "present", so the `conflicts_with_all` lists behave as before.
- Behavior deltas shipped knowingly (CHANGELOG'd): clap-4 help/error text style;
  CLI parse errors exit 2 (was 1; no test asserted `== 1`); per-subcommand `-V`
  gone (never tested, `propagate_version` deliberately not added since clap 2's
  equivalent `GlobalVersion` was never set).
- Post-implementation review script-diffed every arg definition (18 builder
  fields per arg) and every accessor id old-vs-new: **zero drift** — no id
  typos, no dropped conflicts, no accidental settings.

### Security outcome
- Dependabot alerts: 3 → **0** (git2 alert auto-closed on push; rand alert fixed
  by 0.10.2; atty alert fixable *only* by dropping clap 2 — atty is abandoned
  with no patched release).
- RustSec unmaintained advisories cleared: atty (RUSTSEC-2024-0375), ansi_term
  (RUSTSEC-2021-0139).
- Lockfile: 79 → 63 crates.
- MSRV floor after bumps: **1.85** (clap 4.6, rand 0.10, assert_cmd 2.2 — all
  edition 2024). No `rust-version` pin exists; local 1.91 and CI stable satisfy
  it comfortably. Adding `rust-version = "1.85"` was noted as an optional
  follow-up decision, not required.

---

## 7. Verification evidence

Every commit was verified before push with, at minimum:
- `make test` — all 15 test binaries (106 tests) green
- `cargo +1.98.0 clippy --all-targets --all-features -- -D warnings` — the CI
  gate, run on the CI's clippy version (installed side-by-side; local default
  toolchain untouched)
- `cargo fmt --check`
- Manual end-to-end scenario in a scratchpad repo (worktree repro for #48;
  squash-merge → suggestion → prune for #3; dirty-tree error; clap-4 CLI smoke
  runs incl. variadic positionals, conflicts, invalid values, `--strategy-option`
  forwarding)
- CI watched to completion on every push: 5/5 runs green
  (`3609989`…`35b9d1f`), deprecation warnings verified gone after `a1d3809`.

Incident note: an early manual repro of #48 appeared to show the bug "not
reproducing" — traced to `git worktree add … | head -1` killing the command via
SIGPIPE mid-registration. Repro methodology corrected (no pipe truncation on
state-mutating commands); the fix was then confirmed against a genuinely
registered worktree.

---

## 8. Residual risks & known gaps (consolidated)

1. **Prune suggestion**: hidden until the final `--step`; stale-state edge on
   `--continue` after mid-rebase chain edits (both accepted, documented above).
2. **`get_merge_commit_info` defects** (pre-existing, *not* addressed in this
   release; tracked as issue #49): fast-forward merges report "Already up to
   date"; unrelated merges can be misattributed via the loose
   `contains("Merge branch")` match; stats effectively always `None`.
3. **Dead CLI args** `merge --fork-point` / `merge --ff` are declared but never
   read (pre-existing; migrated as-is to keep the port mechanical).
4. **Duplicated `[default: …]`** in two help strings (pre-existing; clap 2 also
   auto-appended it). Cheap cosmetic follow-up.
5. **`between` 0.1.0** is unmaintained (pins itertools 0.10); repo-owned, so
   refreshable if ever needed.
6. **`_ =>` dispatch fallthrough** would silently run `status` for a future
   subcommand added to the CLI without a match arm (pre-existing shape; a
   `Some((_, _)) => unreachable!()` split would make that loud but changes
   future-bug behavior — left as-is, commented in code).

## 9. Independent cumulative audit (fresh-context, full `v0.0.13..v0.0.14` diff)

A fresh-context auditor reviewed the entire release diff (44 commits, including
the pre-session Phase-3 rebase stream) with no anchoring on this session's
assumptions, reproducing findings against the release binary in scratch repos.

**Verdict: ship-able**; build/lint/tests clean, version/tag/lockfile coherent,
every CHANGELOG dependency claim verified exact, both headline fixes confirmed
working end-to-end, clap 4 + colored 3 migrations faithful with no behavioral
drift found (including the tty-detection and `default_value`+`conflicts_with`
edges). Two findings deserve post-release fixes:

### MEDIUM 1 — worktree guard × rebase state machine (interaction bug)
The #48 guard and the rebase state machine were each reviewed alone; their
interaction was not. `checkout_branch` runs inside the rebase loop
(`operations.rs:180`) *after* the state file is written, and `preliminary_checks`
has no worktree pre-flight. A worktree collision therefore aborts mid-loop with:
1. **Wrong advice** — the error's "then retry" is blocked by the
   `state_exists` guard ("A chain rebase is already in progress").
2. **A stranded user** — HEAD is left on a mid-chain branch (the existing
   `tests/worktree.rs` HEAD assertion passes only coincidentally: its original
   branch is also the first chain branch).
3. **Inconsistency** — it is the only rebase-loop exit that neither restores
   `orig_branch` nor cleans/attributes state (the squash-reset and
   rebase-command failure paths do).
Worst case: the *first* chain branch is worktree-held → zero work done, yet a
blocking state file remains. Recovery exists and works (`--continue` resumes
correctly; `--abort` restores everything), so this is a UX dead-end, not data
loss. Recommended fix: add a worktree pre-flight over all chain branches to
`preliminary_checks` (fail before state is written) and/or drop "then retry"
in favor of state-aware advice on the rebase path. Related: `--skip` cannot
help here because the branch is `Pending`, not `Conflict` — see the dead
`InProgress` variant below.

### MEDIUM 2 — summary over-reports "Rebased"
`BranchRebaseStatus::Completed` is set even when a branch's SHA did not move
(`operations.rs:299`), but the summary labels the Completed count "✅ Rebased"
while deciding the closing line from the did-anything-move counter — producing
`✅ Rebased: 2` directly above `Chain c1 is already up-to-date.` Affects all
three completion paths. Reporting-only; fix is to track/label no-op completions
distinctly (e.g. "Up-to-date: N").

### LOW / cosmetic
- `CHANGELOG.md` reference-link definitions stop at 0.0.13 — `[0.0.14]` renders
  as literal text and `[unreleased]` still compares `v0.0.13...HEAD`.
- Detached HEAD renders as `on branch HEAD` in the new uncommitted-changes
  messages (consistent with pre-existing messages).
- `BranchRebaseStatus::InProgress` is never assigned — only compared against
  and displayed; the `--skip` search's `|| InProgress` arm is dead code.
- README documents neither headline fix; CHANGELOG omits the #48/#3 issue refs
  and the Actions bumps.
- `state.root_branch` snapshot vs live chain root can diverge if the user
  runs `git chain move --root` mid-rebase (informational; the snapshot is
  self-consistent with what the rebase actually did).

### Verified clean (highlights)
clap 4 migration (no drift, no lost settings, both `--flag value` and
`--flag=value` forms, exit codes); colored 3 + clap 4 tty detection consistent
(0 ESC bytes piped, styled on a pty); no dead code introduced; prune suggestion
provably consistent with `prune`; version/tag/binary coherence; all dependency
claims exact (`atty`/`ansi_term` absent from the lockfile); README free of
clap-2-era help output; release build + full suite + strict clippy clean.

---

## 9b. Follow-on deep audit of the rebase subsystem

A separate two-agent deep audit of the rebase algorithm (vs. the git sources)
and the chain-rebase state machine was performed after this release; its
consolidated findings — 1 critical, 6 high, 10 medium — and a prioritized
remediation plan live in **`REBASE_AUDIT.md`**. The release-audit findings
MEDIUM 1 and MEDIUM 2 above are subsumed there (as M4 and M6 respectively).

## 10. Process notes

Work was executed by an agent team with the main session as lead: parallel
read-only auditors before each risky change (clap prototype audit, security
simulation, misc-dep empirical audit), a single implementer to avoid tree
conflicts, and an independent reviewer whose findings were triaged by the lead
(fix vs. accept, each with a recorded reason). The lead re-verified every claim
independently before committing — including re-running the full suite, the
CI-version clippy gate, and manual repros — and watched CI to completion on
every push. Two stale AI-authored PRs (#42, #43) were evaluated against current
master before the overlapping work began, closed with explanations, and their
salvageable ideas extracted (error-message improvement) or re-filed properly
(issue #49).
