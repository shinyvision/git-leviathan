//! View composition for `App`.

use iced::widget::{column, Stack};
use iced::{Element, Length};

use crate::message::Message;
use crate::plugin::ui::overlay as plugin_overlay;
use crate::screens::{
    BlankScreen, NoGitScreen, PluginScreen, RepositoryScreen, Screen, ToolbarCtx,
};
use crate::widgets::chrome;

use super::App;

/// Borrowed view of whichever screen is currently active — the single match
/// site for view dispatch. Lives only as long as the borrow of `App`.
enum ActiveScreenRef<'a> {
    Blank(&'a BlankScreen),
    NoGit(&'a NoGitScreen),
    Repository(&'a RepositoryScreen),
    Plugin(&'a PluginScreen),
}

pub fn render(app: &App) -> Element<'_, Message> {
    match active_screen_ref(app) {
        ActiveScreenRef::NoGit(screen) => app_screen_view(screen.view(), app),
        ActiveScreenRef::Blank(screen) => app_screen_view(screen.view(), app),
        ActiveScreenRef::Repository(screen) => repository_view(app, screen),
        ActiveScreenRef::Plugin(screen) => plugin_view(app, screen),
    }
}

fn app_screen_view<'a>(body: Element<'a, Message>, app: &'a App) -> Element<'a, Message> {
    let mut layers: Vec<Element<Message>> = vec![body];
    layers.extend(plugin_overlay::layers(&app.plugin_host));
    if let Some(toast_overlay) = app.toasts.overlay() {
        layers.push(toast_overlay);
    }
    Stack::with_children(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn plugin_view<'a>(app: &'a App, screen: &'a PluginScreen) -> Element<'a, Message> {
    let _screen_nav = (
        <PluginScreen as Screen>::title(screen),
        <PluginScreen as Screen>::breadcrumbs(screen),
        <PluginScreen as Screen>::has_focus(screen),
    );
    let mut content_col = column![chrome::tab_bar_view(
        &app.tab_bar_registry,
        app.plugin_host.tab_snapshot()
    ),]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill);
    let ctx = ToolbarCtx {
        now: app.fetch.started_at(),
        main_bar_registry: &app.main_bar_registry,
    };
    if let Some(toolbar) = <PluginScreen as Screen>::toolbar(screen, &ctx) {
        content_col = content_col.push(toolbar);
    }
    content_col = content_col.push(screen.view_with_host(&app.plugin_host));
    let content: Element<Message> = content_col.into();

    let mut layers: Vec<Element<Message>> = vec![content];
    layers.extend(plugin_overlay::layers(&app.plugin_host));
    if let Some(toast_overlay) = app.toasts.overlay() {
        layers.push(toast_overlay);
    }
    Stack::with_children(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn repository_view<'a>(app: &'a App, screen: &'a RepositoryScreen) -> Element<'a, Message> {
    let mut content_col = column![chrome::tab_bar_view(
        &app.tab_bar_registry,
        app.plugin_host.tab_snapshot()
    ),]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill);
    let ctx = ToolbarCtx {
        now: app.fetch.started_at(),
        main_bar_registry: &app.main_bar_registry,
    };
    if let Some(toolbar) = <RepositoryScreen as Screen>::toolbar(screen, &ctx) {
        content_col = content_col.push(toolbar);
    }
    content_col = content_col
        .push(screen.view_with_repo_region(&app.repo_region_registry, &app.repo_chrome_registry));

    let content: Element<Message> = content_col.into();

    let mut layers: Vec<Element<Message>> = vec![content];
    layers.extend(<RepositoryScreen as Screen>::overlay_layers(screen));
    layers.extend(plugin_overlay::layers(&app.plugin_host));

    if let Some(toast_overlay) = app.toasts.overlay() {
        layers.push(toast_overlay);
    }

    Stack::with_children(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn active_screen_ref(app: &App) -> ActiveScreenRef<'_> {
    if let Some(no_git) = &app.no_git_screen {
        ActiveScreenRef::NoGit(no_git)
    } else if app.tabs.is_empty() {
        ActiveScreenRef::Blank(&app.blank_screen)
    } else if let Some(screen) = app.tabs.active_plugin_screen() {
        ActiveScreenRef::Plugin(screen)
    } else {
        match app.tabs.active_screen() {
            Some(screen) => ActiveScreenRef::Repository(screen),
            None => ActiveScreenRef::Blank(&app.blank_screen),
        }
    }
}
