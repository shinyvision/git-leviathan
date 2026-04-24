//! Assembles the app-level `Subscription<Message>` from iced window/keyboard
//! events, the active screen's own subscription, the file watcher, periodic
//! fetch/animation ticks, and a SIGINT/SIGTERM stream.

use std::time::Duration;

use iced::{event, keyboard, window, Subscription};

use crate::message::{AppMessage, Message};
use crate::screens::Screen;

use super::App;

const FETCH_TICK_INTERVAL: Duration = Duration::from_secs(30);
const ANIMATION_TICK_INTERVAL: Duration = Duration::from_millis(16);

pub fn build(app: &App) -> Subscription<Message> {
    let app_events = event::listen_with(translate_window_event);

    let active_id = app.tabs.active_tab_id();
    // Watch the *focused* worktree's workdir, not just the primary — otherwise
    // file edits in a secondary worktree never fire events and only indirect
    // signals (ref changes, tab switches) refresh the screen.
    let file_watcher_subs: Vec<Subscription<Message>> = app
        .tabs
        .active_screen()
        .map(|screen| {
            let watch_path = screen.active_worktree_path().to_path_buf();
            vec![crate::services::file_watcher::watch_repo_files(
                active_id, watch_path,
            )
            .map(|tab_id| Message::App(AppMessage::RepoFilesChanged(tab_id)))]
        })
        .unwrap_or_default();

    let fetch_tick_sub =
        iced::time::every(FETCH_TICK_INTERVAL).map(|t| Message::App(AppMessage::FetchTick(t)));

    let animation_tick_sub = if app.tabs.is_empty() {
        Subscription::none()
    } else {
        let is_animating = app
            .tabs
            .active_screen()
            .map(|s| s.is_animating())
            .unwrap_or(false)
            || app.toasts.is_animating()
            || app.fetch.is_fetching();
        if is_animating {
            iced::time::every(ANIMATION_TICK_INTERVAL)
                .map(|t| Message::App(AppMessage::AnimationTick(t)))
        } else {
            Subscription::none()
        }
    };

    let screen_sub = app
        .tabs
        .active_screen()
        .map(|s| s.subscription())
        .unwrap_or(Subscription::none());

    let sigterm_sub = Subscription::run(sigterm_stream);

    let mut subs = vec![
        app_events,
        fetch_tick_sub,
        animation_tick_sub,
        screen_sub,
        sigterm_sub,
    ];
    subs.extend(file_watcher_subs);

    Subscription::batch(subs)
}

fn translate_window_event(
    event: iced::Event,
    status: event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::Focused) => {
            Some(Message::App(AppMessage::WindowFocused))
        }
        iced::Event::Window(window::Event::Unfocused) => {
            Some(Message::App(AppMessage::WindowUnfocused))
        }
        iced::Event::Window(window::Event::CloseRequested) => {
            Some(Message::App(AppMessage::ShutdownRequested))
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
            if matches!(status, event::Status::Ignored) =>
        {
            Some(Message::App(AppMessage::KeyPressed(key, modifiers)))
        }
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::App(AppMessage::ModifiersChanged(modifiers)))
        }
        _ => None,
    }
}

fn sigterm_stream() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        1,
        |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigint =
                    signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
                let mut sigterm =
                    signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
                loop {
                    tokio::select! {
                        _ = sigint.recv() => {}
                        _ = sigterm.recv() => {}
                    }
                    let _ = sender.try_send(Message::App(AppMessage::ShutdownRequested));
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
                let _ = sender.try_send(Message::App(AppMessage::ShutdownRequested));
                std::future::pending::<()>().await;
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_positive() {
        assert!(FETCH_TICK_INTERVAL > Duration::ZERO);
        assert!(ANIMATION_TICK_INTERVAL > Duration::ZERO);
    }
}
