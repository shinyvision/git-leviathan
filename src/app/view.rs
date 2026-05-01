//! View composition for `App`.

use iced::widget::{column, Stack};
use iced::{Element, Length};

use crate::core::TabId;
use crate::message::Message;
use crate::plugin::ui::screen as plugin_screen;
use crate::screens::{BlankScreen, NoGitScreen, RepositoryScreen, Screen, ToolbarCtx};
use crate::widgets::chrome;

use super::App;

/// Borrowed view of whichever screen is currently active — the single match
/// site for view dispatch. Lives only as long as the borrow of `App`.
enum ActiveScreenRef<'a> {
    Blank(&'a BlankScreen),
    NoGit(&'a NoGitScreen),
    Repository(&'a RepositoryScreen),
    Plugin,
}

pub fn render(app: &App) -> Element<'_, Message> {
    match active_screen_ref(app) {
        ActiveScreenRef::NoGit(screen) => screen.view(),
        ActiveScreenRef::Blank(screen) => screen.view(),
        ActiveScreenRef::Repository(screen) => repository_view(app, screen),
        ActiveScreenRef::Plugin => plugin_view(app),
    }
}

fn plugin_view(app: &App) -> Element<'_, Message> {
    let body = plugin_screen::view(&app.plugin_host);
    let mut layers: Vec<Element<Message>> = vec![body];
    if let Some(toast_overlay) = app.toasts.overlay() {
        layers.push(toast_overlay);
    }
    Stack::with_children(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn repository_view<'a>(app: &'a App, screen: &'a RepositoryScreen) -> Element<'a, Message> {
    let tab_entries: Vec<(TabId, String)> = app
        .tabs
        .tabs()
        .iter()
        .map(|t| (t.id, t.name.clone()))
        .collect();

    let mut content_col = column![
        chrome::tab_bar_view(tab_entries, app.tabs.active_tab_id(), &app.tab_bar_registry),
    ]
    .spacing(0);
    let ctx = ToolbarCtx {
        now: app.fetch.started_at(),
        main_bar_registry: &app.main_bar_registry,
    };
    if let Some(toolbar) =
        <RepositoryScreen as Screen>::toolbar(screen, &ctx)
    {
        content_col = content_col.push(toolbar);
    }
    content_col = content_col.push(screen.view_with_repo_region(&app.repo_region_registry));

    let content: Element<Message> = content_col.into();

    let mut layers: Vec<Element<Message>> = vec![content];
    layers.extend(<RepositoryScreen as Screen>::overlay_layers(screen));

    if let Some(toast_overlay) = app.toasts.overlay() {
        layers.push(toast_overlay);
    }

    Stack::with_children(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn active_screen_ref(app: &App) -> ActiveScreenRef<'_> {
    if app.plugin_host.active_screen().is_some() {
        return ActiveScreenRef::Plugin;
    }
    if let Some(no_git) = &app.no_git_screen {
        ActiveScreenRef::NoGit(no_git)
    } else if app.tabs.is_empty() {
        ActiveScreenRef::Blank(&app.blank_screen)
    } else {
        match app.tabs.active_screen() {
            Some(screen) => ActiveScreenRef::Repository(screen),
            None => ActiveScreenRef::Blank(&app.blank_screen),
        }
    }
}
