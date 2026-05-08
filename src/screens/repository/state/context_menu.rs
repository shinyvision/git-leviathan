//! Context-menu state structs that the popout controller and `view` layers
//! consume. Kept separate from the controller so the controller file stays
//! focused on the open/close state machine rather than the shape of every
//! menu variant.

use iced::Point;

#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub branch_name: String,
    /// For tag rows: remote names known to hold this tag. Empty for non-tag.
    pub tag_remote_names: Vec<String>,
    /// For tag rows: configured remotes that do not already hold this tag.
    /// Empty for non-tag.
    pub tag_push_remote_names: Vec<String>,
    pub is_remote: bool,
    pub has_remote: bool,
    pub is_tag: bool,
    /// The remote this branch's remote-counterpart lives on (e.g. "origin"),
    /// when applicable. Used to label remote actions as "<remote>/<branch>".
    pub remote_name: Option<String>,
    /// The short branch name on the remote side, when it differs from
    /// `branch_name` (local tracks a renamed remote). Used for remote rename/
    /// delete git operations.
    pub remote_branch_name: Option<String>,
    /// True if current branch can be fast-forwarded to this branch
    /// (current branch is an ancestor of this branch).
    pub can_fast_forward: bool,
    pub position: Point,
}

pub(in crate::screens::repository) fn eligible_tag_push_remote_names(
    configured_remote_names: &[String],
    default_remote_name: Option<&str>,
    tag_remote_names: &[String],
) -> Vec<String> {
    let mut remote_names = configured_remote_names.to_vec();
    if remote_names.is_empty() {
        if let Some(default_remote_name) = default_remote_name {
            remote_names.push(default_remote_name.to_string());
        }
    }

    let mut eligible = Vec::new();
    for remote_name in remote_names {
        if tag_remote_names.iter().any(|name| name == &remote_name)
            || eligible.iter().any(|name| name == &remote_name)
        {
            continue;
        }
        eligible.push(remote_name);
    }
    eligible
}

/// State for the Reset submenu shown when user clicks "Reset ... to this commit".
#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct ResetSubmenuState {
    pub(in crate::screens::repository) commit_hash: String,
    pub(in crate::screens::repository) position: Point,
}

#[derive(Debug, Clone, Default)]
pub(in crate::screens::repository) struct ResetHoverTracker {
    pub(in crate::screens::repository) commit_hash: String,
    pub(in crate::screens::repository) parent_hovered: bool,
    pub(in crate::screens::repository) submenu_hovered: bool,
    pub(in crate::screens::repository) open_gen: u64,
    pub(in crate::screens::repository) close_gen: u64,
}

/// State for commit context menu (right-clicking a commit).
#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct CommitContextMenuState {
    pub(in crate::screens::repository) commit_idx: usize,
    pub(in crate::screens::repository) commit_hash: String,
    pub(in crate::screens::repository) position: Point,
    pub(in crate::screens::repository) stash_index: Option<usize>,
    pub(in crate::screens::repository) stash_display_name: Option<String>,
    pub(in crate::screens::repository) selected_indices: Vec<usize>,
    pub(in crate::screens::repository) selected_hashes: Vec<String>,
}

/// State for dirty file context menu (right-clicking an uncommitted-changes file row).
#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct DirtyFileContextMenuState {
    pub(in crate::screens::repository) path: String,
    pub(in crate::screens::repository) position: Point,
}

/// State for the worktree context menu (right-clicking a worktree sidebar entry).
#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct WorktreeContextMenuState {
    pub(in crate::screens::repository) path: std::path::PathBuf,
    pub(in crate::screens::repository) branch_name: String,
    pub(in crate::screens::repository) is_active: bool,
    pub(in crate::screens::repository) position: Point,
}

#[cfg(test)]
mod tests {
    use super::eligible_tag_push_remote_names;

    #[test]
    fn eligible_tag_push_remotes_exclude_known_tag_remotes() {
        let configured = vec![
            "origin".to_string(),
            "upstream".to_string(),
            "fork".to_string(),
        ];
        let known_tag_remotes = vec!["origin".to_string(), "fork".to_string()];

        assert_eq!(
            eligible_tag_push_remote_names(&configured, Some("origin"), &known_tag_remotes),
            vec!["upstream".to_string()]
        );
    }

    #[test]
    fn eligible_tag_push_remotes_fall_back_to_default_remote() {
        assert_eq!(
            eligible_tag_push_remote_names(&[], Some("origin"), &[]),
            vec!["origin".to_string()]
        );
        assert!(
            eligible_tag_push_remote_names(&[], Some("origin"), &["origin".to_string()]).is_empty()
        );
    }
}
