//! Merged multi-commit diff state — aggregated change spanning multiple
//! commit hashes for a single path.

use std::sync::Arc;

use crate::services::{HighlightedFile, WorkingTreeDiffLine, WorkingTreeDiffResult};

use super::{DiffLoadMode, DiffPanel, DiffPanelAction, SingleFileDiffView};

#[derive(Debug, Clone)]
pub struct MergedFileDiffState {
    pub hashes: Vec<String>,
    pub file_path: String,
    pub diff_lines: Option<Vec<WorkingTreeDiffLine>>,
    pub old_highlighted: Option<Arc<HighlightedFile>>,
    pub new_highlighted: Option<Arc<HighlightedFile>>,
    pub selected_file_idx: usize,
}

impl SingleFileDiffView for MergedFileDiffState {
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

impl DiffLoadMode for MergedFileDiffState {
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
        DiffPanelAction::RunMergedHighlight {
            file_path,
            old_content,
            new_content,
        }
    }
}

impl DiffPanel {
    pub(in crate::screens::repository) fn open_merged_file(
        &mut self,
        hashes: Vec<String>,
        path: String,
        selected_file_idx: usize,
    ) -> DiffPanelAction {
        self.merged_file_diff = Some(MergedFileDiffState {
            hashes: hashes.clone(),
            file_path: path.clone(),
            diff_lines: None,
            old_highlighted: None,
            new_highlighted: None,
            selected_file_idx,
        });
        self.dirty_file_diff = None;
        self.conflict_file_resolution = None;
        self.commit_file_diff = None;
        self.center_view_mode = super::CenterViewMode::DiffView;
        self.diff_selection = None;
        self.diff_scroll_y = 0.0;
        self.text_search = None;
        DiffPanelAction::LoadMergedFileDiff { hashes, path }
    }

    pub(in crate::screens::repository) fn on_merged_diff_loaded(
        &mut self,
        result: Result<WorkingTreeDiffResult, crate::services::GitError>,
    ) -> Option<DiffPanelAction> {
        super::on_diff_loaded(&mut self.merged_file_diff, result)
    }

    pub(in crate::screens::repository) fn on_merged_highlight_ready(
        &mut self,
        file_path: String,
        old: Option<Arc<HighlightedFile>>,
        new: Option<Arc<HighlightedFile>>,
    ) {
        if let Some(state) = self.merged_file_diff.as_mut() {
            if state.file_path == file_path {
                state.old_highlighted = old;
                state.new_highlighted = new;
            }
        }
    }
}
