//! Overlay dialogs shown over the top toolbar (confirmations, text prompts)
//! plus the AddRemote side panel. The [`OverlayManager`] enforces that at most
//! one dialog is active at any moment via [`Option<ActiveDialog>`] — there is
//! no way to "smuggle" a second dialog through mutual-exclusion convention.

use std::sync::Arc;

use iced::{Element, Task};

use crate::{
    core::TabId,
    message::Message,
    services::{presenter::Presenter, SharedRepositoryGateway},
    theme,
};

use super::state::RepositoryData;
use super::{panel_messages::OverlayPanelAction, RepositoryMessage};

pub(crate) mod add_remote;
pub(crate) mod cherry_pick_confirm;
pub(crate) mod conflict_checkout;
pub(crate) mod create_branch;
pub(crate) mod create_tag;
pub(crate) mod create_worktree;
pub(crate) mod delete_branch;
pub(crate) mod delete_tag;
pub(crate) mod discard;
pub(crate) mod force_push;
pub(crate) mod push_behind;
pub(crate) mod remove_worktree;
pub(crate) mod rename_branch;
pub(crate) mod set_upstream;
pub(crate) mod stash_delete;
pub(crate) mod validation;
pub(crate) mod widgets;

pub(crate) use widgets::{CREATE_BUTTON, OVERLAY_ENTER_OFFSET};

const OVERLAY_SLIDE_SPEED_PX_PER_MS: f32 = 6.25;

/// Exactly one dialog is open at a time; this enum carries the active state.
/// AddRemote uses a distinct animation timeline (its own `Instant`); everything
/// else rides on [`OverlayManager::slide_offset`].
pub(crate) enum ActiveDialog {
    ConflictCheckout(conflict_checkout::State),
    DeleteBranch(delete_branch::State),
    StashDelete(stash_delete::State),
    RenameBranch(rename_branch::State),
    CreateBranchHere(create_branch::State),
    Discard(discard::State),
    AddRemote(add_remote::State),
    CreateWorktree(create_worktree::State),
    SetUpstream(set_upstream::State),
    PushBehind(push_behind::State),
    ForcePush(#[allow(dead_code)] force_push::State),
    CreateTagHere(create_tag::State),
    DeleteTag(delete_tag::State),
    CherryPick(cherry_pick_confirm::State),
    RemoveWorktree(remove_worktree::State),
}

impl ActiveDialog {
    fn captures_text_input(&self) -> bool {
        matches!(
            self,
            ActiveDialog::ConflictCheckout(_)
                | ActiveDialog::RenameBranch(_)
                | ActiveDialog::CreateBranchHere(_)
                | ActiveDialog::CreateTagHere(_)
                | ActiveDialog::AddRemote(_)
                | ActiveDialog::CreateWorktree(_)
                | ActiveDialog::SetUpstream(_)
        )
    }
}

/// Dependencies needed to spawn git tasks from dialog confirmations.
pub(crate) struct DialogCtx {
    pub repository: SharedRepositoryGateway,
    pub primary_repository: SharedRepositoryGateway,
    pub presenter: Arc<dyn Presenter>,
    pub tab_id: TabId,
    pub active_path: std::path::PathBuf,
}

/// Outcome of dispatching an [`OverlayPanelAction`]. The screen applies any
/// remaining side-effects (focus change, list-scroll restore) so the overlay
/// layer never reaches across into sibling panels.
pub(crate) enum DialogDispatch {
    /// Plain task — may be `Task::none()` for local-only state changes.
    Task(Task<Message>),
    /// Cancel path: caller must reset `focused_panel` to Center.
    CancelCloseFocus,
    /// Opened a new dialog that requires restoring center-list scroll.
    RestoreCenterListScroll,
}

pub(crate) struct OverlayManager {
    active: Option<ActiveDialog>,
    slide_offset: f32,
}

impl OverlayManager {
    pub(crate) fn new() -> Self {
        Self {
            active: None,
            slide_offset: 0.0,
        }
    }

    pub(crate) fn active(&self) -> Option<&ActiveDialog> {
        self.active.as_ref()
    }

    /// Open a new dialog, replacing any previously-active dialog. Sets the
    /// slide offset appropriate for the dialog's animation timeline.
    pub(crate) fn open(&mut self, dialog: ActiveDialog) {
        self.slide_offset = match &dialog {
            ActiveDialog::AddRemote(_) => 0.0, // uses its own animation
            ActiveDialog::CreateWorktree(_) => 0.0, // uses its own animation
            _ => OVERLAY_ENTER_OFFSET,
        };
        self.active = Some(dialog);
    }

    /// Close the active dialog (if any) and reset animation state.
    pub(crate) fn close(&mut self) {
        self.active = None;
        self.slide_offset = 0.0;
    }

    pub(crate) fn is_text_input_active(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|d| d.captures_text_input())
    }

    pub(crate) fn is_animating(&self) -> bool {
        if self.slide_offset > 0.0 {
            return true;
        }
        if let Some(ActiveDialog::AddRemote(state)) = &self.active {
            if !state.is_animation_done() {
                return true;
            }
        }
        if let Some(ActiveDialog::CreateWorktree(state)) = &self.active {
            if !state.is_animation_done() {
                return true;
            }
        }
        false
    }

    pub(crate) fn as_add_remote_mut(&mut self) -> Option<&mut add_remote::State> {
        match &mut self.active {
            Some(ActiveDialog::AddRemote(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn as_create_worktree_mut(&mut self) -> Option<&mut create_worktree::State> {
        match &mut self.active {
            Some(ActiveDialog::CreateWorktree(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn as_set_upstream_mut(&mut self) -> Option<&mut set_upstream::State> {
        match &mut self.active {
            Some(ActiveDialog::SetUpstream(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn tick_animation(&mut self, delta_ms: f32) -> Option<Task<Message>> {
        // AddRemote runs on its own clock — check before the shared decay.
        if let Some(ActiveDialog::AddRemote(state)) = &self.active {
            if state.is_animation_done() {
                if state.direction == add_remote::Direction::Opening && state.needs_focus {
                    if let Some(ActiveDialog::AddRemote(s)) = self.active.as_mut() {
                        s.needs_focus = false;
                    }
                    return Some(iced::widget::operation::focus(add_remote::input_id()));
                }
                if state.direction == add_remote::Direction::Closing {
                    self.active = None;
                    return None;
                }
            }
        }

        // CreateWorktree runs on its own clock — parallel to AddRemote.
        if let Some(ActiveDialog::CreateWorktree(state)) = &self.active {
            if state.is_animation_done() {
                if state.direction == create_worktree::Direction::Opening && state.needs_focus {
                    if let Some(ActiveDialog::CreateWorktree(s)) = self.active.as_mut() {
                        s.needs_focus = false;
                    }
                    return Some(iced::widget::operation::focus(create_worktree::input_id()));
                }
                if state.direction == create_worktree::Direction::Closing {
                    self.active = None;
                    return None;
                }
            }
        }

        let was_animating = self.slide_offset > 0.0;
        self.slide_offset = (self.slide_offset - OVERLAY_SLIDE_SPEED_PX_PER_MS * delta_ms).max(0.0);
        let still_animating = self.slide_offset > 0.0;

        if was_animating && !still_animating {
            match self.active.as_mut() {
                Some(ActiveDialog::CreateBranchHere(state)) if state.needs_focus => {
                    state.needs_focus = false;
                    return Some(iced::widget::operation::focus(create_branch::input_id()));
                }
                Some(ActiveDialog::RenameBranch(state)) if state.needs_focus => {
                    state.needs_focus = false;
                    return Some(iced::widget::operation::focus(rename_branch::input_id()));
                }
                Some(ActiveDialog::SetUpstream(state)) if state.needs_focus => {
                    state.needs_focus = false;
                    return Some(iced::widget::operation::focus(set_upstream::input_id()));
                }
                Some(ActiveDialog::CreateTagHere(state)) if state.needs_focus => {
                    state.needs_focus = false;
                    return Some(iced::widget::operation::focus(create_tag::input_id()));
                }
                _ => {}
            }
        }

        None
    }

    /// Confirm the discard dialog (called from the DetailAction path). Returns
    /// the task spawned for the git discard, or `Task::none()` if no discard
    /// dialog is currently active.
    pub(crate) fn confirm_discard(&mut self, ctx: DialogCtx) -> Task<Message> {
        let Some(ActiveDialog::Discard(state)) = &self.active else {
            return Task::none();
        };
        let target = state.target.clone();
        self.close();
        spawn_discard_task(target, ctx)
    }

    /// Checks whether deleting a branch is safe (can't delete HEAD locally).
    pub(crate) fn can_confirm_branch_delete(
        &self,
        current_branch: &str,
        branch_name: &str,
        is_remote: bool,
    ) -> bool {
        validation::can_confirm_branch_delete(current_branch, branch_name, is_remote)
    }

    /// Wrap the main toolbar with the currently-active overlay (or pass through
    /// unchanged when nothing is active).
    pub(crate) fn toolbar_overlay<'a>(
        &'a self,
        main_bar: Element<'a, Message>,
        data: &RepositoryData,
    ) -> Element<'a, Message> {
        let Some(active) = self.active.as_ref() else {
            return main_bar;
        };
        let slide = self.slide_offset;

        let overlay_elem = match active {
            ActiveDialog::ConflictCheckout(state) => conflict_checkout::view(state, slide),
            ActiveDialog::DeleteBranch(state) => delete_branch::view(state, slide),
            ActiveDialog::StashDelete(state) => stash_delete::view(state, slide),
            ActiveDialog::RenameBranch(state) => rename_branch::view(state, slide),
            ActiveDialog::CreateBranchHere(state) => {
                let existing = existing_branches(data);
                create_branch::view(state, slide, &existing)
            }
            ActiveDialog::Discard(state) => discard::view(state, slide),
            ActiveDialog::SetUpstream(state) => set_upstream::view(state, slide),
            ActiveDialog::PushBehind(state) => push_behind::view(state, slide),
            ActiveDialog::ForcePush(_) => force_push::view(slide),
            ActiveDialog::CreateTagHere(state) => create_tag::view(state, slide),
            ActiveDialog::DeleteTag(state) => delete_tag::view(state, slide),
            ActiveDialog::CherryPick(_) => cherry_pick_confirm::view(slide),
            ActiveDialog::RemoveWorktree(state) => remove_worktree::view(state, slide),
            // AddRemote and CreateWorktree render as side panels via overlay_layers(),
            // not in the toolbar row.
            ActiveDialog::AddRemote(_) => return main_bar,
            ActiveDialog::CreateWorktree(_) => return main_bar,
        };

        bar_with_overlay(main_bar, overlay_elem, slide)
    }

    /// Any additional overlay layers rendered on top of the main view (e.g.
    /// the AddRemote side panel with its click-anywhere backdrop).
    pub(crate) fn overlay_layers<'a>(&'a self, sidebar_width: f32) -> Vec<Element<'a, Message>> {
        match &self.active {
            Some(ActiveDialog::AddRemote(state)) => {
                add_remote::overlay_layers(state, sidebar_width)
            }
            Some(ActiveDialog::CreateWorktree(state)) => {
                create_worktree::overlay_layers(state, sidebar_width)
            }
            _ => Vec::new(),
        }
    }

    /// Drives a single [`OverlayPanelAction`] through the active dialog. The
    /// screen's overlay handler becomes a two-line wrapper around this call.
    pub(crate) fn dispatch(
        &mut self,
        action: OverlayPanelAction,
        ctx: DialogCtx,
    ) -> DialogDispatch {
        match action {
            OverlayPanelAction::ConflictNewBranchInput(value) => {
                if let Some(ActiveDialog::ConflictCheckout(state)) = self.active.as_mut() {
                    state.new_branch_input = value;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::ConflictCreateBranch => {
                let Some(ActiveDialog::ConflictCheckout(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let new_name = state.new_branch_input.trim().to_string();
                if new_name.is_empty() {
                    return DialogDispatch::Task(Task::none());
                }
                let remote_ref = state.remote_ref.clone();
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .create_branch_from_remote(&new_name, &remote_ref)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| Message::tab(tab_id, RepositoryMessage::RepoLoaded(result)),
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::ConflictResetLocal => {
                let Some(ActiveDialog::ConflictCheckout(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let branch_name = state.branch_name.clone();
                let remote_ref = state.remote_ref.clone();
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .reset_branch_to_remote(&branch_name, &remote_ref)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| Message::tab(tab_id, RepositoryMessage::RepoLoaded(result)),
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::ConflictCancel => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::BranchDeleteConfirmed => {
                let Some(ActiveDialog::DeleteBranch(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let branch_name = state.branch_name.clone();
                let is_remote = state.is_remote;
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let delete_branch_name = branch_name.clone();
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .delete_branch(&delete_branch_name, is_remote)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::BranchDeleted {
                                branch_name: branch_name.clone(),
                                is_remote,
                                result,
                            },
                        )
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::BranchDeleteAllConfirmed => {
                let Some(ActiveDialog::DeleteBranch(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let branch_name = state.branch_name.clone();
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let delete_branch_name = branch_name.clone();
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .delete_branch_all(&delete_branch_name)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::BranchDeleted {
                                branch_name: branch_name.clone(),
                                is_remote: false,
                                result,
                            },
                        )
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::BranchDeleteCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::StashDeleteConfirmed => {
                let Some(ActiveDialog::StashDelete(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let stash_index = state.stash_index;
                self.close();
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .drop_stash(stash_index)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(tab_id, RepositoryMessage::DirtyIndexChanged(result))
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::StashDeleteCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::BranchRenameConfirmed => {
                let Some(ActiveDialog::RenameBranch(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let old_name = state.branch_name.clone();
                let new_name = state.new_branch_input.trim().to_string();
                let is_remote = state.is_remote;
                if new_name.is_empty() || new_name == old_name {
                    self.close();
                    return DialogDispatch::Task(Task::none());
                }
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let old_name_clone = old_name.clone();
                let new_name_clone = new_name.clone();
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .rename_branch(&old_name_clone, &new_name_clone, is_remote)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::BranchRenamed {
                                old_name: old_name.clone(),
                                new_name: new_name.clone(),
                                is_remote,
                                result,
                            },
                        )
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::BranchRenameCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::RenameNewBranchInput(value) => {
                if let Some(ActiveDialog::RenameBranch(state)) = self.active.as_mut() {
                    state.new_branch_input = value;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::CreateBranchHereConfirmed => {
                let Some(ActiveDialog::CreateBranchHere(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let branch_name = state.branch_name_input.trim().to_string();
                let commit_hash = state.commit_hash.clone();
                if branch_name.is_empty() {
                    self.close();
                    return DialogDispatch::Task(Task::none());
                }
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let branch_name_clone = branch_name.clone();
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .create_branch_at_commit(&branch_name_clone, &commit_hash)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::BranchCreated {
                                branch_name: branch_name.clone(),
                                result,
                            },
                        )
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::CreateBranchHereCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::CreateBranchHereInput(value) => {
                if let Some(ActiveDialog::CreateBranchHere(state)) = self.active.as_mut() {
                    state.branch_name_input = value;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::DiscardConfirmed => {
                let Some(ActiveDialog::Discard(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let target = state.target.clone();
                // Note: legacy behaviour didn't clear the dialog here (the
                // snapshot reload does). Preserve that — keep the overlay up
                // until the result arrives.
                DialogDispatch::Task(spawn_discard_task(target, ctx))
            }
            OverlayPanelAction::DiscardCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::AddRemoteOpen => {
                self.open(ActiveDialog::AddRemote(add_remote::State::new()));
                DialogDispatch::RestoreCenterListScroll
            }
            OverlayPanelAction::AddRemoteClose => {
                if let Some(ActiveDialog::AddRemote(state)) = self.active.as_mut() {
                    state.start_close();
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::AddRemoteNameChanged(value) => {
                if let Some(ActiveDialog::AddRemote(state)) = self.active.as_mut() {
                    state.name = value;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::AddRemotePullUrlChanged(value) => {
                if let Some(ActiveDialog::AddRemote(state)) = self.active.as_mut() {
                    state.pull_url = value;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::AddRemotePushUrlChanged(value) => {
                if let Some(ActiveDialog::AddRemote(state)) = self.active.as_mut() {
                    state.push_url = value;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::AddRemoteConfirmed => {
                let Some(ActiveDialog::AddRemote(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let name = state.name.trim().to_string();
                let pull_url = state.pull_url.trim().to_string();
                let push_url = state.push_url.trim().to_string();
                if name.is_empty() || pull_url.is_empty() {
                    return DialogDispatch::Task(Task::none());
                }
                if let Some(ActiveDialog::AddRemote(state)) = self.active.as_mut() {
                    state.submitting = true;
                }
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .add_remote(&name, &pull_url, &push_url)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| Message::tab(tab_id, RepositoryMessage::RemoteAdded(result)),
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::SetUpstreamInput(value) => {
                if let Some(ActiveDialog::SetUpstream(state)) = self.active.as_mut() {
                    state.new_branch_input = value;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::SetUpstreamConfirmed => {
                let Some(ActiveDialog::SetUpstream(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let remote_branch = state.new_branch_input.trim().to_string();
                if remote_branch.is_empty() {
                    return DialogDispatch::Task(Task::none());
                }
                let remote_name = state.remote_name.clone();
                if let Some(ActiveDialog::SetUpstream(state)) = self.active.as_mut() {
                    state.submitting = true;
                }
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .push_and_set_upstream(&remote_name, &remote_branch)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(tab_id, RepositoryMessage::SetUpstreamPushCompleted(result))
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::SetUpstreamCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::PushBehindPullRequested => {
                self.close();
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .pull_current_branch()
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| Message::tab(tab_id, RepositoryMessage::PullCompleted(result)),
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::PushBehindForcePushRequested => {
                let taken = match self.active.take() {
                    Some(ActiveDialog::PushBehind(s)) => Some(s),
                    other => {
                        self.active = other;
                        None
                    }
                };
                self.slide_offset = 0.0;
                if let Some(push_behind) = taken {
                    self.open(ActiveDialog::ForcePush(force_push::State {
                        branch_name: push_behind.branch_name,
                        remote_name: push_behind.remote_name,
                    }));
                }
                DialogDispatch::RestoreCenterListScroll
            }
            OverlayPanelAction::PushBehindCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::ForcePushConfirmed => {
                let Some(ActiveDialog::ForcePush(_)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                self.close();
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .force_push_current_branch()
                            .map(|o| presenter.project_push(o))
                    }),
                    move |result| {
                        Message::tab(tab_id, RepositoryMessage::ForcePushCompleted(result))
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::ForcePushCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::CreateTagHereInput(value) => {
                if let Some(ActiveDialog::CreateTagHere(state)) = self.active.as_mut() {
                    state.tag_name_input = value;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::CreateTagHereConfirmed => {
                let Some(ActiveDialog::CreateTagHere(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let tag_name = state.tag_name_input.trim().to_string();
                let commit_hash = state.commit_hash.clone();
                if tag_name.is_empty() {
                    self.close();
                    return DialogDispatch::Task(Task::none());
                }
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let tag_clone = tag_name.clone();
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .create_tag(&tag_clone, &commit_hash)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::TagCreated {
                                tag_name: tag_name.clone(),
                                result,
                            },
                        )
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::CreateTagHereCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::DeleteTagConfirmed => {
                let Some(ActiveDialog::DeleteTag(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let tag_name = state.tag_name.clone();
                let DialogCtx {
                    repository,
                    presenter,
                    tab_id,
                    ..
                } = ctx;
                let tag_clone = tag_name.clone();
                let task = Task::perform(
                    super::gateway_work(move || {
                        repository
                            .delete_tag(&tag_clone)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::TagDeleted {
                                tag_name: tag_name.clone(),
                                result,
                            },
                        )
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::DeleteTagCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::CherryPickImmediateConfirmed => {
                let Some(ActiveDialog::CherryPick(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let commit_hash = state.commit_hash.clone();
                self.close();
                DialogDispatch::Task(spawn_cherry_pick_task(commit_hash, true, ctx))
            }
            OverlayPanelAction::CherryPickStagedConfirmed => {
                let Some(ActiveDialog::CherryPick(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                let commit_hash = state.commit_hash.clone();
                self.close();
                DialogDispatch::Task(spawn_cherry_pick_task(commit_hash, false, ctx))
            }
            OverlayPanelAction::CherryPickCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::CreateWorktreeOpen { available_refs, default_dir_prefix } => {
                self.open(ActiveDialog::CreateWorktree(
                    create_worktree::State::new(available_refs, default_dir_prefix),
                ));
                DialogDispatch::RestoreCenterListScroll
            }
            OverlayPanelAction::CreateWorktreeClose => {
                if let Some(ActiveDialog::CreateWorktree(state)) = self.active.as_mut() {
                    state.start_close();
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::CreateWorktreeReferenceChanged(choice) => {
                if let Some(state) = self.as_create_worktree_mut() {
                    state.set_reference(choice);
                    state.dropdown_open = false;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::CreateWorktreeDropdownToggled => {
                if let Some(state) = self.as_create_worktree_mut() {
                    state.dropdown_open = !state.dropdown_open;
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::CreateWorktreeBranchNameChanged(value) => {
                if let Some(state) = self.as_create_worktree_mut() {
                    state.set_branch_name(value);
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::CreateWorktreeWorkingDirChanged(value) => {
                if let Some(state) = self.as_create_worktree_mut() {
                    state.set_working_dir(value);
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::CreateWorktreeBrowseRequested => {
                let task = Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose worktree directory")
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    |chosen| {
                        Message::repo(RepositoryMessage::OverlayPanel(
                            OverlayPanelAction::CreateWorktreeBrowseResolved(chosen),
                        ))
                    },
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::CreateWorktreeBrowseResolved(chosen) => {
                if let (Some(state), Some(path)) = (self.as_create_worktree_mut(), chosen) {
                    state.set_working_dir(path.to_string_lossy().to_string());
                }
                DialogDispatch::Task(Task::none())
            }
            OverlayPanelAction::CreateWorktreeConfirmed => {
                let Some(state) = self.as_create_worktree_mut() else {
                    return DialogDispatch::Task(Task::none());
                };
                if !state.can_submit() {
                    return DialogDispatch::Task(Task::none());
                }
                let ref_git = state.reference.as_ref().map(|r| r.git_ref()).unwrap_or_default();
                let branch_name = state.branch_name.trim().to_string();
                let working_dir = std::path::PathBuf::from(state.working_dir.trim());
                state.submitting = true;
                state.error = None;

                let DialogCtx { primary_repository, presenter, tab_id, .. } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        primary_repository
                            .add_worktree(working_dir, Some(branch_name), ref_git)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| Message::tab(tab_id, RepositoryMessage::WorktreeCreated(result)),
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::WorktreeRemoveRequested { path, branch_name, is_active } => {
                self.open(ActiveDialog::RemoveWorktree(remove_worktree::State {
                    path,
                    branch_name,
                    is_active,
                }));
                DialogDispatch::RestoreCenterListScroll
            }
            OverlayPanelAction::WorktreeRemoveConfirmed => {
                let Some(ActiveDialog::RemoveWorktree(state)) = self.active.as_ref() else {
                    return DialogDispatch::Task(Task::none());
                };
                // Live check against DialogCtx.active_path (captured at dispatch
                // time; the snapshotted state.is_active would be stale if focus
                // swapped after the dialog opened).
                if ctx.active_path == state.path {
                    let branch_name = state.branch_name.clone();
                    self.close();
                    return DialogDispatch::Task(iced::Task::done(Message::show_toast(
                        crate::toast::ToastData::error(
                            "Cannot remove focused worktree",
                            format!("Switch away from '{branch_name}' first."),
                        ),
                    )));
                }
                let path = state.path.clone();
                self.close();
                let DialogCtx { primary_repository, presenter, tab_id, .. } = ctx;
                let task = Task::perform(
                    super::gateway_work(move || {
                        primary_repository
                            .remove_worktree(path)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| Message::tab(tab_id, RepositoryMessage::WorktreeRemoved(result)),
                );
                DialogDispatch::Task(task)
            }
            OverlayPanelAction::WorktreeRemoveCanceled => {
                self.close();
                DialogDispatch::CancelCloseFocus
            }
            OverlayPanelAction::None => DialogDispatch::Task(Task::none()),
        }
    }
}

fn spawn_discard_task(target: discard::Target, ctx: DialogCtx) -> Task<Message> {
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    Task::perform(
        super::gateway_work(move || {
            let snapshot = match target {
                discard::Target::All => repository.discard_all_dirty_changes(),
                discard::Target::File(path) => repository.discard_file(&path),
            }?;
            Ok(presenter.project_loaded(snapshot))
        }),
        move |result| Message::tab(tab_id, RepositoryMessage::DirtyIndexChanged(result)),
    )
}

fn spawn_cherry_pick_task(commit_hash: String, commit_now: bool, ctx: DialogCtx) -> Task<Message> {
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    Task::perform(
        super::gateway_work(move || {
            repository
                .cherry_pick_commit(commit_hash, commit_now)
                .map(|o| presenter.project_cherry_pick(o))
        }),
        move |result| Message::tab(tab_id, RepositoryMessage::CherryPickCompleted(result)),
    )
}

fn existing_branches(data: &RepositoryData) -> Vec<String> {
    data.snapshot.sidebar_sections()
        .iter()
        .flat_map(|section| section.branches.iter().map(|b| b.name.clone()))
        .collect()
}

fn bar_with_overlay<'a>(
    main_bar: Element<'a, Message>,
    overlay_elem: Element<'a, Message>,
    slide_offset: f32,
) -> Element<'a, Message> {
    use iced::{
        widget::{container, Stack},
        Length,
    };

    if slide_offset > 0.0 {
        Stack::with_children(vec![main_bar, overlay_elem])
            .width(Length::Fill)
            .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32))
            .into()
    } else {
        container(overlay_elem)
            .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32))
            .width(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_has_no_active_dialog_and_no_slide() {
        let m = OverlayManager::new();
        assert!(m.active().is_none());
        assert_eq!(m.slide_offset, 0.0);
        assert!(!m.is_animating());
        assert!(!m.is_text_input_active());
    }

    #[test]
    fn is_text_input_active_true_when_rename_branch_open() {
        let mut m = OverlayManager::new();
        m.open(ActiveDialog::RenameBranch(rename_branch::State {
            branch_name: "old".into(),
            is_remote: false,
            new_branch_input: String::new(),
            needs_focus: false,
            remote_name: None,
        }));
        assert!(m.is_text_input_active());
    }

    #[test]
    fn is_text_input_active_true_when_create_branch_open() {
        let mut m = OverlayManager::new();
        m.open(ActiveDialog::CreateBranchHere(create_branch::State {
            commit_idx: 0,
            commit_hash: "deadbeef".into(),
            branch_name_input: String::new(),
            needs_focus: false,
        }));
        assert!(m.is_text_input_active());
    }

    #[test]
    fn is_text_input_active_false_for_confirmation_only_dialogs() {
        let mut m = OverlayManager::new();
        m.open(ActiveDialog::DeleteBranch(delete_branch::State {
            branch_name: "feature".into(),
            is_remote: false,
            has_remote: false,
            remote_name: None,
        }));
        assert!(!m.is_text_input_active());

        m = OverlayManager::new();
        m.open(ActiveDialog::StashDelete(stash_delete::State {
            stash_index: 0,
            display_name: "WIP".into(),
        }));
        assert!(!m.is_text_input_active());
    }

    #[test]
    fn is_animating_true_when_slide_offset_positive() {
        let m = OverlayManager::new();
        assert!(!m.is_animating());
        let mut m = OverlayManager::new();
        m.open(ActiveDialog::StashDelete(stash_delete::State {
            stash_index: 0,
            display_name: "WIP".into(),
        }));
        assert!(m.is_animating());
    }

    #[test]
    fn tick_animation_decays_slide_offset_toward_zero() {
        let mut m = OverlayManager::new();
        m.open(ActiveDialog::StashDelete(stash_delete::State {
            stash_index: 0,
            display_name: "WIP".into(),
        }));
        // slide_offset was OVERLAY_ENTER_OFFSET after open.
        let before = m.slide_offset;
        let task = m.tick_animation(1.0);
        assert!((before - m.slide_offset - OVERLAY_SLIDE_SPEED_PX_PER_MS).abs() < 1e-4);
        assert!(task.is_none());
    }

    #[test]
    fn tick_animation_clamps_to_zero_on_large_delta() {
        let mut m = OverlayManager::new();
        m.open(ActiveDialog::StashDelete(stash_delete::State {
            stash_index: 0,
            display_name: "WIP".into(),
        }));
        m.tick_animation(1000.0);
        assert_eq!(m.slide_offset, 0.0);
        assert!(!m.is_animating());
    }

    #[test]
    fn tick_animation_emits_focus_once_when_create_branch_needs_focus() {
        let mut m = OverlayManager::new();
        m.open(ActiveDialog::CreateBranchHere(create_branch::State {
            commit_idx: 0,
            commit_hash: "cafebabe".into(),
            branch_name_input: String::new(),
            needs_focus: true,
        }));
        // Drive slide_offset to 0 in one big tick → first tick emits focus.
        let first = m.tick_animation(1000.0);
        assert!(first.is_some(), "focus task expected on animation end");
        assert_eq!(m.slide_offset, 0.0);

        // Second tick finds slide already at 0 → no re-emit.
        let second = m.tick_animation(16.0);
        assert!(
            second.is_none(),
            "focus must not re-fire once animation already finished"
        );
    }
}
