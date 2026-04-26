//! Dirty-file diff state — working-tree changes vs. index/HEAD for a single
//! path. Backing state for the unstaged / staged file views.

use std::sync::Arc;

use crate::services::{
    DirtyDiffSignature, HighlightedFile, WorkingTreeDiffLine, WorkingTreeDiffResult,
};

use super::{DiffLoadMode, DiffPanel, DiffPanelAction, SingleFileDiffView};

#[derive(Debug, Clone)]
pub struct DirtyFileDiffState {
    pub file_path: String,
    pub is_staged: bool,
    pub diff_lines: Option<Vec<WorkingTreeDiffLine>>,
    pub old_highlighted: Option<Arc<HighlightedFile>>,
    pub new_highlighted: Option<Arc<HighlightedFile>>,
    pub selected_file_idx: usize,
    pub last_signature: Option<DirtyDiffSignature>,
}

impl SingleFileDiffView for DirtyFileDiffState {
    fn file_path(&self) -> &str {
        &self.file_path
    }
    fn diff_lines(&self) -> &[WorkingTreeDiffLine] {
        self.diff_lines.as_deref().unwrap_or(&[])
    }
    fn old_highlighted(&self) -> Option<&HighlightedFile> {
        self.old_highlighted.as_deref()
    }
    fn new_highlighted(&self) -> Option<&HighlightedFile> {
        self.new_highlighted.as_deref()
    }
}

impl DiffLoadMode for DirtyFileDiffState {
    fn file_path(&self) -> &str {
        &self.file_path
    }

    fn set_lines(&mut self, lines: Vec<WorkingTreeDiffLine>) {
        self.diff_lines = Some(lines);
    }

    fn clear_highlights(&mut self) {
        self.old_highlighted = None;
        self.new_highlighted = None;
    }

    fn highlight_action(
        &self,
        file_path: String,
        old_content: Option<String>,
        new_content: Option<String>,
    ) -> DiffPanelAction {
        DiffPanelAction::RunDirtyHighlight {
            file_path,
            is_staged: self.is_staged,
            old_content,
            new_content,
        }
    }
}

impl DiffPanel {
    pub(in crate::screens::repository) fn open_dirty_file_view(
        &mut self,
        path: String,
        is_staged: bool,
        selected_file_idx: usize,
        is_conflicted: bool,
    ) -> DiffPanelAction {
        self.center_view_mode = super::CenterViewMode::DiffView;
        self.commit_file_diff = None;
        self.diff_selection = None;
        self.diff_scroll_y = 0.0;
        self.text_search = None;

        if is_conflicted {
            self.dirty_file_diff = None;
            self.conflict_file_resolution = Some(super::conflict::ConflictFileResolutionState {
                file_path: path.clone(),
                result: None,
                selections: vec![],
                selected_file_idx,
                initial_scroll_done: false,
                ignore_initial_ours_top_scroll: false,
                ignore_initial_theirs_top_scroll: false,
                ignore_next_ours_scroll: false,
                ignore_next_theirs_scroll: false,
                ours_highlighted: None,
                theirs_highlighted: None,
                ours_scroll_offset_y: 0.0,
                theirs_scroll_offset_y: 0.0,
                output_scroll_offset_y: 0.0,
                ours_scroll_offset_x: 0.0,
                theirs_scroll_offset_x: 0.0,
                output_scroll_offset_x: 0.0,
            });
            return DiffPanelAction::LoadConflictResolution { path };
        }

        self.conflict_file_resolution = None;
        self.dirty_file_diff = Some(DirtyFileDiffState {
            file_path: path.clone(),
            is_staged,
            diff_lines: None,
            old_highlighted: None,
            new_highlighted: None,
            selected_file_idx,
            last_signature: None,
        });
        DiffPanelAction::LoadDirtyFileDiff { path, is_staged }
    }

    pub(in crate::screens::repository) fn sync_and_reload_dirty_diff_action(
        &mut self,
        dirty_commit: Option<&crate::core::Commit>,
        compute_signature: impl FnOnce(&str, bool) -> DirtyDiffSignature,
    ) -> Option<DiffPanelAction> {
        let state = self.dirty_file_diff.as_ref()?;
        let path = state.file_path.clone();

        let Some(commit) = dirty_commit else {
            self.close();
            return None;
        };

        let in_conflicted = commit.conflicted_files.iter().any(|f| f.path == path);
        let in_staged = commit.staged_files.iter().any(|f| f.path == path);
        let in_unstaged = commit.unstaged_files.iter().any(|f| f.path == path);

        if !in_conflicted && !in_staged && !in_unstaged {
            self.close();
            return None;
        }

        let prior_is_staged = state.is_staged;
        if let Some(state) = self.dirty_file_diff.as_mut() {
            if state.is_staged && !in_staged && in_unstaged {
                state.is_staged = false;
            } else if !state.is_staged && !in_unstaged && in_staged {
                state.is_staged = true;
            }
        }

        let state = self.dirty_file_diff.as_ref()?;
        let path = state.file_path.clone();
        let is_staged = state.is_staged;
        let new_sig = compute_signature(&path, is_staged);

        let bucket_flipped = prior_is_staged != is_staged;
        if !bucket_flipped {
            if let Some(prev) = state.last_signature.as_ref() {
                if prev == &new_sig {
                    return None;
                }
            }
        }

        Some(DiffPanelAction::LoadDirtyFileDiff { path, is_staged })
    }

    pub(in crate::screens::repository) fn on_dirty_diff_loaded(
        &mut self,
        result: Result<WorkingTreeDiffResult, crate::services::GitError>,
    ) -> Option<DiffPanelAction> {
        let signature = result.as_ref().ok().and_then(|r| r.dirty_signature.clone());
        let action = super::on_diff_loaded(&mut self.dirty_file_diff, result);
        if let Some(state) = self.dirty_file_diff.as_mut() {
            state.last_signature = signature;
        }
        action
    }

    pub(in crate::screens::repository) fn on_dirty_highlight_ready(
        &mut self,
        file_path: String,
        is_staged: bool,
        old: Option<Arc<HighlightedFile>>,
        new: Option<Arc<HighlightedFile>>,
    ) {
        if let Some(state) = self.dirty_file_diff.as_mut() {
            if state.file_path == file_path && state.is_staged == is_staged {
                state.old_highlighted = old;
                state.new_highlighted = new;
            }
        }
    }
}
