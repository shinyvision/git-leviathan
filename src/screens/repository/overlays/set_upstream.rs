//! First-push-for-a-new-branch dialog: pick the remote and upstream name.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogControl, DialogControlId,
    DialogData, DialogDropdown, DialogDropdownOption, DialogId, DialogKey, DialogLabel,
    DialogLabelStyle, DialogMessage, DialogOwner, DialogTextInput,
};

const REMOTE_DROPDOWN_WIDTH: u16 = 150;

pub(crate) const DIALOG_ID: &str = "native.set_upstream";
const OWNER_ID: &str = "set_upstream";
const CONTROL_REMOTE: &str = "remote";
const CONTROL_SEPARATOR: &str = "separator";
const CONTROL_BRANCH_NAME: &str = "branch_name";
const DATA_BRANCH_NAME: &str = "branch_name";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub selected_remote_name: String,
    pub available_remotes: Vec<String>,
    pub remote_dropdown_open: bool,
    pub new_branch_input: String,
    pub submitting: bool,
}

impl State {
    pub(crate) fn new(
        branch_name: String,
        proposed_remote_name: String,
        available_remotes: Vec<String>,
    ) -> Self {
        let available_remotes = normalized_remotes(&proposed_remote_name, available_remotes);
        let proposed_remote_name = proposed_remote_name.trim();
        let selected_remote_name = available_remotes
            .iter()
            .find(|remote| remote.as_str() == proposed_remote_name)
            .cloned()
            .or_else(|| available_remotes.first().cloned())
            .unwrap_or_default();

        Self {
            new_branch_input: branch_name.clone(),
            branch_name,
            selected_remote_name,
            available_remotes,
            remote_dropdown_open: false,
            submitting: false,
        }
    }
}

pub(crate) fn dialog(state: State) -> Dialog {
    let mut dialog = Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!("Push '{}' to", state.branch_name),
        },
        data: vec![DialogData {
            id: DATA_BRANCH_NAME.into(),
            value: state.branch_name,
        }],
        controls: vec![
            DialogControl {
                id: DialogControlId(CONTROL_REMOTE.into()),
                label: None,
                text_input: None,
                dropdown: Some(DialogDropdown {
                    placeholder: "Select remote".into(),
                    options: state
                        .available_remotes
                        .into_iter()
                        .map(|remote| DialogDropdownOption {
                            id: remote.clone(),
                            text: remote,
                        })
                        .collect(),
                    selected_option_id: (!state.selected_remote_name.is_empty())
                        .then_some(state.selected_remote_name),
                    open: state.remote_dropdown_open,
                    width: Some(REMOTE_DROPDOWN_WIDTH),
                    leading_icon: Some("cloud".into()),
                }),
            },
            DialogControl {
                id: DialogControlId(CONTROL_SEPARATOR.into()),
                label: Some(DialogLabel {
                    text: "/".into(),
                    style: DialogLabelStyle("secondary".into()),
                }),
                text_input: None,
                dropdown: None,
            },
            DialogControl {
                id: DialogControlId(CONTROL_BRANCH_NAME.into()),
                label: None,
                text_input: Some(DialogTextInput {
                    placeholder: "remote branch name".into(),
                    value: state.new_branch_input,
                    submit_button_id: Some(DialogButtonId(CONFIRM_BUTTON_ID.into())),
                    width: None,
                }),
                dropdown: None,
            },
        ],
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
                text: "Confirm".into(),
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
    set_submitting(&mut dialog, state.submitting);
    refresh_enabled(&mut dialog);
    dialog
}

pub(crate) fn is_dialog(dialog: &Dialog) -> bool {
    dialog.id.0 == DIALOG_ID && dialog.owner.is_native(OWNER_ID)
}

pub(crate) fn selected_remote_name(dialog: &Dialog) -> Option<String> {
    if !is_dialog(dialog) {
        return None;
    }
    dialog
        .controls
        .iter()
        .find(|control| control.id.0 == CONTROL_REMOTE)?
        .dropdown
        .as_ref()?
        .selected_option_id
        .clone()
}

pub(crate) fn remote_branch_input(dialog: &Dialog) -> Option<String> {
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
    let can_submit = !is_submitting(dialog)
        && selected_remote_name(dialog)
            .as_deref()
            .is_some_and(|remote| !remote.trim().is_empty())
        && remote_branch_input(dialog)
            .as_deref()
            .is_some_and(|branch| !branch.trim().is_empty());
    dialog.set_button_enabled(CONFIRM_BUTTON_ID, can_submit);
}

pub(crate) fn set_submitting(dialog: &mut Dialog, submitting: bool) {
    if !is_dialog(dialog) {
        return;
    }
    set_data_value(dialog, "submitting", submitting.to_string());
}

fn is_submitting(dialog: &Dialog) -> bool {
    dialog
        .data_value("submitting")
        .and_then(|value| value.parse().ok())
        .unwrap_or(false)
}

fn set_data_value(dialog: &mut Dialog, id: &str, value: String) {
    if let Some(item) = dialog.data.iter_mut().find(|item| item.id == id) {
        item.value = value;
    } else {
        dialog.data.push(DialogData {
            id: id.into(),
            value,
        });
    }
}

fn normalized_remotes(proposed_remote_name: &str, available_remotes: Vec<String>) -> Vec<String> {
    let mut remotes = Vec::new();
    push_unique_remote(&mut remotes, proposed_remote_name);
    for remote in available_remotes {
        push_unique_remote(&mut remotes, &remote);
    }
    remotes
}

fn push_unique_remote(remotes: &mut Vec<String>, remote_name: &str) {
    let remote_name = remote_name.trim();
    if remote_name.is_empty() || remotes.iter().any(|remote| remote == remote_name) {
        return;
    }
    remotes.push(remote_name.to_string());
}
