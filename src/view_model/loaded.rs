use std::collections::HashSet;

use crate::view_model::{
    commit_presentation::CommitPresentation,
    diff_view::CommitDiffState,
    graph_view::{GraphRow, ProjectionSignature, RepositoryProjection},
    sidebar_view::SidebarSection,
};

/// Result of taking a raw `RepoSnapshot` through the presenter off the main
/// thread. Carries the UI-ready projection plus the signature used to decide
/// whether the graph canvas tile cache needs invalidation.
#[derive(Debug, Clone)]
pub struct LoadedRepo {
    pub projection: RepositoryProjection,
    pub has_more_commits: bool,
    pub signature: ProjectionSignature,
}

/// Narrow projection of a `RefsSnapshot`. Shape mirrors `LoadedRepo` minus
/// `repo_name`/`current_branch`, which a fetch cannot change.
#[derive(Debug, Clone)]
pub struct LoadedRefs {
    pub commits: Vec<crate::core::Commit>,
    pub commit_presentations: Vec<CommitPresentation>,
    pub commit_diff_states: Vec<CommitDiffState>,
    pub graph_rows: Vec<GraphRow>,
    pub sidebar_sections: Vec<SidebarSection>,
    pub num_lanes: usize,
    pub head_hash: Option<String>,
    pub default_remote_name: Option<String>,
    pub fast_forward_candidates: HashSet<String>,
    pub signature: ProjectionSignature,
    pub has_more_commits: bool,
    /// Local + remote branch refs (tags filtered out). Kept so
    /// plugin-host `leviathan.repository` tables can update after a fetch.
    pub branch_refs: Vec<crate::services::RepoRef>,
}

#[derive(Debug, Clone)]
pub enum LoadedBranchMergeOutcome {
    Merged(LoadedRepo),
    Conflicted(LoadedRepo),
}

#[derive(Debug, Clone)]
pub enum LoadedCherryPickOutcome {
    Committed(LoadedRepo),
    Conflicted(LoadedRepo),
}

#[derive(Debug, Clone)]
pub enum LoadedStashApplyOutcome {
    Applied(LoadedRepo),
    Conflicted(LoadedRepo),
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // Success carries LoadedRepo; boxing churns presenter + message call sites.
pub enum LoadedRemoteCheckoutOutcome {
    Success(LoadedRepo),
    LocalAheadOfRemote {
        branch_name: String,
        remote_ref: String,
    },
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // Pushed carries LoadedRepo; boxing churns presenter + message call sites.
pub enum LoadedPushOutcome {
    Pushed(LoadedRepo),
    NeedsUpstream {
        branch_name: String,
        remote_name: String,
    },
    BehindRemote {
        branch_name: String,
        remote_name: String,
    },
}
