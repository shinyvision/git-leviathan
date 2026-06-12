//! Destructive confirmation before deleting a local tag (and any remote copies).

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogData, DialogId, DialogKey,
    DialogMessage, DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.delete_tag";
const OWNER_ID: &str = "delete_tag";
const DATA_TAG_NAME: &str = "tag_name";
const DATA_REMOTE_NAME: &str = "remote_name";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub tag_name: String,
    /// Remotes that hold this tag — deleted automatically alongside the local
    /// tag after confirmation.
    pub tag_remote_names: Vec<String>,
}

pub(crate) fn dialog(state: State) -> Dialog {
    let remote_suffix = if state.tag_remote_names.is_empty() {
        String::new()
    } else {
        format!(
            " (will also be deleted from {})",
            state.tag_remote_names.join(", ")
        )
    };
    let mut data = vec![DialogData {
        id: DATA_TAG_NAME.into(),
        value: state.tag_name.clone(),
    }];
    data.extend(
        state
            .tag_remote_names
            .into_iter()
            .map(|remote_name| DialogData {
                id: DATA_REMOTE_NAME.into(),
                value: remote_name,
            }),
    );

    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!(
                "This is a destructive operation, are you sure you want to delete tag '{}'{}?",
                state.tag_name, remote_suffix
            ),
        },
        data,
        controls: Vec::new(),
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
                text: "Delete".into(),
                style: DialogButtonStyle("danger".into()),
                keys: vec![DialogKey("y".into())],
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

pub(crate) fn tag_name(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(dialog.data_value(DATA_TAG_NAME)?.to_string())
}

pub(crate) fn remote_names(dialog: &Dialog) -> Vec<String> {
    if !is_dialog(dialog) {
        return Vec::new();
    }
    dialog
        .data
        .iter()
        .filter(|item| item.id == DATA_REMOTE_NAME)
        .map(|item| item.value.clone())
        .collect()
}
