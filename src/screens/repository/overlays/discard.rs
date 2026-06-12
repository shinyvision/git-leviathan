//! Confirmation before discarding dirty changes — either a single file or the
//! entire working tree.

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogData, DialogId, DialogKey,
    DialogMessage, DialogOwner,
};

pub(crate) const DIALOG_ID: &str = "native.discard";
const OWNER_ID: &str = "discard";
const DATA_TARGET_KIND: &str = "target_kind";
const DATA_PATH: &str = "path";
const DATA_COUNT: &str = "count";
const TARGET_ALL: &str = "all";
const TARGET_FILE: &str = "file";
const TARGET_FILES: &str = "files";
pub(crate) const CONFIRM_BUTTON_ID: &str = "confirm";
pub(crate) const CANCEL_BUTTON_ID: &str = "cancel";

#[derive(Debug, Clone)]
pub(crate) enum Target {
    All,
    File(String),
    Files { paths: Vec<String>, count: usize },
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub target: Target,
}

pub(crate) fn dialog(state: State) -> Dialog {
    let (text, confirm_text, data) = match state.target {
        Target::All => (
            "Are you sure you want to discard all changes?".to_string(),
            "Discard All Changes".to_string(),
            vec![DialogData {
                id: DATA_TARGET_KIND.into(),
                value: TARGET_ALL.into(),
            }],
        ),
        Target::File(path) => {
            let file_name = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path.as_str())
                .to_string();
            (
                format!(
                    "Are you sure you want to discard all changes to '{}'?",
                    file_name
                ),
                "Reset File".to_string(),
                vec![
                    DialogData {
                        id: DATA_TARGET_KIND.into(),
                        value: TARGET_FILE.into(),
                    },
                    DialogData {
                        id: DATA_PATH.into(),
                        value: path,
                    },
                ],
            )
        }
        Target::Files { paths, count } => {
            let mut data = vec![
                DialogData {
                    id: DATA_TARGET_KIND.into(),
                    value: TARGET_FILES.into(),
                },
                DialogData {
                    id: DATA_COUNT.into(),
                    value: count.to_string(),
                },
            ];
            data.extend(paths.into_iter().map(|path| DialogData {
                id: DATA_PATH.into(),
                value: path,
            }));
            (
                format!(
                    "Are you sure you want to discard selected changes in {}?",
                    plural_files(count)
                ),
                "Reset Files".to_string(),
                data,
            )
        }
    };

    Dialog {
        id: DialogId(DIALOG_ID.into()),
        owner: DialogOwner::native(OWNER_ID),
        message: DialogMessage { title: None, text },
        data,
        controls: Vec::new(),
        buttons: vec![
            DialogButton {
                id: DialogButtonId(CONFIRM_BUTTON_ID.into()),
                text: confirm_text,
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

pub(crate) fn target(dialog: &Dialog) -> Option<Target> {
    if !is_dialog(dialog) {
        return None;
    }
    match dialog.data_value(DATA_TARGET_KIND)? {
        TARGET_ALL => Some(Target::All),
        TARGET_FILE => Some(Target::File(dialog.data_value(DATA_PATH)?.to_string())),
        TARGET_FILES => {
            let count = dialog.data_value(DATA_COUNT)?.parse().ok()?;
            let paths = dialog
                .data
                .iter()
                .filter(|item| item.id == DATA_PATH)
                .map(|item| item.value.clone())
                .collect();
            Some(Target::Files { paths, count })
        }
        _ => None,
    }
}

fn plural_files(count: usize) -> String {
    if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    }
}
