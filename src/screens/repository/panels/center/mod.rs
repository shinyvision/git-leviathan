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
    pub window_width: Option<f32>,
}

impl CenterPanel {
    pub(in crate::screens::repository) fn view_with<'a>(
        &'a self,
        ctx: &CenterViewCtx<'a>,
        top_slot: Option<Element<'a, Message>>,
        bottom_slot: Option<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let body = self.view(
            ctx.data,
            ctx.selection,
            ctx.dirty_commit_message,
            ctx.commit_search,
            ctx.branch_popout,
            ctx.window_width,
        );
        wrap_with_slots(body, top_slot, bottom_slot)
    }
}

pub(in crate::screens::repository) fn wrap_with_slots<'a>(
    body: Element<'a, Message>,
    top_slot: Option<Element<'a, Message>>,
    bottom_slot: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    use iced::Length;
    if top_slot.is_none() && bottom_slot.is_none() {
        return body;
    }
    let mut col_items: Vec<Element<'a, Message>> = Vec::with_capacity(3);
    if let Some(top) = top_slot {
        col_items.push(top);
    }
    col_items.push(body);
    if let Some(bottom) = bottom_slot {
        col_items.push(bottom);
    }
    iced::widget::column(col_items)
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
