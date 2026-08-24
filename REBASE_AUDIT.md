# git-chain Rebase Subsystem — Deep Audit

**Date:** 2026-08-24 · **Code audited:** `v0.0.14` (`35b9d1f`)
**Ground truth:** git source at `v2.55.0-618-g1a3e64c6c4` (`builtin/rebase.c`,
`sequencer.c`, `commit.c`, `worktree.c`, `Documentation/git-rebase.adoc`) and
the installed git 2.54.
**Method:** two independent audit agents (algorithm-vs-git-semantics; state
machine & failure recovery), every reported finding **reproduced** against the
built git-chain binary in scratch repositories. A third fresh-context release
audit contributed overlapping findings (cross-referenced below). File
references are to `src/…` at `35b9d1f`.

---

## 0. Executive summary

The core algorithm is **sound where it matters most**: pre-computing every
merge-base before any branch moves is correct and load-bearing, the
`--onto <new parent tip> <frozen fork point> <branch>` transplant window is the
right one, `--continue`/`--skip` reuse the persisted bases rather than
recomputing (which would duplicate commits), and squash-merge detection's happy
path and probed edge cases behave safely.

The serious problems cluster in two places:

1. **The `git rebase` invocation is not isolated from user config and forfeits
   git's de-duplication.** Passing the merge-base SHA as `<upstream>` disables
   patch-id skipping (spurious conflicts where plain `git rebase` succeeds),
   and `rebase.updateRefs=true` silently drags backup refs and chain branches
   along mid-run.
2. **The state machine violates git's own core invariant** — git *keeps* rebase
   state on failure; git-chain *deletes* it on the failure path, destroying the
   only record of original refs. `--abort` is an unverified ref clobber that
   always reports success; there is no `--quit` escape hatch; `--cleanup-backups`
   deletes backups it did not create.

**Severity roll-up (deduplicated):** 1 critical · 6 high · 10 medium · ~12 low/info.

---

## 1. What is verified CORRECT (worth preserving in any refactor)

| Property | Where | Why it matters |
|---|---|---|
| All merge-bases computed before the loop; frozen SHAs used per branch | `operations.rs:60-76`, `:184` | After the parent is rebased, `merge-base(parent', child)` collapses backward; a live recomputation would sweep the parent's old commits into the child's transplant window and duplicate them |
| `--continue`/`--skip` read `state.merge_bases[i]`, never recompute | `:568`, `:882` | Same duplication hazard across a pause |
| Conflicted branch marked and resumed at `idx+1`; skipped branch restored to its original ref before children rebase | `:475`, `:489`, `--skip` path | Children of a skipped branch land on its *original* tip — subtree stays internally consistent (verified by ancestor checks) |
| Fork-point fallback semantics match git | `merge.rs:105-176` vs `builtin/rebase.c:1907-1913` | On `--fork-point` failure both degrade to plain merge-base over `upstream..head`; fresh-clone/reflog-expiry modes degrade identically to git |
| Unrelated histories fail during pre-computation, before any ref moves | pre-loop `merge_base` error | Chain never left half-rebased by this class |
| External `git rebase --abort` detection via OID comparison | `:459-472`, `:498-511` | Accurate diagnosis + correct advice; false positive practically unreachable |
| Criss-cross (multiple merge bases) | probed | `--empty=drop` drops the PATCHSAME replay; result byte-identical to `git rebase b1 b2` |
| Squash-detection edge cases: net-zero branch, empty branch, rewritten parent | probed | All return "not squashed" (safe) |
| State written only after `preliminary_checks` passes | `:39-128` | A rejected precondition never leaves a state file |
| State-file write is atomic for crash-consistency (same-dir temp + rename) | `rebase_state.rs:36-56` | Claim in the doc comment holds (not power-loss durable; not claimed either) |

---

## 2. Algorithm findings (vs. git semantics)

### F1 · HIGH — merge-base-as-`<upstream>` disables git's already-applied detection
`operations.rs:253-266` (and `:645`, `:960`) run
`git rebase --keep-empty --onto <prev> <fork_sha> <branch>`.
Git's own `--fork-point` never substitutes the fork point for `<upstream>`; it
layers it as a *negative revision* on top of the real upstream
(`builtin/rebase.c:296-311`), so `sequencer_make_script`'s PATCHSAME marking
(`sequencer.c:6255+`, symmetric-difference left side = the parent's commits)
can skip commits already applied upstream. With the fork SHA as `<upstream>`,
the left side is empty and **no commit can ever be marked PATCHSAME** —
git-chain permanently behaves as if `--reapply-cherry-picks` were passed.

`--empty=drop` masks this when the duplicate replays cleanly; it does **not**
when the parent has since touched the same lines. Reproduced: a fix
cherry-picked from child to parent, then refined in the parent → `git chain
rebase` hits `UU` conflict; in the identical state
`git rebase --fork-point --onto b1 b1 b2` prints
`warning: skipped previously applied commit` and succeeds.

*Fix direction:* use `--onto <prev> --fork-point <prev> <branch>` (validated:
the parent's reflog retains its pre-run tip even after git-chain rebased it),
guarded by agreement with the pre-computed SHA so the reflog-independent
pre-computation remains the source of truth.

### F2 · HIGH — `rebase.updateRefs` never neutralised
No `--no-update-refs` is passed, and `--keep-empty` forces the merge backend
that `--update-refs` requires. With `rebase.updateRefs=true` (precisely the
config stacked-branch users enable):
- **Every `backup-<chain>/<branch>` ref is dragged onto the rebased commits**
  (reproduced) — including the squash-reset safety backup created moments
  before a `git reset --hard`. Once state is gone, backups are the documented
  recovery path, and they now point at the new history.
- **Chain branches can be moved mid-run** (reproduced with an empty child
  branch), invalidating pre-computed bases and inducing a squash-detection
  false positive that takes the destructive reset path (benign in the repro,
  wrong on principle).

*Fix:* pass `--no-update-refs` unconditionally. One flag; restores git-chain's
central invariant (refs move only when git-chain moves them).

### F3 · MEDIUM — `--keep-empty` is a vestigial no-op that misleads
Since git 2.26, `--keep-empty` concerns commits that *start* empty (now the
default, flag hidden — `builtin/rebase.c:143`, `:1204-1207`); commits that
*become* empty are governed by `--empty`, defaulting to `drop` in this
invocation shape (`:1631-1638`). So chain rebases actually run `--empty=drop` —
which **is** the correct choice for chains (verified: `--empty=keep` leaves
stray empty commits after a parent squash) — but the printed command implies
the opposite of what happens, and the behavior rests on an unpinned default.
Side effect relied upon accidentally: `--keep-empty` silently overrides a
user's `rebase.backend=apply`.
*Fix:* drop `--keep-empty`, pass `--empty=drop` explicitly (keeps the merge
backend implied or add it deliberately).

### F4 · MEDIUM-LOW — merge commits flattened; `rebase.rebaseMerges` leaks
Neither `--rebase-merges` nor `--no-rebase-merges` is passed. Default flattens
merge commits in chain branches (matches plain git — but git-chain's own
`merge` subcommand *creates* those merges, so `git chain merge` →
`git chain rebase` destroys its own output). With `rebase.rebaseMerges=true`
the topology is preserved instead — same nondeterminism-by-config root cause
as F2. *Fix:* decide explicitly (likely `--no-rebase-merges` for
predictability, documented).

### F5 · LOW — squash detection fails toward the destructive branch
`merge.rs:84-86` returns `Ok(true)` ("squash-merged" → `reset --hard` under
default handling) when `git cherry` prints nothing; the multi-line path
(`:95-101`) never verifies the dangling commit is among the `-` lines (the
single-line path does). Both currently unreachable-in-anger, but the safe
default for "learned nothing" is `false`.

### F6 · LOW — no fork-point escape hatch on `rebase`; dead `-f` on `merge`
`--no-fork-point` exists only on `merge`; `rebase` cannot opt out when the
reflog misleads. `merge`'s `-f/--fork-point` is parsed but never read.
Test-coverage gap: `tests/fork_point_failure.rs` covers only unrelated
histories, not fresh-clone/expired-reflog/multiple-merge-base modes.

### F7 · INFO — object churn
Each `is_squashed_merged` call leaves one dangling commit object until GC.

Also: `assert_eq!` at `operations.rs:76` is a (trivially-true) panic path in
release builds; an `Err` would fit the codebase's conventions better.

---

## 3. State machine findings (failure recovery)

### C1 · CRITICAL — failure path DELETES the state file
`operations.rs:278-279` (likewise `:667-668`, `:981-982`): when the
`git rebase` subprocess fails with a clean repo (hook refusal, `fatal:` from a
worktree holding the branch, ENOSPC…), git-chain marks the branch `Failed` and
**deletes the state** — after branches `0..index` were already rewritten.
`original_refs` was the only record of the pre-rebase OIDs; no backups are
taken on the normal path. Reproduced: half-rebased chain,
`--abort`/`--status` → "No chain rebase in progress", recovery reflog-only and
unadvertised. Git never removes state on failure — only on success or explicit
abort/quit (`builtin/rebase.c`); failure is exactly when state is kept.
*Fix:* keep state on `Failed`; print "run `git chain rebase --abort` to
restore" — `--abort` already handles this state correctly (verified).

### H1 · HIGH — `--cleanup-backups` deletes backups it did not create
`operations.rs:1192-1229` deletes `backup-<chain>/<branch>` for every state
branch, unconditionally: (a) the squash-reset safety backup created *in the
same run* — advertised, then deleted at the end of the very command whose
`reset --hard` it insures (squash detection is heuristic; a false positive +
cleanup = unpushed commits with no ref); (b) refs from a deliberate
`git chain backup` (same namespace) — deleted right after the rebase rewrote
all branches, i.e. exactly when they were the only pre-rebase pointers.
*Fix:* record backups created by this run in the state and delete only those;
or refuse to delete a backup whose tip isn't reachable from another ref.

### H2 · HIGH — `--abort` is an unverified clobber that always reports success
`operations.rs:1276-1319`: `update-ref` per branch with no old-value CAS, no
`-m` (empty reflog messages), no dirty/existence checks; per-branch failures
`eprintln!` but the summary still claims "All branches restored". Reproduced:
discards commits made during the pause on branches the rebase never touched;
resurrects deliberately deleted branches; a zeroed OID entry is git's
*delete-ref* syntax — printed "Restored branch2 to 0000000" while deleting the
branch. Git's abort resets one branch, with a reflog message, and dies loudly
on failure. *Fix:* CAS against the expected current OID (`update-ref <ref>
<old-oid-from-state-expectations>`… at minimum warn-and-skip when the branch
moved after the pause), pass `-m "chain rebase (abort): …"`, reject zero OIDs,
and report per-branch outcomes honestly.

### H3 · HIGH — corrupt state wedges the repo; no `--quit`
All entry points `read_state` and die on parse failure; bare `rebase` is
blocked by `state_exists` (which swallows its own read error —
`operations.rs:27-31`). Only `rm .git/chain-rebase-state.json` escapes, and
nothing prints the path. Any schema change makes this worse (no
`#[serde(default)]`, `version` written but never checked — L2). *Fix:* add
`rebase --quit` (delete state, touch nothing — git's exact escape hatch), print
the state-file path in parse errors, and validate `version`.

### H4 · HIGH — `--abort` deletes state *after* its final checkout
`operations.rs:1309` (checkout) precedes `:1312` (delete). A checkout failure
(e.g. `original_branch` now worktree-held) leaves the state in place; every
retry re-runs the full unverified `update-ref` sweep (H2) and fails again.
`rebase_continue`/`rebase_skip` order this correctly — abort is the outlier.
*Fix:* delete state before the final checkout (treat checkout failure as a
warning, as the siblings effectively do).

### M1 — `--step` bypasses the state machine
`operations.rs:26` guards only `!step_rebase`. Step mode neither checks nor
updates existing state: reproduced corrupting a paused chain rebase (stale
`merge_bases` re-applied by a later `--continue` → manufactured conflict), and
step mode records no `original_refs`, so step users have no `--abort`.
*Fix:* refuse `--step` while state exists; longer-term, make step participate.

### M2 — `--abort` moves refs but never resets the working tree
After the user follows git-chain's own printed instructions (`git add`,
`git rebase --continue`) and then aborts, the resolved content remains
staged → every subsequent git-chain command is blocked by the dirty check,
and a stray `git commit` re-applies the abandoned resolution. Git's abort does
a hard reset + HEAD update. *Fix:* after restoring refs, reset the current
branch's working tree (`git reset --hard <original OID of orig_branch>`),
which is safe because abort's contract is explicitly "discard".

### M3 — `InProgress` never assigned; crash mid-branch is invisible
`types.rs:153` is only read (`:496`, `:789`, `:1058`). No branch is marked
before its checkout/rebase runs, so after a SIGKILL the state says `Pending`,
`--status` hides the orphaned `.git/rebase-merge`, `--continue` gives wrong
advice, and `--skip` — whose match arm exists for exactly this
(`Conflict || InProgress`) — refuses, *after* having already run its
destructive `git rebase --abort` side effect. (The re-rebase risk itself is
mostly absorbed by git's patch-id dedup — verified — but that safety belongs
to git, not git-chain, and fails when resolutions changed content.)
*Fix:* write `InProgress` before invoking git rebase; order `--skip`'s
validation before its side effects.

### M4 — mid-loop `checkout_branch` failure leaves blocking all-`Pending` state
`operations.rs:180` `?`-propagates with no cleanup, no `orig_branch` restore —
the only loop exit that does neither. With the new worktree guard (#48) this
is now user-triggerable with zero work done: state blocks the retry that the
guard's own error message recommends ("then retry"), `--skip` can't help
(`Pending`, see M3), and HEAD is stranded mid-chain. `--continue`/`--abort` do
recover correctly (verified). *(= release-audit MEDIUM 1.)*
*Fix:* worktree pre-flight over all chain branches in `preliminary_checks`
(fail before state exists) + state-aware advice in the checkout error.

### M5 — chain edits between pause and continue barely validated
`operations.rs:535-551`/`:850-864` only check branch *existence* for `Pending`
entries. Reproduced: a branch removed from the chain mid-pause is still
rebased; a branch added is silently ignored; even deleting the whole chain
still "successfully rebases" it. Reordering/root changes silently use stale
`parent`/`merge_bases`. *Fix:* on continue/skip, re-read the chain and require
name/order/root equality with the state; on mismatch, instruct `--abort`.

### M6 — success claimed on failed restores; misleading post-skip summary
`--skip`'s failed `update-ref` still marks `Skipped` (`:826`); summary prints
`🎉 Successfully rebased chain` after a skip left descendants on the old base,
and counts unmoved branches as "✅ Rebased" *(= release-audit MEDIUM 2:
`Completed` set even when the SHA didn't move — `✅ Rebased: 2` directly above
"already up-to-date")*. *Fix:* propagate restore failures; distinguish
up-to-date from rebased; after skips, say "completed with skipped branches".

### M7 — `--status` never consults `repo.state()`
Renders JSON only; shows `Pending` while `.git/rebase-merge` holds a live or
orphaned git rebase; its `🔧 In Progress` icon is unreachable (M3).

### M8 — state file is per-worktree; guard misses mid-rebase worktrees
`rebase_state.rs:10` uses `repo.path()` — for a linked worktree that's
`.git/worktrees/<name>/`, so two worktrees can run chain rebases on the same
chain, invisible to each other, over shared refs; the loser's `--abort`
"restores" mid-rebase OIDs. Also the #48 guard reads only worktree `HEAD`
files, so a worktree *mid-rebase on* a branch (detached HEAD,
`rebase-merge/head-name`) is missed: `git checkout` refuses, `git chain first`
switches anyway. Git's `is_shared_symref` (`worktree.c` ~500) explicitly
handles the rebase/bisect cases. *Fix:* store state under `commondir()`; teach
the guard to read `rebase-merge/head-name` / `rebase-apply/head-name`.

### Low / informational (state)
- **L1** `current_index` written, never read (observed inconsistent) — remove or trust.
- **L2** `version: 1` never validated; no serde defaults → any schema change ⇒ H3 wedge.
- **L3** `update-ref` without `-m` → empty reflog messages on the last-resort recovery path.
- **L4** `original_refs: HashMap` → nondeterministic abort output order.
- **L5** `delete_state` errors ignored on success paths → "success" then inexplicable block.
- **L6** temp-file name fixed → same-worktree write race (theoretical); no fsync (not claimed).
- **L7** `--step --cleanup-backups` accepted, silently ignored.
- **L8** success path errors if the state file vanished mid-run.
- Detached HEAD renders as "on branch HEAD" in dirty errors (release audit; cosmetic).
- CHANGELOG 0.0.14 link definitions missing / `[unreleased]` compare stale (release audit).

---

## 4. Consolidated remediation plan

**P0 — data safety: ✅ ALL FIXED (2026-08-24), each guarded by inverted
characterization tests:**
1. C1 — fixed in `4815307`: state kept on failure with recovery advice;
   `--continue` additionally retries the Failed branch (found during the fix:
   the old resume logic skipped it, silently rebuilding a broken chain and
   deleting the state).
2. F2 — fixed in `d028442`: all three invocations run
   `-c rebase.updateRefs=false` (the `-c` form avoids a git version floor).
3. H2 — fixed in `e057fda` (folds in H4, L3, L4): abort leaves moved-or-deleted
   untouched branches as-is with warnings, refuses unusable/zero OIDs (can
   never delete a branch), writes reflog messages, reports truthfully,
   keeps state on restore failure, and deletes state before the final
   checkout so it cannot wedge.
4. H1 — fixed in `919ff72`: state records `created_backups`
   (`#[serde(default)]` — old state files still parse) and cleanup deletes
   only those; user/manual backups survive.

**P1 — recovery & correctness:**
5. H3: `rebase --quit` + state-path in errors + version check. *(Still open;
   its characterization test still passes-as-defect.)*
6. H4: ~~reorder abort's delete-state before final checkout~~ — done with H2
   in `e057fda`.
7. F1: fork-point-as-negative-ref invocation (restore patch-id skipping),
   guarded by the pre-computed SHA.
8. M2: abort resets the working tree it owns.
9. M4: worktree pre-flight in `preliminary_checks` + fixed advice (also
   closes release-audit MEDIUM 1).
10. M1: `--step` refuses while state exists.

**P2 — hygiene & honesty:**
11. F3: `--keep-empty` → `--empty=drop`; F4: explicit `--no-rebase-merges`.
12. M3: write `InProgress`; fix `--skip` ordering. M5: chain revalidation on
    continue/skip. M6: truthful summaries (also release-audit MEDIUM 2).
    M7: `--status` consults `repo.state()`.
13. M8: state under `commondir()`; guard reads rebase state of worktrees.
14. F5/F6/F7, L1–L8, cosmetics.

Suggested regression tests to add alongside: hook-refusal mid-chain (C1),
`rebase.updateRefs=true` chain rebase (F2), cherry-picked-then-refined
conflict (F1), abort-after-external-commits (H2), corrupt-state `--quit`
(H3), fresh-clone/expired-reflog fork-point modes (F6 gap).

**Update:** the six headline defects are now PROVEN by characterization
integration tests that assert the current defective behavior (passing today,
to be inverted when fixes land, each with inline `AFTER THE FIX:` guidance):
- `tests/audit_rebase_invocation.rs` — F1 (identical history in twin repos:
  git-chain's invocation conflicts, git's own construction cleanly skips the
  previously-applied patch; only one argument differs) and F2 (backup refs
  proven to move onto post-rebase tips under `rebase.updateRefs=true`;
  fix-sensitivity verified: with updateRefs off, the backups stay put).
- `tests/audit_rebase_state.rs` — C1 (half-rebased chain + state file gone +
  `--abort` finds nothing), H2 (`--abort` reverts a branch the rebase never
  touched, deleting the user's commit, while printing "All branches
  restored"), H3 (all four recovery entry points wedge on a corrupt state
  file and never name its path), H1 (`--cleanup-backups` deletes user-created
  backups pointing at just-rewritten history).

---

## 5. Verdict

The architecture — shell out to `git rebase` per branch with frozen
merge-bases and a persisted chain state — is fundamentally sound, and several
subtle things (base freezing, skip-subtree consistency, fork-point fallback)
are done *right*. But the subsystem currently fails the "what happens when
something goes wrong" test that git itself is built around: state is destroyed
exactly when it's needed (C1), recovery commands over-promise and
under-verify (H2/M6), there's no escape hatch (H3), and two one-flag isolation
gaps (F2, and F1's invocation shape) let user config and lost de-duplication
turn routine stacked-branch workflows into ref damage or spurious conflicts.
The P0 list is small — four localized changes — and would move the subsystem
from "works on the happy path" to "safe under failure", which for a tool whose
whole job is rewriting many branches at once is the property that matters.
