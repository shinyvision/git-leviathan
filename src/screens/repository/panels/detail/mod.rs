//! Detail panel — right column (or stacked bottom strip) showing commit
//! metadata, message (or reword editor), file list, and diff summary. Owns
//! the dirty-commit message editor, reword editor, and the copied-SHA
//! tooltip flash; panel width/height and resize activity live in
//! `RepositoryData::resize`.

use iced::Element;

use crate::{message::Message, services::MergedCommitDiffResult};

use super::super::state::{RepositoryData, SelectionState};

mod state;
mod update;
pub(crate) mod view;

pub(in crate::screens::repository) use state::DetailPanel;
pub(in crate::screens::repository) use state::{dirty_commit_message_text, split_commit_message};
pub(in crate::screens::repository) use update::update as update_detail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::screens::repository) enum DetailOrientation {
    Vertical,
    Horizontal,
}

/// Read-only projection handed to `DetailPanel::view`. Built by the screen
/// from `RepositoryData` + `DiffPanel::active_diff_file_path`; the detail
/// panel never reads sibling panel state directly.
pub(in crate::screens::repository) struct DetailViewCtx<'a> {
    pub data: &'a RepositoryData,
    pub selection: &'a SelectionState,
    pub active_diff_file_path: Option<&'a str>,
    pub merged_diff: Option<&'a MergedCommitDiffResult>,
    pub orientation: DetailOrientation,
    pub width: f32,
}

impl DetailPanel {
    pub(in crate::screens::repository) fn view_with<'a>(
        &'a self,
        ctx: &DetailViewCtx<'a>,
    ) -> Element<'a, Message> {
        self.view(
            ctx.data,
            ctx.selection,
            ctx.active_diff_file_path,
            ctx.merged_diff,
            ctx.orientation,
            ctx.width,
        )
    }
}
