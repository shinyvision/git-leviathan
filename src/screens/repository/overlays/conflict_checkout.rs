//! Conflict bar shown when checking out a remote branch whose local counterpart exists.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogControl, DialogControlId,
    DialogData, DialogId, DialogKey, DialogMessage, DialogOwner, DialogTextInput,
};

pub(crate) const DIALOG_ID: &str = "native.conflict_checkout";
const OWNER_ID: &str = "conflict_checkout";
const CONTROL_BRANCH_NAME: &str = "branch_name";
const DATA_BRANCH_NAME: &str = "branch_name";
const DATA_REMOTE_REF: &str = "remote_ref";
pub(crate) const CREATE_BUTTON_ID: &str = "create";
pub(crate) const RESET_BUTTON_ID: &str = "reset";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub remote_ref: String,
    pub new_branch_input: String,
}

pub(crate) fn dialog(state: State) -> Dialog {
    let mut dialog = Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!("A local '{}' already exists.", state.branch_name),
        },
        data: vec![
            DialogData {
                id: DATA_BRANCH_NAME.into(),
                value: state.branch_name,
            },
            DialogData {
                id: DATA_REMOTE_REF.into(),
                value: state.remote_ref,
            },
        ],
        controls: vec![DialogControl {
            id: DialogControlId(CONTROL_BRANCH_NAME.into()),
            label: None,
            text_input: Some(DialogTextInput {
                placeholder: "branch name".into(),
                value: state.new_branch_input,
                submit_button_id: Some(DialogButtonId(CREATE_BUTTON_ID.into())),
                width: None,
            }),
            dropdown: None,
        }],
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CREATE_BUTTON_ID.into()),
                text: "Create Branch Here".into(),
                style: DialogButtonStyle("create".into()),
                keys: vec![DialogKey("enter".into())],
                closes_dialog: false,
                enabled: false,
            },
            DialogButton {
                id: DialogButtonId(RESET_BUTTON_ID.into()),
                text: "Reset Local to Here".into(),
                style: DialogButtonStyle("danger".into()),
                keys: Vec::new(),
                closes_dialog: false,
                enabled: true,
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
        autofocus: None,
    };
    refresh_enabled(&mut dialog);
    dialog
}

pub(crate) fn is_dialog(dialog: &Dialog) -> bool {
    dialog.id.0 == DIALOG_ID && dialog.owner.is_native(OWNER_ID)
}

pub(crate) fn branch_name(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(dialog.data_value(DATA_BRANCH_NAME)?.to_string())
}

pub(crate) fn remote_ref(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(dialog.data_value(DATA_REMOTE_REF)?.to_string())
}

pub(crate) fn new_branch_input(dialog: &Dialog) -> Option<String> {
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
    let branch_name = new_branch_input(dialog).unwrap_or_default();
    dialog.set_button_enabled(CREATE_BUTTON_ID, !branch_name.trim().is_empty());
}
