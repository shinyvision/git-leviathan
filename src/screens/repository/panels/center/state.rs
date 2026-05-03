use iced::{
    widget::{scrollable, text_editor},
    Element,
};

use crate::{
    core::{Commit, RepoVersion},
    message::Message,
    view_model::{CommitDiffState, CommitPresentation, GraphRow},
};

use super::super::super::{
    commit_search::CommitSearch,
    panel_messages::CenterAction,
    state::{
        center_list_scroll_id, BranchPopoutController, BranchPopoutState, BranchPressOutcome,
        CenterListState, RepositoryData, SelectionState,
    },
};
use super::view as center_view;

#[derive(Debug, Clone)]
pub(crate) struct CenterViewModel<'a> {
    pub(in crate::screens::repository) commits: &'a [Commit],
    pub(in crate::screens::repository) commit_presentations: &'a [CommitPresentation],
    pub(in crate::screens::repository) commit_diff_states: &'a [CommitDiffState],
    pub(in crate::screens::repository) graph_rows: &'a [GraphRow],
    pub(in crate::screens::repository) graph_revision: RepoVersion,
    pub(in crate::screens::repository) selected_indices: Vec<usize>,
    pub(in crate::screens::repository) num_lanes: usize,
    pub(in crate::screens::repository) center_list_offset_y: f32,
    pub(in crate::screens::repository) center_list_viewport_h: f32,
    pub(in crate::screens::repository) branch_popout: Option<BranchPopoutState>,
    pub(in crate::screens::repository) dirty_commit_message: &'a text_editor::Content,
    pub(in crate::screens::repository) commit_search: Option<&'a CommitSearch>,
}

pub(in crate::screens::repository) struct CenterPanel {
    pub center_list: CenterListState,
    pub has_more_commits: bool,
    pub loading_more_commits: bool,
    pub repo_version: RepoVersion,
}

impl CenterPanel {
    pub(in crate::screens::repository) fn new() -> Self {
        Self {
            center_list: CenterListState::new(),
            has_more_commits: false,
            loading_more_commits: false,
            repo_version: RepoVersion::default(),
        }
    }

    pub(in crate::screens::repository) fn on_repo_loaded(
        &mut self,
        commits: &[Commit],
        has_more: bool,
        branch_popout: &mut BranchPopoutController,
    ) {
        branch_popout.sync_commit_indices(commits);
        branch_popout.invalidate_bounds();
        self.has_more_commits = has_more;
        self.loading_more_commits = false;
        self.repo_version = self.repo_version.bump();
    }

    /// Fetch-path reload: updates has_more_commits + repo_version and patches
    /// popout/context-menu stored indices in place. Never closes an open
    /// popout or context menu — those belong to overlay-layer UI state which
    /// the fetch path is not allowed to disturb.
    pub(in crate::screens::repository) fn on_refs_updated(
        &mut self,
        commits: &[Commit],
        has_more: bool,
        branch_popout: &mut BranchPopoutController,
    ) {
        branch_popout.patch_commit_indices_or_noop(commits);
        branch_popout.invalidate_bounds();
        self.has_more_commits = has_more;
        self.loading_more_commits = false;
        self.repo_version = self.repo_version.bump();
    }

    pub(in crate::screens::repository) fn on_more_commits_loaded(
        &mut self,
        repo_version: RepoVersion,
        commits: &[Commit],
        has_more: bool,
        branch_popout: &mut BranchPopoutController,
    ) -> bool {
        if repo_version != self.repo_version {
            return false;
        }
        branch_popout.sync_commit_indices(commits);
        branch_popout.invalidate_bounds();
        self.has_more_commits = has_more;
        self.loading_more_commits = false;
        true
    }

    pub(in crate::screens::repository) fn select_commit(
        &mut self,
        idx: usize,
        selection: &mut SelectionState,
        branch_popout: &mut BranchPopoutController,
    ) -> CenterAction {
        branch_popout.close();
        selection.select_commit(idx);
        CenterAction::CommitSelected(idx)
    }

    pub(in crate::screens::repository) fn scroll_to_commit(
        &self,
        idx: usize,
    ) -> Option<iced::Task<Message>> {
        use crate::widgets::ROW_H;

        let commit_top = idx as f32 * ROW_H;
        let commit_bottom = commit_top + ROW_H;
        let viewport_top = self.center_list.offset_y();
        let viewport_bottom = viewport_top + self.center_list.viewport_h();

        if commit_top >= viewport_top && commit_bottom <= viewport_bottom {
            return None;
        }

        let target_y = if commit_top < viewport_top {
            commit_top
        } else {
            commit_bottom - self.center_list.viewport_h()
        };

        Some(iced::widget::operation::scroll_to(
            center_list_scroll_id(),
            scrollable::AbsoluteOffset {
                x: 0.0,
                y: target_y.max(0.0),
            },
        ))
    }

    pub(in crate::screens::repository) fn handle_branch_label_pressed(
        branch_popout: &mut BranchPopoutController,
        branch_name: String,
        is_remote_only: bool,
    ) -> CenterAction {
        match branch_popout.handle_branch_pressed(branch_name, is_remote_only) {
            BranchPressOutcome::None => CenterAction::None,
            BranchPressOutcome::CheckoutLocal(name) => CenterAction::BranchLabelPressed(name),
            BranchPressOutcome::CheckoutRemote(name) => {
                CenterAction::RemoteBranchLabelPressed(name)
            }
        }
    }

    pub(in crate::screens::repository) fn load_more_commits_if_needed(
        &mut self,
        data: &RepositoryData,
    ) -> CenterAction {
        if self.loading_more_commits
            || !self.has_more_commits
            || !self
                .center_list
                .is_near_bottom(data.snapshot.commits().len())
        {
            return CenterAction::None;
        }

        self.loading_more_commits = true;
        CenterAction::LoadMoreCommitsRequested
    }

    pub(in crate::screens::repository) fn measure_branch_popout_bounds() -> iced::Task<Message> {
        use crate::widgets::branch_label::{
            branch_popout_content_id, branch_popout_panel_id, branch_popout_trigger_id,
        };
        use iced::widget::selector;

        fn extract(target: Option<selector::Target>) -> Option<iced::Rectangle> {
            target.and_then(|t| t.visible_bounds())
        }

        iced::Task::batch([
            selector::find(selector::id(branch_popout_trigger_id())).map(|t| {
                Message::repo(super::super::super::RepositoryMessage::Center(
                    super::super::super::panel_messages::CenterAction::BranchLabelTriggerBounds(
                        extract(t),
                    ),
                ))
            }),
            selector::find(selector::id(branch_popout_panel_id())).map(|t| {
                Message::repo(super::super::super::RepositoryMessage::Center(
                    super::super::super::panel_messages::CenterAction::BranchLabelPopoutBounds(
                        extract(t),
                    ),
                ))
            }),
            selector::find(selector::id(branch_popout_content_id())).map(|t| {
                Message::repo(super::super::super::RepositoryMessage::Center(
                    super::super::super::panel_messages::CenterAction::BranchLabelContentBounds(
                        extract(t),
                    ),
                ))
            }),
        ])
    }

    pub(in crate::screens::repository) fn restore_center_list_scroll(&self) -> iced::Task<Message> {
        iced::widget::operation::scroll_to(
            center_list_scroll_id(),
            scrollable::AbsoluteOffset {
                x: 0.0,
                y: self.center_list.offset_y(),
            },
        )
    }

    fn center_view_model<'a>(
        &'a self,
        data: &'a RepositoryData,
        selection: &SelectionState,
        dirty_commit_message: &'a text_editor::Content,
        commit_search: Option<&'a CommitSearch>,
        active_popout: Option<BranchPopoutState>,
    ) -> CenterViewModel<'a> {
        CenterViewModel {
            commits: data.snapshot.commits(),
            commit_presentations: data.snapshot.commit_presentations(),
            commit_diff_states: data.cache.states(),
            graph_rows: data.snapshot.graph_rows(),
            graph_revision: data.snapshot.graph_revision(),
            selected_indices: selection.selected_indices(),
            num_lanes: data.snapshot.num_lanes(),
            center_list_offset_y: self.center_list.offset_y(),
            center_list_viewport_h: self.center_list.viewport_h(),
            branch_popout: active_popout,
            dirty_commit_message,
            commit_search,
        }
    }

    pub(in crate::screens::repository) fn view<'a>(
        &'a self,
        data: &'a RepositoryData,
        selection: &SelectionState,
        dirty_commit_message: &'a text_editor::Content,
        commit_search: Option<&'a CommitSearch>,
        branch_popout: &BranchPopoutController,
    ) -> Element<'a, Message> {
        center_view::center_panel_view(self.center_view_model(
            data,
            selection,
            dirty_commit_message,
            commit_search,
            branch_popout.active().cloned(),
        ))
    }
}
