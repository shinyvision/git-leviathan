//! Final destructive confirmation before force pushing a branch.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogId, DialogKey, DialogMessage,
    DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.force_push";
const OWNER_ID: &str = "force_push";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

pub(crate) fn dialog(target: crate::services::git::PushTarget) -> Dialog {
    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage {
            title: None,
            text: format!(
                "Force push {} to {}/{}. This cannot be undone.",
                target.local_branch, target.remote_name, target.remote_branch
            ),
        },
        data: super::push_target::data(target),
        controls: Vec::new(),
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
                text: "Force Push".into(),
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

#[cfg(test)]
pub(crate) fn is_dialog(dialog: &Dialog) -> bool {
    dialog.id.0 == DIALOG_ID && dialog.owner.is_native(OWNER_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_native_force_push_dialog() {
        let dialog = dialog(crate::services::git::PushTarget {
            remote_branch: "main".into(),
            local_branch: "main".into(),
            remote_name: "origin".into(),
        });

        assert!(is_dialog(&dialog));
        assert_eq!(
            dialog.message.text,
            "Force push main to origin/main. This cannot be undone."
        );
        assert_eq!(dialog.buttons[0].id.0, CONFIRM_BUTTON_ID);
        assert_eq!(dialog.buttons[1].id.0, CANCEL_BUTTON_ID);
        assert!(dialog.controls.is_empty());
    }
}
