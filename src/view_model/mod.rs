//! UI projections of `core` domain types.
//!
//! Produced by `services::presenter` from service-layer snapshots. Consumed
//! by screens and widgets. No git-specific logic lives here — only the
//! structures the UI renders from.

pub mod commit_presentation;
pub mod diff_view;
pub mod graph_view;
pub mod loaded;
pub mod sidebar_view;

pub use commit_presentation::{BranchDisplayRow, BranchLabel, BranchLabelKind, CommitPresentation};
pub use diff_view::CommitDiffState;
pub use graph_view::{GraphRow, GraphSegment, ProjectionSignature, RepositoryProjection};
pub use loaded::{
    LoadedBranchMergeOutcome, LoadedCherryPickOutcome, LoadedDirtyIndex, LoadedDirtyRow,
    LoadedPushOutcome, LoadedRefs, LoadedRemoteCheckoutOutcome, LoadedRepo, LoadedRevertOutcome,
    LoadedStashApplyOutcome,
};
pub use sidebar_view::{Branch, SidebarSection, SidebarSectionKind, SidebarStash};
