use std::collections::HashSet;
use std::time::Instant;

use crate::view_model::{SidebarSection, SidebarSectionKind};

use super::super::super::state::{SidebarClickState, SIDEBAR_DOUBLE_CLICK_WINDOW};

pub struct SidebarPanel {
    pub(super) last_click: Option<SidebarClickState>,
    pub(super) last_worktree_click: Option<SidebarClickState>, // separate slot for worktree double-click
    expanded_sections: HashSet<SidebarSectionKind>,
    filter_query: String,
}

impl SidebarPanel {
    pub(in crate::screens::repository) fn new(
        expanded_sections: HashSet<SidebarSectionKind>,
    ) -> Self {
        Self {
            last_click: None,
            last_worktree_click: None,
            expanded_sections,
            filter_query: String::new(),
        }
    }

    pub(in crate::screens::repository) fn filter_query(&self) -> &str {
        &self.filter_query
    }

    pub(in crate::screens::repository) fn set_filter_query(&mut self, q: String) {
        self.filter_query = q;
    }

    pub(in crate::screens::repository) fn default_expanded() -> HashSet<SidebarSectionKind> {
        let mut s = HashSet::new();
        s.insert(SidebarSectionKind::Local);
        s.insert(SidebarSectionKind::Remote);
        s
    }

    pub(in crate::screens::repository) fn is_section_expanded(
        &self,
        kind: SidebarSectionKind,
    ) -> bool {
        self.expanded_sections.contains(&kind)
    }

    /// Toggles the section at `idx`. Returns the resolved section kind and
    /// its new expanded state so the caller can persist the change.
    pub(in crate::screens::repository) fn toggle_section(
        &mut self,
        sections: &[SidebarSection],
        idx: usize,
    ) -> Option<(SidebarSectionKind, bool)> {
        let section = sections.get(idx)?;
        let now_expanded = if self.expanded_sections.remove(&section.kind) {
            false
        } else {
            self.expanded_sections.insert(section.kind);
            true
        };
        Some((section.kind, now_expanded))
    }

    /// Double-click gate for a branch row. Returns `true` if the press
    /// qualifies as a confirmed double-click (caller should perform the
    /// checkout); otherwise the click is recorded and `false` is returned.
    pub(super) fn register_branch_click(&mut self, branch_name: &str) -> bool {
        let now = Instant::now();
        let is_double = self.last_click.as_ref().is_some_and(|last| {
            last.branch_name == branch_name && last.at.elapsed() <= SIDEBAR_DOUBLE_CLICK_WINDOW
        });
        if is_double {
            self.last_click = None;
            true
        } else {
            self.last_click = Some(SidebarClickState {
                branch_name: branch_name.to_string(),
                at: now,
            });
            false
        }
    }

    /// Double-click gate for a worktree entry row. Reuses `SidebarClickState`
    /// with the path rendered as a string. Returns `true` on confirmed
    /// double-click (caller performs focus swap); otherwise records the click
    /// and returns `false`.
    pub(super) fn register_worktree_click(&mut self, path: &std::path::Path) -> bool {
        let now = Instant::now();
        let path_key = path.to_string_lossy();
        let is_double = self.last_worktree_click.as_ref().is_some_and(|last| {
            last.branch_name == path_key.as_ref()
                && last.at.elapsed() <= SIDEBAR_DOUBLE_CLICK_WINDOW
        });
        if is_double {
            self.last_worktree_click = None;
            true
        } else {
            self.last_worktree_click = Some(SidebarClickState {
                branch_name: path_key.to_string(),
                at: now,
            });
            false
        }
    }
}

#[derive(Debug, Clone)]
pub enum SidebarAction {
    SectionToggled(usize),
    BranchPressed {
        branch_name: String,
        is_remote: bool,
    },
    BranchRightClicked {
        branch_name: String,
        is_remote: bool,
        is_tag: bool,
        remote_name: Option<String>,
    },
    StashPressed {
        hash: String,
    },
    WorktreeEntryPressed {
        path: std::path::PathBuf,
    },
    WorktreeEntryRightClicked {
        path: std::path::PathBuf,
        branch_name: String,
    },
    ResizeStarted,
    Focused,
    FilterChanged(String),
}
