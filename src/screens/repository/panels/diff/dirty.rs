//! Dirty-file diff state — working-tree changes vs. index/HEAD for a single
//! path. Backing state for the unstaged / staged file views.

use std::sync::Arc;

use crate::services::{
    DiffFallbacks, DirtyDiffSignature, HighlightDocument, WorkingTreeDiffLine,
    WorkingTreeDiffResult,
};
use crate::widgets::diff_canvas::CachedDiffHighlightProvider;

use super::{DiffLoadMode, DiffPanel, DiffPanelAction, SingleFileDiffKind, SingleFileDiffView};

#[derive(Debug, Clone)]
pub struct DirtyFileDiffState {
    pub file_path: String,
    pub is_staged: bool,
    pub diff_lines: Option<Arc<Vec<WorkingTreeDiffLine>>>,
    pub old_highlight_document: Option<HighlightDocument>,
    pub new_highlight_document: Option<HighlightDocument>,
    pub highlight_provider: Arc<CachedDiffHighlightProvider>,
    pub render_data: Option<Arc<crate::widgets::diff_canvas::DiffCanvasData>>,
    pub render_generation: u64,
    pub fallbacks: DiffFallbacks,
    pub selected_file_idx: usize,
    pub last_signature: Option<DirtyDiffSignature>,
}

impl SingleFileDiffView for DirtyFileDiffState {
    fn file_path(&self) -> &str {
        &self.file_path
    }
    fn render_data(&self) -> Option<Arc<crate::widgets::diff_canvas::DiffCanvasData>> {
        self.render_data.clone()
    }
}

impl DiffLoadMode for DirtyFileDiffState {
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
        SingleFileDiffKind::Dirty {
            is_staged: self.is_staged,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct DirtyDiffSyncRequest {
    pub path: String,
    pub is_staged: bool,
    pub force_reload: bool,
}

#[derive(Debug, Clone)]
pub struct DirtyDiffSyncResult {
    pub path: String,
    pub is_staged: bool,
    pub force_reload: bool,
    pub signature: DirtyDiffSignature,
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
        self.diff_viewport_height = None;
        self.text_search = None;
        self.highlight_scheduler.reset();

        if is_conflicted {
            let generation = self.next_diff_generation();
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
                render_generation: generation,
                ours_scroll_offset_y: 0.0,
                theirs_scroll_offset_y: 0.0,
                output_scroll_offset_y: 0.0,
                ours_scroll_offset_x: 0.0,
                theirs_scroll_offset_x: 0.0,
                output_scroll_offset_x: 0.0,
                ours_viewport_height: None,
                theirs_viewport_height: None,
                output_viewport_height: None,
            });
            return DiffPanelAction::LoadConflictResolution { generation, path };
        }

        self.conflict_file_resolution = None;
        let generation = self.next_diff_generation();
        self.dirty_file_diff = Some(DirtyFileDiffState {
            file_path: path.clone(),
            is_staged,
            diff_lines: None,
            old_highlight_document: None,
            new_highlight_document: None,
            highlight_provider: Arc::new(CachedDiffHighlightProvider::new()),
            render_data: None,
            render_generation: generation,
            fallbacks: DiffFallbacks::default(),
            selected_file_idx,
            last_signature: None,
        });
        DiffPanelAction::LoadDirtyFileDiff {
            generation,
            path,
            is_staged,
        }
    }

    pub(in crate::screens::repository) fn dirty_diff_sync_request_after_reload(
        &mut self,
        dirty_commit: Option<&crate::core::Commit>,
    ) -> Option<DirtyDiffSyncRequest> {
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
        Some(DirtyDiffSyncRequest {
            path,
            is_staged,
            force_reload: prior_is_staged != is_staged,
        })
    }

    pub(in crate::screens::repository) fn on_dirty_diff_sync_result(
        &mut self,
        result: Result<DirtyDiffSyncResult, crate::services::GitError>,
    ) -> Option<DiffPanelAction> {
        let Ok(result) = result else {
            return None;
        };
        let state = self.dirty_file_diff.as_mut()?;
        if state.file_path != result.path || state.is_staged != result.is_staged {
            return None;
        }
        if !result.force_reload && state.last_signature.as_ref() == Some(&result.signature) {
            return None;
        }
        let generation = self.next_diff_generation();
        if let Some(state) = self.dirty_file_diff.as_mut() {
            state.render_generation = generation;
            state.render_data = None;
        }
        Some(DiffPanelAction::LoadDirtyFileDiff {
            generation,
            path: result.path,
            is_staged: result.is_staged,
        })
    }

    pub(in crate::screens::repository) fn on_dirty_diff_loaded(
        &mut self,
        generation: u64,
        result: Result<WorkingTreeDiffResult, crate::services::GitError>,
    ) -> Vec<DiffPanelAction> {
        let signature = result.as_ref().ok().and_then(|r| r.dirty_signature.clone());
        let action = super::on_diff_loaded(
            &mut self.dirty_file_diff,
            &mut self.next_generation,
            generation,
            result,
        );
        if let Some(state) = self.dirty_file_diff.as_mut() {
            state.last_signature = signature;
        }
        action
    }
}
