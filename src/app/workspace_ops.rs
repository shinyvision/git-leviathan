//! Workspace handling for `App`: the switcher panel's state machine (list /
//! form / delete confirmation), the tab context menu, and the actions they
//! trigger on `TabManager`. Also rebuilds the `WorkspaceSwitcherModel` the
//! tab bar renders.

use iced::Task;

use crate::assets;
use crate::core::{TabId, WorkspaceId};
use crate::message::{Message, WorkspaceMessage};
use crate::plugin::tab_snapshot::TabRegistryOp;
use crate::toast::ToastData;
use crate::widgets::chrome::workspace_switcher::{
    PanelMode, TabMenu, WorkspaceEntry, WorkspaceForm, WorkspaceSwitcherModel, NAME_INPUT_ID,
};

use super::workspaces::NameError;
use super::App;

/// Workspace UI state owned by `App`.
#[derive(Debug, Default)]
pub struct WorkspaceUi {
    /// Panel mode while the switcher panel is open.
    pub panel: Option<PanelMode>,
    /// Row picked with the arrow keys in the panel list.
    pub highlight: Option<WorkspaceId>,
    /// Open tab context menu.
    pub tab_menu: Option<TabMenu>,
}

impl App {
    pub(super) fn update_workspace(&mut self, msg: WorkspaceMessage) -> Task<Message> {
        match msg {
            WorkspaceMessage::TogglePanel => {
                self.workspace_ui.tab_menu = None;
                self.workspace_ui.panel = match self.workspace_ui.panel {
                    Some(_) => None,
                    None => Some(PanelMode::List),
                };
                Task::none()
            }
            WorkspaceMessage::ClosePanel => {
                self.workspace_ui.panel = None;
                Task::none()
            }
            WorkspaceMessage::PanelEscape => {
                self.workspace_ui.panel = match self.workspace_ui.panel {
                    Some(PanelMode::List) | None => None,
                    Some(_) => Some(PanelMode::List),
                };
                Task::none()
            }
            WorkspaceMessage::BackToList => {
                if self.workspace_ui.panel.is_some() {
                    self.workspace_ui.panel = Some(PanelMode::List);
                }
                Task::none()
            }
            WorkspaceMessage::Select(id) => {
                self.workspace_ui.panel = None;
                self.switch_workspace(id)
            }
            WorkspaceMessage::Next => {
                let id = self
                    .tabs
                    .workspaces()
                    .cycle(self.tabs.active_workspace(), 1);
                self.switch_workspace(id)
            }
            WorkspaceMessage::Previous => {
                let id = self
                    .tabs
                    .workspaces()
                    .cycle(self.tabs.active_workspace(), -1);
                self.switch_workspace(id)
            }
            WorkspaceMessage::Reorder(order) => {
                self.tabs.reorder_workspaces(&order);
                Task::none()
            }
            WorkspaceMessage::StartCreate => self.open_form(None, None),
            WorkspaceMessage::StartCreateWithTab(tab_id) => {
                self.workspace_ui.tab_menu = None;
                self.open_form(None, Some(tab_id))
            }
            WorkspaceMessage::StartEdit(id) => self.open_form(Some(id), None),
            WorkspaceMessage::StartDelete(id) => {
                if self.tabs.workspaces().get(id).is_some() {
                    self.workspace_ui.panel = Some(PanelMode::ConfirmDelete(id));
                }
                Task::none()
            }
            WorkspaceMessage::NameChanged(name) => {
                if let Some(PanelMode::Form(mut form)) = self.workspace_ui.panel.take() {
                    form.name = name;
                    self.validate_form(&mut form, false);
                    self.workspace_ui.panel = Some(PanelMode::Form(form));
                }
                Task::none()
            }
            WorkspaceMessage::IconPicked(icon) => {
                if let Some(PanelMode::Form(form)) = self.workspace_ui.panel.as_mut() {
                    form.icon = assets::workspace_icon_key(icon);
                    // Keep typing flowing into the name after picking an icon.
                    return focus_name_input();
                }
                Task::none()
            }
            WorkspaceMessage::SubmitForm => self.submit_form(),
            WorkspaceMessage::ConfirmDelete => {
                let Some(PanelMode::ConfirmDelete(id)) = self.workspace_ui.panel else {
                    return Task::none();
                };
                self.workspace_ui.panel = Some(PanelMode::List);
                let active_before = self.tabs.active_tab_id();
                let task = self.tabs.delete_workspace(id);
                self.plugin_tabs_dirty = true;
                if self.tabs.active_tab_id() != active_before {
                    Task::batch(vec![task, self.try_start_fetch()])
                } else {
                    task
                }
            }
            WorkspaceMessage::MoveTab { tab_id, workspace } => {
                self.workspace_ui.tab_menu = None;
                self.move_tab(tab_id, workspace)
            }
            WorkspaceMessage::OpenTabMenu { tab_id, position } => {
                self.workspace_ui.panel = None;
                self.workspace_ui.tab_menu = Some(TabMenu { tab_id, position });
                Task::none()
            }
            WorkspaceMessage::CloseTabMenu => {
                self.workspace_ui.tab_menu = None;
                Task::none()
            }
            WorkspaceMessage::CloseTab(tab_id) => {
                self.workspace_ui.tab_menu = None;
                let Some(path) = self.tabs.entry(tab_id).map(|t| t.path_key().to_string()) else {
                    return Task::none();
                };
                self.apply_tab_registry_op(TabRegistryOp::Remove(path))
                    .unwrap_or_else(Task::none)
            }
        }
    }

    /// Switch workspace (no-op for the active or an unknown one) and fetch
    /// the tab it lands on, like selecting a tab does.
    pub(super) fn switch_workspace(&mut self, id: WorkspaceId) -> Task<Message> {
        if id == self.tabs.active_workspace() || !self.tabs.workspaces().contains(id) {
            return Task::none();
        }
        let task = self.tabs.select_workspace(id);
        Task::batch(vec![task, self.try_start_fetch()])
    }

    /// Select a workspace by name (case-insensitive), for commands.
    pub(super) fn select_workspace_named(&mut self, name: &str) -> Task<Message> {
        let wanted = name.trim().to_lowercase();
        let id = self
            .tabs
            .workspaces()
            .ordered_ids()
            .into_iter()
            .find(|id| self.tabs.workspaces().name(*id).to_lowercase() == wanted);
        match id {
            Some(id) => self.switch_workspace(id),
            None => self.show_toast(ToastData::error(
                "Workspace Not Found",
                format!("There is no workspace called “{}”.", name.trim()),
            )),
        }
    }

    fn move_tab(&mut self, tab_id: TabId, workspace: WorkspaceId) -> Task<Message> {
        let Some(tab_name) = self.tabs.entry(tab_id).map(|t| t.name.clone()) else {
            return Task::none();
        };
        if self.tabs.entry(tab_id).map(|t| t.workspace) == Some(workspace)
            || !self.tabs.workspaces().contains(workspace)
        {
            return Task::none();
        }
        let workspace_name = self.tabs.workspaces().name(workspace).to_string();
        let was_active = self.tabs.active_tab_id() == tab_id;
        let task = self.tabs.move_tab_to_workspace(tab_id, workspace, false);
        self.plugin_tabs_dirty = true;
        let toast = self.show_toast(ToastData::success(
            "Tab Moved",
            format!("“{tab_name}” is now in “{workspace_name}”."),
        ));
        let fetch = if was_active {
            self.try_start_fetch()
        } else {
            Task::none()
        };
        Task::batch(vec![task, toast, fetch])
    }

    fn open_form(
        &mut self,
        editing: Option<WorkspaceId>,
        moving_tab: Option<TabId>,
    ) -> Task<Message> {
        let form = match editing {
            Some(id) => {
                let Some(existing) = self.tabs.workspaces().get(id) else {
                    return Task::none();
                };
                WorkspaceForm {
                    editing: Some(id),
                    name: existing.name.clone(),
                    icon: assets::workspace_icon_key(&existing.icon),
                    error: None,
                    valid: true,
                    moving_tab: None,
                }
            }
            None => WorkspaceForm {
                editing: None,
                name: String::new(),
                icon: self.tabs.workspaces().suggested_icon(),
                error: None,
                valid: false,
                moving_tab: moving_tab
                    .and_then(|id| self.tabs.entry(id).map(|t| (id, t.name.clone()))),
            },
        };
        self.workspace_ui.panel = Some(PanelMode::Form(form));
        focus_name_input()
    }

    /// Refresh `valid` / `error`. An empty name only disables the submit
    /// button while typing; on submit it explains itself.
    fn validate_form(&self, form: &mut WorkspaceForm, submitting: bool) -> Option<String> {
        match self
            .tabs
            .workspaces()
            .validate_name(&form.name, form.editing)
        {
            Ok(name) => {
                form.valid = true;
                form.error = None;
                Some(name)
            }
            Err(error) => {
                form.valid = false;
                form.error = (submitting || error != NameError::Empty).then(|| error.message());
                None
            }
        }
    }

    fn submit_form(&mut self) -> Task<Message> {
        let Some(PanelMode::Form(mut form)) = self.workspace_ui.panel.take() else {
            return Task::none();
        };
        let Some(name) = self.validate_form(&mut form, true) else {
            self.workspace_ui.panel = Some(PanelMode::Form(form));
            return focus_name_input();
        };
        let icon = form.icon.to_string();
        match form.editing {
            Some(id) => {
                self.tabs.update_workspace(id, name, icon);
                self.workspace_ui.panel = Some(PanelMode::List);
                Task::none()
            }
            None => {
                let id = self.tabs.create_workspace(name, icon);
                match form.moving_tab {
                    Some((tab_id, _)) => {
                        let task = self.tabs.move_tab_to_workspace(tab_id, id, true);
                        self.plugin_tabs_dirty = true;
                        Task::batch(vec![task, self.try_start_fetch()])
                    }
                    None => self.switch_workspace(id),
                }
            }
        }
    }

    /// Rebuild the switcher snapshot the tab bar renders. Cheap: one entry
    /// per workspace.
    pub(super) fn refresh_workspace_switcher(&mut self) {
        let workspaces = self.tabs.workspaces();
        let entries = workspaces
            .ordered_ids()
            .into_iter()
            .map(|id| WorkspaceEntry {
                id,
                name: workspaces.name(id).to_string(),
                icon: assets::workspace_icon(workspaces.icon(id)),
                tab_count: self.tabs.tab_count(id),
            })
            .collect();
        // Drop panel / menu state that points at something gone (a deleted
        // workspace, a closed tab).
        if let Some(PanelMode::ConfirmDelete(id))
        | Some(PanelMode::Form(WorkspaceForm {
            editing: Some(id), ..
        })) = self.workspace_ui.panel
        {
            if workspaces.get(id).is_none() {
                self.workspace_ui.panel = Some(PanelMode::List);
            }
        }
        if self.workspace_ui.panel != Some(PanelMode::List)
            || self
                .workspace_ui
                .highlight
                .is_some_and(|id| !workspaces.contains(id))
        {
            self.workspace_ui.highlight = None;
        }
        if let Some(menu) = &self.workspace_ui.tab_menu {
            let visible = self
                .tabs
                .entry(menu.tab_id)
                .is_some_and(|t| t.workspace == self.tabs.active_workspace());
            if !visible {
                self.workspace_ui.tab_menu = None;
            }
        }
        // The no-git screen has no tab bar to anchor a popup to; an invisible
        // popup would still swallow the keyboard.
        if self.no_git_screen.is_some() {
            self.workspace_ui.panel = None;
            self.workspace_ui.tab_menu = None;
        }
        self.workspace_switcher = WorkspaceSwitcherModel {
            active: self.tabs.active_workspace(),
            entries,
            panel: self.workspace_ui.panel.clone(),
            highlight: self.workspace_ui.highlight,
        };
    }

    /// Whether the tab bar (and with it the workspace switcher) is on screen
    /// while the active workspace has no tabs. Only a fresh start with
    /// nothing open shows the bare welcome screen — unless the panel is open
    /// (e.g. via `workspace.new`), which needs its button to hang from.
    pub(super) fn workspace_chrome_visible(&self) -> bool {
        self.no_git_screen.is_none()
            && (self.tabs.has_any_tabs()
                || !self.tabs.workspaces().custom().is_empty()
                || self.workspace_ui.panel.is_some())
    }

    /// Keys while the workspace panel or a tab menu is open. Returns `None`
    /// when neither is open. Typing goes to the focused name field before it
    /// gets here; whatever reaches us must not leak into the screen below.
    pub(super) fn handle_workspace_popup_key(
        &mut self,
        key: &iced::keyboard::Key,
    ) -> Option<Task<Message>> {
        use iced::keyboard::{key::Named, Key};
        if self.workspace_ui.tab_menu.is_some() {
            if matches!(key, Key::Named(Named::Escape)) {
                self.workspace_ui.tab_menu = None;
            }
            return Some(Task::none());
        }
        let mode = self.workspace_ui.panel.as_ref()?;
        let task = match (mode, key) {
            (PanelMode::List, Key::Named(Named::ArrowDown)) => {
                self.move_highlight(1);
                Task::none()
            }
            (PanelMode::List, Key::Named(Named::ArrowUp)) => {
                self.move_highlight(-1);
                Task::none()
            }
            (PanelMode::List, Key::Named(Named::Enter)) => match self.workspace_ui.highlight {
                Some(id) => self.update_workspace(WorkspaceMessage::Select(id)),
                None => {
                    self.workspace_ui.panel = None;
                    Task::none()
                }
            },
            (PanelMode::Form(_), Key::Named(Named::Enter)) => self.submit_form(),
            (PanelMode::Form(_), Key::Named(Named::Tab)) => focus_name_input(),
            (PanelMode::ConfirmDelete(_), Key::Named(Named::Enter)) => {
                self.update_workspace(WorkspaceMessage::ConfirmDelete)
            }
            (_, Key::Named(Named::Escape)) => self.update_workspace(WorkspaceMessage::PanelEscape),
            _ => Task::none(),
        };
        Some(task)
    }
}

impl App {
    /// Arrow-key navigation in the panel list. The first press moves away
    /// from the active workspace; the highlight wraps at either end.
    fn move_highlight(&mut self, step: isize) {
        let from = self
            .workspace_ui
            .highlight
            .filter(|id| self.tabs.workspaces().contains(*id))
            .unwrap_or_else(|| self.tabs.active_workspace());
        self.workspace_ui.highlight = Some(self.tabs.workspaces().cycle(from, step));
    }
}

fn focus_name_input() -> Task<Message> {
    let id = iced::widget::Id::new(NAME_INPUT_ID);
    Task::batch([
        iced::widget::operation::focus(id.clone()),
        iced::widget::operation::move_cursor_to_end(id),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::{self, TempRepo};

    fn app_with_repos(count: usize) -> (Vec<TempRepo>, App) {
        let mut app = super::super::test_app();
        let repos: Vec<TempRepo> = (0..count)
            .map(|i| test_support::init_test_repo(&format!("workspace_ops_{i}")).0)
            .collect();
        let _ = app.tabs.load_initial_repos(
            repos
                .iter()
                .map(|r| (r.path_str().to_string(), WorkspaceId::DEFAULT))
                .collect(),
            None,
        );
        app.refresh_workspace_switcher();
        (repos, app)
    }

    fn send(app: &mut App, msg: WorkspaceMessage) {
        let _ = app.update_workspace(msg);
        app.refresh_workspace_switcher();
    }

    fn form(app: &App) -> &WorkspaceForm {
        match app.workspace_ui.panel.as_ref() {
            Some(PanelMode::Form(form)) => form,
            other => panic!("expected form, got {other:?}"),
        }
    }

    fn create(app: &mut App, name: &str) -> WorkspaceId {
        send(app, WorkspaceMessage::TogglePanel);
        send(app, WorkspaceMessage::StartCreate);
        send(app, WorkspaceMessage::NameChanged(name.into()));
        send(app, WorkspaceMessage::SubmitForm);
        app.tabs.active_workspace()
    }

    #[test]
    fn panel_toggles_and_escape_steps_back() {
        let (_repos, mut app) = app_with_repos(1);
        send(&mut app, WorkspaceMessage::TogglePanel);
        assert_eq!(app.workspace_ui.panel, Some(PanelMode::List));
        assert_eq!(app.workspace_switcher.panel, Some(PanelMode::List));
        send(&mut app, WorkspaceMessage::StartCreate);
        assert!(matches!(app.workspace_ui.panel, Some(PanelMode::Form(_))));
        send(&mut app, WorkspaceMessage::PanelEscape);
        assert_eq!(app.workspace_ui.panel, Some(PanelMode::List));
        send(&mut app, WorkspaceMessage::PanelEscape);
        assert_eq!(app.workspace_ui.panel, None);
        send(&mut app, WorkspaceMessage::TogglePanel);
        send(&mut app, WorkspaceMessage::TogglePanel);
        assert_eq!(app.workspace_ui.panel, None);
    }

    #[test]
    fn create_form_validates_then_creates_and_switches() {
        let (_repos, mut app) = app_with_repos(1);
        send(&mut app, WorkspaceMessage::TogglePanel);
        send(&mut app, WorkspaceMessage::StartCreate);
        assert!(!form(&app).valid);
        assert!(form(&app).error.is_none(), "no nagging before typing");

        send(&mut app, WorkspaceMessage::NameChanged("workspace".into()));
        assert!(!form(&app).valid);
        assert!(form(&app).error.is_some());

        send(&mut app, WorkspaceMessage::NameChanged("   ".into()));
        assert!(form(&app).error.is_none());
        send(&mut app, WorkspaceMessage::SubmitForm);
        assert!(
            form(&app).error.is_some(),
            "submitting an empty name explains itself"
        );

        send(&mut app, WorkspaceMessage::IconPicked("rocket"));
        send(
            &mut app,
            WorkspaceMessage::NameChanged("  Client work ".into()),
        );
        assert!(form(&app).valid);
        send(&mut app, WorkspaceMessage::SubmitForm);

        assert_eq!(app.workspace_ui.panel, None);
        let created = app.tabs.active_workspace();
        assert!(!created.is_default());
        assert_eq!(app.tabs.workspaces().name(created), "Client work");
        assert_eq!(app.tabs.workspaces().icon(created), "rocket");
        // The new workspace is empty: no active tab, blank screen.
        assert!(app.tabs.is_empty());
        assert_eq!(app.workspace_switcher.active, created);
        assert_eq!(app.workspace_switcher.entries.len(), 2);
        assert_eq!(app.workspace_switcher.entries[0].tab_count, 1);
        assert_eq!(app.workspace_switcher.entries[1].tab_count, 0);
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let (_repos, mut app) = app_with_repos(1);
        create(&mut app, "Facilo");
        send(&mut app, WorkspaceMessage::TogglePanel);
        send(&mut app, WorkspaceMessage::StartCreate);
        send(&mut app, WorkspaceMessage::NameChanged("FACILO".into()));
        assert!(!form(&app).valid);
        send(&mut app, WorkspaceMessage::SubmitForm);
        assert!(matches!(app.workspace_ui.panel, Some(PanelMode::Form(_))));
        assert_eq!(app.tabs.workspaces().custom().len(), 1);
    }

    #[test]
    fn edit_renames_and_returns_to_list() {
        let (_repos, mut app) = app_with_repos(1);
        let id = create(&mut app, "Facilo");
        send(&mut app, WorkspaceMessage::TogglePanel);
        send(&mut app, WorkspaceMessage::StartEdit(id));
        assert_eq!(form(&app).name, "Facilo");
        assert!(form(&app).valid, "an unchanged name is valid when editing");
        send(&mut app, WorkspaceMessage::NameChanged("Facilo BV".into()));
        send(&mut app, WorkspaceMessage::IconPicked("bug"));
        send(&mut app, WorkspaceMessage::SubmitForm);
        assert_eq!(app.workspace_ui.panel, Some(PanelMode::List));
        assert_eq!(app.tabs.workspaces().name(id), "Facilo BV");
        assert_eq!(app.tabs.workspaces().icon(id), "bug");
    }

    #[test]
    fn move_to_new_workspace_follows_the_tab() {
        let (_repos, mut app) = app_with_repos(2);
        let tab = app.tabs.active_tab_id();
        send(
            &mut app,
            WorkspaceMessage::OpenTabMenu {
                tab_id: tab,
                position: iced::Point::new(10.0, 10.0),
            },
        );
        assert!(app.workspace_ui.tab_menu.is_some());
        send(&mut app, WorkspaceMessage::StartCreateWithTab(tab));
        assert!(app.workspace_ui.tab_menu.is_none());
        assert!(form(&app).moving_tab.is_some());
        send(&mut app, WorkspaceMessage::NameChanged("Facilo".into()));
        send(&mut app, WorkspaceMessage::SubmitForm);

        let created = app.tabs.active_workspace();
        assert!(!created.is_default());
        assert_eq!(app.tabs.active_tab_id(), tab);
        assert_eq!(app.tabs.tab_count(created), 1);
        assert_eq!(app.tabs.tab_count(WorkspaceId::DEFAULT), 1);
    }

    #[test]
    fn move_tab_keeps_workspace_and_select_switches() {
        let (_repos, mut app) = app_with_repos(2);
        let id = create(&mut app, "Facilo");
        send(&mut app, WorkspaceMessage::Select(WorkspaceId::DEFAULT));
        let tab = app.tabs.active_tab_id();
        send(
            &mut app,
            WorkspaceMessage::MoveTab {
                tab_id: tab,
                workspace: id,
            },
        );
        assert_eq!(app.tabs.active_workspace(), WorkspaceId::DEFAULT);
        assert_ne!(app.tabs.active_tab_id(), tab);
        assert_eq!(app.tabs.tab_count(id), 1);

        send(&mut app, WorkspaceMessage::Select(id));
        assert_eq!(app.tabs.active_workspace(), id);
        assert_eq!(app.tabs.active_tab_id(), tab);
        assert_eq!(app.workspace_ui.panel, None);
    }

    #[test]
    fn next_and_previous_cycle_workspaces() {
        let (_repos, mut app) = app_with_repos(1);
        let a = create(&mut app, "A");
        let b = create(&mut app, "B");
        assert_eq!(app.tabs.active_workspace(), b);
        send(&mut app, WorkspaceMessage::Next);
        assert_eq!(app.tabs.active_workspace(), WorkspaceId::DEFAULT);
        send(&mut app, WorkspaceMessage::Next);
        assert_eq!(app.tabs.active_workspace(), a);
        send(&mut app, WorkspaceMessage::Previous);
        assert_eq!(app.tabs.active_workspace(), WorkspaceId::DEFAULT);
    }

    #[test]
    fn reorder_message_updates_sort_order() {
        let (_repos, mut app) = app_with_repos(1);
        let a = create(&mut app, "A");
        let b = create(&mut app, "B");
        send(&mut app, WorkspaceMessage::Reorder(vec![b, a]));
        let ids: Vec<WorkspaceId> = app
            .workspace_switcher
            .entries
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, vec![WorkspaceId::DEFAULT, b, a]);
    }

    #[test]
    fn confirm_delete_moves_tabs_home() {
        let (_repos, mut app) = app_with_repos(2);
        let tab = app.tabs.active_tab_id();
        send(&mut app, WorkspaceMessage::StartCreateWithTab(tab));
        send(&mut app, WorkspaceMessage::NameChanged("Doomed".into()));
        send(&mut app, WorkspaceMessage::SubmitForm);
        let doomed = app.tabs.active_workspace();

        send(&mut app, WorkspaceMessage::TogglePanel);
        send(&mut app, WorkspaceMessage::StartDelete(doomed));
        assert_eq!(
            app.workspace_ui.panel,
            Some(PanelMode::ConfirmDelete(doomed))
        );
        send(&mut app, WorkspaceMessage::ConfirmDelete);
        assert_eq!(app.workspace_ui.panel, Some(PanelMode::List));
        assert!(!app.tabs.workspaces().contains(doomed));
        assert_eq!(app.tabs.active_workspace(), WorkspaceId::DEFAULT);
        assert_eq!(app.tabs.active_tab_id(), tab);
        assert_eq!(app.tabs.tab_count(WorkspaceId::DEFAULT), 2);
    }

    #[test]
    fn default_workspace_cannot_be_edited_or_deleted() {
        let (_repos, mut app) = app_with_repos(1);
        send(&mut app, WorkspaceMessage::TogglePanel);
        send(&mut app, WorkspaceMessage::StartEdit(WorkspaceId::DEFAULT));
        assert_eq!(app.workspace_ui.panel, Some(PanelMode::List));
        send(
            &mut app,
            WorkspaceMessage::StartDelete(WorkspaceId::DEFAULT),
        );
        assert_eq!(app.workspace_ui.panel, Some(PanelMode::List));
    }

    #[test]
    fn popup_keys_are_swallowed_and_enter_submits() {
        use iced::keyboard::{key::Named, Key};
        let (_repos, mut app) = app_with_repos(1);
        assert!(app
            .handle_workspace_popup_key(&Key::Named(Named::Enter))
            .is_none());

        send(&mut app, WorkspaceMessage::TogglePanel);
        assert!(app
            .handle_workspace_popup_key(&Key::Character("j".into()))
            .is_some());
        send(&mut app, WorkspaceMessage::StartCreate);
        send(&mut app, WorkspaceMessage::NameChanged("Keys".into()));
        let _ = app.handle_workspace_popup_key(&Key::Named(Named::Enter));
        assert_eq!(app.workspace_ui.panel, None);
        assert_eq!(
            app.tabs.workspaces().name(app.tabs.active_workspace()),
            "Keys"
        );

        send(&mut app, WorkspaceMessage::Select(WorkspaceId::DEFAULT));
        let tab = app.tabs.active_tab_id();
        send(
            &mut app,
            WorkspaceMessage::OpenTabMenu {
                tab_id: tab,
                position: iced::Point::ORIGIN,
            },
        );
        assert!(app.workspace_ui.tab_menu.is_some());
        assert!(app
            .handle_workspace_popup_key(&Key::Character("j".into()))
            .is_some());
        assert!(app.workspace_ui.tab_menu.is_some());
        let _ = app.handle_workspace_popup_key(&Key::Named(Named::Escape));
        assert!(app.workspace_ui.tab_menu.is_none());
    }

    #[test]
    fn open_panel_keeps_the_tab_bar_on_the_welcome_screen() {
        let mut app = super::super::test_app();
        assert!(!app.workspace_chrome_visible());
        // `workspace.new` from the bare welcome screen.
        send(&mut app, WorkspaceMessage::StartCreate);
        assert!(app.workspace_chrome_visible());
        send(&mut app, WorkspaceMessage::NameChanged("Temp".into()));
        send(&mut app, WorkspaceMessage::SubmitForm);
        let id = app.tabs.active_workspace();
        assert!(app.workspace_chrome_visible());

        // Deleting the only workspace leaves the panel open (and visible);
        // closing it brings the bare welcome screen back and frees the keys.
        send(&mut app, WorkspaceMessage::TogglePanel);
        send(&mut app, WorkspaceMessage::StartDelete(id));
        send(&mut app, WorkspaceMessage::ConfirmDelete);
        assert_eq!(app.workspace_ui.panel, Some(PanelMode::List));
        assert!(app.workspace_chrome_visible());
        send(&mut app, WorkspaceMessage::PanelEscape);
        assert!(!app.workspace_chrome_visible());
        assert!(app
            .handle_workspace_popup_key(&iced::keyboard::Key::Character("j".into()))
            .is_none());
    }

    #[test]
    fn tab_menu_closes_when_its_tab_leaves_view() {
        let (_repos, mut app) = app_with_repos(2);
        let a = create(&mut app, "A");
        send(&mut app, WorkspaceMessage::Select(WorkspaceId::DEFAULT));
        let tab = app.tabs.active_tab_id();
        send(
            &mut app,
            WorkspaceMessage::OpenTabMenu {
                tab_id: tab,
                position: iced::Point::ORIGIN,
            },
        );
        send(&mut app, WorkspaceMessage::Select(a));
        assert!(app.workspace_ui.tab_menu.is_none());
    }

    #[test]
    fn arrow_keys_highlight_and_enter_switches() {
        use iced::keyboard::{key::Named, Key};
        let (_repos, mut app) = app_with_repos(1);
        let a = create(&mut app, "A");
        send(&mut app, WorkspaceMessage::Select(WorkspaceId::DEFAULT));
        send(&mut app, WorkspaceMessage::TogglePanel);
        let _ = app.handle_workspace_popup_key(&Key::Named(Named::ArrowDown));
        app.refresh_workspace_switcher();
        assert_eq!(app.workspace_switcher.highlight, Some(a));
        let _ = app.handle_workspace_popup_key(&Key::Named(Named::ArrowDown));
        assert_eq!(app.workspace_ui.highlight, Some(WorkspaceId::DEFAULT));
        let _ = app.handle_workspace_popup_key(&Key::Named(Named::ArrowUp));
        assert_eq!(app.workspace_ui.highlight, Some(a));
        let _ = app.handle_workspace_popup_key(&Key::Named(Named::Enter));
        app.refresh_workspace_switcher();
        assert_eq!(app.tabs.active_workspace(), a);
        assert_eq!(app.workspace_ui.panel, None);
        assert_eq!(app.workspace_ui.highlight, None);
    }

    #[test]
    fn tab_menu_for_closed_tab_is_dropped() {
        let (_repos, mut app) = app_with_repos(1);
        send(
            &mut app,
            WorkspaceMessage::OpenTabMenu {
                tab_id: crate::core::TabId(999),
                position: iced::Point::ORIGIN,
            },
        );
        assert!(app.workspace_ui.tab_menu.is_none());
    }

    #[test]
    fn plugin_snapshot_shows_only_active_workspace() {
        let (_repos, mut app) = app_with_repos(2);
        app.process_tab_changes();
        assert_eq!(app.plugin_host.tab_snapshot().tabs.len(), 2);
        let tab = app.tabs.active_tab_id();
        send(&mut app, WorkspaceMessage::StartCreateWithTab(tab));
        send(&mut app, WorkspaceMessage::NameChanged("Solo".into()));
        send(&mut app, WorkspaceMessage::SubmitForm);
        app.process_tab_changes();
        let snapshot = app.plugin_host.tab_snapshot();
        assert_eq!(snapshot.tabs.len(), 1);
        assert_eq!(snapshot.active_id, Some(tab));
        assert_eq!(snapshot.workspace_id, app.tabs.active_workspace().raw());
    }
}
