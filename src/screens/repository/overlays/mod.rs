//! Repository toolbar dialogs and side-panel overlays.

use std::sync::Arc;

use iced::{keyboard, Element, Task};

use crate::{
    core::TabId,
    message::Message,
    services::{presenter::Presenter, ModifyDeleteConflictChoice, SharedRepositoryGateway},
    theme,
};

use self::dialog::model::{Dialog, DialogButtonId, DialogControlId, DialogId, DialogOwner};
use self::native_dialog_kind::NativeDialogKind;
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
mod native_dialog_kind;
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
        if self.toolbar_dialog.is_some() && self.toolbar_slide_offset > 0.0 {
            return true;
        }
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
        let Some(dialog_kind) = self
            .toolbar_dialog
            .as_ref()
            .filter(|dialog| &dialog.id == dialog_id)
            .map(NativeDialogKind::from_dialog)
        else {
            return DialogDispatch::Task(Task::none());
        };

        match dialog_kind {
            Some(NativeDialogKind::ForcePush) => {
                self.dispatch_force_push_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::StashDelete) => {
                self.dispatch_stash_delete_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::DeleteTag) => {
                self.dispatch_delete_tag_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::CherryPick) => {
                self.dispatch_cherry_pick_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::Revert) => {
                self.dispatch_revert_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::RemoveWorktree) => {
                self.dispatch_remove_worktree_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::Discard) => {
                self.dispatch_discard_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::DeleteBranch) => {
                self.dispatch_delete_branch_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::RenameBranch) => {
                self.dispatch_rename_branch_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::CreateBranch) => {
                self.dispatch_create_branch_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::ConflictCheckout) => {
                self.dispatch_conflict_checkout_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::SetUpstream) => {
                self.dispatch_set_upstream_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::PushBehind) => {
                self.dispatch_push_behind_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::CreateTag) => {
                self.dispatch_create_tag_dialog_event(dialog_id, event, ctx)
            }
            Some(NativeDialogKind::ModifyDeleteConflict) => {
                self.dispatch_modify_delete_conflict_dialog_event(dialog_id, event, ctx)
            }
            None => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_force_push_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && force_push::is_confirm_button(&button_id) =>
            {
                self.confirm_force_push(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && force_push::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_stash_delete_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && stash_delete::is_confirm_button(&button_id) =>
            {
                self.confirm_stash_delete(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && stash_delete::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_delete_tag_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && delete_tag::is_confirm_button(&button_id) =>
            {
                self.confirm_delete_tag(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && delete_tag::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_cherry_pick_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && cherry_pick_confirm::is_immediate_button(&button_id) =>
            {
                self.confirm_cherry_pick(true, ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && cherry_pick_confirm::is_staged_button(&button_id) =>
            {
                self.confirm_cherry_pick(false, ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && cherry_pick_confirm::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_revert_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && revert_confirm::is_immediate_button(&button_id) =>
            {
                self.confirm_revert(true, ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && revert_confirm::is_in_place_button(&button_id) =>
            {
                self.confirm_revert(false, ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && revert_confirm::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_remove_worktree_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && remove_worktree::is_confirm_button(&button_id) =>
            {
                self.confirm_remove_worktree(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && remove_worktree::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_discard_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && discard::is_confirm_button(&button_id) =>
            {
                self.confirm_discard_dialog(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && discard::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_delete_branch_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && delete_branch::is_confirm_button(&button_id) =>
            {
                self.confirm_delete_branch(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && delete_branch::is_confirm_all_button(&button_id) =>
            {
                self.confirm_delete_branch_all(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && delete_branch::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_rename_branch_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && rename_branch::is_confirm_button(&button_id) =>
            {
                self.confirm_rename_branch(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && rename_branch::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_create_branch_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && create_branch::is_confirm_button(&button_id) =>
            {
                self.confirm_create_branch(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && create_branch::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_conflict_checkout_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && conflict_checkout::is_create_button(&button_id) =>
            {
                self.confirm_conflict_create_branch(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && conflict_checkout::is_reset_button(&button_id) =>
            {
                self.confirm_conflict_reset_local(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && conflict_checkout::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_set_upstream_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && set_upstream::is_confirm_button(&button_id) =>
            {
                self.confirm_set_upstream(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && set_upstream::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_push_behind_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && push_behind::is_pull_button(&button_id) =>
            {
                self.confirm_push_behind_pull(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && push_behind::is_force_button(&button_id) =>
            {
                self.confirm_push_behind_force_push()
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && push_behind::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_create_tag_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && create_tag::is_confirm_button(&button_id) =>
            {
                self.confirm_create_tag(ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && create_tag::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
        }
    }

    fn dispatch_modify_delete_conflict_dialog_event(
        &mut self,
        dialog_id: &DialogId,
        event: DialogEvent,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        match event {
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && modify_delete_conflict::is_keep_modified_button(&button_id) =>
            {
                let Some(ctx) = ctx else {
                    return DialogDispatch::Task(Task::none());
                };
                self.resolve_modify_delete_conflict(ModifyDeleteConflictChoice::KeepModified, ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && modify_delete_conflict::is_delete_file_button(&button_id) =>
            {
                let Some(ctx) = ctx else {
                    return DialogDispatch::Task(Task::none());
                };
                self.resolve_modify_delete_conflict(ModifyDeleteConflictChoice::DeleteFile, ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && modify_delete_conflict::is_keep_base_button(&button_id) =>
            {
                let Some(ctx) = ctx else {
                    return DialogDispatch::Task(Task::none());
                };
                self.resolve_modify_delete_conflict(ModifyDeleteConflictChoice::KeepBase, ctx)
            }
            DialogEvent::ButtonPressed { button_id }
                if self.has_enabled_toolbar_dialog_button(dialog_id, &button_id)
                    && modify_delete_conflict::is_cancel_button(&button_id) =>
            {
                self.close();
                DialogDispatch::CancelClosed
            }
            DialogEvent::Dismissed => {
                self.close();
                DialogDispatch::CancelClosed
            }
            event => self.apply_local_dialog_event(dialog_id, event),
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

    fn confirm_force_push(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_force_push(self, ctx)
    }

    fn confirm_stash_delete(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_stash_delete(self, ctx)
    }

    fn confirm_delete_tag(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_delete_tag(self, ctx)
    }

    fn confirm_cherry_pick(
        &mut self,
        commit_now: bool,
        ctx: Option<DialogCtx<'_>>,
    ) -> DialogDispatch {
        native_actions::confirm_cherry_pick(self, commit_now, ctx)
    }

    fn confirm_revert(&mut self, commit_now: bool, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_revert(self, commit_now, ctx)
    }

    fn confirm_remove_worktree(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_remove_worktree(self, ctx)
    }

    fn confirm_discard_dialog(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_discard_dialog(self, ctx)
    }

    fn confirm_delete_branch(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_delete_branch(self, ctx)
    }

    fn confirm_delete_branch_all(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_delete_branch_all(self, ctx)
    }

    fn confirm_rename_branch(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_rename_branch(self, ctx)
    }

    fn confirm_create_branch(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_create_branch(self, ctx)
    }

    fn confirm_conflict_create_branch(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_conflict_create_branch(self, ctx)
    }

    fn confirm_conflict_reset_local(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_conflict_reset_local(self, ctx)
    }

    fn confirm_set_upstream(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_set_upstream(self, ctx)
    }

    fn confirm_push_behind_pull(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_push_behind_pull(self, ctx)
    }

    fn confirm_push_behind_force_push(&mut self) -> DialogDispatch {
        native_actions::confirm_push_behind_force_push(self)
    }

    fn confirm_create_tag(&mut self, ctx: Option<DialogCtx<'_>>) -> DialogDispatch {
        native_actions::confirm_create_tag(self, ctx)
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
            | action @ OverlayPanelAction::CreateWorktreeOpen { .. }
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
