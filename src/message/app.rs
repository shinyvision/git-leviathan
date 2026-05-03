//! App-level messages: keyboard/modifiers, tab lifecycle, fetch
//! orchestration, shutdown, clipboard — everything `App` handles without
//! routing to an individual screen.

use iced::keyboard;
use std::path::PathBuf;
use std::time::Instant;

use crate::core::TabId;
use crate::plugin::tab_snapshot::TabRegistryOp;
use crate::services::GitError;
use crate::view_model::LoadedRepo;

#[derive(Debug, Clone)]
pub enum AppMessage {
    NoOp,
    KeyPressed {
        key: keyboard::Key,
        modified_key: keyboard::Key,
        modifiers: keyboard::Modifiers,
    },
    ModifiersChanged(keyboard::Modifiers),
    OpenRepoDialog,
    RepoPathChosen(Option<PathBuf>),
    /// Result of opening a tab (initial load or background reload). The
    /// snapshot has already been projected off the main thread.
    TabOpened {
        tab_id: TabId,
        result: Result<Box<LoadedRepo>, GitError>,
    },
    TabRegistryOp(TabRegistryOp),
    AnimationTick(Instant),
    PluginRuntimeTick(Instant),
    /// A repository's files changed on disk; carries the owning tab id.
    RepoFilesChanged(TabId),
    WindowFocused,
    WindowUnfocused,
    FetchTick(Instant),
    /// Debounce timer for post-Ctrl+Tab auto-fetch fired; fetch the now-active tab.
    FetchDebounceElapsed,
    /// Network fetch completed; app clears `fetching`, fires a typed plugin
    /// `FetchFinished` event, then forwards to the tab's screen. The screen
    /// handler dispatches its own scoped refs-reload on success.
    FetchCompleted {
        tab_id: TabId,
        result: Result<(), GitError>,
    },
    /// User requested window close or OS sent SIGTERM/SIGINT.
    ShutdownRequested,
    GitRecheckRequested,
    CopyToClipboard(String),
    OpenUrl(String),
}
