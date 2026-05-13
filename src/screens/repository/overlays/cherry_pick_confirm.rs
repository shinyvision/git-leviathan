//! Cherry-pick-mode picker: commit immediately, stage only, or cancel.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogData, DialogId, DialogKey,
    DialogMessage, DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.cherry_pick_confirm";
const OWNER_ID: &str = "cherry_pick_confirm";
const DATA_COMMIT_HASH: &str = "commit_hash";
pub(crate) const IMMEDIATE_BUTTON_ID: &str = "immediate";
pub(crate) const STAGED_BUTTON_ID: &str = "staged";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub commit_hash: String,
}

pub(crate) fn dialog(state: State) -> Dialog {
    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: "Do you want to immediately commit the cherry picked changes?".into(),
        },
        data: vec![DialogData {
            id: DATA_COMMIT_HASH.into(),
            value: state.commit_hash,
        }],
        controls: Vec::new(),
        buttons: vec![
            DialogButton {
                id: DialogButtonId(IMMEDIATE_BUTTON_ID.into()),
                text: "Yes".into(),
                style: DialogButtonStyle("create".into()),
                keys: Vec::new(),
                closes_dialog: true,
                enabled: true,
            },
            DialogButton {
                id: DialogButtonId(STAGED_BUTTON_ID.into()),
                text: "No".into(),
                style: DialogButtonStyle("neutral".into()),
                keys: Vec::new(),
                closes_dialog: true,
                enabled: true,
            },
            DialogButton {
                id: DialogButtonId(CANCEL_BUTTON_ID.into()),
                text: "Cancel".into(),
                style: DialogButtonStyle("danger".into()),
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

pub(crate) fn commit_hash(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(dialog.data_value(DATA_COMMIT_HASH)?.to_string())
}

pub(crate) fn is_immediate_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == IMMEDIATE_BUTTON_ID
}

pub(crate) fn is_staged_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == STAGED_BUTTON_ID
}

pub(crate) fn is_cancel_button(button_id: &DialogButtonId) -> bool {
    button_id.0 == CANCEL_BUTTON_ID
}

pub(crate) fn is_write_button_action(dialog_id: &DialogId, button_id: &DialogButtonId) -> bool {
    dialog_id.0 == DIALOG_ID && (is_immediate_button(button_id) || is_staged_button(button_id))
}
