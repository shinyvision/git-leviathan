use super::{
    cherry_pick_confirm, conflict_checkout, create_branch, create_tag, delete_branch, delete_tag,
    dialog::model::Dialog, discard, force_push, modify_delete_conflict, push_behind,
    remove_worktree, rename_branch, revert_confirm, set_upstream, stash_delete,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeDialogKind {
    ForcePush,
    StashDelete,
    DeleteTag,
    CherryPick,
    Revert,
    RemoveWorktree,
    Discard,
    DeleteBranch,
    RenameBranch,
    CreateBranch,
    ConflictCheckout,
    SetUpstream,
    PushBehind,
    CreateTag,
    ModifyDeleteConflict,
}

impl NativeDialogKind {
    pub(super) fn from_dialog(dialog: &Dialog) -> Option<Self> {
        if force_push::is_dialog(dialog) {
            return Some(Self::ForcePush);
        }
        if stash_delete::is_dialog(dialog) {
            return Some(Self::StashDelete);
        }
        if delete_tag::is_dialog(dialog) {
            return Some(Self::DeleteTag);
        }
        if cherry_pick_confirm::is_dialog(dialog) {
            return Some(Self::CherryPick);
        }
        if revert_confirm::is_dialog(dialog) {
            return Some(Self::Revert);
        }
        if remove_worktree::is_dialog(dialog) {
            return Some(Self::RemoveWorktree);
        }
        if discard::is_dialog(dialog) {
            return Some(Self::Discard);
        }
        if delete_branch::is_dialog(dialog) {
            return Some(Self::DeleteBranch);
        }
        if rename_branch::is_dialog(dialog) {
            return Some(Self::RenameBranch);
        }
        if create_branch::is_dialog(dialog) {
            return Some(Self::CreateBranch);
        }
        if conflict_checkout::is_dialog(dialog) {
            return Some(Self::ConflictCheckout);
        }
        if set_upstream::is_dialog(dialog) {
            return Some(Self::SetUpstream);
        }
        if push_behind::is_dialog(dialog) {
            return Some(Self::PushBehind);
        }
        if create_tag::is_dialog(dialog) {
            return Some(Self::CreateTag);
        }
        if modify_delete_conflict::is_dialog(dialog) {
            return Some(Self::ModifyDeleteConflict);
        }
        None
    }
}
