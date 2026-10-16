//! Admission of persisted recovery data for execution. No repository writes belong here.

use std::collections::HashSet;

use super::is_usable_commit_id;
use crate::types::{ChainRebaseState, SquashedRebaseHandling};

/// A fixed branch plan assembled only after the persisted collections are validated.
pub(crate) struct ResumePlan {
    branches: Vec<PlannedBranch>,
    squashed_merge_handling: SquashedRebaseHandling,
}

/// Replay inputs for one branch, kept together instead of indexing independent collections.
pub(crate) struct PlannedBranch {
    name: String,
    parent: String,
    frozen_base: String,
    original_tip: String,
}

impl ResumePlan {
    pub(super) fn from_state(state: &ChainRebaseState) -> Result<Self, String> {
        if state.branches.is_empty() {
            return Err("no branches were recorded".into());
        }
        if state.branches.len() != state.merge_bases.len() {
            return Err(format!(
                "{} branches but {} merge bases were recorded",
                state.branches.len(),
                state.merge_bases.len(),
            ));
        }
        if state.chain_name.is_empty()
            || state.root_branch.is_empty()
            || state.original_branch.is_empty()
        {
            return Err("chain name, root branch and original branch must not be empty".into());
        }
        let squashed_merge_handling = match state.options.squashed_merge_handling.as_str() {
            "reset" => SquashedRebaseHandling::Reset,
            "skip" => SquashedRebaseHandling::Skip,
            "rebase" => SquashedRebaseHandling::Rebase,
            policy => return Err(format!("unknown squash policy '{policy}'")),
        };
        let mut names = HashSet::new();
        let mut branches = Vec::with_capacity(state.branches.len());
        let mut expected_parent = state.root_branch.as_str();
        for (branch, base) in state.branches.iter().zip(&state.merge_bases) {
            if branch.name.is_empty() {
                return Err("a recorded branch name is empty".into());
            }
            if !names.insert(branch.name.as_str()) {
                return Err(format!("duplicate branch '{}'", branch.name));
            }
            if branch.name == state.root_branch {
                return Err(format!(
                    "root branch '{}' is also a chain member",
                    state.root_branch
                ));
            }
            if branch.parent != expected_parent {
                return Err(format!(
                    "parent of branch '{}' is '{}', expected '{}'",
                    branch.name, branch.parent, expected_parent,
                ));
            }
            if !is_usable_commit_id(base) {
                return Err(format!("invalid merge base for branch '{}'", branch.name));
            }
            let original_tip = state
                .original_refs
                .get(&branch.name)
                .ok_or_else(|| format!("missing original commit for branch '{}'", branch.name))?;
            if !is_usable_commit_id(original_tip) {
                return Err(format!(
                    "invalid original commit for branch '{}'",
                    branch.name
                ));
            }
            branches.push(PlannedBranch {
                name: branch.name.clone(),
                parent: branch.parent.clone(),
                frozen_base: base.clone(),
                original_tip: original_tip.clone(),
            });
            expected_parent = &branch.name;
        }
        // Historical redundant counters and additive defaults are not admission checks.
        // Ref existence is checked by the existing resume policy, which allows deleted
        // pending members to be skipped. Stored IDs retain the existing 40/64-digit shape;
        // Git remains responsible for resolving the objects when operations need them.
        Ok(Self {
            branches,
            squashed_merge_handling,
        })
    }

    /// Returns the immutable ordered replay plan; status updates do not change membership.
    pub(crate) fn branches(&self) -> &[PlannedBranch] {
        &self.branches
    }

    /// Returns the admitted squash policy without silently defaulting an unknown string.
    pub(crate) fn squashed_merge_handling(&self) -> SquashedRebaseHandling {
        self.squashed_merge_handling
    }
}

impl PlannedBranch {
    /// Returns the local branch to replay.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Returns its recorded parent.
    pub(crate) fn parent(&self) -> &str {
        &self.parent
    }

    /// Returns the base frozen before this run moved any branches.
    pub(crate) fn frozen_base(&self) -> &str {
        &self.frozen_base
    }

    /// Returns the nonzero original tip used by recovery.
    pub(crate) fn original_tip(&self) -> &str {
        &self.original_tip
    }
}
