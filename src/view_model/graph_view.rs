use std::collections::HashSet;

use crate::{
    core::Commit,
    view_model::{
        commit_presentation::CommitPresentation, diff_view::CommitDiffState,
        sidebar_view::SidebarSection,
    },
};

/// A line segment drawn in a single commit row of the graph.
/// from_lane/to_lane are fractional lane positions (0.0 = lane 0 center, 1.0 = lane 1 center…)
#[derive(Debug, Clone)]
pub struct GraphSegment {
    pub from_lane: f32,
    pub to_lane: f32,
}

/// Graph rendering data for one commit row.
#[derive(Debug, Clone)]
pub struct GraphRow {
    /// Segments for the upper half of the row (from top to commit dot).
    pub upper: Vec<GraphSegment>,
    /// Segments for the lower half of the row (from commit dot to bottom).
    pub lower: Vec<GraphSegment>,
    pub dotted_upper: Vec<GraphSegment>,
    pub dotted_lower: Vec<GraphSegment>,
    /// Dotted merge arms used by synthetic dirty merge rows.
    pub dotted_connectors: Vec<GraphSegment>,
    /// Lane index where the commit dot appears.
    pub commit_lane: usize,
    /// Author initials displayed inside the avatar circle (e.g. "RS").
    pub author_initials: String,
    /// True for merge commits — drawn as a solid filled dot with no initials.
    pub is_merge: bool,
    /// Horizontal line segments drawn at mid-row (commit y-level) connecting
    /// a through-branch to the commit dot. from_lane/to_lane are lane indices.
    pub connectors: Vec<GraphSegment>,
    /// False for synthetic rows that reserve vertical space but do not
    /// represent an actual commit node.
    pub has_commit: bool,
    /// True for stash rows — drawn as a dotted square with an inbox glyph
    /// instead of an avatar / merge dot.
    pub is_stash: bool,
}

/// Full projection of a repository snapshot ready to feed the UI.
#[derive(Debug, Clone)]
pub struct RepositoryProjection {
    pub commits: Vec<Commit>,
    pub commit_presentations: Vec<CommitPresentation>,
    /// Per-commit diff state aligned with `commits`. Initially empty for
    /// committed rows (populated on-demand by `load_commit_diff`); for the
    /// dirty row it is filled in at projection time from the `DirtySnapshot`.
    pub commit_diff_states: Vec<CommitDiffState>,
    pub graph_rows: Vec<GraphRow>,
    pub sidebar_sections: Vec<SidebarSection>,
    pub num_lanes: usize,
    pub repo_name: String,
    pub current_branch: String,
    pub head_hash: Option<String>,
    pub default_remote_name: Option<String>,
    pub remote_names: Vec<String>,
    pub fast_forward_candidates: HashSet<String>,
    pub worktrees: Vec<crate::core::WorktreeInfo>,
    /// Refs kept on the projection so the plugin-host
    /// `leviathan.repository` tables can be rebuilt without re-running
    /// libgit2 on every sync.
    pub branch_refs: Vec<crate::services::RepoRef>,
}

/// Hash over the inputs that determine the rendered graph. Two snapshots with
/// the same signature produce the same `graph_rows`, so the canvas tile cache
/// does not need to be invalidated — skipping the `graph_revision` bump avoids
/// a full redraw on every file-watcher cascade tick that only toggled, say,
/// the tag-remotes cache without actually changing refs or commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionSignature(pub u64);
