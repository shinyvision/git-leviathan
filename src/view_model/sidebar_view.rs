use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Branch {
    pub name: String,
    pub is_current: bool,
    pub children: Vec<Branch>,
    pub worktree_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarSectionKind {
    Local,
    Remote,
    Worktrees,
    Stashes,

    Tags,
}

impl SidebarSectionKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Local => "LOCAL",
            Self::Remote => "REMOTE",
            Self::Worktrees => "WORKTREES",
            Self::Stashes => "STASHES",

            Self::Tags => "TAGS",
        }
    }

    /// Stable key used to persist per-section state in the sqlite store.
    /// Separate from `title()` so display labels can change without
    /// invalidating stored rows.
    pub fn db_key(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
            Self::Worktrees => "worktrees",
            Self::Stashes => "stashes",
            Self::Tags => "tags",
        }
    }

    pub fn from_db_key(s: &str) -> Option<Self> {
        match s {
            "local" => Some(Self::Local),
            "remote" => Some(Self::Remote),
            "worktrees" => Some(Self::Worktrees),
            "stashes" => Some(Self::Stashes),
            "tags" => Some(Self::Tags),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SidebarSection {
    pub kind: SidebarSectionKind,
    pub count: u32,
    pub branches: Vec<Branch>,
    pub stashes: Vec<SidebarStash>,
}

#[derive(Debug, Clone)]
pub struct SidebarStash {
    pub stash_index: usize,
    pub hash: String,
    pub display_name: String,
}

impl SidebarSection {
    pub fn title(&self) -> &'static str {
        self.kind.title()
    }
}
