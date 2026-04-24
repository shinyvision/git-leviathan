//! Right-click context menu construction for branch/tag pills.
use iced::Element;

use crate::message::Message;
use crate::screens::repository::{
    panel_messages::CenterAction, state::ContextMenuState, RepositoryMessage,
};
use crate::widgets::context_menu::{context_menu_item, ContextMenu, ContextMenuItem};

pub fn branch_context_menu(
    state: &ContextMenuState,
    current_branch: &str,
) -> Element<'static, Message> {
    let branch_name = &state.branch_name;
    let is_remote = state.is_remote;
    let has_remote = state.has_remote;
    let is_tag = state.is_tag;
    let remote_name = state.remote_name.clone();

    let remote_display = |name: &str| match remote_name.as_deref() {
        Some(remote) => format!("{}/{}", remote, name),
        None => format!("remote {}", name),
    };

    let mut items: Vec<ContextMenuItem> = Vec::new();

    let copy_label = if is_tag {
        "Copy tag name"
    } else {
        "Copy branch name"
    };
    items.push(context_menu_item(
        copy_label,
        Some(Message::repo(RepositoryMessage::Center(
            CenterAction::CopyBranchName(branch_name.clone()),
        ))),
    ));

    if is_tag {
        if let Some(remote) = state.default_remote_name.as_ref() {
            if !state.tag_remote_names.iter().any(|r| r == remote) {
                items.push(context_menu_item(
                    format!("Push tag to {}", remote),
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::PushTagRequested {
                            tag_name: branch_name.clone(),
                            remote_name: remote.clone(),
                        },
                    ))),
                ));
            }
        }
        items.push(context_menu_item(
            "Delete tag",
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::DeleteTagRequested {
                    tag_name: branch_name.clone(),
                    tag_remote_names: state.tag_remote_names.clone(),
                },
            ))),
        ));
    }

    if !is_tag && !is_remote && !current_branch.is_empty() && current_branch != branch_name {
        items.push(context_menu_item(
            format!("Merge {} into {}", branch_name, current_branch),
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::BranchMergeRequested {
                    source_branch: branch_name.clone(),
                    target_branch: current_branch.to_string(),
                },
            ))),
        ));

        if state.can_fast_forward {
            items.push(context_menu_item(
                format!("Fast-forward {} to {}", current_branch, branch_name),
                Some(Message::repo(RepositoryMessage::Center(
                    CenterAction::BranchFastForwardRequested {
                        source_branch: current_branch.to_string(),
                        target_branch: branch_name.clone(),
                    },
                ))),
            ));
        }
    }

    // Rebase current branch onto the right-clicked target. Target may be a
    // local branch or a remote tracking ref (e.g. "origin/main").
    if !is_tag && !current_branch.is_empty() {
        let same_local = !is_remote && branch_name == current_branch;
        if !same_local {
            let (target_ref, target_display) = if is_remote {
                let remote = remote_name.as_deref().unwrap_or("");
                let remote_branch = state
                    .remote_branch_name
                    .as_deref()
                    .unwrap_or(branch_name.as_str());
                let combined = format!("{}/{}", remote, remote_branch);
                (combined.clone(), combined)
            } else {
                (branch_name.clone(), branch_name.clone())
            };
            items.push(context_menu_item(
                format!("Rebase {} onto {}", current_branch, target_display),
                Some(Message::repo(RepositoryMessage::Center(
                    CenterAction::BranchRebaseRequested {
                        source_branch: current_branch.to_string(),
                        target_ref,
                        target_display,
                    },
                ))),
            ));
        }
    }

    // Local rename/delete — shown whenever a local counterpart exists.
    if !is_tag && !is_remote {
        items.push(context_menu_item(
            format!("Rename {}", branch_name),
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::BranchRenameRequested {
                    branch_name: branch_name.clone(),
                    is_remote: false,
                    remote_name: None,
                },
            ))),
        ));
        items.push(context_menu_item(
            format!("Delete {}", branch_name),
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::BranchDeleteRequested {
                    branch_name: branch_name.clone(),
                    is_remote: false,
                    has_remote,
                    remote_name: remote_name.clone(),
                },
            ))),
        ));
    }

    // Remote rename/delete — shown when a remote counterpart exists.
    if !is_tag && has_remote {
        let remote_target = state
            .remote_branch_name
            .clone()
            .unwrap_or_else(|| branch_name.clone());
        items.push(context_menu_item(
            format!("Rename {}", remote_display(&remote_target)),
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::BranchRenameRequested {
                    branch_name: remote_target.clone(),
                    is_remote: true,
                    remote_name: remote_name.clone(),
                },
            ))),
        ));
        items.push(context_menu_item(
            format!("Delete {}", remote_display(&remote_target)),
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::BranchDeleteRequested {
                    branch_name: remote_target.clone(),
                    is_remote: true,
                    has_remote: false,
                    remote_name: remote_name.clone(),
                },
            ))),
        ));
    }

    ContextMenu::new(items).into()
}
