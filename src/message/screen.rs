//! Screen-routed messages. `Active` dispatches to the currently-focused
//! screen (most view-emitted messages); `Tab(id)` binds to a specific tab
//! (background task results that must land on the originating tab even if
//! the user has since switched away).

use crate::core::TabId;
use crate::screens::blank::BlankMessage;
use crate::screens::no_git::NoGitMessage;
use crate::screens::repository::RepositoryMessage;

#[derive(Debug, Clone)]
pub enum ScreenRouted {
    Active(ScreenMessage),
    Tab(TabId, RepositoryMessage),
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // Repository carries LoadedRepo; boxing would churn construction sites.
pub enum ScreenMessage {
    Blank(BlankMessage),
    NoGit(NoGitMessage),
    Repository(RepositoryMessage),
}
