//! Conflict choice for a file modified on one side and deleted on the other.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogData, DialogId, DialogKey,
    DialogMessage, DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.modify_delete_conflict";
const OWNER_ID: &str = "modify_delete_conflict";
const DATA_PATH: &str = "path";
pub(crate) const KEEP_MODIFIED_BUTTON_ID: &str = "keep_modified";
pub(crate) const DELETE_FILE_BUTTON_ID: &str = "delete_file";
pub(crate) const KEEP_BASE_BUTTON_ID: &str = "keep_base";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub path: String,
}

pub(crate) fn dialog(state: State) -> Dialog {
    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: "This file has been modified on one branch, but deleted on the other.".into(),
        },
        data: vec![DialogData {
            id: DATA_PATH.into(),
            value: state.path,
        }],
        controls: Vec::new(),
        buttons: vec![
            DialogButton {
                id: DialogButtonId(KEEP_MODIFIED_BUTTON_ID.into()),
                text: "Keep Modified Version".into(),
                style: DialogButtonStyle("resolve".into()),
                keys: Vec::new(),
                closes_dialog: false,
                enabled: true,
            },
            DialogButton {
                id: DialogButtonId(DELETE_FILE_BUTTON_ID.into()),
                text: "Delete The File".into(),
                style: DialogButtonStyle("danger".into()),
                keys: Vec::new(),
                closes_dialog: false,
                enabled: true,
            },
            DialogButton {
                id: DialogButtonId(KEEP_BASE_BUTTON_ID.into()),
                text: "Keep Base Version".into(),
                style: DialogButtonStyle("browse".into()),
                keys: Vec::new(),
                closes_dialog: false,
                enabled: true,
            },
            DialogButton {
                id: DialogButtonId(CANCEL_BUTTON_ID.into()),
                text: "Cancel".into(),
                style: DialogButtonStyle("cancel".into()),
                keys: vec![DialogKey("esc".into()), DialogKey("n".into())],
                closes_dialog: true,
                enabled: true,
            },
        ],
        dismissible: true,
        autofocus: None,
    }
}

pub(crate) fn is_dialog(dialog: &Dialog) -> bool {
    dialog.id.0 == DIALOG_ID && dialog.owner.is_native(OWNER_ID)
}

pub(crate) fn path(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(dialog.data_value(DATA_PATH)?.to_string())
}

pub(crate) fn is_keep_modified_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == KEEP_MODIFIED_BUTTON_ID
}

pub(crate) fn is_delete_file_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == DELETE_FILE_BUTTON_ID
}

pub(crate) fn is_keep_base_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == KEEP_BASE_BUTTON_ID
}

pub(crate) fn is_cancel_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == CANCEL_BUTTON_ID
}

pub(crate) fn is_write_button_action(dialog_id: &DialogId, button_id: &DialogButtonId) -> bool {
    dialog_id.0 == DIALOG_ID
        && (is_keep_modified_button(button_id)
            || is_delete_file_button(button_id)
            || is_keep_base_button(button_id))
}
