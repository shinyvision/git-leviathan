//! Hash- and branch-name → commit-index lookups.
//!
//! Computed from `RepositorySnapshot`. The index is intentionally stateless:
//! methods take the current commits/presentations slices and return the
//! matching position. Keeps the lookup logic collected in one place without
//! owning a duplicate copy of the data.

use crate::{
    core::{Commit, CommitKind},
    view_model::{BranchLabelKind, CommitPresentation},
};

pub(in crate::screens::repository) struct CommitIndex;

impl CommitIndex {
    pub(in crate::screens::repository) fn new() -> Self {
        Self
    }

    pub(in crate::screens::repository) fn index_for_hash(
        &self,
        commits: &[Commit],
        hash: &str,
    ) -> Option<usize> {
        commits.iter().position(|commit| commit.hash == hash)
    }

    pub(in crate::screens::repository) fn index_for_branch(
        &self,
        presentations: &[CommitPresentation],
        branch_name: &str,
        is_remote: bool,
    ) -> Option<usize> {
        let preferred = presentations.iter().position(|p| {
            p.branch_labels.iter().any(|l| {
                if l.name != branch_name {
                    return false;
                }
                if is_remote {
                    l.kind == BranchLabelKind::Remote
                } else {
                    matches!(
                        l.kind,
                        BranchLabelKind::Local | BranchLabelKind::CurrentLocal
                    )
                }
            })
        });

        if preferred.is_some() {
            return preferred;
        }

        presentations
            .iter()
            .position(|p| p.branch_labels.iter().any(|l| l.name == branch_name))
    }

    pub(in crate::screens::repository) fn commit_hash(
        &self,
        commits: &[Commit],
        idx: usize,
    ) -> Option<String> {
        commits
            .get(idx)
            .filter(|commit| matches!(commit.kind, CommitKind::Commit | CommitKind::Stash))
            .map(|commit| commit.hash.clone())
    }
}
