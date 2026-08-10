//! `SubscriptionBuilder` — assembles the app-level `Subscription<Message>` from
//! iced window/keyboard events, the active screen's own subscription, the
//! file watcher, periodic fetch/animation ticks, and a SIGINT/SIGTERM stream.
//!
//! Extracted from `App` in keymaps. Kept a single function rather than a
//! type because it has no state of its own — subscriptions are built fresh
//! on every view pass.

use std::time::Duration;

use iced::{event, keyboard, window, Subscription};

use crate::message::{AppMessage, Message};
use crate::screens::Screen;

use super::App;

const FETCH_TICK_INTERVAL: Duration = Duration::from_secs(30);
const ANIMATION_TICK_INTERVAL: Duration = Duration::from_millis(16);
const PLUGIN_RUNTIME_TICK_INTERVAL: Duration = Duration::from_millis(50);

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
            vec![
                crate::services::file_watcher::watch_repo_files(active_id, watch_path).map(
                    |(tab_id, path)| Message::App(AppMessage::RepoFilesChanged { tab_id, path }),
                ),
            ]
        })
        .unwrap_or_default();

    let fetch_tick_sub =
        iced::time::every(FETCH_TICK_INTERVAL).map(|t| Message::App(AppMessage::FetchTick(t)));

    // Toasts must keep ticking even with no tab open (e.g. an error toast on
    // the blank/no-git screen), so consult the toast animator independently of
    // whether a screen is present.
    let screen_or_toast_animating = app
        .tabs
        .active_screen()
        .map(|s| s.is_animating())
        .unwrap_or(false)
        || app.toasts.is_animating();
    let animation_tick_sub = if screen_or_toast_animating {
        iced::time::every(ANIMATION_TICK_INTERVAL)
            .map(|t| Message::App(AppMessage::AnimationTick(t)))
    } else {
        Subscription::none()
    };

    let screen_sub = if let Some(screen) = app.tabs.active_plugin_screen() {
        screen.subscription_with_host(&app.plugin_host)
    } else {
        app.tabs
            .active_screen()
            .map(|s| s.subscription())
            .unwrap_or(Subscription::none())
    };
    let plugin_runtime_sub = if app.plugin_host.needs_runtime_tick() {
        iced::time::every(PLUGIN_RUNTIME_TICK_INTERVAL)
            .map(|t| Message::App(AppMessage::PluginRuntimeTick(t)))
    } else {
        Subscription::none()
    };

    let sigterm_sub = Subscription::run(sigterm_stream);
    let eager_grammar_install_sub = Subscription::run(eager_grammar_install_stream);

    let mut subs = vec![
        app_events,
        fetch_tick_sub,
        animation_tick_sub,
        screen_sub,
        plugin_runtime_sub,
        sigterm_sub,
        eager_grammar_install_sub,
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
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            modifiers,
            ..
        }) if matches!(status, event::Status::Ignored) => {
            Some(Message::App(AppMessage::KeyPressed {
                key,
                modified_key,
                modifiers,
            }))
        }
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::App(AppMessage::ModifiersChanged(modifiers)))
        }
        _ => None,
    }
}

fn eager_grammar_install_stream() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        16,
        |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            let Some(mut rx) =
                crate::services::syntax_highlight::take_eager_grammar_install_receiver()
            else {
                return;
            };
            while let Some(language) = rx.recv().await {
                let _ = sender.try_send(Message::App(AppMessage::EagerGrammarInstallRequested(
                    language,
                )));
            }
        },
    )
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
    use iced::keyboard::{key, Location};

    #[test]
    fn intervals_positive() {
        assert!(FETCH_TICK_INTERVAL > Duration::ZERO);
        assert!(ANIMATION_TICK_INTERVAL > Duration::ZERO);
        assert!(PLUGIN_RUNTIME_TICK_INTERVAL > Duration::ZERO);
    }

    #[test]
    fn key_pressed_preserves_modified_key() {
        let message = translate_window_event(
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(";".into()),
                modified_key: keyboard::Key::Character(":".into()),
                physical_key: key::Physical::Code(key::Code::Semicolon),
                location: Location::Standard,
                modifiers: keyboard::Modifiers::SHIFT,
                text: Some(":".into()),
                repeat: false,
            }),
            event::Status::Ignored,
            window::Id::unique(),
        )
        .expect("ignored key press should be forwarded");

        let Message::App(AppMessage::KeyPressed {
            key,
            modified_key,
            modifiers,
        }) = message
        else {
            panic!("expected app key press message");
        };

        assert_eq!(key, keyboard::Key::Character(";".into()));
        assert_eq!(modified_key, keyboard::Key::Character(":".into()));
        assert!(modifiers.shift());
    }
}
