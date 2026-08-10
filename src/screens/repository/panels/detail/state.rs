use iced::{
    widget::{scrollable, text_editor},
    Element, Task,
};

use crate::{
    core::{ChangedFile, Commit, CommitKind},
    message::Message,
    services::MergedCommitDiffResult,
    view_model::CommitDiffState,
};

use super::super::super::panel_messages::{DetailAction, DetailFileListKind};
use super::super::super::state::{OperationId, RepositoryData, SelectionState};
use super::view as detail_view;
use super::{DetailOrientation, DetailViewCtx};

const FILE_LIST_VIEWPORT_FALLBACK_H: f32 = detail_view::FILE_ROW_HEIGHT * 8.0;
const SCROLL_EPSILON: f32 = 0.01;

#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct DetailViewModel<'a> {
    pub(in crate::screens::repository) commit: Option<&'a Commit>,
    pub(in crate::screens::repository) commit_diff_state: Option<&'a CommitDiffState>,
    pub(in crate::screens::repository) commit_idx: usize,
    pub(in crate::screens::repository) width: f32,
    pub(in crate::screens::repository) is_resizing: bool,
    pub(in crate::screens::repository) orientation: DetailOrientation,
    pub(in crate::screens::repository) current_branch: &'a str,
    pub(in crate::screens::repository) dirty_commit_message: &'a text_editor::Content,
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
    pub(in crate::screens::repository) dirty_actions_busy: bool,
    pub(in crate::screens::repository) dirty_fast_actions_busy: bool,
    pub(in crate::screens::repository) dirty_operation_label: Option<&'static str>,
    pub(in crate::screens::repository) commit_file_list_scroll_y: f32,
    pub(in crate::screens::repository) dirty_file_list_scroll_y: f32,
    pub(in crate::screens::repository) merged_file_list_scroll_y: f32,
    pub(in crate::screens::repository) selected_file_path: Option<&'a str>,
    pub(in crate::screens::repository) selected_dirty_files: Vec<DirtyFileKey>,
    pub(in crate::screens::repository) dirty_selection_counts: DirtySelectionCounts,
}

#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct RewordViewModel<'a> {
    pub(in crate::screens::repository) content: &'a text_editor::Content,
}

pub(in crate::screens::repository) struct DetailPanel {
    pub dirty_commit_message: text_editor::Content,
    reword_state: Option<RewordEditState>,
    copied_sha_flash: bool,
    selected_file: Option<DetailFileSelection>,
    selected_dirty_files: Vec<DirtyFileKey>,
    dirty_selection_anchor: Option<DirtyFileKey>,
    pending_dirty_reselection: Option<PendingDirtyReselection>,
    commit_file_list_scroll_y: f32,
    dirty_file_list_scroll_y: f32,
    merged_file_list_scroll_y: f32,
    file_list_viewport_h: f32,
}

struct RewordEditState {
    hash: String,
    content: text_editor::Content,
    needs_focus: bool,
}

#[derive(Debug, Clone)]
struct PendingDirtyReselection {
    operation_id: OperationId,
    source_key: DirtyFileKey,
    source_index: usize,
}

impl DetailPanel {
    pub(in crate::screens::repository) fn new() -> Self {
        Self {
            dirty_commit_message: text_editor::Content::new(),
            reword_state: None,
            copied_sha_flash: false,
            selected_file: None,
            selected_dirty_files: Vec::new(),
            dirty_selection_anchor: None,
            pending_dirty_reselection: None,
            commit_file_list_scroll_y: 0.0,
            dirty_file_list_scroll_y: 0.0,
            merged_file_list_scroll_y: 0.0,
            file_list_viewport_h: FILE_LIST_VIEWPORT_FALLBACK_H,
        }
    }

    pub(in crate::screens::repository) fn set_copied_sha_flash(&mut self) {
        self.copied_sha_flash = true;
    }

    pub(in crate::screens::repository) fn clear_copied_sha_flash(&mut self) {
        self.copied_sha_flash = false;
    }

    pub(in crate::screens::repository) fn set_file_list_scroll_y(
        &mut self,
        kind: DetailFileListKind,
        scroll_y: f32,
    ) {
        match kind {
            DetailFileListKind::Commit => self.commit_file_list_scroll_y = scroll_y.max(0.0),
            DetailFileListKind::Dirty => self.dirty_file_list_scroll_y = scroll_y.max(0.0),
            DetailFileListKind::Merged => self.merged_file_list_scroll_y = scroll_y.max(0.0),
        }
    }

    pub(in crate::screens::repository) fn set_file_list_viewport(
        &mut self,
        kind: DetailFileListKind,
        viewport: scrollable::Viewport,
    ) {
        self.set_file_list_scroll_y(kind, viewport.absolute_offset().y);
        self.file_list_viewport_h = viewport.bounds().height.max(detail_view::FILE_ROW_HEIGHT);
    }

    pub(in crate::screens::repository) fn reset_file_list_scroll(&mut self) -> Task<Message> {
        self.selected_file = None;
        self.clear_dirty_selection();
        self.commit_file_list_scroll_y = 0.0;
        self.dirty_file_list_scroll_y = 0.0;
        self.merged_file_list_scroll_y = 0.0;

        iced::widget::operation::scroll_to(
            detail_file_list_scroll_id(),
            scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
        )
    }

    pub(in crate::screens::repository) fn select_file(
        &mut self,
        navigation: DetailFileNavigation,
        data: &RepositoryData,
        selection: &SelectionState,
        merged_diff: Option<&MergedCommitDiffResult>,
    ) -> Task<Message> {
        let Some((kind, entries)) = current_file_entries(data, selection, merged_diff) else {
            self.selected_file = None;
            self.clear_dirty_selection();
            return Task::none();
        };
        let Some(next_idx) = next_file_index(&entries, self.selected_file.as_ref(), navigation)
        else {
            self.selected_file = None;
            self.clear_dirty_selection();
            return Task::none();
        };
        self.set_selected_file(entries[next_idx].selection.clone());
        self.scroll_file_into_view(kind, entries[next_idx].visual_row)
    }

    pub(in crate::screens::repository) fn extend_file_selection(
        &mut self,
        navigation: DetailFileNavigation,
        data: &RepositoryData,
        selection: &SelectionState,
        merged_diff: Option<&MergedCommitDiffResult>,
    ) -> Task<Message> {
        let Some((kind, entries)) = current_file_entries(data, selection, merged_diff) else {
            self.selected_file = None;
            self.clear_dirty_selection();
            return Task::none();
        };
        let Some(next_idx) = next_file_index(&entries, self.selected_file.as_ref(), navigation)
        else {
            self.selected_file = None;
            self.clear_dirty_selection();
            return Task::none();
        };

        let previous_dirty_key = self
            .selected_file
            .as_ref()
            .and_then(dirty_key_for_selection);
        let next_selection = entries[next_idx].selection.clone();
        match &next_selection {
            DetailFileSelection::Dirty { path, status } => {
                let target = DirtyFileKey {
                    path: path.clone(),
                    status: *status,
                };
                self.extend_dirty_selection_to(
                    target,
                    previous_dirty_key,
                    &entries,
                    data,
                    selection,
                );
                self.selected_file = Some(next_selection);
            }
            DetailFileSelection::Commit { .. } | DetailFileSelection::Merged { .. } => {
                self.set_selected_file(next_selection);
            }
        }
        self.scroll_file_into_view(kind, entries[next_idx].visual_row)
    }

    pub(in crate::screens::repository) fn open_selected_file_action(
        &self,
        data: &RepositoryData,
        selection: &SelectionState,
        merged_diff: Option<&MergedCommitDiffResult>,
    ) -> Option<DetailAction> {
        let (_, entries) = current_file_entries(data, selection, merged_diff)?;
        let selected = self.selected_file.as_ref()?;
        entries
            .iter()
            .find(|entry| &entry.selection == selected)
            .map(|entry| entry.selection.to_action())
    }

    pub(in crate::screens::repository) fn selected_dirty_file(
        &self,
        data: &RepositoryData,
        selection: &SelectionState,
    ) -> Option<(String, bool)> {
        let selected = self.selected_file.as_ref()?;
        let DetailFileSelection::Dirty { path, status } = selected else {
            return None;
        };
        if selection.is_multi() {
            return None;
        }

        let commit = data.selected_commit(selection.selected_commit())?;
        if commit.kind != CommitKind::Dirty {
            return None;
        }

        let path = path.as_str();
        let is_staged = status.is_staged_for_diff();
        if is_staged && commit.staged_files.iter().any(|file| file.path == path) {
            return Some((path.to_string(), true));
        }
        if !is_staged
            && commit
                .conflicted_files
                .iter()
                .chain(commit.unstaged_files.iter())
                .any(|file| file.path == path)
        {
            return Some((path.to_string(), false));
        }
        if commit.staged_files.iter().any(|file| file.path == path) {
            return Some((path.to_string(), true));
        }

        None
    }

    pub(in crate::screens::repository) fn select_dirty_file_from_click(
        &mut self,
        path: String,
        is_staged: bool,
        data: &RepositoryData,
        selection: &SelectionState,
        mode: DirtyFileSelectionMode,
    ) {
        let entries = current_dirty_keys(data, selection);
        let fallback = DirtyFileKey {
            path: path.clone(),
            status: if is_staged {
                DirtyFileStatus::Staged
            } else {
                DirtyFileStatus::Unstaged
            },
        };
        let clicked = entries
            .iter()
            .find(|entry| entry.path == path && entry.status.is_staged_for_diff() == is_staged)
            .cloned()
            .unwrap_or(fallback);

        self.selected_file = Some(DetailFileSelection::Dirty {
            path,
            status: clicked.status,
        });

        if entries.is_empty() {
            self.selected_dirty_files.clear();
            self.dirty_selection_anchor = None;
            return;
        }

        match mode {
            DirtyFileSelectionMode::Replace => {
                self.selected_dirty_files = vec![clicked.clone()];
                self.dirty_selection_anchor = Some(clicked);
            }
            DirtyFileSelectionMode::Toggle => {
                if let Some(idx) = self
                    .selected_dirty_files
                    .iter()
                    .position(|selected| selected == &clicked)
                {
                    self.selected_dirty_files.remove(idx);
                } else if entries.iter().any(|entry| entry == &clicked) {
                    self.selected_dirty_files.push(clicked.clone());
                }
                self.dirty_selection_anchor = Some(clicked);
                self.retain_dirty_selection(data, selection);
            }
            DirtyFileSelectionMode::Range => {
                let selected = self
                    .dirty_selection_anchor
                    .as_ref()
                    .and_then(|anchor| dirty_key_range(&entries, anchor, &clicked))
                    .unwrap_or_else(|| vec![clicked.clone()]);
                self.selected_dirty_files = selected;
                self.dirty_selection_anchor.get_or_insert(clicked);
                self.retain_dirty_selection(data, selection);
            }
        }
    }

    pub(in crate::screens::repository) fn select_commit_file(
        &mut self,
        commit_idx: usize,
        path: String,
    ) {
        self.clear_dirty_selection();
        self.selected_file = Some(DetailFileSelection::Commit { commit_idx, path });
    }

    pub(in crate::screens::repository) fn select_merged_file(&mut self, path: String) {
        self.clear_dirty_selection();
        self.selected_file = Some(DetailFileSelection::Merged { path });
    }

    pub(in crate::screens::repository) fn selected_file_path(&self) -> Option<&str> {
        self.selected_file.as_ref().map(DetailFileSelection::path)
    }

    pub(in crate::screens::repository) fn selected_dirty_paths_for_status(
        &self,
        data: &RepositoryData,
        selection: &SelectionState,
        status: DirtyFileStatus,
    ) -> Vec<String> {
        self.valid_selected_dirty_files(data, selection)
            .into_iter()
            .filter(|key| key.status == status)
            .map(|key| key.path)
            .collect()
    }

    pub(in crate::screens::repository) fn selected_dirty_paths_for_discard(
        &self,
        data: &RepositoryData,
        selection: &SelectionState,
    ) -> Vec<String> {
        unique_dirty_paths(self.valid_selected_dirty_files(data, selection))
    }

    pub(in crate::screens::repository) fn selected_dirty_count(
        &self,
        data: &RepositoryData,
        selection: &SelectionState,
    ) -> usize {
        self.valid_selected_dirty_files(data, selection).len()
    }

    pub(in crate::screens::repository) fn collapse_dirty_selection_to_selected(
        &mut self,
        data: &RepositoryData,
        selection: &SelectionState,
    ) {
        let Some((path, is_staged)) = self.selected_dirty_file(data, selection) else {
            self.clear_dirty_selection();
            return;
        };
        self.select_dirty_file_from_click(
            path,
            is_staged,
            data,
            selection,
            DirtyFileSelectionMode::Replace,
        );
    }

    pub(in crate::screens::repository) fn prepare_dirty_reselection_after_write(
        &mut self,
        operation_id: OperationId,
        target_paths: &[String],
        source_status: DirtyFileStatus,
        data: &RepositoryData,
        selection: &SelectionState,
    ) {
        self.pending_dirty_reselection = None;

        let Some(source_key) = self
            .selected_file
            .as_ref()
            .and_then(dirty_key_for_selection)
        else {
            return;
        };
        if source_key.status != source_status
            || !target_paths.iter().any(|path| path == &source_key.path)
            || !self
                .selected_dirty_files
                .iter()
                .any(|selected| selected == &source_key)
        {
            return;
        }

        let source_keys = dirty_keys_for_status(data, selection, source_status);
        let Some(source_index) = source_keys.iter().position(|key| key == &source_key) else {
            return;
        };

        self.pending_dirty_reselection = Some(PendingDirtyReselection {
            operation_id,
            source_key,
            source_index,
        });
    }

    pub(in crate::screens::repository) fn apply_pending_dirty_reselection_after_reload(
        &mut self,
        operation_id: OperationId,
        data: &RepositoryData,
        selection: &SelectionState,
    ) {
        let Some(pending) = self.pending_dirty_reselection.take() else {
            return;
        };
        if pending.operation_id != operation_id {
            return;
        }

        let source_keys = dirty_keys_for_status(data, selection, pending.source_key.status);
        if source_keys.iter().any(|key| key == &pending.source_key) {
            self.retain_dirty_selection(data, selection);
            return;
        }

        let Some(next_key) = nearest_dirty_key_after_removal(&source_keys, pending.source_index)
        else {
            self.selected_file = None;
            self.clear_dirty_selection();
            return;
        };

        self.set_selected_file(DetailFileSelection::Dirty {
            path: next_key.path,
            status: next_key.status,
        });
    }

    pub(in crate::screens::repository) fn clear_pending_dirty_reselection(
        &mut self,
        operation_id: OperationId,
    ) {
        if self
            .pending_dirty_reselection
            .as_ref()
            .is_some_and(|pending| pending.operation_id == operation_id)
        {
            self.pending_dirty_reselection = None;
        }
    }

    fn set_selected_file(&mut self, selection: DetailFileSelection) {
        match &selection {
            DetailFileSelection::Dirty { path, status } => {
                let key = DirtyFileKey {
                    path: path.clone(),
                    status: *status,
                };
                self.selected_dirty_files = vec![key.clone()];
                self.dirty_selection_anchor = Some(key);
            }
            DetailFileSelection::Commit { .. } | DetailFileSelection::Merged { .. } => {
                self.clear_dirty_selection();
            }
        }
        self.selected_file = Some(selection);
    }

    fn clear_dirty_selection(&mut self) {
        self.selected_dirty_files.clear();
        self.dirty_selection_anchor = None;
    }

    fn retain_dirty_selection(&mut self, data: &RepositoryData, selection: &SelectionState) {
        let keys = current_dirty_keys(data, selection);
        self.selected_dirty_files
            .retain(|selected| keys.iter().any(|key| key == selected));
        if self
            .dirty_selection_anchor
            .as_ref()
            .is_some_and(|anchor| !keys.iter().any(|key| key == anchor))
        {
            self.dirty_selection_anchor = self.selected_dirty_files.last().cloned();
        }
    }

    fn extend_dirty_selection_to(
        &mut self,
        target: DirtyFileKey,
        fallback_anchor: Option<DirtyFileKey>,
        entries: &[DetailFileEntry],
        data: &RepositoryData,
        selection: &SelectionState,
    ) {
        let keys = dirty_keys_from_entries(entries);
        let anchor = self
            .dirty_selection_anchor
            .clone()
            .filter(|anchor| keys.iter().any(|key| key == anchor))
            .or_else(|| fallback_anchor.filter(|anchor| keys.iter().any(|key| key == anchor)))
            .or_else(|| {
                self.selected_dirty_files
                    .iter()
                    .find(|selected| keys.iter().any(|key| key == *selected))
                    .cloned()
            })
            .unwrap_or_else(|| target.clone());

        self.dirty_selection_anchor = Some(anchor.clone());
        self.selected_dirty_files =
            dirty_key_range(&keys, &anchor, &target).unwrap_or_else(|| vec![target]);
        self.retain_dirty_selection(data, selection);
    }

    fn valid_selected_dirty_files(
        &self,
        data: &RepositoryData,
        selection: &SelectionState,
    ) -> Vec<DirtyFileKey> {
        if self.selected_dirty_files.is_empty() {
            return Vec::new();
        }
        let Some(present) = current_dirty_key_set(data, selection) else {
            return Vec::new();
        };
        self.selected_dirty_files
            .iter()
            .filter(|selected| present.contains(&(selected.path.as_str(), selected.status)))
            .cloned()
            .collect()
    }

    fn scroll_file_into_view(
        &mut self,
        kind: DetailFileListKind,
        visual_row: usize,
    ) -> Task<Message> {
        let row_top = visual_row as f32 * detail_view::FILE_ROW_HEIGHT;
        let row_bottom = row_top + detail_view::FILE_ROW_HEIGHT;
        let viewport_h = self.file_list_viewport_h.max(detail_view::FILE_ROW_HEIGHT);
        let current = self.file_list_scroll_y(kind);
        let viewport_bottom = current + viewport_h;

        let next = if row_top < current {
            row_top
        } else if row_bottom > viewport_bottom {
            (row_bottom - viewport_h).max(0.0)
        } else {
            current
        };

        if (next - current).abs() <= SCROLL_EPSILON {
            return Task::none();
        }
        self.set_file_list_scroll_y(kind, next);
        iced::widget::operation::scroll_to(
            detail_file_list_scroll_id(),
            scrollable::AbsoluteOffset { x: 0.0, y: next },
        )
    }

    fn file_list_scroll_y(&self, kind: DetailFileListKind) -> f32 {
        match kind {
            DetailFileListKind::Commit => self.commit_file_list_scroll_y,
            DetailFileListKind::Dirty => self.dirty_file_list_scroll_y,
            DetailFileListKind::Merged => self.merged_file_list_scroll_y,
        }
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
            needs_focus: true,
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

    pub(in crate::screens::repository) fn request_reword_focus(&mut self) {
        if let Some(s) = self.reword_state.as_mut() {
            s.needs_focus = true;
        }
    }

    pub(in crate::screens::repository) fn take_reword_needs_focus(&mut self) -> bool {
        let Some(s) = self.reword_state.as_mut() else {
            return false;
        };
        let needs_focus = s.needs_focus;
        s.needs_focus = false;
        needs_focus
    }

    pub(in crate::screens::repository) fn reword_needs_focus(&self) -> bool {
        self.reword_state
            .as_ref()
            .is_some_and(|state| state.needs_focus)
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

        let actions_busy = data.operations.is_writing();
        let fast_actions_busy = actions_busy;
        let selected_dirty_files = self.valid_selected_dirty_files(data, selection);
        let dirty_selection_counts = DirtySelectionCounts::from_keys(&selected_dirty_files);

        DetailViewModel {
            commit: selected,
            commit_diff_state: data.cache.state(selection.selected_commit()),
            commit_idx: selection.selected_commit(),
            width,
            is_resizing: data.resize.detail_resizing,
            orientation,
            current_branch: data.snapshot.current_branch(),
            dirty_commit_message: &self.dirty_commit_message,
            multi_commits,
            merged_diff,
            reword,
            reword_allowed,
            reword_descendant_count,
            copied_sha_flash: self.copied_sha_flash,
            dirty_actions_busy: actions_busy,
            dirty_fast_actions_busy: fast_actions_busy,
            dirty_operation_label: data.operations.active_label(),
            commit_file_list_scroll_y: self.commit_file_list_scroll_y,
            dirty_file_list_scroll_y: self.dirty_file_list_scroll_y,
            merged_file_list_scroll_y: self.merged_file_list_scroll_y,
            selected_file_path: self
                .selected_file
                .as_ref()
                .map(DetailFileSelection::path)
                .or(active_diff_file_path),
            selected_dirty_files,
            dirty_selection_counts,
        }
    }

    pub(in crate::screens::repository) fn view<'a>(
        &'a self,
        ctx: &DetailViewCtx<'a>,
        top_slot: Option<Element<'a, Message>>,
        bottom_slot: Option<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        detail_view::detail_panel_view(
            self.detail_view_model(
                ctx.data,
                ctx.selection,
                ctx.active_diff_file_path,
                ctx.merged_diff,
                ctx.orientation,
                ctx.width,
            ),
            top_slot,
            bottom_slot,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::screens::repository) enum DetailFileSelection {
    Dirty {
        path: String,
        status: DirtyFileStatus,
    },
    Commit {
        commit_idx: usize,
        path: String,
    },
    Merged {
        path: String,
    },
}

impl DetailFileSelection {
    fn path(&self) -> &str {
        match self {
            Self::Dirty { path, .. } | Self::Commit { path, .. } | Self::Merged { path } => path,
        }
    }

    fn to_action(&self) -> DetailAction {
        match self {
            Self::Dirty { path, status } => DetailAction::DirtyFileOpened {
                path: path.clone(),
                is_staged: status.is_staged_for_diff(),
            },
            Self::Commit { commit_idx, path } => DetailAction::CommitFileClicked {
                commit_idx: *commit_idx,
                path: path.clone(),
            },
            Self::Merged { path } => DetailAction::MergedFileClicked { path: path.clone() },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::screens::repository) enum DetailFileNavigation {
    Previous,
    Next,
    First,
    Last,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::screens::repository) enum DirtyFileSelectionMode {
    Replace,
    Toggle,
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::screens::repository) enum DirtyFileStatus {
    Conflicted,
    Unstaged,
    Staged,
}

impl DirtyFileStatus {
    pub(in crate::screens::repository) fn is_staged_for_diff(self) -> bool {
        matches!(self, Self::Staged)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::screens::repository) struct DirtyFileKey {
    pub(in crate::screens::repository) path: String,
    pub(in crate::screens::repository) status: DirtyFileStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::screens::repository) struct DirtySelectionCounts {
    pub(in crate::screens::repository) total: usize,
    pub(in crate::screens::repository) unstaged: usize,
    pub(in crate::screens::repository) staged: usize,
}

impl DirtySelectionCounts {
    fn from_keys(keys: &[DirtyFileKey]) -> Self {
        let mut counts = Self {
            total: keys.len(),
            ..Self::default()
        };
        for key in keys {
            match key.status {
                DirtyFileStatus::Conflicted => {}
                DirtyFileStatus::Unstaged => counts.unstaged += 1,
                DirtyFileStatus::Staged => counts.staged += 1,
            }
        }
        counts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetailFileEntry {
    selection: DetailFileSelection,
    visual_row: usize,
}

fn current_file_entries(
    data: &RepositoryData,
    selection: &SelectionState,
    merged_diff: Option<&MergedCommitDiffResult>,
) -> Option<(DetailFileListKind, Vec<DetailFileEntry>)> {
    if selection.is_multi() {
        let merged = merged_diff?;
        return Some((
            DetailFileListKind::Merged,
            file_entries(merged.files.iter(), |file, _idx| {
                DetailFileSelection::Merged {
                    path: file.path.clone(),
                }
            }),
        ));
    }

    let commit_idx = selection.selected_commit();
    let commit = data.selected_commit(commit_idx)?;
    match commit.kind {
        CommitKind::Dirty => Some((DetailFileListKind::Dirty, dirty_file_entries(commit))),
        CommitKind::Commit | CommitKind::Stash => {
            let diff = data.cache.state(commit_idx)?;
            Some((
                DetailFileListKind::Commit,
                file_entries(diff.files.iter(), |file, _idx| {
                    DetailFileSelection::Commit {
                        commit_idx,
                        path: file.path.clone(),
                    }
                }),
            ))
        }
    }
}

fn file_entries<'a>(
    files: impl Iterator<Item = &'a ChangedFile>,
    map: impl Fn(&ChangedFile, usize) -> DetailFileSelection,
) -> Vec<DetailFileEntry> {
    files
        .enumerate()
        .map(|(idx, file)| DetailFileEntry {
            selection: map(file, idx),
            visual_row: idx,
        })
        .collect()
}

fn dirty_file_entries(commit: &Commit) -> Vec<DetailFileEntry> {
    let mut entries = Vec::new();
    let mut visual_row = 0;
    push_dirty_section(
        &mut entries,
        &mut visual_row,
        &commit.conflicted_files,
        DirtyFileStatus::Conflicted,
    );
    push_dirty_section(
        &mut entries,
        &mut visual_row,
        &commit.unstaged_files,
        DirtyFileStatus::Unstaged,
    );
    push_dirty_section(
        &mut entries,
        &mut visual_row,
        &commit.staged_files,
        DirtyFileStatus::Staged,
    );
    entries
}

fn push_dirty_section(
    entries: &mut Vec<DetailFileEntry>,
    visual_row: &mut usize,
    files: &[ChangedFile],
    status: DirtyFileStatus,
) {
    *visual_row += 1;
    if files.is_empty() {
        *visual_row += 1;
        return;
    }
    for file in files {
        entries.push(DetailFileEntry {
            selection: DetailFileSelection::Dirty {
                path: file.path.clone(),
                status,
            },
            visual_row: *visual_row,
        });
        *visual_row += 1;
    }
}

fn current_dirty_keys(data: &RepositoryData, selection: &SelectionState) -> Vec<DirtyFileKey> {
    if selection.is_multi() {
        return Vec::new();
    }
    let Some(commit) = data.selected_commit(selection.selected_commit()) else {
        return Vec::new();
    };
    if commit.kind != CommitKind::Dirty {
        return Vec::new();
    }
    dirty_file_entries(commit)
        .into_iter()
        .filter_map(|entry| match entry.selection {
            DetailFileSelection::Dirty { path, status } => Some(DirtyFileKey { path, status }),
            DetailFileSelection::Commit { .. } | DetailFileSelection::Merged { .. } => None,
        })
        .collect()
}

fn current_dirty_key_set<'a>(
    data: &'a RepositoryData,
    selection: &SelectionState,
) -> Option<std::collections::HashSet<(&'a str, DirtyFileStatus)>> {
    if selection.is_multi() {
        return None;
    }
    let commit = data.selected_commit(selection.selected_commit())?;
    if commit.kind != CommitKind::Dirty {
        return None;
    }
    let mut set = std::collections::HashSet::new();
    for (files, status) in [
        (&commit.conflicted_files, DirtyFileStatus::Conflicted),
        (&commit.unstaged_files, DirtyFileStatus::Unstaged),
        (&commit.staged_files, DirtyFileStatus::Staged),
    ] {
        for file in files {
            set.insert((file.path.as_str(), status));
        }
    }
    Some(set)
}

fn dirty_keys_for_status(
    data: &RepositoryData,
    selection: &SelectionState,
    status: DirtyFileStatus,
) -> Vec<DirtyFileKey> {
    current_dirty_keys(data, selection)
        .into_iter()
        .filter(|key| key.status == status)
        .collect()
}

fn nearest_dirty_key_after_removal(
    keys: &[DirtyFileKey],
    removed_index: usize,
) -> Option<DirtyFileKey> {
    keys.get(removed_index.min(keys.len().saturating_sub(1)))
        .cloned()
}

fn dirty_keys_from_entries(entries: &[DetailFileEntry]) -> Vec<DirtyFileKey> {
    entries
        .iter()
        .filter_map(|entry| dirty_key_for_selection(&entry.selection))
        .collect()
}

fn dirty_key_for_selection(selection: &DetailFileSelection) -> Option<DirtyFileKey> {
    match selection {
        DetailFileSelection::Dirty { path, status } => Some(DirtyFileKey {
            path: path.clone(),
            status: *status,
        }),
        DetailFileSelection::Commit { .. } | DetailFileSelection::Merged { .. } => None,
    }
}

fn dirty_key_range(
    entries: &[DirtyFileKey],
    anchor: &DirtyFileKey,
    clicked: &DirtyFileKey,
) -> Option<Vec<DirtyFileKey>> {
    let anchor_idx = entries.iter().position(|entry| entry == anchor)?;
    let clicked_idx = entries.iter().position(|entry| entry == clicked)?;
    let (start, end) = if anchor_idx <= clicked_idx {
        (anchor_idx, clicked_idx)
    } else {
        (clicked_idx, anchor_idx)
    };
    Some(entries[start..=end].to_vec())
}

fn unique_dirty_paths(keys: Vec<DirtyFileKey>) -> Vec<String> {
    let mut paths = Vec::new();
    for key in keys {
        if !paths.iter().any(|path| path == &key.path) {
            paths.push(key.path);
        }
    }
    paths
}

fn next_file_index(
    entries: &[DetailFileEntry],
    selected_file: Option<&DetailFileSelection>,
    navigation: DetailFileNavigation,
) -> Option<usize> {
    if entries.is_empty() {
        return None;
    }
    match navigation {
        DetailFileNavigation::First => return Some(0),
        DetailFileNavigation::Last => return entries.len().checked_sub(1),
        DetailFileNavigation::Previous | DetailFileNavigation::Next => {}
    }

    let current = selected_file.and_then(|selected| {
        entries
            .iter()
            .position(|entry| &entry.selection == selected)
    });
    match (navigation, current) {
        (DetailFileNavigation::Next, Some(idx)) => Some((idx + 1).min(entries.len() - 1)),
        (DetailFileNavigation::Previous, Some(idx)) => Some(idx.saturating_sub(1)),
        (DetailFileNavigation::Next, None) => Some(0),
        (DetailFileNavigation::Previous, None) => entries.len().checked_sub(1),
        (DetailFileNavigation::First | DetailFileNavigation::Last, _) => unreachable!(),
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::{
        core::{ChangeKind, ChangedFile},
        screens::repository::state::OperationKind,
        services::{presenter::projection, CommitSnapshot, DirtySnapshot, RepoRef, RepoSnapshot},
    };

    fn changed(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.to_string(),
            kind: ChangeKind::Modified,
        }
    }

    fn commit_snapshot(hash: &str) -> CommitSnapshot {
        CommitSnapshot {
            hash: hash.to_string(),
            message: "initial".to_string(),
            author_name: "Ada".to_string(),
            authored_at: 1_700_000_000,
            authored_offset_minutes: 0,
            parent_hashes: vec![],
        }
    }

    fn dirty_snapshot(
        conflicted: Vec<&str>,
        unstaged: Vec<&str>,
        staged: Vec<&str>,
    ) -> DirtySnapshot {
        DirtySnapshot {
            conflicted_files: conflicted.into_iter().map(changed).collect(),
            staged_files: staged.into_iter().map(changed).collect(),
            unstaged_files: unstaged.into_iter().map(changed).collect(),
            parent_hashes: vec!["aaaaaaaa".to_string()],
        }
    }

    fn loaded_repo(dirty: DirtySnapshot) -> crate::view_model::LoadedRepo {
        projection::project_loaded(RepoSnapshot {
            commits: vec![commit_snapshot("aaaaaaaa")],
            refs: Vec::<RepoRef>::new(),
            dirty: Some(dirty),
            stashes: vec![],
            repo_name: "repo".to_string(),
            current_branch: Some("main".to_string()),
            head_hash: Some("aaaaaaaa".to_string()),
            has_more_commits: false,
            default_remote_name: None,
            remote_names: Vec::new(),
            fast_forward_candidates: Default::default(),
            worktrees: vec![],
            active_worktree_path: Default::default(),
        })
    }

    fn data_with_dirty(dirty: DirtySnapshot) -> RepositoryData {
        let mut data = RepositoryData::loading_with_repo_name("repo".to_string());
        data.replace_loaded(loaded_repo(dirty));
        data
    }

    #[test]
    fn detail_file_navigation_starts_with_first_file() {
        let data = data_with_dirty(dirty_snapshot(vec![], vec!["b.txt"], vec!["c.txt"]));
        let mut panel = DetailPanel::new();

        let _ = panel.select_file(DetailFileNavigation::Next, &data, &data.selection, None);

        assert!(matches!(
            panel.selected_file.as_ref(),
            Some(DetailFileSelection::Dirty { path, status: DirtyFileStatus::Unstaged }) if path == "b.txt"
        ));
        assert!(matches!(
            panel.open_selected_file_action(&data, &data.selection, None),
            Some(DetailAction::DirtyFileOpened { path, is_staged: false }) if path == "b.txt"
        ));
    }

    #[test]
    fn detail_file_first_and_last_skip_dirty_headers_and_empty_rows() {
        let data = data_with_dirty(dirty_snapshot(
            vec!["conflict.txt"],
            vec![],
            vec!["staged.txt"],
        ));
        let mut panel = DetailPanel::new();

        let _ = panel.select_file(DetailFileNavigation::Last, &data, &data.selection, None);
        assert!(matches!(
            panel.selected_file.as_ref(),
            Some(DetailFileSelection::Dirty { path, status: DirtyFileStatus::Staged }) if path == "staged.txt"
        ));

        let _ = panel.select_file(DetailFileNavigation::First, &data, &data.selection, None);
        assert!(matches!(
            panel.selected_file.as_ref(),
            Some(DetailFileSelection::Dirty { path, status: DirtyFileStatus::Conflicted }) if path == "conflict.txt"
        ));
    }

    #[test]
    fn dirty_file_ctrl_click_toggles_individual_files() {
        let data = data_with_dirty(dirty_snapshot(vec![], vec!["a.txt", "b.txt"], vec![]));
        let mut panel = DetailPanel::new();

        panel.select_dirty_file_from_click(
            "a.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Replace,
        );
        panel.select_dirty_file_from_click(
            "b.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Toggle,
        );

        assert_eq!(
            dirty_selection_paths(&panel),
            vec!["a.txt".to_string(), "b.txt".to_string()]
        );
        assert_eq!(panel.selected_dirty_count(&data, &data.selection), 2);

        panel.select_dirty_file_from_click(
            "a.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Toggle,
        );

        assert_eq!(dirty_selection_paths(&panel), vec!["b.txt".to_string()]);
        assert_eq!(panel.selected_dirty_count(&data, &data.selection), 1);
    }

    #[test]
    fn dirty_file_shift_click_selects_visual_range() {
        let data = data_with_dirty(dirty_snapshot(
            vec!["conflict.txt"],
            vec!["a.txt", "b.txt"],
            vec!["staged.txt"],
        ));
        let mut panel = DetailPanel::new();

        panel.select_dirty_file_from_click(
            "a.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Replace,
        );
        panel.select_dirty_file_from_click(
            "staged.txt".to_string(),
            true,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Range,
        );

        assert_eq!(
            dirty_selection_paths(&panel),
            vec![
                "a.txt".to_string(),
                "b.txt".to_string(),
                "staged.txt".to_string()
            ]
        );
        assert_eq!(
            panel.selected_dirty_paths_for_status(
                &data,
                &data.selection,
                DirtyFileStatus::Unstaged
            ),
            vec!["a.txt".to_string(), "b.txt".to_string()]
        );
        assert_eq!(
            panel.selected_dirty_paths_for_status(&data, &data.selection, DirtyFileStatus::Staged),
            vec!["staged.txt".to_string()]
        );
    }

    #[test]
    fn dirty_file_keyboard_extend_uses_existing_anchor_and_cursor() {
        let data = data_with_dirty(dirty_snapshot(
            vec![],
            vec!["a.txt", "b.txt", "c.txt"],
            vec![],
        ));
        let mut panel = DetailPanel::new();

        let _ = panel.select_file(DetailFileNavigation::Next, &data, &data.selection, None);
        let _ =
            panel.extend_file_selection(DetailFileNavigation::Next, &data, &data.selection, None);
        let _ =
            panel.extend_file_selection(DetailFileNavigation::Next, &data, &data.selection, None);

        assert_eq!(
            dirty_selection_paths(&panel),
            vec![
                "a.txt".to_string(),
                "b.txt".to_string(),
                "c.txt".to_string()
            ]
        );

        let _ = panel.extend_file_selection(
            DetailFileNavigation::Previous,
            &data,
            &data.selection,
            None,
        );

        assert_eq!(
            dirty_selection_paths(&panel),
            vec!["a.txt".to_string(), "b.txt".to_string()]
        );
    }

    #[test]
    fn selected_stage_paths_do_not_include_conflicts() {
        let data = data_with_dirty(dirty_snapshot(vec!["conflict.txt"], vec!["a.txt"], vec![]));
        let mut panel = DetailPanel::new();

        panel.select_dirty_file_from_click(
            "conflict.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Replace,
        );
        panel.select_dirty_file_from_click(
            "a.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Range,
        );

        assert_eq!(
            dirty_selection_statuses(&panel),
            vec![DirtyFileStatus::Conflicted, DirtyFileStatus::Unstaged]
        );
        assert_eq!(
            panel.selected_dirty_paths_for_status(
                &data,
                &data.selection,
                DirtyFileStatus::Unstaged
            ),
            vec!["a.txt".to_string()]
        );
    }

    #[test]
    fn dirty_reselection_after_staging_selected_file_prefers_next_unstaged_neighbor() {
        let mut data = data_with_dirty(dirty_snapshot(
            vec![],
            vec!["a.txt", "b.txt", "c.txt"],
            vec![],
        ));
        let mut panel = DetailPanel::new();
        panel.select_dirty_file_from_click(
            "b.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Replace,
        );

        let path = "b.txt".to_string();
        let operation_id = data
            .operations
            .begin_write_kind(OperationKind::StageFile)
            .unwrap();
        panel.prepare_dirty_reselection_after_write(
            operation_id,
            std::slice::from_ref(&path),
            DirtyFileStatus::Unstaged,
            &data,
            &data.selection,
        );

        let updated = data_with_dirty(dirty_snapshot(
            vec![],
            vec!["a.txt", "c.txt"],
            vec!["b.txt"],
        ));
        panel.apply_pending_dirty_reselection_after_reload(
            operation_id,
            &updated,
            &updated.selection,
        );

        assert_selected_dirty_file(&panel, "c.txt", DirtyFileStatus::Unstaged);
    }

    #[test]
    fn dirty_reselection_after_staging_selected_file_falls_back_to_previous_neighbor() {
        let mut data = data_with_dirty(dirty_snapshot(vec![], vec!["a.txt", "b.txt"], vec![]));
        let mut panel = DetailPanel::new();
        panel.select_dirty_file_from_click(
            "b.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Replace,
        );

        let path = "b.txt".to_string();
        let operation_id = data
            .operations
            .begin_write_kind(OperationKind::StageFile)
            .unwrap();
        panel.prepare_dirty_reselection_after_write(
            operation_id,
            std::slice::from_ref(&path),
            DirtyFileStatus::Unstaged,
            &data,
            &data.selection,
        );

        let updated = data_with_dirty(dirty_snapshot(vec![], vec!["a.txt"], vec!["b.txt"]));
        panel.apply_pending_dirty_reselection_after_reload(
            operation_id,
            &updated,
            &updated.selection,
        );

        assert_selected_dirty_file(&panel, "a.txt", DirtyFileStatus::Unstaged);
    }

    #[test]
    fn dirty_reselection_after_staging_unselected_file_keeps_current_selection() {
        let mut data = data_with_dirty(dirty_snapshot(vec![], vec!["a.txt", "b.txt"], vec![]));
        let mut panel = DetailPanel::new();
        panel.select_dirty_file_from_click(
            "a.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Replace,
        );

        let path = "b.txt".to_string();
        let operation_id = data
            .operations
            .begin_write_kind(OperationKind::StageFile)
            .unwrap();
        panel.prepare_dirty_reselection_after_write(
            operation_id,
            std::slice::from_ref(&path),
            DirtyFileStatus::Unstaged,
            &data,
            &data.selection,
        );

        let updated = data_with_dirty(dirty_snapshot(vec![], vec!["a.txt"], vec!["b.txt"]));
        panel.apply_pending_dirty_reselection_after_reload(
            operation_id,
            &updated,
            &updated.selection,
        );

        assert_eq!(dirty_selection_paths(&panel), vec!["a.txt".to_string()]);
        assert_eq!(
            dirty_selection_statuses(&panel),
            vec![DirtyFileStatus::Unstaged]
        );
    }

    #[test]
    fn dirty_reselection_ignores_last_cursor_when_file_is_not_visually_selected() {
        let mut data = data_with_dirty(dirty_snapshot(
            vec![],
            vec!["a.txt", "b.txt", "c.txt"],
            vec![],
        ));
        let mut panel = DetailPanel::new();
        panel.select_dirty_file_from_click(
            "a.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Replace,
        );
        panel.select_dirty_file_from_click(
            "b.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Toggle,
        );
        panel.select_dirty_file_from_click(
            "b.txt".to_string(),
            false,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Toggle,
        );

        let path = "b.txt".to_string();
        let operation_id = data
            .operations
            .begin_write_kind(OperationKind::StageFile)
            .unwrap();
        panel.prepare_dirty_reselection_after_write(
            operation_id,
            std::slice::from_ref(&path),
            DirtyFileStatus::Unstaged,
            &data,
            &data.selection,
        );

        let updated = data_with_dirty(dirty_snapshot(
            vec![],
            vec!["a.txt", "c.txt"],
            vec!["b.txt"],
        ));
        panel.apply_pending_dirty_reselection_after_reload(
            operation_id,
            &updated,
            &updated.selection,
        );

        assert_eq!(dirty_selection_paths(&panel), vec!["a.txt".to_string()]);
        assert_eq!(
            dirty_selection_statuses(&panel),
            vec![DirtyFileStatus::Unstaged]
        );
    }

    #[test]
    fn dirty_reselection_after_unstaging_selected_file_prefers_next_staged_neighbor() {
        let mut data = data_with_dirty(dirty_snapshot(
            vec![],
            vec![],
            vec!["a.txt", "b.txt", "c.txt"],
        ));
        let mut panel = DetailPanel::new();
        panel.select_dirty_file_from_click(
            "b.txt".to_string(),
            true,
            &data,
            &data.selection,
            DirtyFileSelectionMode::Replace,
        );

        let path = "b.txt".to_string();
        let operation_id = data
            .operations
            .begin_write_kind(OperationKind::UnstageFile)
            .unwrap();
        panel.prepare_dirty_reselection_after_write(
            operation_id,
            std::slice::from_ref(&path),
            DirtyFileStatus::Staged,
            &data,
            &data.selection,
        );

        let updated = data_with_dirty(dirty_snapshot(
            vec![],
            vec!["b.txt"],
            vec!["a.txt", "c.txt"],
        ));
        panel.apply_pending_dirty_reselection_after_reload(
            operation_id,
            &updated,
            &updated.selection,
        );

        assert_selected_dirty_file(&panel, "c.txt", DirtyFileStatus::Staged);
    }

    fn assert_selected_dirty_file(
        panel: &DetailPanel,
        expected_path: &str,
        expected_status: DirtyFileStatus,
    ) {
        assert!(matches!(
            panel.selected_file.as_ref(),
            Some(DetailFileSelection::Dirty { path, status })
                if path == expected_path && *status == expected_status
        ));
        assert_eq!(
            panel.selected_dirty_files,
            vec![DirtyFileKey {
                path: expected_path.to_string(),
                status: expected_status,
            }]
        );
    }

    fn dirty_selection_paths(panel: &DetailPanel) -> Vec<String> {
        panel
            .selected_dirty_files
            .iter()
            .map(|key| key.path.clone())
            .collect()
    }

    fn dirty_selection_statuses(panel: &DetailPanel) -> Vec<DirtyFileStatus> {
        panel
            .selected_dirty_files
            .iter()
            .map(|key| key.status)
            .collect()
    }
}

pub(in crate::screens::repository) fn detail_file_list_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("detail-file-list")
}

pub(in crate::screens::repository) fn dirty_commit_message_editor_id() -> iced::widget::Id {
    iced::widget::Id::new("dirty-commit-message-editor")
}

pub(in crate::screens::repository) fn reword_commit_message_editor_id() -> iced::widget::Id {
    iced::widget::Id::new("reword-commit-message-editor")
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
