//! "Create Worktree" side panel — same animation model as AddRemote (its own
//! Instant-based timeline). State + view + styles split as with AddRemote.

use std::time::Instant;

mod styles;
mod view;

pub(crate) use view::overlay_layers;

pub const PANEL_WIDTH: f32 = 400.0;
pub const ENTER_OFFSET: f32 = 400.0;
const SLIDE_DURATION_MS: f32 = 400.0;

pub(crate) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("create-worktree-branch-name-input")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Opening,
    Closing,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RefChoice {
    LocalBranch(String),
    RemoteBranch { remote: String, branch: String },
}

impl RefChoice {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::LocalBranch(name) => name.clone(),
            Self::RemoteBranch { remote, branch } => format!("{remote}/{branch}"),
        }
    }

    pub(crate) fn git_ref(&self) -> String {
        // Reuses the display form: local branches are looked up by short name,
        // remote-tracking branches by "<remote>/<branch>".
        self.display()
    }
}

pub(crate) struct State {
    pub reference: Option<RefChoice>,
    pub branch_name: String,
    pub branch_name_user_edited: bool,
    pub working_dir: String,
    pub working_dir_user_edited: bool,
    pub available_refs: Vec<RefChoice>,
    pub default_dir_prefix: String, // "<parent_of_main>/<main_name>.worktrees"
    pub animation_start: Instant,
    pub direction: Direction,
    pub needs_focus: bool,
    pub submitting: bool,
    pub error: Option<String>,
    pub dropdown_open: bool,
    /// Cached result of the filesystem checks in `can_submit` (ancestor exists,
    /// target empty if present). Recomputed only when `working_dir` changes so
    /// the per-frame view path never issues `stat`/`read_dir` syscalls.
    working_dir_fs_valid: bool,
}

impl State {
    pub(crate) fn new(available_refs: Vec<RefChoice>, default_dir_prefix: String) -> Self {
        Self {
            reference: None,
            branch_name: String::new(),
            branch_name_user_edited: false,
            working_dir: String::new(),
            working_dir_user_edited: false,
            available_refs,
            default_dir_prefix,
            animation_start: Instant::now(),
            direction: Direction::Opening,
            needs_focus: true,
            submitting: false,
            error: None,
            dropdown_open: false,
            working_dir_fs_valid: false,
        }
    }

    /// Derive default working dir from the current branch name + prefix.
    /// Empty when branch name is blank. Slashes in the branch name are
    /// replaced with `-` so `feat/foo` becomes a single dir component
    /// `<prefix>/feat-foo` instead of a nested `<prefix>/feat/foo`.
    pub(crate) fn derived_default_dir(&self) -> String {
        let trimmed = self.branch_name.trim();
        if trimmed.is_empty() || self.default_dir_prefix.is_empty() {
            String::new()
        } else {
            let safe = trimmed.replace('/', "-");
            format!("{}/{}", self.default_dir_prefix, safe)
        }
    }

    /// Called when branch name input changes. Updates the working_dir alongside
    /// if the user hasn't touched it yet. Marks branch_name as user-edited so
    /// later `set_reference` calls don't overwrite it.
    pub(crate) fn set_branch_name(&mut self, new_name: String) {
        self.branch_name = new_name;
        self.branch_name_user_edited = true;
        if !self.working_dir_user_edited {
            self.working_dir = self.derived_default_dir();
        }
        self.recompute_working_dir_fs_valid();
    }

    pub(crate) fn set_working_dir(&mut self, new_dir: String) {
        self.working_dir = new_dir;
        self.working_dir_user_edited = true;
        self.recompute_working_dir_fs_valid();
    }

    /// Called when the ref dropdown selection changes. Auto-fills the branch
    /// name (and by extension the working dir) from the selected ref — unless
    /// the user has already typed something. Remote refs get their `<remote>/`
    /// prefix stripped so the new local branch isn't named e.g. `origin/foo`.
    pub(crate) fn set_reference(&mut self, choice: RefChoice) {
        self.reference = Some(choice.clone());
        if !self.branch_name_user_edited {
            self.branch_name = match &choice {
                RefChoice::LocalBranch(name) => name.clone(),
                RefChoice::RemoteBranch { branch, .. } => branch.clone(),
            };
            if !self.working_dir_user_edited {
                self.working_dir = self.derived_default_dir();
            }
        }
        self.recompute_working_dir_fs_valid();
    }

    pub(crate) fn can_submit(&self) -> bool {
        if self.submitting
            || self.reference.is_none()
            || self.branch_name.trim().is_empty()
            || self.working_dir.trim().is_empty()
        {
            return false;
        }
        if !std::path::Path::new(self.working_dir.trim()).is_absolute() {
            return false;
        }
        self.working_dir_fs_valid
    }

    /// Runs the blocking filesystem checks once and caches the result. Called
    /// from the `working_dir` mutators so `can_submit` (per-frame) stays cheap.
    fn recompute_working_dir_fs_valid(&mut self) {
        self.working_dir_fs_valid = working_dir_fs_valid(self.working_dir.trim());
    }

    pub(crate) fn slide_offset(&self) -> f32 {
        let elapsed_ms = self.animation_start.elapsed().as_millis() as f32;
        let t = (elapsed_ms / SLIDE_DURATION_MS).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        match self.direction {
            Direction::Opening => ENTER_OFFSET * (1.0 - eased),
            Direction::Closing => ENTER_OFFSET * eased,
        }
    }

    pub(crate) fn is_animation_done(&self) -> bool {
        self.animation_start.elapsed().as_millis() >= SLIDE_DURATION_MS as u128
    }

    pub(crate) fn start_close(&mut self) {
        self.direction = Direction::Closing;
        self.animation_start = Instant::now();
    }
}

/// Filesystem portion of `can_submit`, factored out so it can be cached. An
/// empty or relative `working_dir` returns `false`; the caller's cheap checks
/// already reject those before the cached flag is consulted.
fn working_dir_fs_valid(working_dir: &str) -> bool {
    let target = std::path::Path::new(working_dir);
    if working_dir.is_empty() || !target.is_absolute() {
        return false;
    }
    // Some ancestor must exist — we'll create the rest on submit. A path
    // whose entire chain is missing (e.g. typo on the root component) is
    // rejected. An absolute path will always hit `/` so this primarily
    // rejects empty-parent edge cases.
    if !target.ancestors().skip(1).any(|a| a.exists()) {
        return false;
    }
    // Target, if it exists, must be empty.
    if target.exists() {
        let is_empty = target
            .read_dir()
            .map(|mut it| it.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> State {
        State::new(
            vec![RefChoice::LocalBranch("main".into())],
            "/home/me/myrepo.worktrees".into(),
        )
    }

    #[test]
    fn default_working_dir_follows_branch_name_until_user_edits() {
        let mut s = make_state();
        s.set_branch_name("feat".into());
        assert_eq!(s.working_dir, "/home/me/myrepo.worktrees/feat");

        s.set_branch_name("feat-2".into());
        assert_eq!(s.working_dir, "/home/me/myrepo.worktrees/feat-2");

        s.set_working_dir("/elsewhere".into());
        s.set_branch_name("feat-3".into());
        // User-edited dir should STICK, not re-derive.
        assert_eq!(s.working_dir, "/elsewhere");
    }

    #[test]
    fn can_submit_requires_ref_branch_name_and_dir() {
        // Use a real tmpdir as default_dir_prefix so can_submit's
        // parent-exists check is satisfied.
        let tmp = std::env::temp_dir();
        let prefix = tmp.to_string_lossy().to_string();
        let mut s = State::new(vec![RefChoice::LocalBranch("main".into())], prefix);
        assert!(!s.can_submit(), "fresh state: no ref, no name, no dir");

        s.reference = Some(RefChoice::LocalBranch("main".into()));
        assert!(!s.can_submit(), "still no branch name");

        s.set_branch_name("feat_test_xyz".into());
        assert!(
            s.can_submit(),
            "ref + name + derived dir with real parent => submittable"
        );

        s.working_dir = String::new();
        assert!(!s.can_submit(), "empty dir blocks submit");
    }

    #[test]
    fn set_reference_prefills_branch_name_from_local_ref() {
        let mut s = make_state();
        s.set_reference(RefChoice::LocalBranch("feature-x".into()));
        assert_eq!(s.branch_name, "feature-x");
        assert_eq!(s.working_dir, "/home/me/myrepo.worktrees/feature-x");
    }

    #[test]
    fn set_reference_strips_remote_prefix_when_filling_branch_name() {
        let mut s = make_state();
        s.set_reference(RefChoice::RemoteBranch {
            remote: "origin".into(),
            branch: "feat/nested".into(),
        });
        // Branch name keeps the slash (git allows it); the working-dir
        // component replaces slashes with `-` so it stays a single dir.
        assert_eq!(s.branch_name, "feat/nested");
        assert_eq!(s.working_dir, "/home/me/myrepo.worktrees/feat-nested");
    }

    #[test]
    fn derived_dir_replaces_slashes_in_branch_name() {
        let mut s = make_state();
        s.set_branch_name("feat/foo/bar".into());
        assert_eq!(s.working_dir, "/home/me/myrepo.worktrees/feat-foo-bar");
    }

    #[test]
    fn set_reference_preserves_user_typed_branch_name() {
        let mut s = make_state();
        s.set_branch_name("my-custom".into());
        s.set_reference(RefChoice::LocalBranch("feature-x".into()));
        assert_eq!(s.branch_name, "my-custom");
    }

    #[test]
    fn can_submit_rejects_relative_path() {
        let mut s = make_state();
        s.reference = Some(RefChoice::LocalBranch("main".into()));
        s.set_branch_name("feat".into());
        s.set_working_dir("relative/path".into());
        assert!(!s.can_submit());
    }

    #[test]
    fn can_submit_allows_missing_immediate_parent_when_ancestor_exists() {
        // "<tmp>/nonexistent-<unique>/child" — parent is missing but its
        // ancestor (tmp) exists. add_worktree creates the chain on submit.
        let tmp = std::env::temp_dir();
        let prefix = format!(
            "{}/cw-test-ancestor-{}",
            tmp.to_string_lossy(),
            std::process::id(),
        );
        let mut s = State::new(vec![RefChoice::LocalBranch("main".into())], prefix);
        s.reference = Some(RefChoice::LocalBranch("main".into()));
        s.set_branch_name("feat".into());
        assert!(
            s.can_submit(),
            "missing immediate parent is OK if an ancestor exists"
        );
    }
}
