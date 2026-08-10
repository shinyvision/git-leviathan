//! Commit-file diff state — single-commit change vs. its parent for a path.

use std::sync::Arc;

use crate::services::{
    DiffFallbacks, HighlightDocument, WorkingTreeDiffLine, WorkingTreeDiffResult,
};
use crate::widgets::diff_canvas::CachedDiffHighlightProvider;

use super::{DiffLoadMode, DiffPanel, DiffPanelAction, SingleFileDiffKind, SingleFileDiffView};

#[derive(Debug, Clone)]
pub struct CommitFileDiffState {
    pub commit_idx: usize,
    pub commit_hash: String,
    pub file_path: String,
    pub diff_lines: Option<Arc<Vec<WorkingTreeDiffLine>>>,
    pub old_highlight_document: Option<HighlightDocument>,
    pub new_highlight_document: Option<HighlightDocument>,
    pub highlight_provider: Arc<CachedDiffHighlightProvider>,
    pub render_data: Option<Arc<crate::widgets::diff_canvas::DiffCanvasData>>,
    pub render_generation: u64,
    pub fallbacks: DiffFallbacks,
    pub selected_file_idx: usize,
}

impl SingleFileDiffView for CommitFileDiffState {
    fn file_path(&self) -> &str {
        &self.file_path
    }
    fn render_data(&self) -> Option<Arc<crate::widgets::diff_canvas::DiffCanvasData>> {
        self.render_data.clone()
    }
}

impl DiffLoadMode for CommitFileDiffState {
    fn file_path(&self) -> &str {
        &self.file_path
    }

    fn lines(&self) -> Option<Arc<Vec<WorkingTreeDiffLine>>> {
        self.diff_lines.clone()
    }

    fn set_lines(&mut self, lines: Arc<Vec<WorkingTreeDiffLine>>) {
        self.diff_lines = Some(lines);
        self.render_data = None;
    }

    fn generation(&self) -> u64 {
        self.render_generation
    }

    fn set_generation(&mut self, generation: u64) {
        self.render_generation = generation;
    }

    fn fallbacks(&self) -> DiffFallbacks {
        self.fallbacks.clone()
    }

    fn set_fallbacks(&mut self, fallbacks: DiffFallbacks) {
        self.fallbacks = fallbacks;
    }

    fn set_highlight_documents(
        &mut self,
        old: Option<HighlightDocument>,
        new: Option<HighlightDocument>,
    ) {
        self.old_highlight_document = old;
        self.new_highlight_document = new;
    }

    fn reset_highlight_provider(&mut self) {
        self.highlight_provider = Arc::new(CachedDiffHighlightProvider::new());
    }

    fn highlight_provider(&self) -> Arc<CachedDiffHighlightProvider> {
        self.highlight_provider.clone()
    }

    fn render_kind(&self) -> SingleFileDiffKind {
        SingleFileDiffKind::Commit {
            commit_hash: self.commit_hash.clone(),
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
        let generation = self.next_diff_generation();
        self.commit_file_diff = Some(CommitFileDiffState {
            commit_idx,
            commit_hash: commit_hash.clone(),
            file_path: path.clone(),
            diff_lines: None,
            old_highlight_document: None,
            new_highlight_document: None,
            highlight_provider: Arc::new(CachedDiffHighlightProvider::new()),
            render_data: None,
            render_generation: generation,
            fallbacks: DiffFallbacks::default(),
            selected_file_idx,
        });
        self.dirty_file_diff = None;
        self.conflict_file_resolution = None;
        self.merged_file_diff = None;
        self.center_view_mode = super::CenterViewMode::DiffView;
        self.diff_selection = None;
        self.diff_scroll_y = 0.0;
        self.diff_scroll_x = 0.0;
        self.diff_viewport_height = None;
        self.text_search = None;
        self.highlight_scheduler.reset();
        DiffPanelAction::LoadCommitFileDiff {
            generation,
            commit_hash,
            path,
        }
    }

    pub(in crate::screens::repository) fn on_commit_diff_loaded(
        &mut self,
        generation: u64,
        result: Result<WorkingTreeDiffResult, crate::services::GitError>,
    ) -> Vec<DiffPanelAction> {
        super::on_diff_loaded(
            &mut self.commit_file_diff,
            &mut self.next_generation,
            generation,
            result,
        )
    }
}
