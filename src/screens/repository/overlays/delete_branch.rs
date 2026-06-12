//! Destructive confirmation before deleting a local and/or remote branch.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogData, DialogId, DialogKey,
    DialogMessage, DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.delete_branch";
const OWNER_ID: &str = "delete_branch";
const DATA_BRANCH_NAME: &str = "branch_name";
const DATA_IS_REMOTE: &str = "is_remote";
const DATA_REMOTE_REF: &str = "remote_ref";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CONFIRM_ALL_BUTTON_ID: &str = "confirm_all";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub is_remote: bool,
    pub has_remote: bool,
    pub remote_name: Option<String>,
    pub remote_ref: Option<String>,
}

pub(crate) fn dialog(state: State) -> Dialog {
    let display_name = match (state.is_remote, state.remote_name.as_deref()) {
        (true, Some(remote)) => format!("{}/{}", remote, state.branch_name),
        _ => state.branch_name.clone(),
    };

    let mut buttons = vec![DialogButton {
        id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
        text: "Delete".into(),
        style: DialogButtonStyle("danger".into()),
        keys: Vec::new(),
        closes_dialog: false,
        enabled: true,
    }];
    if !state.is_remote && state.has_remote {
        buttons.push(DialogButton {
            id: DialogButtonId(CONFIRM_ALL_BUTTON_ID.into()),
            text: "Delete local and remote".into(),
            style: DialogButtonStyle("danger".into()),
            keys: Vec::new(),
            closes_dialog: false,
            enabled: true,
        });
    }
    buttons.push(DialogButton {
        id: DialogButtonId(CANCEL_BUTTON_ID.into()),
        text: "Cancel".into(),
        style: DialogButtonStyle("cancel".into()),
        keys: vec![DialogKey("esc".into()), DialogKey("n".into())],
        closes_dialog: true,
        enabled: true,
    });

    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!(
                "This is a destructive operation, are you sure you want to delete '{}'?",
                display_name
            ),
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
        controls: Vec::new(),
        buttons,
        dismissible: true,
        autofocus: None,
    }
}

pub(crate) fn is_dialog(dialog: &Dialog) -> bool {
    dialog.id.0 == DIALOG_ID && dialog.owner.is_native(OWNER_ID)
}

pub(crate) fn branch_name(dialog: &Dialog) -> Option<String> {
    is_dialog(dialog).then(|| dialog.data_value(DATA_BRANCH_NAME).map(str::to_string))?
}

pub(crate) fn is_remote(dialog: &Dialog) -> Option<bool> {
    if !is_dialog(dialog) {
        return None;
    }
    dialog.data_value(DATA_IS_REMOTE)?.parse().ok()
}

pub(crate) fn branch_ref(dialog: &Dialog) -> Option<String> {
    let branch_name = branch_name(dialog)?;
    let is_remote = is_remote(dialog)?;
    if !is_remote {
        return Some(branch_name);
    }
    Some(
        dialog
            .data_value(DATA_REMOTE_REF)
            .filter(|remote_ref| !remote_ref.is_empty())
            .unwrap_or(&branch_name)
            .to_string(),
    )
}
