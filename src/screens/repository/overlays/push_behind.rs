//! Bar shown when a push is rejected because the branch is behind its remote.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogId, DialogKey, DialogMessage,
    DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.push_behind";
const OWNER_ID: &str = "push_behind";
pub(crate) const PULL_BUTTON_ID: &str = "pull";
pub(crate) const FORCE_BUTTON_ID: &str = "force";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

pub(crate) fn dialog(target: crate::services::git::PushTarget) -> Dialog {
    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!(
                "Push rejected. Pull from {}/{} to integrate its changes into {}.",
                target.remote_name, target.remote_branch, target.local_branch
            ),
        },
        data: super::push_target::data(target),
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
