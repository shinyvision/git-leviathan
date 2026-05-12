mod animation;
mod commands;
mod commit_search;
mod input;
mod messages;
mod overlays;
pub(crate) mod panel_messages;
mod panels;
pub(crate) mod state;
mod view;

pub use messages::{RepositoryFocusTarget, RepositoryMessage};

use iced::{event, keyboard, mouse, Element, Subscription, Task};
use std::sync::Arc;

use crate::{
    core::TabId,
    message::{AppMessage, Message},
    screens::screen_trait::{Screen, ToolbarCtx},
    services::{presenter::Presenter, SettingsService, COMMIT_LOAD_LIMIT},
    view_model::SidebarSectionKind,
    work::{git_read_work, git_write_work},
};

use self::{
    input::InputState,
    overlays::{DialogCtx, OverlayManager},
    panel_messages::{CenterAction, DetailAction, DiffPanelAction, OverlayPanelAction},
    panels::{Panels, ScreenCtx},
    state::{FocusedPanel, MergedDiffCache, RepositoryData},
};

#[derive(Debug, Clone)]
pub enum SelectedDiffTarget {
    Dirty { path: String, is_staged: bool },
    Commit { commit_idx: usize, path: String },
}

pub use panels::diff::CenterViewMode;

/// Eight fields, one purpose each: identity (`tab_id`), collaborators
/// (`fleet`, `presenter`), domain state (`data`), UI composition
/// (`panels`, `overlay_manager`), transient input (`input`), and the
/// merged-diff projection cache (`merged_diff`). Everything else lives
/// inside one of those sub-structs.
///
/// `fleet` owns the per-tab gateway cache keyed by workdir path. The active
/// gateway is sourced via `fleet.active().clone()` when building `ScreenCtx`.
pub struct RepositoryScreen {
    pub(in crate::screens::repository) tab_id: TabId,
    pub(in crate::screens::repository) fleet: crate::screens::repository::state::GatewayFleet,
    pub(in crate::screens::repository) presenter: Arc<dyn Presenter>,
    pub(in crate::screens::repository) data: RepositoryData,
    pub(in crate::screens::repository) panels: Panels,
    pub(in crate::screens::repository) overlay_manager: OverlayManager,
    pub(in crate::screens::repository) input: InputState,
    pub(in crate::screens::repository) merged_diff: MergedDiffCache,
}

/// Reads persisted sidebar section expanded/collapsed state for `repo_path`
/// from the sqlite settings store. Falls back to `SidebarPanel::default_expanded`
/// when no rows exist yet, or when the settings layer fails to open.
fn load_sidebar_expanded(
    repo_path: &std::path::Path,
) -> std::collections::HashSet<SidebarSectionKind> {
    let key = repo_path.to_string_lossy();
    let settings = match SettingsService::new() {
        Ok(s) => s,
        Err(_) => return panels::sidebar::SidebarPanel::default_expanded(),
    };
    match settings.load_sidebar_sections(&key) {
        Ok(Some(rows)) => {
            let mut set = std::collections::HashSet::new();
            for (kind_key, expanded) in rows {
                if expanded {
                    if let Some(kind) = SidebarSectionKind::from_db_key(&kind_key) {
                        set.insert(kind);
                    }
                }
            }
            set
        }
        _ => panels::sidebar::SidebarPanel::default_expanded(),
    }
}

impl RepositoryScreen {
    pub fn new(
        fleet: crate::screens::repository::state::GatewayFleet,
        presenter: Arc<dyn Presenter>,
        tab_id: TabId,
    ) -> Self {
        let repo_name = fleet
            .primary_path()
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| "Loading…".to_string());
        let sidebar_expanded = load_sidebar_expanded(fleet.primary_path());
        Self {
            tab_id,
            fleet,
            presenter,
            data: RepositoryData::loading_with_repo_name(repo_name),
            panels: Panels::new(sidebar_expanded),
            overlay_manager: OverlayManager::new(),
            input: InputState::new(),
            merged_diff: MergedDiffCache::new(),
        }
    }

    /// Branch refs (local + remote) from the latest snapshot. Exposed so
    /// `App::sync_repository_to_plugins` can rebuild the plugin-host
    /// `leviathan.repository` tables without reaching into screen internals.
    pub(crate) fn branch_refs(&self) -> &[crate::services::RepoRef] {
        self.data.snapshot.branch_refs()
    }

    pub(crate) fn repo_name(&self) -> &str {
        self.data.snapshot.repo_name()
    }

    pub(crate) fn current_branch(&self) -> &str {
        self.data.snapshot.current_branch()
    }

    pub(crate) fn head_hash(&self) -> Option<&str> {
        self.data.snapshot.head_hash()
    }

    pub(crate) fn keymap_context(&self) -> &'static str {
        match self.input.focused_panel {
            FocusedPanel::Sidebar => "repository.sidebar",
            FocusedPanel::Center if self.panels.diff.is_active() => "repository.diff",
            FocusedPanel::Center => "repository.graph",
            FocusedPanel::Detail if self.selected_commit_has_commit_message_editor() => {
                "repository.details.dirty"
            }
            FocusedPanel::Detail => "repository.details",
        }
    }

    pub(crate) fn focus_surface(&self) -> crate::plugin::ui::focus::RepositoryFocusSurface {
        use crate::plugin::ui::focus::RepositoryFocusSurface;
        match self.input.focused_panel {
            FocusedPanel::Sidebar => RepositoryFocusSurface::Sidebar,
            FocusedPanel::Center if self.panels.diff.is_active() => RepositoryFocusSurface::Diff,
            FocusedPanel::Center => RepositoryFocusSurface::Graph,
            FocusedPanel::Detail => RepositoryFocusSurface::Details,
        }
    }

    pub(crate) fn default_remote_name(&self) -> Option<&str> {
        self.data.snapshot.default_remote_name()
    }

    #[allow(dead_code)]
    pub(crate) fn remote_names(&self) -> &[String] {
        self.data.snapshot.remote_names()
    }

    pub fn initial_load_task(&self, tab_id: TabId) -> Task<Message> {
        let repo = self.fleet.active().clone();
        let presenter = self.presenter.clone();
        let repo_path = self.fleet.active_path().display().to_string();
        Task::perform(
            git_read_work(move || {
                let _span = crate::perf::Span::new("git.full_repo_load")
                    .field("tab", tab_id)
                    .field("repo", &repo_path)
                    .field("limit", COMMIT_LOAD_LIMIT);
                repo.load_repo(COMMIT_LOAD_LIMIT)
                    .map(|s| Box::new(presenter.project_loaded(s)))
            }),
            move |result| Message::App(AppMessage::TabOpened { tab_id, result }),
        )
    }

    pub fn hibernate(&mut self) {
        self.data.hibernate();
        self.merged_diff.reset();
        crate::services::release_syntax_caches();
        crate::services::release_text_caches();
    }

    pub fn is_hibernated(&self) -> bool {
        self.data.snapshot.commits().is_empty()
    }

    pub fn rehydrate_task(&self) -> Task<Message> {
        self.reload_task()
    }

    pub fn reload_task(&self) -> Task<Message> {
        let repo = self.fleet.active().clone();
        let presenter = self.presenter.clone();
        let tab_id = self.tab_id;
        let repo_path = self.fleet.active_path().display().to_string();
        Task::perform(
            git_read_work(move || {
                let _span = crate::perf::Span::new("git.full_repo_load")
                    .field("tab", tab_id)
                    .field("repo", &repo_path)
                    .field("limit", COMMIT_LOAD_LIMIT);
                repo.load_repo(COMMIT_LOAD_LIMIT)
                    .map(|s| presenter.project_loaded(s))
            }),
            move |result| Message::tab(tab_id, RepositoryMessage::RepoLoaded(result)),
        )
    }

    /// Task for background ref/commit reload after file-watcher events.
    ///
    /// Formerly called `reload_refs` AND `load_repo` back-to-back (each taking
    /// the gateway's write-lock separately). `load_repo` already reloads refs
    /// as part of building the snapshot, so the separate call was dead weight
    /// that doubled lock contention.
    pub fn reload_refs_task(&self) -> Task<Message> {
        let repo = self.fleet.active().clone();
        let presenter = self.presenter.clone();
        let tab_id = self.tab_id;
        let commit_limit = self.data.snapshot.commits().len().max(COMMIT_LOAD_LIMIT);
        let repo_path = self.fleet.active_path().display().to_string();
        Task::perform(
            git_read_work(move || {
                let _span = crate::perf::Span::new("git.refs_reload")
                    .field("tab", tab_id)
                    .field("repo", &repo_path)
                    .field("limit", commit_limit);
                repo.load_repo(commit_limit)
                    .map(|s| presenter.project_loaded(s))
            }),
            move |result| Message::tab(tab_id, RepositoryMessage::RefsReloaded(result)),
        )
    }

    pub fn get_selected_commit_index(&self) -> usize {
        self.data.selection.selected_commit()
    }

    pub(crate) fn selected_commit_hash(&self) -> Option<(usize, String)> {
        let idx = self.data.selection.selected_commit();
        let commit = self.data.selected_commit(idx)?;
        if commit.kind == crate::core::CommitKind::Commit {
            Some((idx, commit.hash.clone()))
        } else {
            None
        }
    }

    pub(crate) fn selection_context_snapshot(
        &self,
    ) -> crate::plugin::ui::context::SelectionContextSnapshot {
        let available = !self.data.snapshot.commits().is_empty();
        let selected_commit_id = self.selected_commit_hash().map(|(_, hash)| hash);
        let selected_file_path = self
            .panels
            .detail
            .selected_file_path()
            .map(ToOwned::to_owned);
        let kind = if !available {
            "none"
        } else if selected_file_path.is_some() {
            "file"
        } else if self.data.selection.is_multi() {
            "commits"
        } else {
            "commit"
        };

        crate::plugin::ui::context::SelectionContextSnapshot {
            available,
            kind: kind.to_string(),
            selected_commit_id,
            selected_file_path,
        }
    }

    pub(crate) fn selected_commit_reword_seed(&self) -> Option<(String, String)> {
        let idx = self.data.selection.selected_commit();
        let commit = self.data.selected_commit(idx)?;
        if commit.kind == crate::core::CommitKind::Commit {
            Some((commit.hash.clone(), commit.message.clone()))
        } else {
            None
        }
    }

    pub(crate) fn selected_diff_target(&self) -> Option<SelectedDiffTarget> {
        let idx = self.data.selection.selected_commit();
        let commit = self.data.selected_commit(idx)?;
        match commit.kind {
            crate::core::CommitKind::Dirty => commit
                .staged_files
                .first()
                .map(|file| SelectedDiffTarget::Dirty {
                    path: file.path.clone(),
                    is_staged: true,
                })
                .or_else(|| {
                    commit
                        .unstaged_files
                        .first()
                        .map(|file| SelectedDiffTarget::Dirty {
                            path: file.path.clone(),
                            is_staged: false,
                        })
                })
                .or_else(|| {
                    commit
                        .conflicted_files
                        .first()
                        .map(|file| SelectedDiffTarget::Dirty {
                            path: file.path.clone(),
                            is_staged: false,
                        })
                }),
            crate::core::CommitKind::Commit | crate::core::CommitKind::Stash => self
                .data
                .cache
                .state(idx)
                .and_then(|state| state.files.first())
                .map(|file| SelectedDiffTarget::Commit {
                    commit_idx: idx,
                    path: file.path.clone(),
                }),
        }
    }

    pub(crate) fn create_branch_at_selected(
        &mut self,
        commit_idx: Option<usize>,
        hash: Option<String>,
    ) -> Task<Message> {
        let Some((idx, commit_hash)) = commit_idx.zip(hash).or_else(|| self.selected_commit_hash())
        else {
            return Task::none();
        };
        self.handle_center_action(CenterAction::CreateBranchHereRequested {
            commit_idx: idx,
            commit_hash,
        })
    }

    pub(crate) fn delete_branch_direct(
        &mut self,
        branch_name: String,
        is_remote: bool,
        remote_ref: Option<String>,
    ) -> Task<Message> {
        let branch_ref = if is_remote {
            remote_ref.unwrap_or_else(|| branch_name.clone())
        } else {
            branch_name.clone()
        };
        let Some(operation_id) = self.begin_git_write() else {
            return Task::none();
        };
        let repo = self.fleet.active().clone();
        let presenter = self.presenter.clone();
        let tab_id = self.tab_id;
        Task::perform(
            git_write_work(move || {
                repo.delete_branch(&branch_ref, is_remote)
                    .map(|s| presenter.project_loaded(s))
            }),
            move |result| {
                Message::tab(
                    tab_id,
                    RepositoryMessage::BranchDeleted {
                        operation_id: Some(operation_id),
                        branch_name: branch_name.clone(),
                        is_remote,
                        result,
                    },
                )
            },
        )
    }

    pub(crate) fn delete_branch_local_and_remote(&mut self, branch_name: String) -> Task<Message> {
        let Some(operation_id) = self.begin_git_write() else {
            return Task::none();
        };
        let repo = self.fleet.active().clone();
        let presenter = self.presenter.clone();
        let tab_id = self.tab_id;
        let delete_branch_name = branch_name.clone();
        Task::perform(
            git_write_work(move || {
                repo.delete_branch_all(&delete_branch_name)
                    .map(|s| presenter.project_loaded(s))
            }),
            move |result| {
                Message::tab(
                    tab_id,
                    RepositoryMessage::BranchDeleted {
                        operation_id: Some(operation_id),
                        branch_name: branch_name.clone(),
                        is_remote: false,
                        result,
                    },
                )
            },
        )
    }

    pub(crate) fn copy_commit_hash(&mut self, hash: Option<String>) -> Task<Message> {
        let hash = hash.or_else(|| self.selected_commit_hash().map(|(_, hash)| hash));
        match hash {
            Some(hash) => self.handle_detail_action(DetailAction::CopyCommitShaRequested(hash)),
            None => Task::none(),
        }
    }

    pub(crate) fn open_selected_diff(&mut self) -> Task<Message> {
        match self.selected_diff_target() {
            Some(SelectedDiffTarget::Dirty { path, is_staged }) => {
                self.handle_detail_action(DetailAction::DirtyFileOpened { path, is_staged })
            }
            Some(SelectedDiffTarget::Commit { commit_idx, path }) => {
                self.handle_detail_action(DetailAction::CommitFileClicked { commit_idx, path })
            }
            None => Task::none(),
        }
    }

    pub(crate) fn start_reword_selected(&mut self) -> Task<Message> {
        let Some((hash, message)) = self.selected_commit_reword_seed() else {
            return Task::none();
        };
        self.handle_detail_action(DetailAction::RewordStarted {
            hash,
            original_message: message,
        })
    }

    pub(crate) fn focus_panel(&mut self, target: RepositoryFocusTarget) -> Task<Message> {
        match target.resolve(self.panels.diff.is_active()) {
            Ok(panel) => {
                self.input.focused_panel = panel;
            }
            Err(reason) => {
                eprintln!("git_leviathan: repository.focus_panel skipped: {reason}");
            }
        }
        Task::none()
    }

    pub(crate) fn clear_multi_selection(&mut self) -> Task<Message> {
        let commit_count = self.data.snapshot.commits().len();
        if self.data.selection.is_multi() && commit_count > 0 {
            let current = self.data.selection.cursor().min(commit_count - 1);
            self.data.selection.replace_with_single(current);
        }

        self.panels
            .detail
            .collapse_dirty_selection_to_selected(&self.data, &self.data.selection);
        Task::none()
    }

    pub(crate) fn escape_pressed(&mut self) -> Task<Message> {
        input::on_key_pressed(
            self,
            keyboard::Key::Named(keyboard::key::Named::Escape),
            keyboard::Modifiers::default(),
        )
        .unwrap_or_else(Task::none)
    }

    pub(crate) fn focus_commit_message(&mut self) -> Task<Message> {
        if self.input.focused_panel != FocusedPanel::Detail
            || !self.selected_commit_has_commit_message_editor()
        {
            return Task::none();
        }

        iced::widget::operation::focus(panels::detail::dirty_commit_message_editor_id())
    }

    fn selected_commit_has_commit_message_editor(&self) -> bool {
        !self.data.selection.is_multi()
            && self
                .data
                .selected_commit(self.data.selection.selected_commit())
                .is_some_and(|commit| commit.kind == crate::core::CommitKind::Dirty)
    }

    pub(crate) fn stage_selected_dirty_file(&mut self) -> Task<Message> {
        if self
            .panels
            .detail
            .selected_dirty_count(&self.data, &self.data.selection)
            > 1
        {
            return self.handle_detail_action(DetailAction::StageSelectedFiles);
        }
        let Some((path, is_staged)) = self
            .panels
            .detail
            .selected_dirty_file(&self.data, &self.data.selection)
        else {
            return Task::none();
        };
        if is_staged {
            return Task::none();
        }
        self.handle_detail_action(DetailAction::StageFile(path))
    }

    pub(crate) fn unstage_selected_dirty_file(&mut self) -> Task<Message> {
        if self
            .panels
            .detail
            .selected_dirty_count(&self.data, &self.data.selection)
            > 1
        {
            return self.handle_detail_action(DetailAction::UnstageSelectedFiles);
        }
        let Some((path, is_staged)) = self
            .panels
            .detail
            .selected_dirty_file(&self.data, &self.data.selection)
        else {
            return Task::none();
        };
        if !is_staged {
            return Task::none();
        }
        self.handle_detail_action(DetailAction::UnstageFile(path))
    }

    pub(crate) fn discard_selected_dirty_file(&mut self) -> Task<Message> {
        if self
            .panels
            .detail
            .selected_dirty_count(&self.data, &self.data.selection)
            > 1
        {
            return self.handle_detail_action(DetailAction::DiscardSelectedFilesRequested);
        }
        let Some((path, _is_staged)) = self
            .panels
            .detail
            .selected_dirty_file(&self.data, &self.data.selection)
        else {
            return Task::none();
        };
        self.handle_detail_action(DetailAction::DiscardFileRequested(path))
    }

    pub(crate) fn move_selection(&mut self, action: CenterAction, extend: bool) -> Task<Message> {
        if !extend {
            return self.handle_center_action(action);
        }

        if self.input.focused_panel == FocusedPanel::Detail {
            return match action {
                CenterAction::NavigateFirst => {
                    self.handle_detail_action(DetailAction::ExtendFileSelectionFirst)
                }
                CenterAction::NavigateLast => {
                    self.handle_detail_action(DetailAction::ExtendFileSelectionLast)
                }
                CenterAction::NavigateUp => {
                    self.handle_detail_action(DetailAction::ExtendFileSelectionUp)
                }
                CenterAction::NavigateDown => {
                    self.handle_detail_action(DetailAction::ExtendFileSelectionDown)
                }
                _ => Task::none(),
            };
        }

        if self.input.focused_panel == FocusedPanel::Center && self.panels.diff.is_active() {
            return self.handle_center_action(action);
        }

        if self.input.focused_panel != FocusedPanel::Center {
            return Task::none();
        }

        self.extend_center_selection(action)
    }

    pub fn select_commit(&mut self, idx: usize) -> Task<Message> {
        let action = self.panels.center.select_commit(
            idx,
            &mut self.data.selection,
            &mut self.data.branch_popout,
        );
        self.handle_center_action(action)
    }

    fn extend_center_selection(&mut self, action: CenterAction) -> Task<Message> {
        let commit_count = self.data.snapshot.commits().len();
        if commit_count == 0 {
            return Task::none();
        }

        let current = self.data.selection.cursor().min(commit_count - 1);
        let target = match action {
            CenterAction::NavigateFirst => 0,
            CenterAction::NavigateLast => commit_count - 1,
            CenterAction::NavigateUp => current.saturating_sub(1),
            CenterAction::NavigateDown => (current + 1).min(commit_count - 1),
            _ => return Task::none(),
        };
        if target == current {
            return Task::none();
        }

        self.data.branch_popout.close();
        self.data.selection.extend_range_to(target);
        let (mut ctx, panels) = self.ctx_and_panels();
        Task::batch(vec![
            panels.detail.reset_file_list_scroll(),
            panels::sidebar::commit_selection_changed_task(&mut ctx, target),
            panels
                .center
                .scroll_to_commit(target)
                .unwrap_or(Task::none()),
        ])
    }

    pub fn fetch_task(&self, operation_id: state::OperationId) -> Task<Message> {
        let repo = self.fleet.active().clone();
        let tab_id = self.tab_id;
        Task::perform(
            git_write_work(move || repo.fetch_remotes()),
            move |result| {
                Message::App(AppMessage::FetchCompleted {
                    tab_id,
                    operation_id,
                    result,
                })
            },
        )
    }

    pub(crate) fn begin_git_write(&mut self) -> Option<state::OperationId> {
        self.data.operations.begin_write()
    }

    pub(crate) fn begin_git_fetch(&mut self) -> Option<state::OperationId> {
        self.data.operations.begin_fetch()
    }

    pub(crate) fn finish_git_write(&mut self, operation_id: state::OperationId) -> bool {
        self.data.operations.finish_write(operation_id)
    }

    pub(crate) fn is_git_write_in_flight(&self) -> bool {
        self.data.operations.is_writing()
    }

    pub(crate) fn is_blocking_git_write_in_flight(&self) -> bool {
        self.data.operations.is_blocking_write()
    }

    pub(crate) fn mark_watcher_reload_pending(&mut self, path: impl Into<std::path::PathBuf>) {
        self.data.operations.mark_watcher_reload_pending(path);
    }

    /// Read-only task that turns local ref state into a graph + sidebar
    /// projection. Dispatched by the `FetchFinished` handler once the network
    /// fetch has finished. Kept separate so the reload travels as an explicit
    /// `GraphAndRefsReloaded` message rather than a side-effect of fetching.
    fn reload_graph_and_refs_task(&self) -> Task<Message> {
        let repo = self.fleet.active().clone();
        let presenter = self.presenter.clone();
        let tab_id = self.tab_id;
        let commit_limit = self.data.snapshot.commits().len().max(COMMIT_LOAD_LIMIT);
        let repo_path = self.fleet.active_path().display().to_string();
        Task::perform(
            git_read_work(move || {
                let _span = crate::perf::Span::new("git.refs_reload")
                    .field("tab", tab_id)
                    .field("repo", &repo_path)
                    .field("limit", commit_limit);
                repo.load_refs_snapshot(commit_limit)
                    .map(|s| presenter.project_refs(s))
            }),
            move |result| Message::tab(tab_id, RepositoryMessage::GraphAndRefsReloaded(result)),
        )
    }

    /// Path of the worktree currently focused in this tab (primary workdir
    /// when no secondary is focused). The file watcher keys off this so that
    /// edits in the *focused* worktree's workdir fire events rather than only
    /// edits in the primary.
    pub fn active_worktree_path(&self) -> &std::path::Path {
        self.fleet.active_path()
    }

    /// Cheap-clone of the active gateway. plugin Git APIs use this through the
    /// plugin host every time the active screen changes so plugin
    /// `leviathan.git.*` and `leviathan.repository.{status,...}` calls
    /// route through the same `SharedRepositoryGateway` the built-in UI
    /// already uses.
    pub fn active_gateway(&self) -> crate::services::SharedRepositoryGateway {
        self.fleet.active().clone()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => Some(Message::repo(
                RepositoryMessage::Center(CenterAction::PointerMoved(position)),
            )),
            iced::Event::Mouse(mouse::Event::CursorLeft) => Some(Message::repo(
                RepositoryMessage::Center(CenterAction::PointerLeftWindow),
            )),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(
                Message::repo(RepositoryMessage::Center(CenterAction::ResizeReleased)),
            ),
            iced::Event::Window(iced::window::Event::Opened { size, .. })
            | iced::Event::Window(iced::window::Event::Resized(size)) => Some(Message::repo(
                RepositoryMessage::Center(CenterAction::WindowResized { width: size.width }),
            )),
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) => Some(Message::repo(
                RepositoryMessage::Center(CenterAction::ModifiersChanged(mods)),
            )),
            _ => None,
        })
    }

    /// Splits `&mut self` into a `ScreenCtx` plus a mutable `&mut Panels`.
    ///
    /// The panel update methods need to touch one panel (e.g. `panels.sidebar`)
    /// while simultaneously reading/writing sibling panels (`panels.center`)
    /// and screen-level state (`data`, `overlay_manager`, ...). Destructuring
    /// `self` here yields disjoint borrows the compiler can prove non-aliasing,
    /// so each handler below is a two-liner rather than a twelve-line ctx
    /// literal duplicated four times.
    fn ctx_and_panels(&mut self) -> (ScreenCtx<'_>, &mut Panels) {
        let Self {
            tab_id,
            fleet,
            presenter,
            data,
            panels,
            overlay_manager,
            input,
            merged_diff,
        } = self;

        let ctx = ScreenCtx {
            tab_id: *tab_id,
            repository: fleet.active().clone(),
            presenter,
            data,
            overlay_manager,
            input,
            merged_diff,
            fleet,
        };
        (ctx, panels)
    }

    pub fn update(&mut self, msg: RepositoryMessage) -> Task<Message> {
        let _span = crate::perf::Span::new("ui.repository_update")
            .field("tab", self.tab_id)
            .field("repo", self.fleet.active_path().display());
        match msg {
            RepositoryMessage::Sidebar(action) => {
                let (mut ctx, panels) = self.ctx_and_panels();
                panels.sidebar.update(action, &mut ctx, &mut panels.center)
            }
            RepositoryMessage::Center(action) => self.handle_center_action(action),
            RepositoryMessage::Detail(action) => self.handle_detail_action(action),
            RepositoryMessage::DiffPanel(action) => self.handle_diff_panel_action(action),
            RepositoryMessage::CommitSearch(action) => commit_search::handle_action(self, action),
            RepositoryMessage::OpenCommitSearch => commit_search::open(self),
            RepositoryMessage::OverlayPanel(action) => self.dispatch_overlay_action(action),
            RepositoryMessage::RefreshRequested => self.reload_refs_task(),
            RepositoryMessage::CreateBranchAtSelected { commit_idx, hash } => {
                self.create_branch_at_selected(commit_idx, hash)
            }
            RepositoryMessage::CopyCommitHash { hash } => self.copy_commit_hash(hash),
            RepositoryMessage::OpenSelectedDiff => self.open_selected_diff(),
            RepositoryMessage::MoveSelection { action, extend } => {
                self.move_selection(action, extend)
            }
            RepositoryMessage::ClearMultiSelection => self.clear_multi_selection(),
            RepositoryMessage::EscapePressed => self.escape_pressed(),
            RepositoryMessage::FocusCommitMessage => self.focus_commit_message(),
            RepositoryMessage::StartRewordSelected => self.start_reword_selected(),
            RepositoryMessage::FocusPanel(target) => self.focus_panel(target),
            RepositoryMessage::StageSelectedDirtyFile => self.stage_selected_dirty_file(),
            RepositoryMessage::UnstageSelectedDirtyFile => self.unstage_selected_dirty_file(),
            RepositoryMessage::DiscardSelectedDirtyFile => self.discard_selected_dirty_file(),
            RepositoryMessage::DeleteBranchDirect {
                branch_name,
                is_remote,
                remote_ref,
            } => self.delete_branch_direct(branch_name, is_remote, remote_ref),
            RepositoryMessage::DeleteBranchLocalAndRemote { branch_name } => {
                self.delete_branch_local_and_remote(branch_name)
            }
            other => commands::dispatch_result(self, other),
        }
    }

    fn handle_center_action(&mut self, action: CenterAction) -> Task<Message> {
        if self.input.focused_panel == FocusedPanel::Detail {
            match &action {
                CenterAction::NavigateFirst => {
                    return self.handle_detail_action(DetailAction::NavigateFileFirst);
                }
                CenterAction::NavigateLast => {
                    return self.handle_detail_action(DetailAction::NavigateFileLast);
                }
                CenterAction::NavigateUp => {
                    return self.handle_detail_action(DetailAction::NavigateFileUp);
                }
                CenterAction::NavigateDown => {
                    return self.handle_detail_action(DetailAction::NavigateFileDown);
                }
                _ => {}
            }
        }
        if self.input.focused_panel == FocusedPanel::Center && self.panels.diff.is_active() {
            match &action {
                CenterAction::NavigateFirst => {
                    return self.handle_diff_panel_action(DiffPanelAction::ScrollTop);
                }
                CenterAction::NavigateLast => {
                    return self.handle_diff_panel_action(DiffPanelAction::ScrollBottom);
                }
                CenterAction::NavigateUp => {
                    return self.handle_diff_panel_action(DiffPanelAction::ScrollUp);
                }
                CenterAction::NavigateDown => {
                    return self.handle_diff_panel_action(DiffPanelAction::ScrollDown);
                }
                _ => {}
            }
        }
        let (mut ctx, panels) = self.ctx_and_panels();
        panels::center::update_center(&mut panels.center, &mut panels.detail, action, &mut ctx)
    }

    fn handle_detail_action(&mut self, action: DetailAction) -> Task<Message> {
        let (mut ctx, panels) = self.ctx_and_panels();
        panels::detail::update_detail(
            &mut panels.detail,
            action,
            &mut ctx,
            &mut panels.center,
            &mut panels.diff,
        )
    }

    fn handle_diff_panel_action(&mut self, action: DiffPanelAction) -> Task<Message> {
        let (mut ctx, panels) = self.ctx_and_panels();
        panels::diff::update_diff(&mut panels.diff, action, &mut ctx)
    }

    fn dispatch_overlay_action(&mut self, action: OverlayPanelAction) -> Task<Message> {
        // Close any open context menu when triggering a dialog from one of its
        // items — otherwise the menu's backdrop stays as an invisible input sink
        // behind the toolbar overlay.
        if matches!(action, OverlayPanelAction::WorktreeRemoveRequested { .. }) {
            self.data.branch_popout.close_context_menu();
        }

        let ctx = DialogCtx {
            repository: self.fleet.active().clone(),
            primary_repository: self.fleet.primary().clone(),
            presenter: self.presenter.clone(),
            tab_id: self.tab_id,
            active_path: self.fleet.active_path().to_path_buf(),
            operations: &mut self.data.operations,
        };
        match self.overlay_manager.dispatch(action, ctx) {
            overlays::DialogDispatch::Task(task) => task,
            overlays::DialogDispatch::CancelClosed => Task::none(),
            overlays::DialogDispatch::RestoreCenterListScroll => {
                self.panels.center.restore_center_list_scroll()
            }
        }
    }

    pub fn on_modifiers_changed(&mut self, modifiers: keyboard::Modifiers) {
        input::on_modifiers_changed(self, modifiers);
    }

    pub fn handle_key_pressed(
        &mut self,
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        input::on_key_pressed(self, key, modifiers)
    }

    pub fn handle_overlay_key_pressed(
        &mut self,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        input::on_overlay_key_pressed(self, key, modifiers)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use iced::keyboard;

    use super::*;
    use crate::{
        core::{ChangeKind, ChangedFile},
        services::{
            presenter::projection, test_support, CommitSnapshot, DefaultPresenter, DirtySnapshot,
            GitRepositoryGateway, RepoRef, RepoSnapshot,
        },
    };

    fn test_screen() -> (test_support::TempRepo, RepositoryScreen) {
        let (repo_dir, _repo) = test_support::init_test_repo("overlay_focus");
        let gateway = GitRepositoryGateway::from_path(repo_dir.path_str());
        let fleet = state::GatewayFleet::new(
            repo_dir.path.clone(),
            gateway.clone(),
            repo_dir.path.clone(),
            gateway,
        );
        let screen = RepositoryScreen::new(fleet, Arc::new(DefaultPresenter::new()), TabId(1));
        (repo_dir, screen)
    }

    fn commit_snapshot(hash: &str) -> CommitSnapshot {
        CommitSnapshot {
            hash: hash.to_string(),
            message: "initial".to_string(),
            author_name: "Ada".to_string(),
            authored_at: 1_700_000_000,
            authored_offset_minutes: 0,
            parent_hashes: vec![],
        }
    }

    fn loaded_repo_with_dirty_commit() -> crate::view_model::LoadedRepo {
        projection::project_loaded(RepoSnapshot {
            commits: vec![commit_snapshot("aaaaaaaa")],
            refs: Vec::<RepoRef>::new(),
            dirty: Some(DirtySnapshot {
                conflicted_files: vec![],
                staged_files: vec![],
                unstaged_files: vec![ChangedFile {
                    path: "file.txt".to_string(),
                    kind: ChangeKind::Modified,
                }],
                parent_hashes: vec!["aaaaaaaa".to_string()],
            }),
            stashes: vec![],
            repo_name: "repo".to_string(),
            current_branch: Some("main".to_string()),
            head_hash: Some("aaaaaaaa".to_string()),
            has_more_commits: false,
            default_remote_name: None,
            remote_names: Vec::new(),
            fast_forward_candidates: Default::default(),
            worktrees: vec![],
            active_worktree_path: Default::default(),
        })
    }

    #[test]
    fn details_keymap_context_is_dirty_only_for_single_working_changes_commit() {
        let (_repo_dir, mut screen) = test_screen();
        screen.data.replace_loaded(loaded_repo_with_dirty_commit());
        screen.input.focused_panel = FocusedPanel::Detail;

        assert_eq!(screen.keymap_context(), "repository.details.dirty");

        screen.data.selection.select_commit(1);
        assert_eq!(screen.keymap_context(), "repository.details");

        screen.data.selection.select_commit(0);
        screen.data.selection.extend_range_to(1);
        assert_eq!(screen.keymap_context(), "repository.details");
    }

    #[test]
    fn escape_canceling_confirmation_preserves_tracked_panel_focus() {
        for focused_panel in [FocusedPanel::Sidebar, FocusedPanel::Detail] {
            let (_repo_dir, mut screen) = test_screen();
            screen.input.focused_panel = focused_panel;
            screen
                .overlay_manager
                .open(overlays::ActiveDialog::DeleteBranch(
                    overlays::delete_branch::State {
                        branch_name: "feature".into(),
                        is_remote: false,
                        has_remote: false,
                        remote_name: None,
                        remote_ref: None,
                    },
                ));

            let task = screen.handle_overlay_key_pressed(
                &keyboard::Key::Named(keyboard::key::Named::Escape),
                keyboard::Modifiers::default(),
            );

            assert!(task.is_some(), "Esc should keep cancel semantics");
            assert!(
                screen.overlay_manager.active().is_none(),
                "Esc should close the active overlay"
            );
            assert_eq!(screen.input.focused_panel, focused_panel);
        }
    }
}

impl RepositoryScreen {
    /// Render the repository view, passing in the plugin slot registry so that
    /// top/bottom extension sections are populated. Called from `app/view.rs`.
    pub fn view_with_repo_region<'a>(
        &'a self,
        registry: &'a crate::widgets::chrome::repo_region::RepoRegionRegistry,
        chrome_registry: &'a crate::widgets::chrome::repo_region::RepoChromeRegistry,
    ) -> iced::Element<'a, crate::message::Message> {
        view::view_with_repo_region(self, registry, chrome_registry)
    }
}

impl Screen for RepositoryScreen {
    type Message = RepositoryMessage;

    fn update(&mut self, msg: RepositoryMessage) -> Task<Message> {
        RepositoryScreen::update(self, msg)
    }

    fn view(&self) -> Element<'_, Message> {
        // Unreachable: `app::view::repository_view` calls
        // `RepositoryScreen::view_with_repo_region` directly, bypassing
        // the trait method. The repo screen needs the host's
        // `RepoRegionRegistry` to render plugin slots, which the trait
        // signature doesn't expose. Kept here only to satisfy the
        // `Screen` trait contract.
        iced::widget::text("").into()
    }

    fn toolbar<'a>(&'a self, ctx: &ToolbarCtx<'a>) -> Option<Element<'a, Message>> {
        Some(view::toolbar(self, ctx))
    }

    fn tick_animation(&mut self, dt_ms: f32) -> Option<Task<Message>> {
        animation::tick(self, dt_ms)
    }

    fn is_animating(&self) -> bool {
        animation::is_animating(self)
    }

    fn overlay_layers(&self) -> Vec<Element<'_, Message>> {
        view::overlay_layers(self)
    }
}
