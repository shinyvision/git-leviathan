use iced::{widget::text_editor, Element};

use crate::{
    core::Commit, message::Message, services::MergedCommitDiffResult, view_model::CommitDiffState,
};

use super::super::super::{
    state::{RepositoryData, SelectionState},
    FileView,
};
use super::view as detail_view;
use super::DetailOrientation;

#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct DetailViewModel<'a> {
    pub(in crate::screens::repository) commit: Option<&'a Commit>,
    pub(in crate::screens::repository) commit_diff_state: Option<&'a CommitDiffState>,
    pub(in crate::screens::repository) commit_idx: usize,
    pub(in crate::screens::repository) file_view: FileView,
    pub(in crate::screens::repository) width: f32,
    pub(in crate::screens::repository) is_resizing: bool,
    pub(in crate::screens::repository) orientation: DetailOrientation,
    pub(in crate::screens::repository) current_branch: &'a str,
    pub(in crate::screens::repository) dirty_commit_message: &'a text_editor::Content,
    pub(in crate::screens::repository) active_diff_file_path: Option<&'a str>,
    /// When multi-select is active, the commits in the selection (sorted by
    /// newest-first / index ascending as in the commit list).
    pub(in crate::screens::repository) multi_commits: Vec<&'a Commit>,
    pub(in crate::screens::repository) merged_diff: Option<&'a MergedCommitDiffResult>,
    pub(in crate::screens::repository) reword: Option<RewordViewModel<'a>>,
    /// Whether the selected commit can be reworded (on first-parent chain from HEAD).
    pub(in crate::screens::repository) reword_allowed: bool,
    /// Number of commits that would be rebased if the selected commit were reworded.
    pub(in crate::screens::repository) reword_descendant_count: usize,
    /// Whether to show the "Copied" tooltip next to the commit SHA chip.
    pub(in crate::screens::repository) copied_sha_flash: bool,
}

#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct RewordViewModel<'a> {
    pub(in crate::screens::repository) content: &'a text_editor::Content,
}

pub(in crate::screens::repository) struct DetailPanel {
    pub dirty_commit_message: text_editor::Content,
    reword_state: Option<RewordEditState>,
    copied_sha_flash: bool,
}

struct RewordEditState {
    hash: String,
    content: text_editor::Content,
}

impl DetailPanel {
    pub(in crate::screens::repository) fn new() -> Self {
        Self {
            dirty_commit_message: text_editor::Content::new(),
            reword_state: None,
            copied_sha_flash: false,
        }
    }

    pub(in crate::screens::repository) fn set_copied_sha_flash(&mut self) {
        self.copied_sha_flash = true;
    }

    pub(in crate::screens::repository) fn clear_copied_sha_flash(&mut self) {
        self.copied_sha_flash = false;
    }

    pub(in crate::screens::repository) fn clear_commit_message(&mut self) {
        self.dirty_commit_message = text_editor::Content::new();
    }

    pub(in crate::screens::repository) fn start_reword(
        &mut self,
        hash: String,
        original_message: String,
    ) {
        self.reword_state = Some(RewordEditState {
            hash,
            content: text_editor::Content::with_text(&original_message),
        });
    }

    pub(in crate::screens::repository) fn cancel_reword(&mut self) {
        self.reword_state = None;
    }

    pub(in crate::screens::repository) fn reword_state_for(
        &self,
        hash: &str,
    ) -> Option<&text_editor::Content> {
        self.reword_state
            .as_ref()
            .filter(|s| s.hash == hash)
            .map(|s| &s.content)
    }

    pub(in crate::screens::repository) fn reword_active(&self) -> Option<(&str, String)> {
        let s = self.reword_state.as_ref()?;
        Some((&s.hash, dirty_commit_message_text(&s.content)))
    }

    pub(in crate::screens::repository) fn perform_reword_action(
        &mut self,
        action: text_editor::Action,
    ) {
        if let Some(s) = self.reword_state.as_mut() {
            s.content.perform(action);
        }
    }

    pub(in crate::screens::repository) fn set_commit_message_from_merge(
        &mut self,
        source: &str,
        target: &str,
    ) {
        self.dirty_commit_message =
            text_editor::Content::with_text(&merge_commit_message(source, target));
    }

    fn detail_view_model<'a>(
        &'a self,
        data: &'a RepositoryData,
        selection: &SelectionState,
        active_diff_file_path: Option<&'a str>,
        merged_diff: Option<&'a MergedCommitDiffResult>,
        orientation: DetailOrientation,
        width: f32,
    ) -> DetailViewModel<'a> {
        let multi_commits: Vec<&'a Commit> = if selection.is_multi() {
            selection
                .selected_indices()
                .into_iter()
                .filter_map(|i| data.selected_commit(i))
                .collect()
        } else {
            Vec::new()
        };

        let selected = data.selected_commit(selection.selected_commit());
        let (reword, reword_allowed, reword_descendant_count) = if let Some(commit) = selected {
            let dist = data.snapshot.first_parent_distance_from_head(&commit.hash);
            let allowed = matches!(commit.kind, crate::core::CommitKind::Commit) && dist.is_some();
            let count = dist.unwrap_or(0);
            let editing = self
                .reword_state_for(&commit.hash)
                .map(|content| RewordViewModel { content });
            (editing, allowed, count)
        } else {
            (None, false, 0)
        };

        DetailViewModel {
            commit: selected,
            commit_diff_state: data.cache.state(selection.selected_commit()),
            commit_idx: selection.selected_commit(),
            file_view: selection.file_view(),
            width,
            is_resizing: data.resize.detail_resizing,
            orientation,
            current_branch: data.snapshot.current_branch(),
            dirty_commit_message: &self.dirty_commit_message,
            active_diff_file_path,
            multi_commits,
            merged_diff,
            reword,
            reword_allowed,
            reword_descendant_count,
            copied_sha_flash: self.copied_sha_flash,
        }
    }

    pub(in crate::screens::repository) fn view<'a>(
        &'a self,
        data: &'a RepositoryData,
        selection: &SelectionState,
        active_diff_file_path: Option<&'a str>,
        merged_diff: Option<&'a MergedCommitDiffResult>,
        orientation: DetailOrientation,
        width: f32,
    ) -> Element<'a, Message> {
        detail_view::detail_panel_view(self.detail_view_model(
            data,
            selection,
            active_diff_file_path,
            merged_diff,
            orientation,
            width,
        ))
    }
}

pub(in crate::screens::repository) fn dirty_commit_message_text(
    content: &text_editor::Content,
) -> String {
    content
        .text()
        .trim_end_matches(['\r', '\n'])
        .trim()
        .to_string()
}

pub(in crate::screens::repository) fn split_commit_message(message: &str) -> (String, String) {
    let mut lines = message.lines();
    let summary = lines.next().unwrap_or("").trim().to_string();
    let description = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (summary, description)
}

pub(in crate::screens::repository) fn merge_commit_message(
    source_branch: &str,
    target_branch: &str,
) -> String {
    format!("Merge branch '{}' into {}", source_branch, target_branch)
}
