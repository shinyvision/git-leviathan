//! Commit-file diff state — single-commit change vs. its parent for a path.

use std::sync::Arc;

use crate::services::{HighlightedFile, WorkingTreeDiffLine, WorkingTreeDiffResult};

use super::{DiffLoadMode, DiffPanel, DiffPanelAction, SingleFileDiffView};

#[derive(Debug, Clone)]
pub struct CommitFileDiffState {
    pub commit_idx: usize,
    pub commit_hash: String,
    pub file_path: String,
    pub diff_lines: Option<Vec<WorkingTreeDiffLine>>,
    pub old_highlighted: Option<Arc<HighlightedFile>>,
    pub new_highlighted: Option<Arc<HighlightedFile>>,
    pub selected_file_idx: usize,
}

impl SingleFileDiffView for CommitFileDiffState {
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

impl DiffLoadMode for CommitFileDiffState {
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
        DiffPanelAction::RunCommitHighlight {
            commit_hash: self.commit_hash.clone(),
            file_path,
            old_content,
            new_content,
        }
    }
}

impl DiffPanel {
    pub(in crate::screens::repository) fn open_commit_file(
        &mut self,
        commit_idx: usize,
        commit_hash: String,
        path: String,
        selected_file_idx: usize,
    ) -> DiffPanelAction {
        self.commit_file_diff = Some(CommitFileDiffState {
            commit_idx,
            commit_hash: commit_hash.clone(),
            file_path: path.clone(),
            diff_lines: None,
            old_highlighted: None,
            new_highlighted: None,
            selected_file_idx,
        });
        self.dirty_file_diff = None;
        self.conflict_file_resolution = None;
        self.merged_file_diff = None;
        self.center_view_mode = super::CenterViewMode::DiffView;
        self.diff_selection = None;
        self.diff_scroll_y = 0.0;
        self.text_search = None;
        DiffPanelAction::LoadCommitFileDiff { commit_hash, path }
    }

    pub(in crate::screens::repository) fn on_commit_diff_loaded(
        &mut self,
        result: Result<WorkingTreeDiffResult, crate::services::GitError>,
    ) -> Option<DiffPanelAction> {
        super::on_diff_loaded(&mut self.commit_file_diff, result)
    }

    pub(in crate::screens::repository) fn on_commit_highlight_ready(
        &mut self,
        commit_hash: String,
        file_path: String,
        old: Option<Arc<HighlightedFile>>,
        new: Option<Arc<HighlightedFile>>,
    ) {
        if let Some(state) = self.commit_file_diff.as_mut() {
            if state.file_path == file_path && state.commit_hash == commit_hash {
                state.old_highlighted = old;
                state.new_highlighted = new;
            }
        }
    }
}
