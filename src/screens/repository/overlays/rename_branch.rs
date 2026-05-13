//! Rename a local or remote branch via inline text input.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogControl, DialogControlId,
    DialogData, DialogId, DialogKey, DialogMessage, DialogOwner, DialogTextInput,
};

pub(crate) const DIALOG_ID: &str = "native.rename_branch";
const OWNER_ID: &str = "rename_branch";
const CONTROL_BRANCH_NAME: &str = "branch_name";
const DATA_BRANCH_NAME: &str = "old_name";
const DATA_IS_REMOTE: &str = "is_remote";
const DATA_REMOTE_REF: &str = "remote_ref";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub is_remote: bool,
    pub new_branch_input: String,
    pub remote_name: Option<String>,
    pub remote_ref: Option<String>,
}

pub(crate) fn dialog(state: State) -> Dialog {
    let display_name = match (state.is_remote, state.remote_name.as_deref()) {
        (true, Some(remote)) => format!("{}/{}", remote, state.branch_name),
        _ => state.branch_name.clone(),
    };

    let mut dialog = Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!("Rename '{}' to:", display_name),
        },
        data: vec![
            DialogData {
                id: DATA_BRANCH_NAME.into(),
                value: state.branch_name,
            },
            DialogData {
                id: DATA_IS_REMOTE.into(),
                value: state.is_remote.to_string(),
            },
            DialogData {
                id: DATA_REMOTE_REF.into(),
                value: state.remote_ref.unwrap_or_default(),
            },
        ],
        controls: vec![DialogControl {
            id: DialogControlId(CONTROL_BRANCH_NAME.into()),
            label: None,
            text_input: Some(DialogTextInput {
                placeholder: "branch name".into(),
                value: state.new_branch_input,
                submit_button_id: Some(DialogButtonId(CONFIRM_BUTTON_ID.into())),
                width: None,
            }),
            dropdown: None,
        }],
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
                text: "Submit".into(),
                style: DialogButtonStyle("create".into()),
                keys: vec![DialogKey("enter".into())],
                closes_dialog: false,
                enabled: false,
            },
            DialogButton {
                id: DialogButtonId(CANCEL_BUTTON_ID.into()),
                text: "Cancel".into(),
                style: DialogButtonStyle("cancel".into()),
                keys: vec![DialogKey("esc".into())],
                closes_dialog: true,
                enabled: true,
            },
        ],
        dismissible: true,
        autofocus: Some(DialogControlId(CONTROL_BRANCH_NAME.into())),
    };
    refresh_enabled(&mut dialog);
    dialog
}

pub(crate) fn is_dialog(dialog: &Dialog) -> bool {
    dialog.id.0 == DIALOG_ID && dialog.owner.is_native(OWNER_ID)
}

pub(crate) fn old_name(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(dialog.data_value(DATA_BRANCH_NAME)?.to_string())
}

pub(crate) fn old_ref(dialog: &Dialog) -> Option<String> {
    let old_name = old_name(dialog)?;
    if !is_remote(dialog)? {
        return Some(old_name);
    }
    Some(
        dialog
            .data_value(DATA_REMOTE_REF)
            .filter(|remote_ref| !remote_ref.is_empty())
            .unwrap_or(&old_name)
            .to_string(),
    )
}

pub(crate) fn is_remote(dialog: &Dialog) -> Option<bool> {
    if !is_dialog(dialog) {
        return None;
    }
    dialog.data_value(DATA_IS_REMOTE)?.parse().ok()
}

pub(crate) fn new_name_input(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    dialog
        .controls
        .iter()
        .find(|control| control.id.0 == CONTROL_BRANCH_NAME)?
        .text_input
        .as_ref()
        .map(|input| input.value.clone())
}

pub(crate) fn refresh_enabled(dialog: &mut Dialog) {
    if !is_dialog(dialog) {
        return;
    }
    let new_name = new_name_input(dialog).unwrap_or_default();
    set_button_enabled(dialog, CONFIRM_BUTTON_ID, !new_name.trim().is_empty());
}

fn set_button_enabled(dialog: &mut Dialog, button_id: &str, enabled: bool) {
    if let Some(button) = dialog
        .buttons
        .iter_mut()
        .find(|button| button.id.0 == button_id)
    {
        button.enabled = enabled;
    }
}

pub(crate) fn is_confirm_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == CONFIRM_BUTTON_ID
}

pub(crate) fn is_cancel_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == CANCEL_BUTTON_ID
}

pub(crate) fn is_confirm_button_action(dialog_id: &DialogId, button_id: &DialogButtonId) -> bool {
    dialog_id.0 == DIALOG_ID && is_confirm_button(button_id)
}
