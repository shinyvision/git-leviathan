//! "Create branch here" dialog triggered from a commit's right-click menu.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogControl, DialogControlId,
    DialogData, DialogId, DialogKey, DialogMessage, DialogOwner, DialogTextInput,
};
use super::validation::validate_branch_name;

pub(crate) const DIALOG_ID: &str = "native.create_branch";
const OWNER_ID: &str = "create_branch";
const CONTROL_BRANCH_NAME: &str = "branch_name";
const DATA_COMMIT_HASH: &str = "commit_hash";
const DATA_EXISTING_BRANCH: &str = "existing_branch";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub commit_hash: String,
    pub branch_name_input: String,
}

pub(crate) fn dialog(state: State, existing_branches: &[String]) -> Dialog {
    let mut dialog = Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: "Enter branch name".into(),
        },
        data: create_data(state.commit_hash, existing_branches),
        controls: vec![DialogControl {
            id: DialogControlId(CONTROL_BRANCH_NAME.into()),
            label: None,
            text_input: Some(DialogTextInput {
                placeholder: "branch name".into(),
                value: state.branch_name_input,
                submit_button_id: Some(DialogButtonId(CONFIRM_BUTTON_ID.into())),
                width: None,
            }),
            dropdown: None,
        }],
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
                text: "Create Branch".into(),
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

fn create_data(commit_hash: String, existing_branches: &[String]) -> Vec<DialogData> {
    let mut data = vec![DialogData {
        id: DATA_COMMIT_HASH.into(),
        value: commit_hash,
    }];
    data.extend(existing_branches.iter().cloned().map(|branch| DialogData {
        id: DATA_EXISTING_BRANCH.into(),
        value: branch,
    }));
    data
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

pub(crate) fn branch_name_input(dialog: &Dialog) -> Option<String> {
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
    let branch_name = branch_name_input(dialog).unwrap_or_default();
    let enabled = validate_branch_name(&branch_name, existing_branches(dialog)).is_ok();
    dialog.set_button_enabled(CONFIRM_BUTTON_ID, enabled);
}

fn existing_branches(dialog: &Dialog) -> impl Iterator<Item = &str> {
    dialog
        .data
        .iter()
        .filter(|item| item.id == DATA_EXISTING_BRANCH)
        .map(|item| item.value.as_str())
}
