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

pub use messages::RepositoryMessage;

use iced::{event, keyboard, mouse, Element, Subscription, Task};
use std::sync::Arc;

use crate::{
    core::TabId,
    message::{AppMessage, Message},
    screens::screen_trait::{Screen, ToolbarCtx},
    services::{presenter::Presenter, GitError, SettingsService, COMMIT_LOAD_LIMIT},
    view_model::SidebarSectionKind,
};

use self::{
    input::InputState,
    overlays::{DialogCtx, OverlayManager},
    panel_messages::{CenterAction, DetailAction, DiffPanelAction, OverlayPanelAction},
    panels::{Panels, ScreenCtx},
    state::{FocusedPanel, MergedDiffCache, RepositoryData},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileView {
    Path,
    Tree,
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

/// Runs a blocking gateway call (libgit2 / `git` subprocess) on tokio's
/// blocking pool and returns a future suitable for `Task::perform`. Use this
/// for every `SharedRepositoryGateway` call so iced/tokio runtime worker
/// threads do not get pinned by synchronous I/O. Also the right place to
/// chain the presenter (`project_loaded`) so projection runs off the main
/// thread before the message reaches `update`.
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

pub(super) async fn gateway_work<T>(
    f: impl FnOnce() -> Result<T, GitError> + Send + 'static,
) -> Result<T, GitError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|e| Err(GitError::Other(format!("gateway task panicked: {e}"))))
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

    pub(crate) fn default_remote_name(&self) -> Option<&str> {
        self.data.snapshot.default_remote_name()
    }

    pub fn initial_load_task(&self, tab_id: TabId) -> Task<Message> {
        let repo = self.fleet.active().clone();
        let presenter = self.presenter.clone();
        Task::perform(
            gateway_work(move || {
                repo.load_repo(COMMIT_LOAD_LIMIT)
                    .map(|s| presenter.project_loaded(s))
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
        Task::perform(
            gateway_work(move || {
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
        Task::perform(
            gateway_work(move || {
                repo.load_repo(commit_limit)
                    .map(|s| presenter.project_loaded(s))
            }),
            move |result| Message::tab(tab_id, RepositoryMessage::RefsReloaded(result)),
        )
    }

    pub fn get_selected_commit_index(&self) -> usize {
        self.data.selection.selected_commit()
    }

    pub fn select_commit(&mut self, idx: usize) -> Task<Message> {
        let action = self.panels.center.select_commit(
            idx,
            &mut self.data.selection,
            &mut self.data.branch_popout,
        );
        self.handle_center_action(action)
    }

    pub fn fetch_task(&self) -> Task<Message> {
        let repo = self.fleet.active().clone();
        let tab_id = self.tab_id;
        Task::perform(gateway_work(move || repo.fetch_remotes()), move |result| {
            Message::App(AppMessage::FetchCompleted { tab_id, result })
        })
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
        Task::perform(
            gateway_work(move || {
                repo.load_refs_snapshot(commit_limit)
                    .map(|s| presenter.project_refs(s))
            }),
            move |result| Message::tab(tab_id, RepositoryMessage::GraphAndRefsReloaded(result)),
        )
    }

    /// True while a user-initiated network op (push/pull) is in flight on this
    /// screen. The app-level file-watcher cascade uses this to skip spawning
    /// `reload_refs_task`s that would only queue behind the writer lock —
    /// the in-flight op already publishes a post-op snapshot on completion.
    pub fn is_network_op_in_flight(&self) -> bool {
        self.data.animation.network_op_in_flight()
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
            other => commands::dispatch_result(self, other),
        }
    }

    fn handle_center_action(&mut self, action: CenterAction) -> Task<Message> {
        let (mut ctx, panels) = self.ctx_and_panels();
        panels::center::update_center(&mut panels.center, action, &mut ctx)
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
        };
        match self.overlay_manager.dispatch(action, ctx) {
            overlays::DialogDispatch::Task(task) => task,
            overlays::DialogDispatch::CancelCloseFocus => {
                self.input.focused_panel = FocusedPanel::Center;
                Task::none()
            }
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
}

impl RepositoryScreen {
    /// Render the repository view, passing in the plugin slot registry so that
    /// top/bottom extension sections are populated. Called from `app/view.rs`.
    pub fn view_with_repo_region<'a>(
        &'a self,
        registry: &'a crate::widgets::chrome::repo_region::RepoRegionRegistry,
    ) -> iced::Element<'a, crate::message::Message> {
        view::view_with_repo_region(self, registry)
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
