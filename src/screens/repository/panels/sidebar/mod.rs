//! Sidebar panel — left column showing local/remote branches, worktrees,
//! stashes, tags. Owns its expanded-section set and double-click tracker;
//! width and resize activity live in `RepositoryData::resize`.

use iced::Element;

use crate::message::Message;

use super::center::CenterPanel;

mod state;
mod update;
mod view;

pub(in crate::screens::repository) use state::{SidebarAction, SidebarPanel};
pub(in crate::screens::repository) use update::{
    commit_selection_changed_task, load_more_commits, try_resolve_pending_focus,
};
pub(in crate::screens::repository) use view::filter_input_id;

/// Read-only view projection the sidebar consumes. Built by the screen from
/// `RepositoryData`; the sidebar never reads sibling panel state directly.
#[derive(Clone, Copy)]
pub(in crate::screens::repository) struct SidebarViewCtx<'a> {
    pub sections: &'a [crate::view_model::SidebarSection],
    pub commit_count: usize,
    pub width: f32,
    pub is_resizing: bool,
    pub active_worktree_path: &'a std::path::Path,
}

impl SidebarPanel {
    pub(in crate::screens::repository) fn view<'a>(
        &'a self,
        ctx: &SidebarViewCtx<'a>,
    ) -> Element<'a, Message> {
        view::view(self, ctx)
    }

    pub(in crate::screens::repository) fn update(
        &mut self,
        action: SidebarAction,
        ctx: &mut super::ScreenCtx<'_>,
        repository_panel: &mut CenterPanel,
    ) -> iced::Task<Message> {
        update::update(self, action, ctx, repository_panel)
    }
}
