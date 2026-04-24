//! Panel components for the repository screen.
//!
//! Each panel owns its state, an action enum, an update method, and a view
//! method. Screen-owned state the panel may mutate during update is handed
//! in through a shared `ScreenCtx`; sibling panels that need mutation are
//! passed alongside as explicit arguments (keeps the borrow checker happy
//! without hand-rolled split_at_mut plumbing).

use std::collections::HashSet;
use std::sync::Arc;

use crate::core::TabId;
use crate::services::presenter::Presenter;
use crate::services::SharedRepositoryGateway;
use crate::view_model::SidebarSectionKind;

use super::input::InputState;
use super::overlays::OverlayManager;
use super::state::{MergedDiffCache, RepositoryData};

pub(super) mod center;
pub(super) mod detail;
pub(super) mod diff;
pub(super) mod sidebar;

/// The four panels that compose the repository screen body. Grouped so the
/// screen struct owns a single `panels` field rather than four siblings,
/// and so split-borrows between "one panel + everything else" are a single
/// `&mut self.panels.sidebar` / `&mut self.panels.center` at call sites.
pub(in crate::screens::repository) struct Panels {
    pub sidebar: sidebar::SidebarPanel,
    pub center: center::CenterPanel,
    pub detail: detail::DetailPanel,
    pub diff: diff::DiffPanel,
}

impl Panels {
    pub(in crate::screens::repository) fn new(
        sidebar_expanded: HashSet<SidebarSectionKind>,
    ) -> Self {
        Self {
            sidebar: sidebar::SidebarPanel::new(sidebar_expanded),
            center: center::CenterPanel::new(),
            detail: detail::DetailPanel::new(),
            diff: diff::DiffPanel::new(),
        }
    }
}

/// Screen-level mutable state exposed to panel update methods.
///
/// Collapsed from twelve fields to seven by grouping transient input into
/// `InputState`, collapsing the merged-diff cache fields into
/// `MergedDiffCache`, and moving `branch_popout`/`commit_search` into
/// `RepositoryData` (reached via `ctx.data.*`). Sibling panels are not part
/// of this ctx — each update method declares the peer refs it needs as
/// explicit parameters, so borrows stay disjoint.
///
/// `repository` is an owned `Arc` clone sourced from `fleet.active()`. Owning
/// rather than borrowing is necessary because holding `&fleet.active()` while
/// also holding `&mut fleet` in the same ctx would fail the borrow checker.
/// Arc-clone is cheap so ownership is acceptable.
pub(in crate::screens::repository) struct ScreenCtx<'a> {
    pub tab_id: TabId,
    pub repository: SharedRepositoryGateway,  // OWNED (Arc clone from fleet.active())
    pub presenter: &'a Arc<dyn Presenter>,
    pub data: &'a mut RepositoryData,
    pub overlay_manager: &'a mut OverlayManager,
    pub input: &'a mut InputState,
    pub merged_diff: &'a mut MergedDiffCache,
    pub fleet: &'a mut crate::screens::repository::state::GatewayFleet,
}
