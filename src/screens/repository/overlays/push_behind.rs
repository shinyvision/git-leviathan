//! Bar shown when a push is rejected because the branch is behind its remote.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogData, DialogId, DialogKey,
    DialogMessage, DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.push_behind";
const OWNER_ID: &str = "push_behind";
const DATA_BRANCH_NAME: &str = "branch_name";
const DATA_REMOTE_NAME: &str = "remote_name";
pub(crate) const PULL_BUTTON_ID: &str = "pull";
pub(crate) const FORCE_BUTTON_ID: &str = "force";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub remote_name: String,
}

pub(crate) fn dialog(state: State) -> Dialog {
    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!(
                "'{}' is behind '{}/{}' Update your branch by doing a Pull.",
                state.branch_name, state.remote_name, state.branch_name
            ),
        },
        data: vec![
            DialogData {
                id: DATA_BRANCH_NAME.into(),
                value: state.branch_name,
            },
            DialogData {
                id: DATA_REMOTE_NAME.into(),
                value: state.remote_name,
            },
        ],
        controls: Vec::new(),
        buttons: vec![
            DialogButton {
                id: DialogButtonId(PULL_BUTTON_ID.into()),
                text: "Pull (fast-forward if possible)".into(),
                style: DialogButtonStyle("create".into()),
                keys: Vec::new(),
                closes_dialog: false,
                enabled: true,
            },
            DialogButton {
                id: DialogButtonId(FORCE_BUTTON_ID.into()),
                text: "Force Push".into(),
                style: DialogButtonStyle("danger".into()),
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

pub(crate) fn branch_name(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(dialog.data_value(DATA_BRANCH_NAME)?.to_string())
}

pub(crate) fn remote_name(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(dialog.data_value(DATA_REMOTE_NAME)?.to_string())
}
