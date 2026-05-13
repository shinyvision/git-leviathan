//! Destructive confirmation before dropping a stash entry.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogData, DialogId, DialogKey,
    DialogMessage, DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.stash_delete";
const OWNER_ID: &str = "stash_delete";
const DATA_STASH_INDEX: &str = "stash_index";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub stash_index: usize,
    pub display_name: String,
}

pub(crate) fn dialog(state: State) -> Dialog {
    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!(
                "This is a destructive operation, are you sure you want to delete stash '{}'?",
                state.display_name
            ),
        },
        data: vec![DialogData {
            id: DATA_STASH_INDEX.into(),
            value: state.stash_index.to_string(),
        }],
        controls: Vec::new(),
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
                text: "Delete".into(),
                style: DialogButtonStyle("danger".into()),
                keys: Vec::new(),
                closes_dialog: true,
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

pub(crate) fn stash_index(dialog: &Dialog) -> Option<usize> {
    if !is_dialog(dialog) {
        return None;
    }
    dialog.data_value(DATA_STASH_INDEX)?.parse().ok()
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
