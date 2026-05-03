//! Screen-routed messages.
//!
//! Two routes: `Active` for the currently-focused screen (most view-emitted
//! messages), and `Tab(id)` for messages bound to a specific tab (background
//! task results that must land on the originating tab even if the user has
//! since switched away).

use crate::core::TabId;
use crate::screens::blank::BlankMessage;
use crate::screens::no_git::NoGitMessage;
use crate::screens::repository::RepositoryMessage;

#[derive(Debug, Clone)]
pub enum ScreenRouted {
    /// Dispatch to whichever screen is active. Used for view-emitted events.
    Active(ScreenMessage),
    /// Dispatch to a specific tab's repository screen regardless of which
    /// tab is currently active — used by background task completions.
    Tab(TabId, Box<RepositoryMessage>),
}

#[derive(Debug, Clone)]
pub enum ScreenMessage {
    Blank(BlankMessage),
    NoGit(NoGitMessage),
    Repository(Box<RepositoryMessage>),
}
