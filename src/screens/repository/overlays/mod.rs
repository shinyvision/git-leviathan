//! Repository toolbar dialogs and side-panel overlays.

use std::sync::Arc;

use iced::{keyboard, Element, Task};

use crate::{
    core::TabId,
    message::Message,
    plugin::ui::context::{
        ToolbarDialogButtonContextSnapshot, ToolbarDialogContextSnapshot,
        ToolbarDialogControlContextSnapshot, ToolbarDialogDataContextSnapshot,
    },
    services::{presenter::Presenter, ModifyDeleteConflictChoice, SharedRepositoryGateway},
    theme,
};

use self::dialog::model::{Dialog, DialogButtonId, DialogControlId, DialogId, DialogOwner};
use super::panel_messages::OverlayPanelAction;
use super::state::{OperationCoordinator, RepositoryData};

pub(crate) mod add_remote;
pub(crate) mod cherry_pick_confirm;
pub(crate) mod conflict_checkout;
pub(crate) mod create_branch;
pub(crate) mod create_tag;
pub(crate) mod create_worktree;
pub(crate) mod delete_branch;
pub(crate) mod delete_tag;
pub(crate) mod dialog;
mod dialog_requests;
pub(crate) mod discard;
pub(crate) mod force_push;
pub(crate) mod modify_delete_conflict;
mod native_actions;
pub(crate) mod push_behind;
pub(crate) mod remove_worktree;
pub(crate) mod rename_branch;
pub(crate) mod revert_confirm;
pub(crate) mod set_upstream;
mod side_panel_dispatch;
pub(crate) mod stash_delete;
pub(crate) mod validation;
pub(crate) mod widgets;

pub(crate) use widgets::{CREATE_BUTTON, OVERLAY_ENTER_OFFSET};

const OVERLAY_SLIDE_SPEED_PX_PER_MS: f32 = 6.25;

const NATIVE_CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeButtonAction {
    Cancel,
    ForcePush,
    StashDelete,
    DeleteTag,
    CherryPick { commit_now: bool },
    Revert { commit_now: bool },
    RemoveWorktree,
    Discard,
    DeleteBranch,
    DeleteBranchAll,
    RenameBranch,
    CreateBranch,
    ConflictCreateBranch,
    ConflictResetLocal,
    SetUpstream,
    PushBehindPull,
    PushBehindForcePush,
    CreateTag,
    ResolveModifyDelete(ModifyDeleteConflictChoice),
}

pub(crate) enum SidePanelOverlay {
    AddRemote(add_remote::State),
    CreateWorktree(create_worktree::State),
}

impl SidePanelOverlay {
    fn captures_text_input(&self) -> bool {
        matches!(
            self,
            SidePanelOverlay::AddRemote(_) | SidePanelOverlay::CreateWorktree(_)
        )
    }
}

/// Dependencies needed to spawn git tasks from dialog confirmations.
pub(crate) struct DialogCtx<'a> {
    pub repository: SharedRepositoryGateway,
    pub primary_repository: SharedRepositoryGateway,
    pub presenter: Arc<dyn Presenter>,
    pub tab_id: TabId,
    pub active_path: std::path::PathBuf,
    pub sidebar_sections: &'a [crate::view_model::SidebarSection],
    pub operations: &'a mut OperationCoordinator,
}

/// Outcome of dispatching an [`OverlayPanelAction`]. The screen applies any
/// remaining side-effects (such as list-scroll restore) so the overlay layer
/// never reaches across into sibling panels.
pub(crate) enum DialogDispatch {
    /// Plain task — may be `Task::none()` for local-only state changes.
    Task(Task<Message>),
    /// Plugin-owned toolbar dialog button callback. The screen turns this into
    /// a top-level plugin message; it never calls the plugin host directly.
    PluginDialogButtonPressed {
        plugin_id: String,
        dialog_id: String,
        button_id: String,
    },
    /// Cancel path: caller closes the dialog without changing panel focus.
    CancelClosed,
    /// Opened a new dialog that requires restoring center-list scroll.
    RestoreCenterListScroll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogOwnerRoute {
    Native,
    Plugin,
}

enum DialogEvent {
    ButtonPressed {
        button_id: DialogButtonId,
    },
    InputChanged {
        control_id: DialogControlId,
        value: String,
    },
    DropdownToggled {
        control_id: DialogControlId,
    },
    DropdownChanged {
        control_id: DialogControlId,
        option_id: String,
    },
    Dismissed,
}

pub(crate) struct OverlayManager {
    toolbar_dialog: Option<Dialog>,
    side_panel: Option<SidePanelOverlay>,
    toolbar_slide_offset: f32,
}

fn dialog_owner_route(owner: &DialogOwner) -> DialogOwnerRoute {
    match owner {
        DialogOwner::Native(_) => DialogOwnerRoute::Native,
        DialogOwner::Plugin { .. } => DialogOwnerRoute::Plugin,
    }
}

impl OverlayManager {
    pub(crate) fn new() -> Self {
        Self {
            toolbar_dialog: None,
            side_panel: None,
            toolbar_slide_offset: 0.0,
        }
    }

    pub(crate) fn has_toolbar_dialog(&self) -> bool {
        self.toolbar_dialog.is_some()
    }

    pub(crate) fn open_toolbar_dialog(&mut self, dialog: Dialog) {
        self.side_panel = None;
        self.toolbar_slide_offset = OVERLAY_ENTER_OFFSET;
        self.toolbar_dialog = Some(dialog);
    }

    pub(crate) fn close_plugin_toolbar_dialog(&mut self, plugin_id: &str, dialog_id: &str) -> bool {
        if self.plugin_toolbar_dialog_matches(plugin_id, dialog_id) {
            self.close();
            true
        } else {
            false
        }
    }

    pub(crate) fn plugin_toolbar_dialog_matches(&self, plugin_id: &str, dialog_id: &str) -> bool {
        self.toolbar_dialog.as_ref().is_some_and(|dialog| {
            dialog.id.0 == dialog_id && dialog.owner.plugin_id() == Some(plugin_id)
        })
    }

    pub(crate) fn toolbar_dialog_control_focus_id(
        &self,
        dialog_id: &str,
        control_id: &str,
    ) -> Option<iced::widget::Id> {
        let dialog = self.toolbar_dialog.as_ref()?;
        if dialog.id.0 != dialog_id {
            return None;
        }
        let control = dialog
            .controls
            .iter()
            .find(|control| control.id.0 == control_id)?;
        control.text_input.as_ref()?;
        Some(dialog::view::input_id(dialog, &control.id))
    }

    pub(crate) fn toolbar_dialog_context_snapshot(&self) -> ToolbarDialogContextSnapshot {
        let Some(dialog) = self.toolbar_dialog.as_ref() else {
            return ToolbarDialogContextSnapshot::none();
        };

        let (owner, plugin_id) = match &dialog.owner {
            DialogOwner::Native(_) => ("native".to_string(), None),
            DialogOwner::Plugin { plugin_id } => ("plugin".to_string(), Some(plugin_id.clone())),
        };

        ToolbarDialogContextSnapshot {
            active: true,
            id: dialog.id.0.clone(),
            owner,
            plugin_id,
            data: dialog
                .data
                .iter()
                .map(|item| ToolbarDialogDataContextSnapshot {
                    id: item.id.clone(),
                    value: item.value.clone(),
                })
                .collect(),
            controls: dialog
                .controls
                .iter()
                .map(|control| {
                    if let Some(input) = control.text_input.as_ref() {
                        ToolbarDialogControlContextSnapshot {
                            id: control.id.0.clone(),
                            kind: "text_input".to_string(),
                            value: Some(input.value.clone()),
                        }
                    } else if let Some(dropdown) = control.dropdown.as_ref() {
                        ToolbarDialogControlContextSnapshot {
                            id: control.id.0.clone(),
                            kind: "dropdown".to_string(),
                            value: dropdown.selected_option_id.clone(),
                        }
                    } else {
                        ToolbarDialogControlContextSnapshot {
                            id: control.id.0.clone(),
                            kind: "unknown".to_string(),
                            value: None,
                        }
                    }
                })
                .collect(),
            buttons: dialog
                .buttons
                .iter()
                .map(|button| ToolbarDialogButtonContextSnapshot {
                    id: button.id.0.clone(),
                    text: button.text.clone(),
                    enabled: button.enabled,
                })
                .collect(),
        }
    }

    fn open_side_panel(&mut self, side_panel: SidePanelOverlay) {
        self.toolbar_dialog = None;
        self.toolbar_slide_offset = 0.0;
        self.side_panel = Some(side_panel);
    }

    pub(crate) fn close(&mut self) {
        self.toolbar_dialog = None;
        self.side_panel = None;
        self.toolbar_slide_offset = 0.0;
    }

    pub(crate) fn is_text_input_active(&self) -> bool {
        self.toolbar_dialog.as_ref().is_some_and(|dialog| {
            dialog
                .controls
                .iter()
                .any(|control| control.text_input.is_some())
        }) || self
            .side_panel
            .as_ref()
            .is_some_and(|panel| panel.captures_text_input())
    }

    pub(crate) fn toolbar_dialog_key_action(
        &self,
        key: &keyboard::Key,
    ) -> Option<OverlayPanelAction> {
        let dialog = self.toolbar_dialog.as_ref()?;
        let key_name = dialog_requests::dialog_key_name(key)?;
        if let Some(button) = dialog.enabled_button_for_key(&key_name) {
            return Some(OverlayPanelAction::DialogButtonPressed {
                dialog_id: dialog.id.clone(),
                button_id: button.id.clone(),
            });
        }
        if key_name == "esc" && dialog.dismissible {
            return Some(OverlayPanelAction::DialogDismissed {
                dialog_id: dialog.id.clone(),
            });
        }
        None
    }

    pub(crate) fn is_animating(&self) -> bool {
        if self.toolbar_slide_offset > 0.0 {
            return true;
        }
        if let Some(SidePanelOverlay::AddRemote(state)) = &self.side_panel {
            if !state.is_animation_done() {
                return true;
            }
        }
        if let Some(SidePanelOverlay::CreateWorktree(state)) = &self.side_panel {
            if !state.is_animation_done() {
                return true;
            }
        }
        false
    }

    pub(crate) fn as_add_remote_mut(&mut self) -> Option<&mut add_remote::State> {
        match &mut self.side_panel {
            Some(SidePanelOverlay::AddRemote(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn as_create_worktree_mut(&mut self) -> Option<&mut create_worktree::State> {
        match &mut self.side_panel {
            Some(SidePanelOverlay::CreateWorktree(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn is_add_remote_open(&self) -> bool {
        matches!(
            self.side_panel.as_ref(),
            Some(SidePanelOverlay::AddRemote(_))
        )
    }

    pub(crate) fn is_create_worktree_open(&self) -> bool {
        matches!(
            self.side_panel.as_ref(),
            Some(SidePanelOverlay::CreateWorktree(_))
        )
    }

    pub(crate) fn tick_animation(&mut self, delta_ms: f32) -> Option<Task<Message>> {
        if let Some(SidePanelOverlay::AddRemote(state)) = &self.side_panel {
            if state.is_animation_done() {
                if state.direction == add_remote::Direction::Opening && state.needs_focus {
                    if let Some(SidePanelOverlay::AddRemote(s)) = self.side_panel.as_mut() {
                        s.needs_focus = false;
                    }
                    return Some(iced::widget::operation::focus(add_remote::input_id()));
                }
                if state.direction == add_remote::Direction::Closing {
                    self.side_panel = None;
                    return None;
                }
            }
        }

        if let Some(SidePanelOverlay::CreateWorktree(state)) = &self.side_panel {
            if state.is_animation_done() {
                if state.direction == create_worktree::Direction::Opening && state.needs_focus {
                    if let Some(SidePanelOverlay::CreateWorktree(s)) = self.side_panel.as_mut() {
                        s.needs_focus = false;
                    }
                    return Some(iced::widget::operation::focus(create_worktree::input_id()));
                }
                if state.direction == create_worktree::Direction::Closing {
                    self.side_panel = None;
                    return None;
                }
            }
        }

        let was_animating = self.toolbar_slide_offset > 0.0;
        self.toolbar_slide_offset =
            (self.toolbar_slide_offset - OVERLAY_SLIDE_SPEED_PX_PER_MS * delta_ms).max(0.0);
        let still_animating = self.toolbar_slide_offset > 0.0;

        if was_animating && !still_animating {
            if let Some(dialog) = self.toolbar_dialog.as_mut() {
                if let Some(control_id) = dialog.autofocus.take() {
                    return Some(iced::widget::operation::focus(dialog::view::input_id(
                        dialog,
                        &control_id,
                    )));
                }
            }
        }

        None
    }

    pub(crate) fn is_discard_dialog_open(&self) -> bool {
        self.toolbar_dialog.as_ref().is_some_and(discard::is_dialog)
    }

    pub(crate) fn delete_tag_remote_names(&self) -> Vec<String> {
        self.toolbar_dialog
            .as_ref()
            .map(delete_tag::remote_names)
            .unwrap_or_default()
    }

    pub(crate) fn is_delete_tag_dialog_open(&self) -> bool {
        self.toolbar_dialog
            .as_ref()
            .is_some_and(delete_tag::is_dialog)
    }

    pub(crate) fn is_conflict_checkout_dialog_open(&self) -> bool {
        self.toolbar_dialog
            .as_ref()
            .is_some_and(conflict_checkout::is_dialog)
    }

    pub(crate) fn is_delete_branch_dialog_open(&self) -> bool {
        self.toolbar_dialog
            .as_ref()
            .is_some_and(delete_branch::is_dialog)
    }

    pub(crate) fn is_rename_branch_dialog_open(&self) -> bool {
        self.toolbar_dialog
            .as_ref()
            .is_some_and(rename_branch::is_dialog)
    }

    pub(crate) fn is_create_branch_dialog_open(&self) -> bool {
        self.toolbar_dialog
            .as_ref()
            .is_some_and(create_branch::is_dialog)
    }

    pub(crate) fn is_set_upstream_dialog_open(&self) -> bool {
        self.toolbar_dialog
            .as_ref()
            .is_some_and(set_upstream::is_dialog)
    }

    pub(crate) fn is_create_tag_dialog_open(&self) -> bool {
        self.toolbar_dialog
            .as_ref()
            .is_some_and(create_tag::is_dialog)
    }

    pub(crate) fn reset_set_upstream_submitting(&mut self) {
        if let Some(dialog) = self.toolbar_dialog.as_mut() {
            set_upstream::set_submitting(dialog, false);
            set_upstream::refresh_enabled(dialog);
        }
    }

    fn resolve_modify_delete_conflict(
        &mut self,
        choice: ModifyDeleteConflictChoice,
        ctx: DialogCtx<'_>,
    ) -> DialogDispatch {
        native_actions::resolve_modify_delete_conflict(self, choice, ctx)
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
        _data: &RepositoryData,
    ) -> Element<'a, Message> {
        if let Some(dialog) = self.toolbar_dialog.as_ref() {
            let slide = self.toolbar_slide_offset;
            let overlay_elem = dialog::view::view(dialog, slide);
            return bar_with_overlay(main_bar, overlay_elem, slide);
        }

        main_bar
    }

    pub(crate) fn overlay_layers<'a>(&'a self, sidebar_width: f32) -> Vec<Element<'a, Message>> {
        match &self.side_panel {
            Some(SidePanelOverlay::AddRemote(state)) => {
                add_remote::overlay_layers(state, sidebar_width)
            }
            Some(SidePanelOverlay::CreateWorktree(state)) => {
                create_worktree::overlay_layers(state, sidebar_width)
            }
            _ => Vec::new(),
        }
    }

    fn dispatch_toolbar_dialog_event_with_ctx(
        &mut self,
        dialog_id: DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        let route = self
            .toolbar_dialog
            .as_ref()
            .filter(|dialog| dialog.id == dialog_id)
            .map(|dialog| dialog_owner_route(&dialog.owner));

        match route {
            Some(DialogOwnerRoute::Native) => {
                self.dispatch_native_dialog_event(&dialog_id, event, ctx)
            }
            Some(DialogOwnerRoute::Plugin) => self.dispatch_plugin_dialog_event(&dialog_id, event),
            None => DialogDispatch::Task(Task::none()),
        }
    }

    fn dispatch_native_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        let DialogEvent::ButtonPressed { button_id } = &event else {
            if matches!(event, DialogEvent::Dismissed) {
                self.close();
                return DialogDispatch::CancelClosed;
            }
            return self.apply_local_dialog_event(dialog_id, event);
        };

        if !self.has_enabled_toolbar_dialog_button(dialog_id, button_id) {
            return self.apply_local_dialog_event(dialog_id, event);
        }

        match self.native_button_action(dialog_id, button_id) {
            Some(NativeButtonAction::Cancel) => {
                self.close();
                DialogDispatch::CancelClosed
            }
            Some(action) => self.run_native_button_action(action, ctx),
            None => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn native_button_action(
        &self,
        dialog_id: &DialogId,
        button_id: &DialogButtonId,
    ) -> Option<NativeButtonAction> {
        let id = dialog_id.0.as_str();
        let button = button_id.0.as_str();
        let action = match (id, button) {
            (force_push::DIALOG_ID, force_push::CONFIRM_BUTTON_ID) => NativeButtonAction::ForcePush,
            (stash_delete::DIALOG_ID, stash_delete::CONFIRM_BUTTON_ID) => {
                NativeButtonAction::StashDelete
            }
            (delete_tag::DIALOG_ID, delete_tag::CONFIRM_BUTTON_ID) => NativeButtonAction::DeleteTag,
            (cherry_pick_confirm::DIALOG_ID, cherry_pick_confirm::IMMEDIATE_BUTTON_ID) => {
                NativeButtonAction::CherryPick { commit_now: true }
            }
            (cherry_pick_confirm::DIALOG_ID, cherry_pick_confirm::STAGED_BUTTON_ID) => {
                NativeButtonAction::CherryPick { commit_now: false }
            }
            (revert_confirm::DIALOG_ID, revert_confirm::IMMEDIATE_BUTTON_ID) => {
                NativeButtonAction::Revert { commit_now: true }
            }
            (revert_confirm::DIALOG_ID, revert_confirm::IN_PLACE_BUTTON_ID) => {
                NativeButtonAction::Revert { commit_now: false }
            }
            (remove_worktree::DIALOG_ID, remove_worktree::CONFIRM_BUTTON_ID) => {
                NativeButtonAction::RemoveWorktree
            }
            (discard::DIALOG_ID, discard::CONFIRM_BUTTON_ID) => NativeButtonAction::Discard,
            (delete_branch::DIALOG_ID, delete_branch::CONFIRM_BUTTON_ID) => {
                NativeButtonAction::DeleteBranch
            }
            (delete_branch::DIALOG_ID, delete_branch::CONFIRM_ALL_BUTTON_ID) => {
                NativeButtonAction::DeleteBranchAll
            }
            (rename_branch::DIALOG_ID, rename_branch::CONFIRM_BUTTON_ID) => {
                NativeButtonAction::RenameBranch
            }
            (create_branch::DIALOG_ID, create_branch::CONFIRM_BUTTON_ID) => {
                NativeButtonAction::CreateBranch
            }
            (conflict_checkout::DIALOG_ID, conflict_checkout::CREATE_BUTTON_ID) => {
                NativeButtonAction::ConflictCreateBranch
            }
            (conflict_checkout::DIALOG_ID, conflict_checkout::RESET_BUTTON_ID) => {
                NativeButtonAction::ConflictResetLocal
            }
            (set_upstream::DIALOG_ID, set_upstream::CONFIRM_BUTTON_ID) => {
                NativeButtonAction::SetUpstream
            }
            (push_behind::DIALOG_ID, push_behind::PULL_BUTTON_ID) => {
                NativeButtonAction::PushBehindPull
            }
            (push_behind::DIALOG_ID, push_behind::FORCE_BUTTON_ID) => {
                NativeButtonAction::PushBehindForcePush
            }
            (create_tag::DIALOG_ID, create_tag::CONFIRM_BUTTON_ID) => NativeButtonAction::CreateTag,
            (
                modify_delete_conflict::DIALOG_ID,
                modify_delete_conflict::KEEP_MODIFIED_BUTTON_ID,
            ) => NativeButtonAction::ResolveModifyDelete(ModifyDeleteConflictChoice::KeepModified),
            (modify_delete_conflict::DIALOG_ID, modify_delete_conflict::DELETE_FILE_BUTTON_ID) => {
                NativeButtonAction::ResolveModifyDelete(ModifyDeleteConflictChoice::DeleteFile)
            }
            (modify_delete_conflict::DIALOG_ID, modify_delete_conflict::KEEP_BASE_BUTTON_ID) => {
                NativeButtonAction::ResolveModifyDelete(ModifyDeleteConflictChoice::KeepBase)
            }
            (_, NATIVE_CANCEL_BUTTON_ID) => NativeButtonAction::Cancel,
            _ => return None,
        };
        Some(action)
    }

    fn run_native_button_action(
        &mut self,
        action: NativeButtonAction,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match action {
            NativeButtonAction::Cancel => {
                self.close();
                DialogDispatch::CancelClosed
            }
            NativeButtonAction::ForcePush => native_actions::confirm_force_push(self, ctx),
            NativeButtonAction::StashDelete => native_actions::confirm_stash_delete(self, ctx),
            NativeButtonAction::DeleteTag => native_actions::confirm_delete_tag(self, ctx),
            NativeButtonAction::CherryPick { commit_now } => {
                native_actions::confirm_cherry_pick(self, commit_now, ctx)
            }
            NativeButtonAction::Revert { commit_now } => {
                native_actions::confirm_revert(self, commit_now, ctx)
            }
            NativeButtonAction::RemoveWorktree => {
                native_actions::confirm_remove_worktree(self, ctx)
            }
            NativeButtonAction::Discard => native_actions::confirm_discard_dialog(self, ctx),
            NativeButtonAction::DeleteBranch => native_actions::confirm_delete_branch(self, ctx),
            NativeButtonAction::DeleteBranchAll => {
                native_actions::confirm_delete_branch_all(self, ctx)
            }
            NativeButtonAction::RenameBranch => native_actions::confirm_rename_branch(self, ctx),
            NativeButtonAction::CreateBranch => native_actions::confirm_create_branch(self, ctx),
            NativeButtonAction::ConflictCreateBranch => {
                native_actions::confirm_conflict_create_branch(self, ctx)
            }
            NativeButtonAction::ConflictResetLocal => {
                native_actions::confirm_conflict_reset_local(self, ctx)
            }
            NativeButtonAction::SetUpstream => native_actions::confirm_set_upstream(self, ctx),
            NativeButtonAction::PushBehindPull => {
                native_actions::confirm_push_behind_pull(self, ctx)
            }
            NativeButtonAction::PushBehindForcePush => {
                native_actions::confirm_push_behind_force_push(self)
            }
            NativeButtonAction::CreateTag => native_actions::confirm_create_tag(self, ctx),
            NativeButtonAction::ResolveModifyDelete(choice) => {
                let Some(ctx) = ctx else {
                    return DialogDispatch::Task(Task::none());
                };
                self.resolve_modify_delete_conflict(choice, ctx)
            }
        }
    }

    fn has_enabled_toolbar_dialog_button(
        &self,
        dialog_id: &DialogId,
        button_id: &DialogButtonId,
    ) -> bool {
        self.toolbar_dialog
            .as_ref()
            .filter(|dialog| &dialog.id == dialog_id)
            .is_some_and(|dialog| {
                dialog
                    .buttons
                    .iter()
                    .any(|button| &button.id == button_id && button.enabled)
            })
    }

    fn dispatch_plugin_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
    ) -> DialogDispatch {
        let DialogEvent::ButtonPressed { button_id } = event else {
            return self.apply_local_dialog_event(dialog_id, event);
        };

        let Some(dialog) = self.toolbar_dialog.as_ref() else {
            return DialogDispatch::Task(Task::none());
        };
        if &dialog.id != dialog_id {
            return DialogDispatch::Task(Task::none());
        }
        let Some(plugin_id) = dialog.owner.plugin_id().map(ToOwned::to_owned) else {
            return DialogDispatch::Task(Task::none());
        };
        let closes_dialog = match dialog
            .buttons
            .iter()
            .find(|button| button.id == button_id && button.enabled)
        {
            Some(button) => button.closes_dialog,
            None => return DialogDispatch::Task(Task::none()),
        };

        let dialog_id = dialog_id.0.clone();
        let button_id = button_id.0;
        if closes_dialog {
            self.close();
        }

        DialogDispatch::PluginDialogButtonPressed {
            plugin_id,
            dialog_id,
            button_id,
        }
    }

    fn apply_local_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
    ) -> DialogDispatch {
        if matches!(event, DialogEvent::Dismissed) {
            let should_close = self
                .toolbar_dialog
                .as_ref()
                .is_some_and(|dialog| &dialog.id == dialog_id && dialog.dismissible);
            if should_close {
                self.close();
            }
            return DialogDispatch::Task(Task::none());
        }

        let Some(dialog) = self.toolbar_dialog.as_mut() else {
            return DialogDispatch::Task(Task::none());
        };
        if &dialog.id != dialog_id {
            return DialogDispatch::Task(Task::none());
        }

        match event {
            DialogEvent::ButtonPressed { button_id } => {
                let closes_dialog = match dialog
                    .buttons
                    .iter()
                    .find(|button| button.id == button_id && button.enabled)
                {
                    Some(button) => button.closes_dialog,
                    None => return DialogDispatch::Task(Task::none()),
                };
                if closes_dialog {
                    self.close();
                }
            }
            DialogEvent::InputChanged { control_id, value } => {
                if let Some(input) = dialog
                    .controls
                    .iter_mut()
                    .find(|control| control.id == control_id)
                    .and_then(|control| control.text_input.as_mut())
                {
                    input.value = value;
                }
            }
            DialogEvent::DropdownToggled { control_id } => {
                if let Some(dropdown) = dialog
                    .controls
                    .iter_mut()
                    .find(|control| control.id == control_id)
                    .and_then(|control| control.dropdown.as_mut())
                {
                    dropdown.open = !dropdown.open;
                }
            }
            DialogEvent::DropdownChanged {
                control_id,
                option_id,
            } => {
                if let Some(dropdown) = dialog
                    .controls
                    .iter_mut()
                    .find(|control| control.id == control_id)
                    .and_then(|control| control.dropdown.as_mut())
                {
                    if dropdown.options.iter().any(|option| option.id == option_id) {
                        dropdown.selected_option_id = Some(option_id);
                        dropdown.open = false;
                    }
                }
            }
            DialogEvent::Dismissed => {}
        }

        self.refresh_current_dialog();
        DialogDispatch::Task(Task::none())
    }

    fn refresh_current_dialog(&mut self) {
        let Some(dialog) = self.toolbar_dialog.as_mut() else {
            return;
        };
        if conflict_checkout::is_dialog(dialog) {
            conflict_checkout::refresh_enabled(dialog);
        } else if create_branch::is_dialog(dialog) {
            create_branch::refresh_enabled(dialog);
        } else if rename_branch::is_dialog(dialog) {
            rename_branch::refresh_enabled(dialog);
        } else if create_tag::is_dialog(dialog) {
            create_tag::refresh_enabled(dialog);
        } else if set_upstream::is_dialog(dialog) {
            set_upstream::refresh_enabled(dialog);
        }
    }

    /// Drives a single [`OverlayPanelAction`] through the active dialog. The
    /// screen's overlay handler becomes a two-line wrapper around this call.
    pub(crate) fn dispatch(
        &mut self,
        action: OverlayPanelAction,
        ctx: DialogCtx<'_>,
    ) -> DialogDispatch {
        match action {
            OverlayPanelAction::DialogButtonPressed {
                dialog_id,
                button_id,
            } => self.dispatch_toolbar_dialog_event_with_ctx(
                dialog_id,
                DialogEvent::ButtonPressed { button_id },
                Some(ctx),
            ),
            OverlayPanelAction::DialogInputChanged {
                dialog_id,
                control_id,
                value,
            } => self.dispatch_toolbar_dialog_event_with_ctx(
                dialog_id,
                DialogEvent::InputChanged { control_id, value },
                Some(ctx),
            ),
            OverlayPanelAction::DialogDropdownToggled {
                dialog_id,
                control_id,
            } => self.dispatch_toolbar_dialog_event_with_ctx(
                dialog_id,
                DialogEvent::DropdownToggled { control_id },
                Some(ctx),
            ),
            OverlayPanelAction::DialogDropdownChanged {
                dialog_id,
                control_id,
                option_id,
            } => self.dispatch_toolbar_dialog_event_with_ctx(
                dialog_id,
                DialogEvent::DropdownChanged {
                    control_id,
                    option_id,
                },
                Some(ctx),
            ),
            OverlayPanelAction::DialogDismissed { dialog_id } => self
                .dispatch_toolbar_dialog_event_with_ctx(
                    dialog_id,
                    DialogEvent::Dismissed,
                    Some(ctx),
                ),
            action @ OverlayPanelAction::AddRemoteOpen
            | action @ OverlayPanelAction::AddRemoteClose
            | action @ OverlayPanelAction::AddRemoteNameChanged(_)
            | action @ OverlayPanelAction::AddRemotePullUrlChanged(_)
            | action @ OverlayPanelAction::AddRemotePushUrlChanged(_)
            | action @ OverlayPanelAction::AddRemoteConfirmed
            | action @ OverlayPanelAction::CreateWorktreeOpen
            | action @ OverlayPanelAction::CreateWorktreeClose
            | action @ OverlayPanelAction::CreateWorktreeReferenceChanged(_)
            | action @ OverlayPanelAction::CreateWorktreeDropdownToggled
            | action @ OverlayPanelAction::CreateWorktreeBranchNameChanged(_)
            | action @ OverlayPanelAction::CreateWorktreeWorkingDirChanged(_)
            | action @ OverlayPanelAction::CreateWorktreeBrowseRequested
            | action @ OverlayPanelAction::CreateWorktreeBrowseResolved(_)
            | action @ OverlayPanelAction::CreateWorktreeConfirmed
            | action @ OverlayPanelAction::WorktreeRemoveRequested { .. } => {
                side_panel_dispatch::dispatch(self, action, ctx)
            }
            OverlayPanelAction::None => DialogDispatch::Task(Task::none()),
        }
    }
}

pub(crate) fn existing_branches(data: &RepositoryData) -> Vec<String> {
    data.snapshot
        .sidebar_sections()
        .iter()
        .flat_map(|section| section.branches.iter().map(|b| b.name.clone()))
        .collect()
}

/// Whether pressing `button_id` on dialog `dialog_id` begins a git write, and
/// thus must route through the serialized git queue. Buttons that only open
/// another dialog (e.g. `push_behind`'s Force, which opens `force_push`) are
/// not writes. This is the single source of truth feeding
/// `RepositoryMessage::git_write_intent`.
pub(crate) fn dialog_button_writes(dialog_id: &DialogId, button_id: &DialogButtonId) -> bool {
    let id = dialog_id.0.as_str();
    let button = button_id.0.as_str();
    match id {
        force_push::DIALOG_ID => button == force_push::CONFIRM_BUTTON_ID,
        stash_delete::DIALOG_ID => button == stash_delete::CONFIRM_BUTTON_ID,
        delete_tag::DIALOG_ID => button == delete_tag::CONFIRM_BUTTON_ID,
        cherry_pick_confirm::DIALOG_ID => {
            button == cherry_pick_confirm::IMMEDIATE_BUTTON_ID
                || button == cherry_pick_confirm::STAGED_BUTTON_ID
        }
        revert_confirm::DIALOG_ID => {
            button == revert_confirm::IMMEDIATE_BUTTON_ID
                || button == revert_confirm::IN_PLACE_BUTTON_ID
        }
        remove_worktree::DIALOG_ID => button == remove_worktree::CONFIRM_BUTTON_ID,
        discard::DIALOG_ID => button == discard::CONFIRM_BUTTON_ID,
        delete_branch::DIALOG_ID => {
            button == delete_branch::CONFIRM_BUTTON_ID
                || button == delete_branch::CONFIRM_ALL_BUTTON_ID
        }
        rename_branch::DIALOG_ID => button == rename_branch::CONFIRM_BUTTON_ID,
        create_branch::DIALOG_ID => button == create_branch::CONFIRM_BUTTON_ID,
        conflict_checkout::DIALOG_ID => {
            button == conflict_checkout::CREATE_BUTTON_ID
                || button == conflict_checkout::RESET_BUTTON_ID
        }
        set_upstream::DIALOG_ID => button == set_upstream::CONFIRM_BUTTON_ID,
        push_behind::DIALOG_ID => button == push_behind::PULL_BUTTON_ID,
        create_tag::DIALOG_ID => button == create_tag::CONFIRM_BUTTON_ID,
        modify_delete_conflict::DIALOG_ID => {
            button == modify_delete_conflict::KEEP_MODIFIED_BUTTON_ID
                || button == modify_delete_conflict::DELETE_FILE_BUTTON_ID
                || button == modify_delete_conflict::KEEP_BASE_BUTTON_ID
        }
        _ => false,
    }
}

fn bar_with_overlay<'a>(
    main_bar: Element<'a, Message>,
    overlay_elem: Element<'a, Message>,
    toolbar_slide_offset: f32,
) -> Element<'a, Message> {
    use iced::{
        widget::{container, Stack},
        Length,
    };

    if toolbar_slide_offset > 0.0 {
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
mod tests;
