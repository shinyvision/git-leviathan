use crate::services::{
    BranchMergeOutcome, CherryPickOutcome, PushGatewayOutcome, RefsSnapshot, RemoteCheckoutOutcome,
    RepoSnapshot, StashApplyOutcome,
};
use crate::view_model::{
    LoadedBranchMergeOutcome, LoadedCherryPickOutcome, LoadedDirtyIndex, LoadedPushOutcome,
    LoadedRefs, LoadedRemoteCheckoutOutcome, LoadedRepo, LoadedStashApplyOutcome,
};

use super::projection;
use super::Presenter;

/// Default presenter: runs projection synchronously on the calling thread.
///
/// Used in production by `App` (shared via `Arc`). Tests may swap in a fake
/// via the `Presenter` trait.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPresenter;

impl DefaultPresenter {
    pub const fn new() -> Self {
        Self
    }
}

impl Presenter for DefaultPresenter {
    fn project_loaded(&self, snapshot: RepoSnapshot) -> LoadedRepo {
        let span = crate::perf::Span::new("cpu.presenter_projection").field("kind", "loaded");
        let loaded = projection::project_loaded(snapshot);
        span.finish_with("commits", loaded.projection.commits.len());
        loaded
    }

    fn project_refs(&self, snapshot: RefsSnapshot) -> LoadedRefs {
        let span = crate::perf::Span::new("cpu.presenter_projection").field("kind", "refs");
        let loaded = projection::project_refs(snapshot);
        span.finish_with("commits", loaded.commits.len());
        loaded
    }

    fn project_dirty_index(
        &self,
        snapshot: Option<crate::services::DirtySnapshot>,
    ) -> LoadedDirtyIndex {
        projection::project_dirty_index(snapshot)
    }

    fn project_remote_checkout(
        &self,
        outcome: RemoteCheckoutOutcome,
    ) -> LoadedRemoteCheckoutOutcome {
        match outcome {
            RemoteCheckoutOutcome::Success(s) => {
                LoadedRemoteCheckoutOutcome::Success(Box::new(projection::project_loaded(*s)))
            }
            RemoteCheckoutOutcome::LocalAheadOfRemote {
                branch_name,
                remote_ref,
            } => LoadedRemoteCheckoutOutcome::LocalAheadOfRemote {
                branch_name,
                remote_ref,
            },
        }
    }

    fn project_branch_merge(&self, outcome: BranchMergeOutcome) -> LoadedBranchMergeOutcome {
        match outcome {
            BranchMergeOutcome::Merged(s) => {
                LoadedBranchMergeOutcome::Merged(projection::project_loaded(s))
            }
            BranchMergeOutcome::Conflicted(s) => {
                LoadedBranchMergeOutcome::Conflicted(projection::project_loaded(s))
            }
        }
    }

    fn project_cherry_pick(&self, outcome: CherryPickOutcome) -> LoadedCherryPickOutcome {
        match outcome {
            CherryPickOutcome::Committed(s) => {
                LoadedCherryPickOutcome::Committed(projection::project_loaded(s))
            }
            CherryPickOutcome::Conflicted(s) => {
                LoadedCherryPickOutcome::Conflicted(projection::project_loaded(s))
            }
        }
    }

    fn project_push(&self, outcome: PushGatewayOutcome) -> LoadedPushOutcome {
        match outcome {
            PushGatewayOutcome::Pushed(s) => {
                LoadedPushOutcome::Pushed(Box::new(projection::project_loaded(*s)))
            }
            PushGatewayOutcome::NeedsUpstream {
                branch_name,
                remote_name,
            } => LoadedPushOutcome::NeedsUpstream {
                branch_name,
                remote_name,
            },
            PushGatewayOutcome::BehindRemote {
                branch_name,
                remote_name,
            } => LoadedPushOutcome::BehindRemote {
                branch_name,
                remote_name,
            },
        }
    }

    fn project_stash_apply(&self, outcome: StashApplyOutcome) -> LoadedStashApplyOutcome {
        match outcome {
            StashApplyOutcome::Applied(s) => {
                LoadedStashApplyOutcome::Applied(projection::project_loaded(s))
            }
            StashApplyOutcome::Conflicted(s) => {
                LoadedStashApplyOutcome::Conflicted(projection::project_loaded(s))
            }
        }
    }
}
