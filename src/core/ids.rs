//! Newtype identifiers used across the app.
//!
//! These wrap raw `u64` so that distinct id spaces (tab ids, repo versions)
//! cannot be silently swapped at call sites. Cheap, `Copy`, and serde-free —
//! they exist to lean on the type checker, nothing more.

use std::fmt;

/// Identifier for an open tab. Allocated by `App::next_tab_id` at tab creation
/// and threaded through every per-tab task and message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

impl TabId {
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Monotonic version counter for a repository snapshot. Bumped on every reload
/// so stale `Task` results can be dropped without applying their payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RepoVersion(pub u64);

impl RepoVersion {
    pub const fn raw(self) -> u64 {
        self.0
    }

    pub fn bump(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

impl fmt::Display for RepoVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
