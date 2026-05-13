//! Remove worktree confirmation dialog — toolbar overlay, same pattern as
//! delete_branch. Blocks confirmation when the worktree is the active focus.

use std::path::PathBuf;

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogData, DialogId, DialogKey,
    DialogMessage, DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.remove_worktree";
const OWNER_ID: &str = "remove_worktree";
const DATA_PATH: &str = "path";
const DATA_BRANCH_NAME: &str = "branch_name";
const DATA_IS_ACTIVE: &str = "is_active";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

pub(crate) struct State {
    pub path: PathBuf,
    pub branch_name: String,
    pub is_active: bool,
}

pub(crate) fn dialog(state: State) -> Dialog {
    let prompt = if state.is_active {
        format!(
            "Cannot remove '{}': it is the focused worktree. Switch away first.",
            state.branch_name,
        )
    } else {
        format!(
            "Remove worktree '{}' at {}?",
            state.branch_name,
            state.path.display()
        )
    };

    let mut buttons = Vec::new();
    if !state.is_active {
        buttons.push(DialogButton {
            id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
            text: "Remove".into(),
            style: DialogButtonStyle("danger".into()),
            keys: Vec::new(),
            closes_dialog: true,
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
            text: prompt,
        },
        data: vec![
            DialogData {
                id: DATA_PATH.into(),
                value: state.path.to_string_lossy().into_owned(),
            },
            DialogData {
                id: DATA_BRANCH_NAME.into(),
                value: state.branch_name,
            },
            DialogData {
                id: DATA_IS_ACTIVE.into(),
                value: state.is_active.to_string(),
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

pub(crate) fn state(dialog: &Dialog) -> Option<State> {
    if !is_dialog(dialog) {
        return None;
    }
    Some(State {
        path: PathBuf::from(dialog.data_value(DATA_PATH)?),
        branch_name: dialog.data_value(DATA_BRANCH_NAME)?.to_string(),
        is_active: dialog.data_value(DATA_IS_ACTIVE)? == "true",
    })
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
