//! Merged multi-commit diff state — aggregated change spanning multiple
//! commit hashes for a single path.

use std::sync::Arc;

use crate::services::{DiffFallbacks, HighlightedFile, WorkingTreeDiffLine, WorkingTreeDiffResult};

use super::{DiffLoadMode, DiffPanel, DiffPanelAction, SingleFileDiffKind, SingleFileDiffView};

#[derive(Debug, Clone)]
pub struct MergedFileDiffState {
    pub hashes: Vec<String>,
    pub file_path: String,
    pub diff_lines: Option<Arc<Vec<WorkingTreeDiffLine>>>,
    pub old_highlighted: Option<Arc<HighlightedFile>>,
    pub new_highlighted: Option<Arc<HighlightedFile>>,
    pub render_data: Option<Arc<crate::widgets::diff_canvas::DiffCanvasData>>,
    pub render_generation: u64,
    pub fallbacks: DiffFallbacks,
    pub selected_file_idx: usize,
}

impl SingleFileDiffView for MergedFileDiffState {
    fn file_path(&self) -> &str {
        &self.file_path
    }
    fn render_data(&self) -> Option<Arc<crate::widgets::diff_canvas::DiffCanvasData>> {
        self.render_data.clone()
    }
}

impl DiffLoadMode for MergedFileDiffState {
    fn file_path(&self) -> &str {
        &self.file_path
    }

    fn set_lines(&mut self, lines: Arc<Vec<WorkingTreeDiffLine>>) {
        self.diff_lines = Some(lines);
        self.render_data = None;
    }

    fn lines(&self) -> Option<Arc<Vec<WorkingTreeDiffLine>>> {
        self.diff_lines.clone()
    }

    fn generation(&self) -> u64 {
        self.render_generation
    }

    fn set_generation(&mut self, generation: u64) {
        self.render_generation = generation;
    }

    fn set_fallbacks(&mut self, fallbacks: DiffFallbacks) {
        self.fallbacks = fallbacks;
    }

    fn fallbacks(&self) -> DiffFallbacks {
        self.fallbacks.clone()
    }

    fn clear_highlights(&mut self) {
        self.old_highlighted = None;
        self.new_highlighted = None;
    }

    fn highlight_action(
        &self,
        generation: u64,
        file_path: String,
        old_content: Option<String>,
        new_content: Option<String>,
    ) -> DiffPanelAction {
        DiffPanelAction::RunMergedHighlight {
            generation,
            file_path,
            old_content,
            new_content,
        }
    }

    fn render_kind(&self) -> SingleFileDiffKind {
        SingleFileDiffKind::Merged
    }
}

impl DiffPanel {
    pub(in crate::screens::repository) fn open_merged_file(
        &mut self,
        hashes: Vec<String>,
        path: String,
        selected_file_idx: usize,
    ) -> DiffPanelAction {
        let generation = self.next_diff_generation();
        self.merged_file_diff = Some(MergedFileDiffState {
            hashes: hashes.clone(),
            file_path: path.clone(),
            diff_lines: None,
            old_highlighted: None,
            new_highlighted: None,
            render_data: None,
            render_generation: generation,
            fallbacks: DiffFallbacks::default(),
            selected_file_idx,
        });
        self.dirty_file_diff = None;
        self.conflict_file_resolution = None;
        self.commit_file_diff = None;
        self.center_view_mode = super::CenterViewMode::DiffView;
        self.diff_selection = None;
        self.diff_scroll_y = 0.0;
        self.text_search = None;
        DiffPanelAction::LoadMergedFileDiff {
            generation,
            hashes,
            path,
        }
    }

    pub(in crate::screens::repository) fn on_merged_diff_loaded(
        &mut self,
        generation: u64,
        result: Result<WorkingTreeDiffResult, crate::services::GitError>,
    ) -> Vec<DiffPanelAction> {
        super::on_diff_loaded(
            &mut self.merged_file_diff,
            &mut self.next_generation,
            generation,
            result,
        )
    }

    pub(in crate::screens::repository) fn on_merged_highlight_ready(
        &mut self,
        generation: u64,
        file_path: String,
        old: Option<Arc<HighlightedFile>>,
        new: Option<Arc<HighlightedFile>>,
    ) -> Option<DiffPanelAction> {
        if let Some(state) = self.merged_file_diff.as_mut() {
            if state.file_path == file_path && state.render_generation == generation {
                state.old_highlighted = old.clone();
                state.new_highlighted = new.clone();
                let lines = state.lines();
                let fallbacks = state.fallbacks();
                let generation = self.next_diff_generation();
                if let Some(state) = self.merged_file_diff.as_mut() {
                    state.render_generation = generation;
                }
                return lines.map(|lines| DiffPanelAction::RunSingleFileRenderBuild {
                    generation,
                    kind: SingleFileDiffKind::Merged,
                    file_path,
                    lines,
                    fallbacks,
                    old_highlighted: old,
                    new_highlighted: new,
                });
            }
        }
        None
    }
}
