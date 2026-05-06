//! Presenter: projects raw service snapshots into UI-ready view models.
//!
//! Injected into `RepositoryScreen` as `Arc<dyn Presenter>` so tests can swap
//! in a fake. The default implementation runs synchronously and is pure: same
//! inputs always produce the same outputs.

pub mod default;
pub mod projection;
pub mod signature;

use crate::services::{
    BranchMergeOutcome, CherryPickOutcome, PushGatewayOutcome, RefsSnapshot, RemoteCheckoutOutcome,
    RepoSnapshot, RevertOutcome, StashApplyOutcome,
};
use crate::view_model::{
    LoadedBranchMergeOutcome, LoadedCherryPickOutcome, LoadedDirtyIndex, LoadedPushOutcome,
    LoadedRefs, LoadedRemoteCheckoutOutcome, LoadedRepo, LoadedRevertOutcome,
    LoadedStashApplyOutcome,
};

pub use default::DefaultPresenter;

pub trait Presenter: Send + Sync + 'static {
    fn project_loaded(&self, snap: RepoSnapshot) -> LoadedRepo;
    fn project_refs(&self, snap: RefsSnapshot) -> LoadedRefs;
    fn project_dirty_index(&self, snap: Option<crate::services::DirtySnapshot>)
        -> LoadedDirtyIndex;
    fn project_remote_checkout(
        &self,
        outcome: RemoteCheckoutOutcome,
    ) -> LoadedRemoteCheckoutOutcome;
    fn project_branch_merge(&self, outcome: BranchMergeOutcome) -> LoadedBranchMergeOutcome;
    fn project_cherry_pick(&self, outcome: CherryPickOutcome) -> LoadedCherryPickOutcome;
    fn project_revert(&self, outcome: RevertOutcome) -> LoadedRevertOutcome;
    fn project_push(&self, outcome: PushGatewayOutcome) -> LoadedPushOutcome;
    fn project_stash_apply(&self, outcome: StashApplyOutcome) -> LoadedStashApplyOutcome;
}
