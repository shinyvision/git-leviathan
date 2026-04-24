//! Center panel — commit list + graph. Owns viewport tracking and
//! pagination; branch-label popout and pending-focus live on the screen /
//! `RepositoryData`.

use iced::{widget::text_editor, Element};

use crate::message::Message;

use super::super::commit_search::CommitSearch;
use super::super::state::{BranchPopoutController, RepositoryData, SelectionState};

mod state;
mod update;
pub(crate) mod view;

pub(in crate::screens::repository) use state::CenterPanel;
pub(in crate::screens::repository) use update::update as update_center;

/// Read-only projection handed to `CenterPanel::view`. Panels never reach
/// into sibling-panel or screen state directly; the screen builds this ctx.
pub(in crate::screens::repository) struct CenterViewCtx<'a> {
    pub data: &'a RepositoryData,
    pub selection: &'a SelectionState,
    pub dirty_commit_message: &'a text_editor::Content,
    pub commit_search: Option<&'a CommitSearch>,
    pub branch_popout: &'a BranchPopoutController,
}

impl CenterPanel {
    pub(in crate::screens::repository) fn view_with<'a>(
        &'a self,
        ctx: &CenterViewCtx<'a>,
    ) -> Element<'a, Message> {
        self.view(
            ctx.data,
            ctx.selection,
            ctx.dirty_commit_message,
            ctx.commit_search,
            ctx.branch_popout,
        )
    }
}
