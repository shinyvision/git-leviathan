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
use crate::screens::repository::RepositoryMessage;

#[derive(Debug, Clone)]
pub enum CoreCommandAction {
    App(AppMessage),
    Repository(Box<RepositoryMessage>),
    OpenRepositoryPath(String),
    CloseTab {
        path: Option<String>,
    },
    SelectTab {
        path: String,
    },
    ReorderTabs {
        paths: Vec<String>,
    },
    Refresh,
    Fetch,
    CreateBranchAtSelected {
        commit_idx: Option<usize>,
        hash: Option<String>,
    },
    CopyCommitHash {
        hash: Option<String>,
    },
    OpenSelectedDiff,
    StartRewordSelected,
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
        .action(|_| Ok(CoreCommandAction::Fetch)),
        spec(
            "repository.refresh",
            "Repository: Refresh",
            "Refresh the active repository.",
            "repository",
        )
        .cap("repo:read")
        .action(|_| Ok(CoreCommandAction::Refresh)),
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
        .cap("git:write:branch")
        .destructive()
        .action(delete_branch),
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
            "Start rewording a commit.",
            "repository",
        )
        .cap("git:write:commit")
        .cap("git:write:rebase")
        .action(|_| Ok(CoreCommandAction::StartRewordSelected)),
        spec(
            "repository.open_search",
            "Repository: Open Search",
            "Open commit search.",
            "repository",
        )
        .cap("command:invoke:repository.open_search")
        .action(|_| Ok(repository(RepositoryMessage::OpenCommitSearch))),
        spec(
            "repository.open_diff",
            "Repository: Open Diff",
            "Open the selected commit diff.",
            "repository",
        )
        .cap("repo:read")
        .action(|_| Ok(CoreCommandAction::OpenSelectedDiff)),
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
    Ok(CoreCommandAction::CreateBranchAtSelected {
        commit_idx: args
            .get("commit_idx")
            .and_then(|v| v.as_i64())
            .map(|v| v.max(0) as usize),
        hash: string_arg(args, "hash"),
    })
}

fn delete_branch(args: &Value) -> Result<CoreCommandAction, String> {
    let remote_name = string_arg(args, "remote_name");
    Ok(repository(RepositoryMessage::Center(
        CenterAction::BranchDeleteRequested {
            branch_name: required_string_arg(args, "name")?,
            is_remote: bool_arg(args, "is_remote"),
            has_remote: bool_arg(args, "has_remote"),
            remote_name,
        },
    )))
}

fn rename_branch(args: &Value) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::Center(
        CenterAction::BranchRenameRequested {
            branch_name: required_string_arg(args, "name")?,
            is_remote: bool_arg(args, "is_remote"),
            remote_name: string_arg(args, "remote_name"),
        },
    )))
}

fn checkout_branch(args: &Value) -> Result<CoreCommandAction, String> {
    let branch = required_string_arg(args, "ref")?;
    let action = if bool_arg(args, "remote") {
        CenterAction::RemoteBranchLabelPressed(branch)
    } else {
        CenterAction::BranchLabelPressed(branch)
    };
    stash(action)
}

fn stash(action: CenterAction) -> Result<CoreCommandAction, String> {
    Ok(repository(RepositoryMessage::Center(action)))
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
    Ok(CoreCommandAction::CopyCommitHash {
        hash: string_arg(args, "hash"),
    })
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
            "branch.delete",
            "stash.pop",
            "repository.open_search",
            "commit.copy_hash",
            "conflict.save_resolution",
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
}
