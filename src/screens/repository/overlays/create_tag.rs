//! "Create tag here" dialog triggered from a commit's right-click menu.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogControl, DialogControlId,
    DialogData, DialogId, DialogKey, DialogMessage, DialogOwner, DialogTextInput,
};

pub(crate) const DIALOG_ID: &str = "native.create_tag";
const OWNER_ID: &str = "create_tag";
const CONTROL_TAG_NAME: &str = "tag_name";
const DATA_COMMIT_HASH: &str = "commit_hash";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub commit_hash: String,
    pub tag_name_input: String,
}

pub(crate) fn dialog(state: State) -> Dialog {
    let mut dialog = Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: "Enter tag name".into(),
        },
        data: vec![DialogData {
            id: DATA_COMMIT_HASH.into(),
            value: state.commit_hash,
        }],
        controls: vec![DialogControl {
            id: DialogControlId(CONTROL_TAG_NAME.into()),
            label: None,
            text_input: Some(DialogTextInput {
                placeholder: "tag name".into(),
                value: state.tag_name_input,
                submit_button_id: Some(DialogButtonId(CONFIRM_BUTTON_ID.into())),
                width: None,
            }),
            dropdown: None,
        }],
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
                text: "Create Tag".into(),
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
        autofocus: Some(DialogControlId(CONTROL_TAG_NAME.into())),
    };
    refresh_enabled(&mut dialog);
    dialog
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

pub(crate) fn tag_name_input(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    dialog
        .controls
        .iter()
        .find(|control| control.id.0 == CONTROL_TAG_NAME)?
        .text_input
        .as_ref()
        .map(|input| input.value.clone())
}

pub(crate) fn refresh_enabled(dialog: &mut Dialog) {
    if !is_dialog(dialog) {
        return;
    }
    let tag_name = tag_name_input(dialog).unwrap_or_default();
    dialog.set_button_enabled(CONFIRM_BUTTON_ID, !tag_name.trim().is_empty());
}
