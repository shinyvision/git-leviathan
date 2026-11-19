use super::dialog::model::{Dialog, DialogData};
use crate::services::git::PushTarget;

pub(super) fn data(target: PushTarget) -> Vec<DialogData> {
    [
        ("branch_name", target.local_branch),
        ("remote_name", target.remote_name),
        ("remote_branch", target.remote_branch),
    ]
    .into_iter()
    .map(|(id, value)| DialogData {
        id: id.into(),
        value,
    })
    .collect()
}

pub(super) fn read(dialog: &Dialog) -> Option<PushTarget> {
    Some(PushTarget {
        local_branch: dialog.data_value("branch_name")?.to_owned(),
        remote_name: dialog.data_value("remote_name")?.to_owned(),
        remote_branch: dialog.data_value("remote_branch")?.to_owned(),
    })
}
