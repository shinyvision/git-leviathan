//! Keyboard input dispatch.
//!
//! Global key handling lives here so `mod.rs` is just composition. The screen
//! forwards key presses and modifier-change events into these free fns, which
//! then drop into the relevant panel/commit-search path. The `focused_panel`
//! gate lives here too — navigation keys (j/k/arrows) only route to the
//! center panel when it has focus, to keep typing in the commit-message
//! editor from triggering list navigation.

use iced::{keyboard, Point, Task};

use crate::message::Message;

use super::panel_messages::{CenterAction, DetailAction, DiffPanelAction};
use super::state::FocusedPanel;
use super::RepositoryScreen;

/// Transient keyboard/mouse input state for the repository screen.
///
/// Three pieces that only make sense together:
/// - `focused_panel` — which of sidebar/center/detail currently owns key nav.
/// - `last_pointer_position` — most recent cursor position, cached so
///   context-menu opens can place the menu under the pointer without having
///   to thread the `CursorMoved` event through every action path.
/// - `modifiers` — current shift/ctrl/alt state, cached from the screen-level
///   subscription so background tabs don't observe stale modifier state after
///   a tab switch.
pub(in crate::screens::repository) struct InputState {
    pub(in crate::screens::repository) focused_panel: FocusedPanel,
    pub(in crate::screens::repository) last_pointer_position: Option<Point>,
    pub(in crate::screens::repository) modifiers: keyboard::Modifiers,
}

impl InputState {
    pub(in crate::screens::repository) fn new() -> Self {
        Self {
            focused_panel: FocusedPanel::Center,
            last_pointer_position: None,
            modifiers: keyboard::Modifiers::default(),
        }
    }
}

pub(super) fn on_modifiers_changed(screen: &mut RepositoryScreen, modifiers: keyboard::Modifiers) {
    // Single source of truth for the screen's cached modifier state: the
    // screen-level subscription only fires while this tab is active, so
    // without syncing here a background tab's `modifiers` stays frozen at
    // whatever was held the last time it was active — causing bogus
    // shift/ctrl clicks after a tab switch.
    screen.input.modifiers = modifiers;
    screen.panels.diff.shift_held = modifiers.shift();
}

pub(super) fn on_overlay_key_pressed(
    screen: &mut RepositoryScreen,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> Option<Task<Message>> {
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }

    if screen.overlay_manager.has_toolbar_dialog() {
        let action = screen.overlay_manager.toolbar_dialog_key_action(key)?;
        return Some(screen.dispatch_overlay_action(action));
    }

    None
}

pub(super) fn on_key_pressed(
    screen: &mut RepositoryScreen,
    key: keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> Option<Task<Message>> {
    // Ctrl+C on an active diff selection → copy.
    if modifiers.control()
        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "c")
        && screen.panels.diff.is_active()
        && screen.panels.diff.diff_selection.is_some()
    {
        return Some(screen.handle_diff_panel_action(DiffPanelAction::DiffCopyRequested));
    }

    // Ctrl+F opens a search bar. Which bar depends on which view is active:
    //   - Diff view (or conflict view fullscreen): a text-buffer search
    //     targeting whichever canvas the user last interacted with (falls
    //     back to the main diff canvas / conflict output).
    //   - Graph view: the commit-list search.
    //   - Any overlay owning the text-input focus: ignored.
    //
    // Focus of the input is deferred to `tick_animation` so the text_input
    // widget is guaranteed to exist in the widget tree before the focus
    // operation runs.
    // Ctrl+Alt+F focuses the sidebar filter input. Checked before the plain
    // Ctrl+F branch so the combined chord doesn't also open the graph /
    // text-buffer search bar.
    if modifiers.control()
        && modifiers.alt()
        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "f")
    {
        return Some(iced::widget::operation::focus(
            super::panels::sidebar::filter_input_id(),
        ));
    }

    if modifiers.control()
        && !modifiers.alt()
        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "f")
    {
        if screen.overlay_manager.is_text_input_active() {
            return Some(Task::none());
        }
        if screen.panels.diff.is_active() {
            screen.panels.diff.open_text_search();
            return Some(Task::none());
        }
        return Some(super::commit_search::open(screen));
    }
    if let Some(task) = media_key(screen, &key, modifiers) {
        return Some(task);
    }
    match key {
        keyboard::Key::Named(keyboard::key::Named::Escape) => {
            if screen.data.commit_search.is_some() {
                screen.data.commit_search = None;
                // Restore navigation focus to the center panel — without
                // this, the `focused_panel` at the moment Ctrl+F was hit
                // (possibly Sidebar/Detail) persists, so j/k silently bail
                // out of `handle_navigate_key` after search closes.
                screen.input.focused_panel = FocusedPanel::Center;
                return Some(Task::none());
            }
            if screen.panels.diff.text_search.is_some() {
                screen.panels.diff.close_text_search();
                return Some(Task::none());
            }
            if screen.panels.diff.is_active() {
                screen.panels.diff.close();
                return Some(screen.panels.center.restore_center_list_scroll());
            }
            Some(Task::none())
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
            Some(handle_navigate(screen, CenterAction::NavigateUp))
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
            Some(handle_navigate(screen, CenterAction::NavigateDown))
        }
        keyboard::Key::Named(keyboard::key::Named::Enter) => Some(handle_enter(screen)),
        keyboard::Key::Character(ref c) if c == "j" => {
            Some(handle_navigate(screen, CenterAction::NavigateDown))
        }
        keyboard::Key::Character(ref c) if c == "k" => {
            Some(handle_navigate(screen, CenterAction::NavigateUp))
        }
        _ => None,
    }
}

/// Transport / viewer shortcuts while a media diff is open and the center
/// panel owns keyboard focus. Returns `None` for keys the media view doesn't
/// claim so the regular handling continues.
fn media_key(
    screen: &mut RepositoryScreen,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> Option<Task<Message>> {
    use super::panels::diff::{MediaAction, TransportCommand, SEEK_STEP_SECS};
    if !screen.panels.diff.is_media_active()
        || screen.overlay_manager.is_text_input_active()
        || screen.input.focused_panel != FocusedPanel::Center
        || modifiers.control()
        || modifiers.alt()
        || modifiers.logo()
    {
        return None;
    }
    let action = match key {
        keyboard::Key::Named(keyboard::key::Named::Space) => {
            MediaAction::KeyTransport(TransportCommand::TogglePlay)
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
            if modifiers.shift() {
                MediaAction::KeyTransport(TransportCommand::StepFrame(-1))
            } else {
                MediaAction::KeyTransport(TransportCommand::SeekRelative(-SEEK_STEP_SECS))
            }
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
            if modifiers.shift() {
                MediaAction::KeyTransport(TransportCommand::StepFrame(1))
            } else {
                MediaAction::KeyTransport(TransportCommand::SeekRelative(SEEK_STEP_SECS))
            }
        }
        keyboard::Key::Named(keyboard::key::Named::Home) => {
            MediaAction::KeyTransport(TransportCommand::Stop)
        }
        keyboard::Key::Named(keyboard::key::Named::End) => {
            MediaAction::KeyTransport(TransportCommand::SeekEnd)
        }
        keyboard::Key::Character(c) => match c.as_str() {
            "m" => MediaAction::KeyTransport(TransportCommand::ToggleMute),
            "l" => MediaAction::KeyTransport(TransportCommand::ToggleLoop),
            "," => MediaAction::KeyTransport(TransportCommand::StepFrame(-1)),
            "." => MediaAction::KeyTransport(TransportCommand::StepFrame(1)),
            "+" | "=" => MediaAction::ZoomIn,
            "-" | "_" => MediaAction::ZoomOut,
            "0" | "f" => MediaAction::ZoomFit,
            "1" => MediaAction::ZoomActual,
            "i" => MediaAction::ToggleInfo,
            "c" => MediaAction::ToggleCheckerboard,
            "g" => MediaAction::TogglePixelGrid,
            _ => return None,
        },
        _ => return None,
    };
    Some(screen.handle_diff_panel_action(DiffPanelAction::Media(action)))
}

fn handle_navigate(screen: &mut RepositoryScreen, action: CenterAction) -> Task<Message> {
    if screen.overlay_manager.is_text_input_active() {
        return Task::none();
    }

    if screen.input.focused_panel == FocusedPanel::Detail {
        if let Some(action) = detail_navigation_action(&action) {
            return screen.handle_detail_action(action);
        }
    }

    if screen.input.focused_panel != FocusedPanel::Center {
        return Task::none();
    }

    if screen.panels.diff.is_active() {
        if let Some(action) = diff_scroll_action(&action) {
            return screen.handle_diff_panel_action(action);
        }
        return Task::none();
    }

    screen.handle_center_action(action)
}

fn handle_enter(screen: &mut RepositoryScreen) -> Task<Message> {
    if screen.overlay_manager.is_text_input_active() {
        return Task::none();
    }
    if screen.input.focused_panel == FocusedPanel::Detail {
        return screen.handle_detail_action(DetailAction::OpenSelectedFile);
    }
    Task::none()
}

pub(in crate::screens::repository) fn detail_navigation_action(
    action: &CenterAction,
) -> Option<DetailAction> {
    match action {
        CenterAction::NavigateUp => Some(DetailAction::NavigateFileUp),
        CenterAction::NavigateDown => Some(DetailAction::NavigateFileDown),
        CenterAction::NavigateFirst => Some(DetailAction::NavigateFileFirst),
        CenterAction::NavigateLast => Some(DetailAction::NavigateFileLast),
        _ => None,
    }
}

pub(in crate::screens::repository) fn extend_detail_navigation_action(
    action: &CenterAction,
) -> Option<DetailAction> {
    match action {
        CenterAction::NavigateUp => Some(DetailAction::ExtendFileSelectionUp),
        CenterAction::NavigateDown => Some(DetailAction::ExtendFileSelectionDown),
        CenterAction::NavigateFirst => Some(DetailAction::ExtendFileSelectionFirst),
        CenterAction::NavigateLast => Some(DetailAction::ExtendFileSelectionLast),
        _ => None,
    }
}

pub(in crate::screens::repository) fn diff_scroll_action(
    action: &CenterAction,
) -> Option<DiffPanelAction> {
    match action {
        CenterAction::NavigateUp => Some(DiffPanelAction::ScrollUp),
        CenterAction::NavigateDown => Some(DiffPanelAction::ScrollDown),
        CenterAction::NavigateFirst => Some(DiffPanelAction::ScrollTop),
        CenterAction::NavigateLast => Some(DiffPanelAction::ScrollBottom),
        _ => None,
    }
}
