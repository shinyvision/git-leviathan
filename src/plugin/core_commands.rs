use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Value};

use crate::message::AppMessage;
use crate::plugin::commands::{
    CommandArg, CommandArgType, CommandAvailability, CommandBody, CommandContext,
    CommandDescriptor, CommandHooks, CommandRegistry, HOST_COMMAND_PLUGIN_ID,
};
use crate::plugin::diagnostic::DiagnosticStore;
use crate::screens::repository::panel_messages::{CenterAction, DetailAction, DiffPanelAction};
use crate::screens::repository::{RepositoryFocusTarget, RepositoryMessage};

#[derive(Debug, Clone)]
pub enum CoreCommandAction {
    App(AppMessage),
    Repository(Box<RepositoryMessage>),
    OpenRepositoryPath(String),
    CloseTab { path: Option<String> },
    SelectTab { path: String },
    ReorderTabs { paths: Vec<String> },
    RefreshGrammarRegistry { path: String },
    InstallGrammar { language: String },
    UpdateGrammar { language: String },
    UninstallGrammar { language: String },
}

fn repository(message: RepositoryMessage) -> CoreCommandAction {
    CoreCommandAction::Repository(Box::new(message))
}

#[derive(Clone, Default)]
pub struct CoreCommandActions {
    inner: Rc<RefCell<Vec<CoreCommandAction>>>,
}

impl CoreCommandActions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, action: CoreCommandAction) {
        self.inner.borrow_mut().push(action);
    }

    pub fn drain(&self) -> Vec<CoreCommandAction> {
        std::mem::take(&mut *self.inner.borrow_mut())
    }
}

struct CoreCommandSpec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    context: &'static str,
    args: Vec<CommandArg>,
    destructive: bool,
    plugin_caps: Vec<&'static str>,
    enabled: CommandAvailability,
    keymap: bool,
    palette: bool,
    action: CoreActionKind,
}

#[derive(Clone, Copy)]
enum CoreActionKind {
    Queue(fn(&Value) -> Result<CoreCommandAction, String>),
    Unimplemented(&'static str),
}

pub fn register(
    registry: &mut CommandRegistry,
    actions: CoreCommandActions,
    diagnostics: &DiagnosticStore,
) {
    for spec in specs() {
        let name = spec.name.to_string();
        let action_kind = spec.action;
        let actions_for_body = actions.clone();
        let body = CommandBody::Host(Box::new(move |args| match action_kind {
            CoreActionKind::Queue(build) => {
                actions_for_body.push(build(args)?);
                Ok(())
            }
            CoreActionKind::Unimplemented(reason) => Err(reason.to_string()),
        }));
        registry.register(
            CommandDescriptor {
                name,
                title: spec.title.into(),
                description: spec.description.into(),
                plugin_id: HOST_COMMAND_PLUGIN_ID.into(),
                generation_id: None,
                context: spec.context.into(),
                args: spec.args,
                destructive: spec.destructive,
                capabilities: Vec::new(),
                plugin_invocation_capabilities: spec
                    .plugin_caps
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                availability: spec.enabled,
                keymap_eligible: spec.keymap,
                palette_visible: spec.palette,
                hooks: CommandHooks::result_event_only(),
                run: body,
            },
            diagnostics,
        );
    }
}

fn specs() -> Vec<CoreCommandSpec> {
    use CommandArgType::{Boolean, Integer, String as StringArg};
    vec![
        spec(
            "app.open_repository",
            "App: Open Repository",
            "Open a repository.",
            CommandContext::GLOBAL,
        )
        .arg("path", StringArg, false, None, "Repository path.")
        .cap("command:invoke:app.open_repository")
        .action(open_repository),
        spec(
            "repository.open",
            "Repository: Open",
            "Open a repository.",
            CommandContext::GLOBAL,
        )
        .arg("path", StringArg, false, None, "Repository path.")
        .cap("command:invoke:app.open_repository")
        .action(open_repository),
        spec(
            "tab.close",
            "Tab: Close",
            "Close a repository tab.",
            "tab_bar",
        )
        .arg(
            "path",
            StringArg,
            false,
            None,
            "Tab path; defaults to active.",
        )
        .cap("command:invoke:tab.close")
        .action(close_tab),
        spec(
            "tab.select",
            "Tab: Select",
            "Select a repository tab.",
            "tab_bar",
        )
        .arg("path", StringArg, true, None, "Tab path.")
        .cap("command:invoke:tab.select")
        .action(select_tab),
        spec(
            "tab.reorder",
            "Tab: Reorder",
            "Reorder repository tabs.",
            "tab_bar",
        )
        .arg("paths", StringArg, true, None, "Comma-separated paths.")
        .cap("command:invoke:tab.reorder")
        .action(reorder_tabs),
        spec(
            "repository.fetch",
            "Repository: Fetch",
            "Fetch the active repository.",
            "repository",
        )
        .cap("git:write:fetch")
        .action(|_| Ok(repository(RepositoryMessage::FetchRequested))),
        spec(
            "repository.refresh",
            "Repository: Refresh",
            "Refresh the active repository.",
            "repository",
        )
        .cap("repo:read")
        .action(|_| Ok(repository(RepositoryMessage::RefreshRequested))),
        spec(
            "repository.pull",
            "Repository: Pull",
            "Pull the active branch.",
            "repository",
        )
        .cap("git:write:fetch")
        .cap("git:write:merge")
        .action(|_| Ok(repository(RepositoryMessage::PullRequested))),
        spec(
            "repository.push",
            "Repository: Push",
            "Push the active branch.",
            "repository",
        )
        .cap("git:write:push")
        .action(|_| Ok(repository(RepositoryMessage::PushRequested))),
        spec(
            "branch.create",
            "Branch: Create",
            "Create a branch at a commit.",
            "repository",
        )
        .arg(
            "commit_idx",
            Integer,
            false,
            None,
            "Commit row index; defaults to selected.",
        )
        .arg(
            "hash",
            StringArg,
            false,
            None,
            "Commit hash; defaults to selected.",
        )
        .cap("git:write:branch")
        .action(create_branch),
        spec(
            "tag.create",
            "Tag: Create",
            "Create a tag at a commit.",
            "repository",
        )
        .arg("hash", StringArg, true, None, "Commit hash.")
        .cap("git:write:tag")
        .action(create_tag),
        spec(
            "tag.delete",
            "Tag: Delete",
            "Open delete-tag confirmation.",
            "repository",
        )
        .arg("name", StringArg, true, None, "Tag name.")
        .arg(
            "remote_names",
            StringArg,
            false,
            Some(json!("")),
            "Comma-separated remote names that hold the tag.",
        )
        .cap("git:write:tag")
        .destructive()
        .action(delete_tag),
        spec(
            "tag.push",
            "Tag: Push",
            "Push a tag to a remote.",
            "repository",
        )
        .arg("tag_name", StringArg, true, None, "Tag name.")
        .arg("remote_name", StringArg, true, None, "Remote name.")
        .cap("git:write:push")
        .action(push_tag),
        spec(
            "branch.delete",
            "Branch: Delete",
            "Open delete-branch confirmation.",
            "repository",
        )
        .arg("name", StringArg, true, None, "Branch name.")
        .arg(
            "is_remote",
            Boolean,
            false,
            Some(json!(false)),
            "Remote branch.",
        )
        .arg(
            "has_remote",
            Boolean,
            false,
            Some(json!(false)),
            "Has remote branch.",
        )
        .arg("remote_name", StringArg, false, None, "Remote name.")
        .arg("remote_ref", StringArg, false, None, "Exact remote ref.")
        .cap("git:write:branch")
        .destructive()
        .action(delete_branch),
        spec(
            "branch.delete.local",
            "Branch: Delete Local",
            "Delete a local branch without opening a confirmation.",
            "repository",
        )
        .arg("name", StringArg, true, None, "Branch name.")
        .cap("git:write:branch")
        .destructive()
        .action(delete_branch_local_direct),
        spec(
            "branch.delete.remote",
            "Branch: Delete Remote",
            "Delete a remote branch without opening a confirmation.",
            "repository",
        )
        .arg("name", StringArg, true, None, "Branch name.")
        .arg("remote_name", StringArg, false, None, "Remote name.")
        .arg("remote_ref", StringArg, false, None, "Exact remote ref.")
        .cap("git:write:branch")
        .destructive()
        .action(delete_branch_remote_direct),
        spec(
            "branch.delete.local_and_remote",
            "Branch: Delete Local And Remote",
            "Delete a local branch and matching remote branch without opening a confirmation.",
            "repository",
        )
        .arg("name", StringArg, true, None, "Branch name.")
        .cap("git:write:branch")
        .destructive()
        .action(delete_branch_local_and_remote_direct),
        spec(
            "branch.rename",
            "Branch: Rename",
            "Open rename-branch prompt.",
            "repository",
        )
        .arg("name", StringArg, true, None, "Branch name.")
        .arg(
            "is_remote",
            Boolean,
            false,
            Some(json!(false)),
            "Remote branch.",
        )
        .arg("remote_name", StringArg, false, None, "Remote name.")
        .arg("remote_ref", StringArg, false, None, "Exact remote ref.")
        .cap("git:write:branch")
        .action(rename_branch),
        spec(
            "branch.checkout",
            "Branch: Checkout",
            "Check out a branch.",
            "repository",
        )
        .arg("ref", StringArg, true, None, "Branch or remote branch.")
        .arg(
            "remote",
            Boolean,
            false,
            Some(json!(false)),
            "Treat ref as remote.",
        )
        .cap("git:write:checkout")
        .action(checkout_branch),
        spec(
            "branch.merge",
            "Branch: Merge",
            "Merge a branch into another branch.",
            "repository",
        )
        .arg(
            "source_branch",
            StringArg,
            true,
            None,
            "Branch to merge from.",
        )
        .arg(
            "target_branch",
            StringArg,
            true,
            None,
            "Branch to merge into.",
        )
        .cap("git:write:merge")
        .destructive()
        .action(merge_branch),
        spec(
            "branch.reset",
            "Branch: Reset",
            "Reset the current branch to a commit.",
            "repository",
        )
        .arg("hash", StringArg, true, None, "Commit hash to reset to.")
        .arg(
            "mode",
            CommandArgType::Enum(vec!["soft".into(), "mixed".into(), "hard".into()]),
            false,
            Some(json!("mixed")),
            "soft | mixed | hard",
        )
        .cap("git:write:reset")
        .destructive()
        .action(reset_branch),
        spec("stash.push", "Stash: Push", "Create a stash.", "repository")
            .cap("git:write:stash")
            .action(|_| stash(CenterAction::StashCreateRequested)),
        spec("stash.pop", "Stash: Pop", "Pop a stash.", "repository")
            .arg("index", Integer, false, Some(json!(0)), "Stash index.")
            .cap("git:write:stash")
            .action(stash_pop),
        spec(
            "commit.create",
            "Commit: Create",
            "Commit staged changes.",
            "repository",
        )
        .cap("git:write:commit")
        .action(|_| detail(DetailAction::CommitConfirmed)),
        spec("commit.amend", "Commit: Amend", "Amend HEAD.", "repository")
            .cap("git:write:commit")
            .disabled("commit amend is not implemented")
            .unimplemented("commit amend is not implemented"),
        spec(
            "commit.reword",
            "Commit: Reword",
            "Start rewording the selected commit.",
            "repository",
        )
        .cap("git:write:commit")
        .cap("git:write:rebase")
        .action(|_| Ok(repository(RepositoryMessage::StartRewordSelected))),
        spec(
            "commit.squash_selected",
            "Commit: Squash Selected",
            "Squash the selected commits.",
            "repository",
        )
        .cap("git:write:commit")
        .cap("git:write:rebase")
        .cap("git:write:reset")
        .destructive()
        .action(|_| stash(CenterAction::SquashSelectedCommitsRequested)),
        spec(
            "commit.reword.focus_message",
            "Commit: Focus Reword Message",
            "Focus the active reword message editor.",
            "repository.details",
        )
        .cap("command:invoke:commit.reword.focus_message")
        .action(|_| Ok(repository(RepositoryMessage::FocusRewordMessage))),
        spec(
            "commit.reword.confirm",
            "Commit: Confirm Reword",
            "Confirm the active commit reword.",
            "repository.details",
        )
        .cap("git:write:commit")
        .cap("git:write:rebase")
        .action(|_| detail(DetailAction::RewordConfirmed)),
        spec(
            "commit.reword.cancel",
            "Commit: Cancel Reword",
            "Cancel the active commit reword.",
            "repository.details",
        )
        .cap("command:invoke:commit.reword.cancel")
        .action(|_| detail(DetailAction::RewordCanceled)),
        spec(
            "repository.open_search",
            "Repository: Open Search",
            "Open commit search.",
            "repository",
        )
        .cap("command:invoke:repository.open_search")
        .action(|_| Ok(repository(RepositoryMessage::OpenCommitSearch))),
        spec(
            "repository.jump_top",
            "Repository: Jump Top",
            "Select the first item in the focused repository pane.",
            "repository",
        )
        .cap("repo:read")
        .action(|_| stash(CenterAction::NavigateFirst)),
        spec(
            "repository.jump_bottom",
            "Repository: Jump Bottom",
            "Select the last item in the focused repository pane.",
            "repository",
        )
        .cap("repo:read")
        .action(|_| stash(CenterAction::NavigateLast)),
        spec(
            "repository.open_diff",
            "Repository: Open Diff",
            "Open the selected commit diff.",
            "repository",
        )
        .cap("repo:read")
        .action(|_| Ok(repository(RepositoryMessage::OpenSelectedDiff))),
        spec(
            "repository.move_selection",
            "Repository: Move Selection",
            "Move or extend the selection in the focused repository pane.",
            "repository",
        )
        .arg(
            "direction",
            StringArg,
            true,
            None,
            "first | last | previous | next",
        )
        .arg(
            "extend",
            Boolean,
            false,
            Some(json!(false)),
            "Extend the current selection instead of replacing it.",
        )
        .cap("repo:read")
        .action(move_selection),
        spec(
            "repository.clear_multi_selection",
            "Repository: Clear Multi Selection",
            "Collapse the active repository multi-selection to the focused item.",
            "repository",
        )
        .cap("command:invoke:repository.clear_multi_selection")
        .action(|_| Ok(repository(RepositoryMessage::ClearMultiSelection))),
        spec(
            "repository.escape",
            "Repository: Escape",
            "Run the repository screen's native Escape behavior.",
            "repository",
        )
        .cap("command:invoke:repository.escape")
        .action(|_| Ok(repository(RepositoryMessage::EscapePressed))),
        spec(
            "repository.stage_selected_file",
            "Repository: Stage Selected File",
            "Stage the selected dirty file.",
            "repository",
        )
        .cap("git:write:commit")
        .action(|_| Ok(repository(RepositoryMessage::StageSelectedDirtyFile))),
        spec(
            "repository.unstage_selected_file",
            "Repository: Unstage Selected File",
            "Unstage the selected dirty file.",
            "repository",
        )
        .cap("git:write:reset")
        .action(|_| Ok(repository(RepositoryMessage::UnstageSelectedDirtyFile))),
        spec(
            "repository.discard_selected_file",
            "Repository: Discard Selected File",
            "Open discard confirmation for the selected dirty file.",
            "repository",
        )
        .cap("git:write:reset")
        .destructive()
        .action(|_| Ok(repository(RepositoryMessage::DiscardSelectedDirtyFile))),
        spec(
            "repository.focus_panel",
            "Repository: Focus Panel",
            "Move focus to a repository panel.",
            "repository",
        )
        .arg(
            "panel",
            CommandArgType::Enum(vec![
                "sidebar".into(),
                "center".into(),
                "graph".into(),
                "details".into(),
                "diff".into(),
            ]),
            true,
            None,
            "sidebar | center | graph | details | diff",
        )
        .cap("command:invoke:repository.focus_panel")
        .action(focus_panel),
        spec(
            "repository.focus_commit_message",
            "Repository: Focus Commit Message",
            "Focus the dirty commit-message editor when the details panel is active.",
            "repository.details.dirty",
        )
        .cap("command:invoke:repository.focus_commit_message")
        .action(|_| Ok(repository(RepositoryMessage::FocusCommitMessage))),
        spec(
            "syntax.grammar.refresh_registry",
            "Syntax: Refresh Grammar Registry",
            "Refresh the runtime grammar registry from a local file.",
            CommandContext::GLOBAL,
        )
        .arg("path", StringArg, true, None, "Registry JSON path.")
        .cap("syntax:grammar:manage")
        .action(refresh_grammar_registry),
        spec(
            "syntax.grammar.install",
            "Syntax: Install Grammar",
            "Install a queued runtime grammar.",
            CommandContext::GLOBAL,
        )
        .arg("language", StringArg, true, None, "Language id.")
        .cap("syntax:grammar:manage")
        .action(install_grammar),
        spec(
            "syntax.grammar.update",
            "Syntax: Update Grammar",
            "Update an installed runtime grammar.",
            CommandContext::GLOBAL,
        )
        .arg("language", StringArg, true, None, "Language id.")
        .cap("syntax:grammar:manage")
        .action(update_grammar),
        spec(
            "syntax.grammar.uninstall",
            "Syntax: Uninstall Grammar",
            "Remove an installed runtime grammar.",
            CommandContext::GLOBAL,
        )
        .arg("language", StringArg, true, None, "Language id.")
        .cap("syntax:grammar:manage")
        .destructive()
        .action(uninstall_grammar),
        spec(
            "commit.copy_hash",
            "Commit: Copy Hash",
            "Copy a commit hash.",
            "repository",
        )
        .arg(
            "hash",
            StringArg,
            false,
            None,
            "Hash; defaults to selected.",
        )
        .cap("clipboard:write")
        .action(copy_hash),
        spec(
            "conflict.mark_file_resolved",
            "Conflict: Mark File Resolved",
            "Mark a conflict file resolved.",
            "repository.diff",
        )
        .arg("path", StringArg, true, None, "Conflict file path.")
        .cap("git:write:commit")
        .action(mark_file_resolved),
        spec(
            "conflict.mark_all_resolved",
            "Conflict: Mark All Resolved",
            "Mark all conflict files resolved.",
            "repository.diff",
        )
        .cap("git:write:commit")
        .action(|_| detail(DetailAction::MarkAllConflictsResolved)),
        spec(
            "conflict.save_resolution",
            "Conflict: Save Resolution",
            "Save the open conflict resolution.",
            "repository.diff",
        )
        .cap("git:write:commit")
        .action(|_| diff(DiffPanelAction::ConflictResolutionSaveRequested)),
        spec(
            "conflict.abort_merge",
            "Conflict: Abort Merge",
            "Abort the merge in progress.",
            "repository",
        )
        .cap("git:write:merge")
        .destructive()
        .action(|_| detail(DetailAction::AbortMergeConfirmed)),
    ]
}

fn spec(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    context: &'static str,
) -> CoreCommandSpec {
    CoreCommandSpec {
        name,
        title,
        description,
        context,
        args: Vec::new(),
        destructive: false,
        plugin_caps: Vec::new(),
        enabled: CommandAvailability::enabled(),
        keymap: true,
        palette: true,
        action: CoreActionKind::Unimplemented("unimplemented"),
    }
}

impl CoreCommandSpec {
    fn arg(
        mut self,
        name: &'static str,
        ty: CommandArgType,
        required: bool,
        default: Option<Value>,
        doc: &'static str,
    ) -> Self {
        self.args.push(CommandArg {
            name: name.into(),
            ty,
            required,
            default,
            doc: doc.into(),
        });
        self
    }

    fn cap(mut self, cap: &'static str) -> Self {
        self.plugin_caps.push(cap);
        self
    }

    fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    fn disabled(mut self, reason: &'static str) -> Self {
        self.enabled = CommandAvailability::disabled(reason);
        self
    }

    fn action(mut self, build: fn(&Value) -> Result<CoreCommandAction, String>) -> Self {
        self.action = CoreActionKind::Queue(build);
        self
    }

    fn unimplemented(mut self, reason: &'static str) -> Self {
        self.action = CoreActionKind::Unimplemented(reason);
        self
    }
}

fn open_repository(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(match string_arg(args, "path") {
        Some(path) => CoreCommandAction::OpenRepositoryPath(path),
        None => CoreCommandAction::App(AppMessage::OpenRepoDialog),
    })
}

fn close_tab(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(CoreCommandAction::CloseTab {
        path: string_arg(args, "path"),
    })
}

fn select_tab(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(CoreCommandAction::SelectTab {
        path: required_string_arg(args, "path")?,
    })
}

fn reorder_tabs(args: &Value) -> Result<CoreCommandAction, String> {
    let paths = required_string_arg(args, "paths")?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    Ok(CoreCommandAction::ReorderTabs { paths })
}

fn create_branch(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::CreateBranchAtSelected {
        commit_idx: args
            .get("commit_idx")
            .and_then(|v| v.as_i64())
            .map(|v| v.max(0) as usize),
        hash: string_arg(args, "hash"),
    }))
}

fn create_tag(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::Center(
        CenterAction::CreateTagHereRequested {
            commit_hash: required_string_arg(args, "hash")?,
        },
    )))
}

fn delete_tag(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::Center(
        CenterAction::DeleteTagRequested {
            tag_name: required_string_arg(args, "name")?,
            tag_remote_names: string_list_arg(args, "remote_names"),
        },
    )))
}

fn push_tag(args: &Value) -> Result<CoreCommandAction, String> {
    stash(CenterAction::PushTagRequested {
        tag_name: required_string_arg(args, "tag_name")?,
        remote_name: required_string_arg(args, "remote_name")?,
    })
}

fn delete_branch(args: &Value) -> Result<CoreCommandAction, String> {
    let remote_name = string_arg(args, "remote_name");
    let branch_name = required_string_arg(args, "name")?;
    let remote_ref = string_arg(args, "remote_ref").or_else(|| {
        remote_name
            .as_ref()
            .filter(|remote| !remote.trim().is_empty())
            .map(|remote| format!("{}/{}", remote.trim(), branch_name.trim()))
    });
    Ok(repository(RepositoryMessage::Center(
        CenterAction::BranchDeleteRequested {
            branch_name,
            is_remote: bool_arg(args, "is_remote"),
            has_remote: bool_arg(args, "has_remote"),
            remote_name,
            remote_ref,
        },
    )))
}

fn delete_branch_local_direct(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::DeleteBranchDirect {
        branch_name: required_string_arg(args, "name")?,
        is_remote: false,
        remote_ref: None,
    }))
}

fn delete_branch_remote_direct(args: &Value) -> Result<CoreCommandAction, String> {
    let remote_name = string_arg(args, "remote_name");
    let branch_name = required_string_arg(args, "name")?;
    let remote_ref = string_arg(args, "remote_ref").or_else(|| {
        remote_name
            .as_ref()
            .filter(|remote| !remote.trim().is_empty())
            .map(|remote| format!("{}/{}", remote.trim(), branch_name.trim()))
    });
    Ok(repository(RepositoryMessage::DeleteBranchDirect {
        branch_name,
        is_remote: true,
        remote_ref,
    }))
}

fn delete_branch_local_and_remote_direct(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::DeleteBranchLocalAndRemote {
        branch_name: required_string_arg(args, "name")?,
    }))
}

fn rename_branch(args: &Value) -> Result<CoreCommandAction, String> {
    let remote_name = string_arg(args, "remote_name");
    let branch_name = required_string_arg(args, "name")?;
    let remote_ref = string_arg(args, "remote_ref").or_else(|| {
        remote_name
            .as_ref()
            .filter(|remote| !remote.trim().is_empty())
            .map(|remote| format!("{}/{}", remote.trim(), branch_name.trim()))
    });
    Ok(repository(RepositoryMessage::Center(
        CenterAction::BranchRenameRequested {
            branch_name,
            is_remote: bool_arg(args, "is_remote"),
            remote_name,
            remote_ref,
        },
    )))
}

fn checkout_branch(args: &Value) -> Result<CoreCommandAction, String> {
    let branch = required_string_arg(args, "ref")?;
    let action = if bool_arg(args, "remote") {
        CenterAction::RemoteBranchLabelPressed {
            branch_name: branch,
            remote_ref: None,
        }
    } else {
        CenterAction::BranchLabelPressed(branch)
    };
    stash(action)
}

fn merge_branch(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::Center(
        CenterAction::BranchMergeRequested {
            source_branch: required_string_arg(args, "source_branch")?,
            target_branch: required_string_arg(args, "target_branch")?,
        },
    )))
}

fn reset_branch(args: &Value) -> Result<CoreCommandAction, String> {
    use crate::services::ResetMode;
    let mode = match string_arg(args, "mode").as_deref().unwrap_or("mixed") {
        "soft" => ResetMode::Soft,
        "mixed" => ResetMode::Mixed,
        "hard" => ResetMode::Hard,
        other => {
            return Err(format!(
                "unknown reset mode `{other}`; expected soft, mixed, or hard"
            ))
        }
    };
    Ok(repository(RepositoryMessage::Center(
        CenterAction::ResetToCommitRequested {
            commit_hash: required_string_arg(args, "hash")?,
            mode,
        },
    )))
}

fn stash(action: CenterAction) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::Center(action)))
}

fn move_selection(args: &Value) -> Result<CoreCommandAction, String> {
    let direction = required_string_arg(args, "direction")?;
    let action = match direction.as_str() {
        "first" | "top" => CenterAction::NavigateFirst,
        "last" | "bottom" => CenterAction::NavigateLast,
        "previous" | "prev" | "up" => CenterAction::NavigateUp,
        "next" | "down" => CenterAction::NavigateDown,
        other => {
            return Err(format!(
                "unknown direction `{other}`; expected first, last, previous, or next"
            ));
        }
    };
    Ok(repository(RepositoryMessage::MoveSelection {
        action,
        extend: bool_arg(args, "extend"),
    }))
}

fn stash_pop(args: &Value) -> Result<CoreCommandAction, String> {
    stash(CenterAction::StashPopRequested {
        stash_index: args
            .get("index")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            .max(0) as usize,
    })
}

fn detail(action: DetailAction) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::Detail(action)))
}

fn diff(action: DiffPanelAction) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::DiffPanel(action)))
}

fn copy_hash(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::CopyCommitHash {
        hash: string_arg(args, "hash"),
    }))
}

fn refresh_grammar_registry(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(CoreCommandAction::RefreshGrammarRegistry {
        path: required_string_arg(args, "path")?,
    })
}

fn install_grammar(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(CoreCommandAction::InstallGrammar {
        language: required_string_arg(args, "language")?,
    })
}

fn update_grammar(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(CoreCommandAction::UpdateGrammar {
        language: required_string_arg(args, "language")?,
    })
}

fn uninstall_grammar(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(CoreCommandAction::UninstallGrammar {
        language: required_string_arg(args, "language")?,
    })
}

fn focus_panel(args: &Value) -> Result<CoreCommandAction, String> {
    let raw = required_string_arg(args, "panel")?;
    let target = match raw.as_str() {
        "sidebar" => RepositoryFocusTarget::Sidebar,
        "center" => RepositoryFocusTarget::Center,
        "graph" => RepositoryFocusTarget::Graph,
        "details" => RepositoryFocusTarget::Details,
        "diff" => RepositoryFocusTarget::Diff,
        other => {
            return Err(format!(
                "unknown panel `{other}` (expected sidebar | center | graph | details | diff)"
            ))
        }
    };
    Ok(repository(RepositoryMessage::FocusPanel(target)))
}

fn mark_file_resolved(args: &Value) -> Result<CoreCommandAction, String> {
    detail(DetailAction::MarkConflictResolved(required_string_arg(
        args, "path",
    )?))
}

fn string_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn required_string_arg(args: &Value, key: &str) -> Result<String, String> {
    string_arg(args, key).ok_or_else(|| format!("missing required arg `{key}`"))
}

fn string_list_arg(args: &Value, key: &str) -> Vec<String> {
    string_arg(args, key)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn bool_arg(args: &Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::commands::CommandRegistry;
    use crate::plugin::diagnostic::NullSink;
    use std::sync::Arc;

    fn store() -> DiagnosticStore {
        DiagnosticStore::with_sink(Arc::new(NullSink))
    }

    #[test]
    fn registers_practical_core_commands() {
        let mut registry = CommandRegistry::new();
        register(&mut registry, CoreCommandActions::new(), &store());
        let names: Vec<String> = registry.summaries().into_iter().map(|s| s.name).collect();
        for expected in [
            "app.open_repository",
            "tab.close",
            "repository.pull",
            "repository.push",
            "branch.create",
            "tag.create",
            "tag.delete",
            "tag.push",
            "branch.delete",
            "branch.delete.local",
            "branch.delete.remote",
            "branch.delete.local_and_remote",
            "branch.merge",
            "branch.reset",
            "stash.pop",
            "repository.open_search",
            "repository.jump_top",
            "repository.jump_bottom",
            "repository.move_selection",
            "repository.clear_multi_selection",
            "repository.escape",
            "repository.stage_selected_file",
            "repository.unstage_selected_file",
            "repository.discard_selected_file",
            "repository.focus_panel",
            "repository.focus_commit_message",
            "commit.squash_selected",
            "commit.copy_hash",
            "conflict.save_resolution",
            "syntax.grammar.install",
            "syntax.grammar.update",
            "syntax.grammar.uninstall",
        ] {
            assert!(names.iter().any(|name| name == expected), "{expected}");
        }
    }

    #[test]
    fn pull_command_queues_native_repository_message() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry.find("repository.pull").unwrap().descriptor;
        match &desc.run {
            CommandBody::Host(run) => run(&Value::Null).unwrap(),
            _ => panic!("expected host command"),
        }
        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(message.as_ref(), RepositoryMessage::PullRequested)
        ));
    }

    #[test]
    fn fetch_command_queues_repository_request() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry.find("repository.fetch").unwrap().descriptor;
        match &desc.run {
            CommandBody::Host(run) => run(&Value::Null).unwrap(),
            _ => panic!("expected host command"),
        }
        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(message.as_ref(), RepositoryMessage::FetchRequested)
        ));
    }

    fn is_stage_selected_dirty_file(message: &RepositoryMessage) -> bool {
        matches!(message, RepositoryMessage::StageSelectedDirtyFile)
    }

    fn is_unstage_selected_dirty_file(message: &RepositoryMessage) -> bool {
        matches!(message, RepositoryMessage::UnstageSelectedDirtyFile)
    }

    fn is_discard_selected_dirty_file(message: &RepositoryMessage) -> bool {
        matches!(message, RepositoryMessage::DiscardSelectedDirtyFile)
    }

    fn is_clear_multi_selection(message: &RepositoryMessage) -> bool {
        matches!(message, RepositoryMessage::ClearMultiSelection)
    }

    fn is_escape_pressed(message: &RepositoryMessage) -> bool {
        matches!(message, RepositoryMessage::EscapePressed)
    }

    fn is_focus_commit_message(message: &RepositoryMessage) -> bool {
        matches!(message, RepositoryMessage::FocusCommitMessage)
    }

    fn is_focus_reword_message(message: &RepositoryMessage) -> bool {
        matches!(message, RepositoryMessage::FocusRewordMessage)
    }

    fn is_start_reword_selected(message: &RepositoryMessage) -> bool {
        matches!(message, RepositoryMessage::StartRewordSelected)
    }

    fn is_reword_confirmed(message: &RepositoryMessage) -> bool {
        matches!(
            message,
            RepositoryMessage::Detail(DetailAction::RewordConfirmed)
        )
    }

    fn is_reword_canceled(message: &RepositoryMessage) -> bool {
        matches!(
            message,
            RepositoryMessage::Detail(DetailAction::RewordCanceled)
        )
    }

    fn is_squash_selected_commits(message: &RepositoryMessage) -> bool {
        matches!(
            message,
            RepositoryMessage::Center(CenterAction::SquashSelectedCommitsRequested)
        )
    }

    #[test]
    fn tag_create_command_queues_native_create_dialog() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry.find("tag.create").unwrap().descriptor;
        let args = CommandRegistry::validate_args(
            desc,
            &json!({
                "hash": "abc123",
            }),
        )
        .expect("tag.create args should validate");

        match &desc.run {
            CommandBody::Host(run) => run(&args).unwrap(),
            _ => panic!("expected host command"),
        }

        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(
                    message.as_ref(),
                    RepositoryMessage::Center(CenterAction::CreateTagHereRequested {
                        commit_hash,
                    }) if commit_hash == "abc123"
                )
        ));
    }

    #[test]
    fn tag_delete_command_queues_native_delete_confirmation() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry.find("tag.delete").unwrap().descriptor;
        let args = CommandRegistry::validate_args(
            desc,
            &json!({
                "name": "v1.0.0",
                "remote_names": "origin, upstream,",
            }),
        )
        .expect("tag.delete args should validate");

        match &desc.run {
            CommandBody::Host(run) => run(&args).unwrap(),
            _ => panic!("expected host command"),
        }

        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(
                    message.as_ref(),
                    RepositoryMessage::Center(CenterAction::DeleteTagRequested {
                        tag_name,
                        tag_remote_names,
                    }) if tag_name == "v1.0.0"
                        && tag_remote_names == &vec!["origin".to_string(), "upstream".to_string()]
                )
        ));
    }

    #[test]
    fn tag_push_command_queues_native_push_request() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry.find("tag.push").unwrap().descriptor;
        let args = CommandRegistry::validate_args(
            desc,
            &json!({
                "tag_name": "v1.0.0",
                "remote_name": "origin",
            }),
        )
        .expect("tag.push args should validate");

        match &desc.run {
            CommandBody::Host(run) => run(&args).unwrap(),
            _ => panic!("expected host command"),
        }

        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(
                    message.as_ref(),
                    RepositoryMessage::Center(CenterAction::PushTagRequested {
                        tag_name,
                        remote_name,
                    }) if tag_name == "v1.0.0" && remote_name == "origin"
                )
        ));
    }

    #[test]
    fn selected_file_commands_queue_native_repository_messages() {
        for (command, matches_expected) in [
            (
                "repository.stage_selected_file",
                is_stage_selected_dirty_file as fn(&RepositoryMessage) -> bool,
            ),
            (
                "repository.unstage_selected_file",
                is_unstage_selected_dirty_file,
            ),
            (
                "repository.discard_selected_file",
                is_discard_selected_dirty_file,
            ),
        ] {
            let actions = CoreCommandActions::new();
            let mut registry = CommandRegistry::new();
            register(&mut registry, actions.clone(), &store());
            let desc = &registry.find(command).unwrap().descriptor;
            match &desc.run {
                CommandBody::Host(run) => run(&Value::Null).unwrap(),
                _ => panic!("expected host command"),
            }
            let queued = actions.drain();
            assert!(matches!(
                queued.as_slice(),
                [CoreCommandAction::Repository(message)] if matches_expected(message.as_ref())
            ));
        }
    }

    #[test]
    fn repository_ui_commands_queue_native_repository_messages() {
        for (command, matches_expected) in [
            (
                "repository.clear_multi_selection",
                is_clear_multi_selection as fn(&RepositoryMessage) -> bool,
            ),
            ("repository.escape", is_escape_pressed),
            ("repository.focus_commit_message", is_focus_commit_message),
        ] {
            let actions = CoreCommandActions::new();
            let mut registry = CommandRegistry::new();
            register(&mut registry, actions.clone(), &store());
            let desc = &registry.find(command).unwrap().descriptor;
            match &desc.run {
                CommandBody::Host(run) => run(&Value::Null).unwrap(),
                _ => panic!("expected host command"),
            }
            let queued = actions.drain();
            assert!(matches!(
                queued.as_slice(),
                [CoreCommandAction::Repository(message)] if matches_expected(message.as_ref())
            ));
        }
    }

    #[test]
    fn branch_merge_command_queues_native_merge_request() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry.find("branch.merge").unwrap().descriptor;
        let args = CommandRegistry::validate_args(
            desc,
            &json!({
                "source_branch": "feature",
                "target_branch": "main",
            }),
        )
        .expect("branch.merge args should validate");

        match &desc.run {
            CommandBody::Host(run) => run(&args).unwrap(),
            _ => panic!("expected host command"),
        }

        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(
                    message.as_ref(),
                    RepositoryMessage::Center(CenterAction::BranchMergeRequested {
                        source_branch,
                        target_branch,
                    }) if source_branch == "feature" && target_branch == "main"
                )
        ));
    }

    #[test]
    fn branch_delete_command_accepts_exact_remote_ref() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry.find("branch.delete").unwrap().descriptor;
        let args = CommandRegistry::validate_args(
            desc,
            &json!({
                "name": "feature",
                "is_remote": true,
                "remote_name": "origin",
                "remote_ref": "origin/feature"
            }),
        )
        .expect("branch.delete args should validate");

        match &desc.run {
            CommandBody::Host(run) => run(&args).unwrap(),
            _ => panic!("expected host command"),
        }

        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(
                    message.as_ref(),
                    RepositoryMessage::Center(CenterAction::BranchDeleteRequested {
                        branch_name,
                        is_remote: true,
                        has_remote: false,
                        remote_name: Some(remote_name),
                        remote_ref: Some(remote_ref),
                    }) if branch_name == "feature"
                        && remote_name == "origin"
                        && remote_ref == "origin/feature"
                )
        ));
    }

    #[test]
    fn direct_branch_delete_commands_queue_exact_repository_messages() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());

        let local = &registry.find("branch.delete.local").unwrap().descriptor;
        match &local.run {
            CommandBody::Host(run) => run(&json!({ "name": "feature" })).unwrap(),
            _ => panic!("expected host command"),
        }
        let remote = &registry.find("branch.delete.remote").unwrap().descriptor;
        let args = CommandRegistry::validate_args(
            remote,
            &json!({
                "name": "feature",
                "remote_name": "origin",
                "remote_ref": "origin/feature"
            }),
        )
        .expect("remote delete args should validate");
        match &remote.run {
            CommandBody::Host(run) => run(&args).unwrap(),
            _ => panic!("expected host command"),
        }
        let all = &registry
            .find("branch.delete.local_and_remote")
            .unwrap()
            .descriptor;
        match &all.run {
            CommandBody::Host(run) => run(&json!({ "name": "feature" })).unwrap(),
            _ => panic!("expected host command"),
        }

        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [
                CoreCommandAction::Repository(local_msg),
                CoreCommandAction::Repository(remote_msg),
                CoreCommandAction::Repository(all_msg),
            ] if matches!(
                local_msg.as_ref(),
                RepositoryMessage::DeleteBranchDirect {
                    branch_name,
                    is_remote: false,
                    remote_ref: None,
                } if branch_name == "feature"
            ) && matches!(
                remote_msg.as_ref(),
                RepositoryMessage::DeleteBranchDirect {
                    branch_name,
                    is_remote: true,
                    remote_ref: Some(remote_ref),
                } if branch_name == "feature" && remote_ref == "origin/feature"
            ) && matches!(
                all_msg.as_ref(),
                RepositoryMessage::DeleteBranchLocalAndRemote { branch_name }
                    if branch_name == "feature"
            )
        ));
    }

    fn run_focus_panel(args: Value) -> (Result<(), String>, Vec<CoreCommandAction>) {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry.find("repository.focus_panel").unwrap().descriptor;
        let result = match &desc.run {
            CommandBody::Host(run) => run(&args),
            _ => panic!("expected host command"),
        };
        (result, actions.drain())
    }

    #[test]
    fn focus_panel_command_queues_focus_panel_action() {
        let (result, queued) = run_focus_panel(json!({ "panel": "details" }));
        result.unwrap();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(message.as_ref(), RepositoryMessage::FocusPanel(RepositoryFocusTarget::Details))
        ));
    }

    #[test]
    fn focus_panel_command_rejects_unknown_panel() {
        let (result, queued) = run_focus_panel(json!({ "panel": "weird" }));
        let err = result.unwrap_err();
        assert!(err.contains("unknown panel"), "{err}");
        assert!(queued.is_empty());
    }

    #[test]
    fn focus_panel_command_requires_panel_arg() {
        let (result, queued) = run_focus_panel(Value::Null);
        let err = result.unwrap_err();
        assert!(err.contains("missing required arg"), "{err}");
        assert!(queued.is_empty());
    }

    #[test]
    fn reword_commands_queue_native_repository_messages() {
        for (command, matches_expected) in [
            (
                "commit.reword",
                is_start_reword_selected as fn(&RepositoryMessage) -> bool,
            ),
            (
                "commit.reword.focus_message",
                is_focus_reword_message as fn(&RepositoryMessage) -> bool,
            ),
            ("commit.reword.confirm", is_reword_confirmed),
            ("commit.reword.cancel", is_reword_canceled),
            ("commit.squash_selected", is_squash_selected_commits),
        ] {
            let actions = CoreCommandActions::new();
            let mut registry = CommandRegistry::new();
            register(&mut registry, actions.clone(), &store());
            let desc = &registry.find(command).unwrap().descriptor;
            match &desc.run {
                CommandBody::Host(run) => run(&Value::Null).unwrap(),
                _ => panic!("expected host command"),
            }

            let queued = actions.drain();
            assert!(matches!(
                queued.as_slice(),
                [CoreCommandAction::Repository(message)] if matches_expected(message.as_ref())
            ));
        }
    }

    #[test]
    fn move_selection_command_queues_generic_selection_action() {
        let actions = CoreCommandActions::new();
        let mut registry = CommandRegistry::new();
        register(&mut registry, actions.clone(), &store());
        let desc = &registry
            .find("repository.move_selection")
            .unwrap()
            .descriptor;
        match &desc.run {
            CommandBody::Host(run) => run(&json!({ "direction": "next", "extend": true })).unwrap(),
            _ => panic!("expected host command"),
        }
        let queued = actions.drain();
        assert!(matches!(
            queued.as_slice(),
            [CoreCommandAction::Repository(message)]
                if matches!(
                    message.as_ref(),
                    RepositoryMessage::MoveSelection {
                        action: CenterAction::NavigateDown,
                        extend: true,
                    }
                )
        ));
    }
}
