//! Branch-label popout and context-menu controller.
//!
//! Tracks which overlay popouts (branch pill list, context menus) are active
//! and enforces the "open popout survives while a context menu is up" rule
//! documented alongside the tests.

use std::time::{Duration, Instant};

use iced::{Point, Rectangle};

use super::context_menu::{
    CommitContextMenuState, ContextMenuState, DirtyFileContextMenuState, ResetHoverTracker,
    ResetSubmenuState, WorktreeContextMenuState,
};

const BRANCH_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct BranchPopoutState {
    pub(in crate::screens::repository) commit_idx: usize,
    pub(in crate::screens::repository) commit_hash: String,
    pub(in crate::screens::repository) trigger_bounds: Option<Rectangle>,
    pub(in crate::screens::repository) panel_bounds: Option<Rectangle>,
    pub(in crate::screens::repository) content_bounds: Option<Rectangle>,
}

#[derive(Debug, Clone)]
struct BranchClickState {
    branch_name: String,
    remote_ref: Option<String>,
    at: Instant,
}

pub(in crate::screens::repository) enum BranchPressOutcome {
    None,
    CheckoutLocal(String),
    CheckoutRemote {
        branch_name: String,
        remote_ref: Option<String>,
    },
}

pub(in crate::screens::repository) struct SidebarContextMenuRequest {
    pub(in crate::screens::repository) branch_name: String,
    pub(in crate::screens::repository) is_remote: bool,
    pub(in crate::screens::repository) is_tag: bool,
    pub(in crate::screens::repository) remote_name: Option<String>,
    pub(in crate::screens::repository) tag_remote_names: Vec<String>,
    pub(in crate::screens::repository) tag_push_remote_names: Vec<String>,
    pub(in crate::screens::repository) can_fast_forward: bool,
    pub(in crate::screens::repository) position: Point,
}

#[derive(Debug, Default)]
pub(in crate::screens::repository) struct BranchPopoutController {
    popout: Option<BranchPopoutState>,
    context_menu: Option<ContextMenuState>,
    commit_context_menu: Option<CommitContextMenuState>,
    dirty_file_context_menu: Option<DirtyFileContextMenuState>,
    worktree_context_menu: Option<WorktreeContextMenuState>,
    reset_submenu: Option<ResetSubmenuState>,
    reset_hover: ResetHoverTracker,
    last_branch_click: Option<BranchClickState>,
}

impl BranchPopoutController {
    pub(in crate::screens::repository) fn active(&self) -> Option<&BranchPopoutState> {
        self.popout.as_ref()
    }

    pub(in crate::screens::repository) fn active_context_menu(&self) -> Option<&ContextMenuState> {
        self.context_menu.as_ref()
    }

    pub(in crate::screens::repository) fn active_commit_context_menu(
        &self,
    ) -> Option<&CommitContextMenuState> {
        self.commit_context_menu.as_ref()
    }

    pub(in crate::screens::repository) fn active_dirty_file_context_menu(
        &self,
    ) -> Option<&DirtyFileContextMenuState> {
        self.dirty_file_context_menu.as_ref()
    }

    pub(in crate::screens::repository) fn active_worktree_context_menu(
        &self,
    ) -> Option<&WorktreeContextMenuState> {
        self.worktree_context_menu.as_ref()
    }

    pub(in crate::screens::repository) fn active_reset_submenu(
        &self,
    ) -> Option<&ResetSubmenuState> {
        self.reset_submenu.as_ref()
    }

    pub(in crate::screens::repository) fn close_reset_submenu_only(&mut self) {
        self.reset_submenu = None;
        self.reset_hover = ResetHoverTracker::default();
    }

    pub(in crate::screens::repository) fn reset_hover_mut(&mut self) -> &mut ResetHoverTracker {
        &mut self.reset_hover
    }

    pub(in crate::screens::repository) fn reset_hover(&self) -> &ResetHoverTracker {
        &self.reset_hover
    }

    pub(in crate::screens::repository) fn open_reset_submenu(
        &mut self,
        commit_hash: String,
        fallback_position: Point,
    ) {
        let position = self
            .commit_context_menu
            .as_ref()
            .map(|state| state.position)
            .unwrap_or(fallback_position);
        self.reset_submenu = Some(ResetSubmenuState {
            commit_hash,
            position,
        });
    }

    pub(in crate::screens::repository) fn open_dirty_file_context_menu(
        &mut self,
        path: String,
        position: Point,
    ) {
        self.dirty_file_context_menu = Some(DirtyFileContextMenuState { path, position });
    }

    pub(in crate::screens::repository) fn open_worktree_context_menu(
        &mut self,
        path: std::path::PathBuf,
        branch_name: String,
        is_active: bool,
        position: Point,
    ) {
        self.worktree_context_menu = Some(WorktreeContextMenuState {
            path,
            branch_name,
            is_active,
            position,
        });
    }

    pub(in crate::screens::repository) fn open_context_menu(&mut self, state: ContextMenuState) {
        self.context_menu = Some(state);
    }

    pub(in crate::screens::repository) fn open_commit_context_menu(
        &mut self,
        state: CommitContextMenuState,
    ) {
        self.commit_context_menu = Some(state);
    }

    pub(in crate::screens::repository) fn open_sidebar_context_menu(
        &mut self,
        request: SidebarContextMenuRequest,
    ) {
        let SidebarContextMenuRequest {
            branch_name,
            is_remote,
            is_tag,
            remote_name,
            tag_remote_names,
            tag_push_remote_names,
            can_fast_forward,
            position,
        } = request;

        self.context_menu = Some(ContextMenuState {
            branch_name: branch_name.clone(),
            tag_remote_names,
            tag_push_remote_names,
            is_remote,
            has_remote: is_remote,
            is_tag,
            remote_name,
            remote_branch_name: if is_remote { Some(branch_name) } else { None },
            can_fast_forward,
            position,
        });
    }

    pub(in crate::screens::repository) fn close_context_menu(&mut self) {
        self.popout = None;
        self.context_menu = None;
        self.commit_context_menu = None;
        self.dirty_file_context_menu = None;
        self.worktree_context_menu = None;
        self.reset_submenu = None;
        self.reset_hover = ResetHoverTracker::default();
    }

    pub(in crate::screens::repository) fn close_popout(&mut self) {
        self.popout = None;
    }

    pub(in crate::screens::repository) fn close(&mut self) {
        self.popout = None;
        self.context_menu = None;
        self.commit_context_menu = None;
        self.dirty_file_context_menu = None;
        self.worktree_context_menu = None;
        self.reset_submenu = None;
        self.reset_hover = ResetHoverTracker::default();
    }

    /// Keep the popout open but mark its bounds as stale so they get
    /// re-measured after a content update (e.g. branch checkout).
    pub(in crate::screens::repository) fn sync_commit_indices(
        &mut self,
        commits: &[crate::core::Commit],
    ) {
        if let Some(state) = self.popout.as_mut() {
            if let Some(idx) = commits.iter().position(|c| c.hash == state.commit_hash) {
                state.commit_idx = idx;
            } else {
                self.popout = None;
            }
        }
        if let Some(state) = self.commit_context_menu.as_mut() {
            if let Some(idx) = commits.iter().position(|c| c.hash == state.commit_hash) {
                state.commit_idx = idx;
            } else {
                self.commit_context_menu = None;
            }
        }
    }

    /// Fetch-path variant of `sync_commit_indices`: patches stored indices in
    /// place, but never closes the popout or context menu when the hash is no
    /// longer in view. A fetch must not disturb open UI; if the hash has
    /// genuinely gone away the next user action will close it.
    pub(in crate::screens::repository) fn patch_commit_indices_or_noop(
        &mut self,
        commits: &[crate::core::Commit],
    ) {
        if let Some(state) = self.popout.as_mut() {
            if let Some(idx) = commits.iter().position(|c| c.hash == state.commit_hash) {
                state.commit_idx = idx;
            }
        }
        if let Some(state) = self.commit_context_menu.as_mut() {
            if let Some(idx) = commits.iter().position(|c| c.hash == state.commit_hash) {
                state.commit_idx = idx;
            }
        }
    }

    pub(in crate::screens::repository) fn invalidate_bounds(&mut self) {
        if let Some(state) = self.popout.as_mut() {
            state.trigger_bounds = None;
            state.panel_bounds = None;
            state.content_bounds = None;
        }
    }

    pub(in crate::screens::repository) fn open(
        &mut self,
        commit_idx: usize,
        commit_hash: String,
    ) -> bool {
        match self.popout {
            Some(ref state) if state.commit_idx == commit_idx => {
                state.trigger_bounds.is_none()
                    || state.panel_bounds.is_none()
                    || state.content_bounds.is_none()
            }
            _ => {
                self.popout = Some(BranchPopoutState {
                    commit_idx,
                    commit_hash,
                    trigger_bounds: None,
                    panel_bounds: None,
                    content_bounds: None,
                });
                true
            }
        }
    }

    pub(in crate::screens::repository) fn update_trigger_bounds(
        &mut self,
        bounds: Option<Rectangle>,
        cursor: Option<Point>,
    ) {
        if let Some(state) = self.popout.as_mut() {
            state.trigger_bounds = bounds;
        }
        if let Some(cursor) = cursor {
            self.close_if_cursor_outside(cursor);
        }
    }

    pub(in crate::screens::repository) fn update_panel_bounds(
        &mut self,
        bounds: Option<Rectangle>,
        cursor: Option<Point>,
    ) {
        if let Some(state) = self.popout.as_mut() {
            state.panel_bounds = bounds;
        }
        if let Some(cursor) = cursor {
            self.close_if_cursor_outside(cursor);
        }
    }

    pub(in crate::screens::repository) fn update_content_bounds(
        &mut self,
        bounds: Option<Rectangle>,
    ) {
        if let Some(state) = self.popout.as_mut() {
            state.content_bounds = bounds;
        }
    }

    pub(in crate::screens::repository) fn needs_panel_bounds_measurement(&self) -> bool {
        self.popout.as_ref().is_some_and(|state| {
            state.trigger_bounds.is_some()
                && state.content_bounds.is_some()
                && state.panel_bounds.is_none()
        })
    }

    pub(in crate::screens::repository) fn pointer_moved(&mut self, position: Point) {
        if self.commit_context_menu.is_none()
            && self.dirty_file_context_menu.is_none()
            && self.worktree_context_menu.is_none()
        {
            self.close_if_cursor_outside(position);
        }
    }

    pub(in crate::screens::repository) fn pointer_left_window(&mut self) {
        if self.context_menu.is_none()
            && self.commit_context_menu.is_none()
            && self.dirty_file_context_menu.is_none()
            && self.worktree_context_menu.is_none()
        {
            self.popout = None;
        };
    }

    pub(in crate::screens::repository) fn handle_branch_pressed(
        &mut self,
        branch_name: String,
        is_remote_only: bool,
        remote_ref: Option<String>,
    ) -> BranchPressOutcome {
        if self.branch_label_was_double_clicked(&branch_name, remote_ref.as_deref()) {
            self.last_branch_click = None;
            return if is_remote_only {
                BranchPressOutcome::CheckoutRemote {
                    branch_name,
                    remote_ref,
                }
            } else {
                BranchPressOutcome::CheckoutLocal(branch_name)
            };
        }

        self.last_branch_click = Some(BranchClickState {
            branch_name,
            remote_ref,
            at: Instant::now(),
        });
        BranchPressOutcome::None
    }

    fn close_if_cursor_outside(&mut self, cursor_position: Point) {
        if self.context_menu.is_some()
            || self.commit_context_menu.is_some()
            || self.dirty_file_context_menu.is_some()
            || self.worktree_context_menu.is_some()
        {
            return;
        }

        let should_close = self.popout.as_ref().is_some_and(|state| {
            let Some(trigger_bounds) = state.trigger_bounds else {
                return false;
            };
            let Some(panel_bounds) = state.panel_bounds else {
                return false;
            };

            !trigger_bounds.contains(cursor_position) && !panel_bounds.contains(cursor_position)
        });

        if should_close {
            self.popout = None;
        }
    }

    fn branch_label_was_double_clicked(&self, branch_name: &str, remote_ref: Option<&str>) -> bool {
        self.last_branch_click.as_ref().is_some_and(|last_click| {
            last_click.branch_name == branch_name
                && last_click.remote_ref.as_deref() == remote_ref
                && last_click.at.elapsed() <= BRANCH_DOUBLE_CLICK_WINDOW
        })
    }
}

#[cfg(test)]
#[path = "popout_tests.rs"]
mod tests;
