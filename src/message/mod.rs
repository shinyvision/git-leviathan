//! Root `Message` hierarchy, grouped by destination: `App` for app-level
//! state, `Screen` for the active screen (or a specific tab), `Toast` for
//! the global toast stack. Helper constructors on `Message` keep the
//! deep-nested common cases readable at call sites.

pub mod app;
pub mod screen;
pub mod toast;

pub use app::AppMessage;
pub use screen::{ScreenMessage, ScreenRouted};
pub use toast::ToastMessage;

use crate::core::TabId;
use crate::plugin::PluginMessage;
use crate::screens::blank::BlankMessage;
use crate::screens::no_git::NoGitMessage;
use crate::screens::repository::RepositoryMessage;
use crate::toast::ToastData;

#[derive(Debug, Clone)]
pub enum Message {
    App(AppMessage),
    Screen(ScreenRouted),
    Toast(ToastMessage),
    Plugin(PluginMessage),
}

impl Message {
    pub fn repo(msg: RepositoryMessage) -> Self {
        Self::Screen(ScreenRouted::Active(ScreenMessage::Repository(Box::new(
            msg,
        ))))
    }

    /// Deliver a repository message to a specific tab (used by background
    /// task results that fire independently of the active tab).
    pub fn tab(tab_id: TabId, msg: RepositoryMessage) -> Self {
        Self::Screen(ScreenRouted::Tab(tab_id, Box::new(msg)))
    }

    pub fn no_git(msg: NoGitMessage) -> Self {
        Self::Screen(ScreenRouted::Active(ScreenMessage::NoGit(msg)))
    }

    pub fn blank(msg: BlankMessage) -> Self {
        Self::Screen(ScreenRouted::Active(ScreenMessage::Blank(msg)))
    }

    pub fn show_toast(data: ToastData) -> Self {
        Self::Toast(ToastMessage::Show(data))
    }

    pub fn dismiss_toast(id: u64) -> Self {
        Self::Toast(ToastMessage::Dismiss(id))
    }

    /// Iced buttons require an `on_press` even when we only want hover
    /// feedback; this is the sink for those.
    pub fn noop() -> Self {
        Self::App(AppMessage::NoOp)
    }
}

impl From<AppMessage> for Message {
    fn from(m: AppMessage) -> Self {
        Self::App(m)
    }
}

impl From<ToastMessage> for Message {
    fn from(m: ToastMessage) -> Self {
        Self::Toast(m)
    }
}
