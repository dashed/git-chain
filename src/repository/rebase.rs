//! Native rebase command construction and execution.

use std::ffi::OsString;
use std::io;
use std::process::Output;

use super::RepoContext;
use crate::process::{CommandSpec, ProcessRunner};

/// Builds and runs rebase commands in one captured repository context.
///
/// The runner is injected so command mechanics can be tested without simulating Git graphs.
pub struct RebaseCommands<R> {
    context: RepoContext,
    runner: R,
}

impl<R: ProcessRunner> RebaseCommands<R> {
    /// Binds native rebase operations to a repository context and process runner.
    pub fn new(context: RepoContext, runner: R) -> Self {
        Self { context, runner }
    }

    /// Runs a built command, preserving its raw output, status, and spawn error.
    pub fn execute(&self, command: &CommandSpec) -> io::Result<Output> {
        self.runner.output(command)
    }

    /// Build the `git rebase` invocation that transplants `branch` onto `parent`, together
    /// with the exact command string to echo. Single-sourced so the three call sites
    /// (`rebase`, `rebase --continue`, `rebase --skip`) cannot drift apart.
    ///
    /// There are two forms, and the choice is a correctness matter, not a stylistic one.
    ///
    /// **Dedup form** — `--fork-point --onto <parent> <parent> <branch>`. `<upstream>` is
    /// the parent *ref*, so the left side of the `<upstream>...<branch>` symmetric
    /// difference that `sequencer_make_script` walks is the parent's own commits. That is
    /// what lets git mark a commit PATCHSAME and skip one whose patch is already applied on
    /// the parent. `--fork-point` then layers the fork point on as a negative revision
    /// (`builtin/rebase.c`), so the replayed window is still `<fork point>..<branch>` — the
    /// same window the frozen SHA names; the todo within it is what changes (commits git
    /// marks as already applied upstream are dropped).
    ///
    /// **Frozen form** — `--onto <parent> <fork point> <branch>`, what git-chain has always
    /// run. Passing the fork point as `<upstream>` makes that left side empty, so no commit
    /// can ever be marked PATCHSAME: git-chain behaves as though `--reapply-cherry-picks`
    /// were always in effect, and a commit already applied on the parent gets replayed —
    /// conflicting whenever the parent has since touched the same lines. That is
    /// REBASE_AUDIT.md finding F1.
    ///
    /// `allow_fork_point` is false under `--no-fork-point`, which forces the frozen form
    /// outright: the user has told us the reflog is not to be trusted.
    ///
    /// The dedup form is used only when git's own fork-point calculation, run now, agrees
    /// exactly with the SHA pre-computed before any branch in the chain moved. That
    /// pre-computation is reflog-independent and remains the source of truth; `--fork-point`
    /// consults the parent's reflog, which can be expired or misleading. When the two
    /// disagree — or the calculation fails outright — the frozen form is used and the
    /// replay window is exactly what it has always been.
    ///
    /// # The rest of the flag set
    ///
    /// `--empty=drop` pins what a chain rebase needs: a commit that *becomes* empty because
    /// the parent already carries its changes — the normal result of a squash-merge upstream
    /// — is dropped rather than left as a stray empty commit. This was already the effective
    /// behavior, but only via an unpinned default. It also keeps forcing the merge backend,
    /// which the previous `--keep-empty` did as a side effect: `builtin/rebase.c:1509-1510`
    /// runs `imply_merge(&options, "--empty")` for any explicit `--empty`.
    ///
    /// It replaces `--keep-empty`, which has been a no-op since git 2.26 (REBASE_AUDIT.md
    /// F3). The two flags govern different things and are not alternatives: `--empty`
    /// controls commits that *become* empty, while commits that *start* empty are kept
    /// unless `--no-keep-empty` is passed — `keep_empty` defaults to 1
    /// (`builtin/rebase.c:143`), and git-rebase.adoc says so outright. Dropping
    /// `--keep-empty` therefore changes nothing about deliberately-empty commits: they are
    /// still kept.
    ///
    /// `--no-rebase-merges` pins the flattening git-chain has always relied on, so a user's
    /// `rebase.rebaseMerges=true` cannot change chain semantics mid-run — the same
    /// config-isolation principle as `-c rebase.updateRefs=false` (REBASE_AUDIT.md F4). Note
    /// it does *not* imply the merge backend (`builtin/rebase.c:1597` only implies for
    /// `== 1`); `--empty` is what does.
    ///
    /// Version floor: `--empty` arrived in git 2.26 (absent in v2.25.0, present in v2.26.0).
    /// `--rebase-merges` has been negatable since it was added, and the
    /// `rebase.rebaseMerges` config that `--no-rebase-merges` guards against only exists
    /// from git 2.41 — which is also where an explicit `--no-rebase-merges` gained the
    /// power to override it. So the flag is effective exactly where the hazard exists, and
    /// harmless before it. The effective floor is **git 2.26**.
    pub fn build(
        &self,
        parent: &str,
        fork_point: &str,
        branch: &str,
        allow_fork_point: bool,
    ) -> (String, CommandSpec) {
        let mut args: Vec<OsString> = [
            "-c",
            "rebase.updateRefs=false",
            "rebase",
            "--empty=drop",
            "--no-rebase-merges",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();

        if allow_fork_point && self.fork_point_matches(parent, fork_point, branch) {
            args.extend(["--fork-point", "--onto", parent, parent, branch].map(OsString::from));
            let printable = format!(
                "git -c rebase.updateRefs=false rebase --empty=drop --no-rebase-merges \
                 --fork-point --onto {} {} {}",
                parent, parent, branch
            );
            return (printable, self.context.git_command(args));
        }

        args.extend(["--onto", parent, fork_point, branch].map(OsString::from));
        let printable = format!(
            "git -c rebase.updateRefs=false rebase --empty=drop --no-rebase-merges --onto {} {} {}",
            parent, fork_point, branch
        );
        (printable, self.context.git_command(args))
    }

    /// True when git's fork-point calculation, run right now, yields exactly the SHA that
    /// was pre-computed before any branch in the chain moved.
    ///
    /// The parent's reflog retains its pre-run tip even after this run already rebased the
    /// parent, so this normally agrees. It does not when the reflog has been expired or
    /// rewritten, and a disagreement means the replay window would change — so the caller
    /// falls back to the frozen SHA rather than trusting the reflog.
    fn fork_point_matches(&self, parent: &str, fork_point: &str, branch: &str) -> bool {
        let command = self.context.git_command(
            ["merge-base", "--fork-point", parent, branch]
                .map(OsString::from)
                .to_vec(),
        );
        let output = self.runner.output(&command);

        match output {
            Ok(result) if result.status.success() => {
                String::from_utf8_lossy(&result.stdout).trim() == fork_point
            }
            _ => false,
        }
    }
}
