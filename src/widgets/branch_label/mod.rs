//! Branch + tag label rendering: inline pills, overflow popouts, and the
//! right-click context menu.

mod cell;
mod context_menu;
mod layout;
mod popout;

pub use cell::{branch_label_cell, branch_stack_background};
pub use context_menu::branch_context_menu;
pub use layout::any_name_truncated;
pub use popout::branch_popout_panel;

use std::borrow::Cow;

pub const BRANCH_POPOUT_RADIUS: f32 = 6.0;
pub const BRANCH_LABEL_INSET_X: u16 = 6;

pub(crate) fn remote_ref_name(remote_name: Option<&str>, branch_name: &str) -> Option<String> {
    let remote = remote_name?.trim();
    let branch = branch_name.trim();
    if remote.is_empty() || branch.is_empty() {
        return None;
    }

    let prefix = format!("{remote}/");
    if branch.starts_with(&prefix) {
        Some(branch.to_string())
    } else {
        Some(format!("{remote}/{branch}"))
    }
}

pub(crate) fn remote_branch_short_name<'a>(
    remote_name: Option<&str>,
    branch_name: &'a str,
) -> Cow<'a, str> {
    let Some(remote) = remote_name
        .map(str::trim)
        .filter(|remote| !remote.is_empty())
    else {
        return Cow::Borrowed(branch_name);
    };

    let prefix = format!("{remote}/");
    match branch_name.strip_prefix(&prefix) {
        Some(short) => Cow::Borrowed(short),
        None => Cow::Borrowed(branch_name),
    }
}

// ─── Container IDs ────────────────────────────────────────────────────────────

pub fn branch_popout_trigger_id() -> iced::widget::Id {
    iced::widget::Id::new("branch-popout-trigger")
}

pub fn branch_popout_panel_id() -> iced::widget::Id {
    iced::widget::Id::new("branch-popout-panel")
}

pub fn branch_popout_content_id() -> iced::widget::Id {
    iced::widget::Id::new("branch-popout-content")
}

#[cfg(test)]
mod tests {
    use super::{remote_branch_short_name, remote_ref_name};

    #[test]
    fn remote_ref_name_combines_remote_and_short_branch() {
        assert_eq!(
            remote_ref_name(Some("origin"), "feature/foo").as_deref(),
            Some("origin/feature/foo")
        );
    }

    #[test]
    fn remote_ref_name_preserves_already_qualified_branch() {
        assert_eq!(
            remote_ref_name(Some("origin"), "origin/feature/foo").as_deref(),
            Some("origin/feature/foo")
        );
        assert_eq!(
            remote_branch_short_name(Some("origin"), "origin/feature/foo").as_ref(),
            "feature/foo"
        );
    }
}
