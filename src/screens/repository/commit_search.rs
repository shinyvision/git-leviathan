//! Commit-list search. Adapter over `widgets::search_widget::SearchWidget`
//! that keeps the domain-specific matching logic (case-insensitive match
//! against commit message, author, and hash) and the "first match at or
//! after anchor" selection semantics. UI, cycling, and focus plumbing live
//! in the generic widget.

use iced::Element;

use crate::{
    core::Commit,
    message::Message,
    widgets::search_widget::{self as sw, SearchWidget, SearchWidgetId},
};

use super::RepositoryMessage;

pub(super) const COMMITS_SEARCH_ID: SearchWidgetId = SearchWidgetId("commits");

pub(super) fn commit_search_input_id() -> iced::widget::Id {
    sw::input_id(COMMITS_SEARCH_ID)
}

pub use sw::SearchWidgetMessage as CommitSearchMessage;

#[derive(Debug, Clone)]
pub(super) struct CommitSearch {
    widget: SearchWidget<usize>,
}

impl CommitSearch {
    pub(super) fn new() -> Self {
        Self {
            widget: SearchWidget::new(COMMITS_SEARCH_ID),
        }
    }

    pub(super) fn needs_focus(&self) -> bool {
        self.widget.needs_focus()
    }

    pub(super) fn take_needs_focus(&mut self) -> bool {
        self.widget.take_needs_focus()
    }

    pub(super) fn request_focus(&mut self) {
        self.widget.request_focus();
    }

    pub(super) fn query(&self) -> &str {
        self.widget.query()
    }

    pub(super) fn matches(&self) -> &[usize] {
        self.widget.matches()
    }

    pub(super) fn current_match_idx(&self) -> Option<usize> {
        self.widget.current_match()
    }

    pub(super) fn is_dimming_active(&self) -> bool {
        self.widget.is_dimming_active()
    }

    pub(super) fn is_match(&self, idx: usize) -> bool {
        self.widget.matches().binary_search(&idx).is_ok()
    }

    pub(super) fn set_query(&mut self, query: String, commits: &[Commit], anchor_idx: usize) {
        self.widget.set_query(query);
        self.recompute_matches(commits, Some(anchor_idx));
    }

    pub(super) fn recompute_matches(&mut self, commits: &[Commit], anchor_idx: Option<usize>) {
        let needle = self.widget.query().trim().to_ascii_lowercase();
        if needle.is_empty() {
            self.widget.clear_matches();
            return;
        }
        let mut matches: Vec<usize> = Vec::new();
        for (idx, commit) in commits.iter().enumerate() {
            if commit_matches(commit, &needle) {
                matches.push(idx);
            }
        }
        if matches.is_empty() {
            self.widget.set_matches(matches, None);
            return;
        }
        // Select first match going DOWN from anchor (higher idx, inclusive).
        // If nothing at/after anchor, wrap to first match overall.
        let initial = anchor_idx
            .and_then(|anchor| matches.iter().position(|&m| m >= anchor))
            .unwrap_or(0);
        self.widget.set_matches(matches, Some(initial));
    }

    pub(super) fn go_next(&mut self) -> Option<usize> {
        self.widget.go_next()
    }

    pub(super) fn go_previous(&mut self) -> Option<usize> {
        self.widget.go_previous()
    }
}

fn commit_matches(commit: &Commit, needle_lower: &str) -> bool {
    commit.message.to_ascii_lowercase().contains(needle_lower)
        || commit.author.to_ascii_lowercase().contains(needle_lower)
        || commit.hash.to_ascii_lowercase().contains(needle_lower)
}

use iced::Task;

use super::state::FocusedPanel;
use super::RepositoryScreen;

pub(super) fn open(screen: &mut RepositoryScreen) -> Task<Message> {
    if let Some(existing) = screen.data.commit_search.as_mut() {
        existing.request_focus();
    } else {
        let mut search = CommitSearch::new();
        search.recompute_matches(
            screen.data.snapshot.commits(),
            Some(screen.data.selection.anchor()),
        );
        screen.data.commit_search = Some(search);
    }
    Task::none()
}

pub(super) fn handle_action(
    screen: &mut RepositoryScreen,
    action: CommitSearchMessage,
) -> Task<Message> {
    match action {
        CommitSearchMessage::Close => {
            screen.data.commit_search = None;
            screen.input.focused_panel = FocusedPanel::Center;
            Task::none()
        }
        CommitSearchMessage::BarClicked => {
            if let Some(search) = screen.data.commit_search.as_mut() {
                search.request_focus();
            }
            Task::none()
        }
        CommitSearchMessage::Submit => {
            let step_back = screen.input.modifiers.shift();
            let next_idx = screen.data.commit_search.as_mut().and_then(|search| {
                if step_back {
                    search.go_previous()
                } else {
                    search.go_next()
                }
            });
            match next_idx {
                Some(idx) => select_and_scroll(screen, idx),
                None => Task::none(),
            }
        }
        CommitSearchMessage::InputChanged(query) => {
            let matched_idx = {
                let Some(search) = screen.data.commit_search.as_mut() else {
                    return Task::none();
                };
                search.set_query(
                    query,
                    screen.data.snapshot.commits(),
                    screen.data.selection.anchor(),
                );
                search.current_match_idx()
            };
            match matched_idx {
                Some(idx) => select_and_scroll(screen, idx),
                None => Task::none(),
            }
        }
        CommitSearchMessage::Next => {
            let Some(idx) = screen
                .data
                .commit_search
                .as_mut()
                .and_then(|search| search.go_next())
            else {
                return Task::none();
            };
            select_and_scroll(screen, idx)
        }
        CommitSearchMessage::Previous => {
            let Some(idx) = screen
                .data
                .commit_search
                .as_mut()
                .and_then(|search| search.go_previous())
            else {
                return Task::none();
            };
            select_and_scroll(screen, idx)
        }
    }
}

fn select_and_scroll(screen: &mut RepositoryScreen, idx: usize) -> Task<Message> {
    let select_task = screen.select_commit(idx);
    let scroll_task = screen
        .panels
        .center
        .scroll_to_commit(idx)
        .unwrap_or(Task::none());
    Task::batch([select_task, scroll_task])
}

/// Recomputes the active search's match set against the current commit list
/// (e.g., after a repo reload shifted indices). No-op when search is closed.
pub(super) fn refresh_matches(screen: &mut RepositoryScreen) {
    if let Some(search) = screen.data.commit_search.as_mut() {
        search.recompute_matches(
            screen.data.snapshot.commits(),
            Some(screen.data.selection.anchor()),
        );
    }
}

fn search_bar_view<'a>(search: &'a CommitSearch) -> Element<'a, Message> {
    sw::search_bar_view(
        COMMITS_SEARCH_ID,
        search.query(),
        search.widget.current_match_index(),
        search.matches().len(),
        |msg| Message::repo(RepositoryMessage::CommitSearch(msg)),
    )
}

/// Wraps a list content element with a search bar overlay layer. The
/// wrapper structure is kept identical whether or not the search is active
/// so the scrollable inside `content` keeps its internal scroll offset
/// across open/close — swapping in a different parent (Stack vs plain
/// content) rebuilds the widget tree and resets scroll state.
pub(super) fn overlay_if_active<'a>(
    content: Element<'a, Message>,
    search: Option<&'a CommitSearch>,
) -> Element<'a, Message> {
    let bar = search.map(search_bar_view);
    sw::overlay(content, bar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CommitKind;

    fn mk(hash: &str, msg: &str, author: &str) -> Commit {
        Commit {
            kind: CommitKind::Commit,
            hash: hash.to_string(),
            short_hash: hash.chars().take(7).collect(),
            message: msg.to_string(),
            author: author.to_string(),
            date: String::new(),
            parent_hashes: vec![],
            is_merge_in_progress: false,
            conflicted_files: vec![],
            staged_files: vec![],
            unstaged_files: vec![],
        }
    }

    fn sample_commits() -> Vec<Commit> {
        vec![
            mk("aaa111", "Add login flow", "Alice"),
            mk("bbb222", "Fix bug in parser", "Bob"),
            mk("ccc333", "Refactor LOGIN util", "Carol"),
            mk("ddd444", "Docs update", "Alice"),
            mk("eee555", "Login regression test", "Dave"),
        ]
    }

    #[test]
    fn empty_query_clears_matches() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("".into(), &commits, 0);
        assert!(s.matches().is_empty());
        assert!(!s.is_dimming_active());
    }

    #[test]
    fn matches_message_case_insensitively() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("login".into(), &commits, 0);
        assert_eq!(s.matches(), &[0, 2, 4]);
        assert_eq!(s.current_match_idx(), Some(0));
    }

    #[test]
    fn matches_author() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("alice".into(), &commits, 0);
        assert_eq!(s.matches(), &[0, 3]);
    }

    #[test]
    fn matches_sha_prefix() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("ccc".into(), &commits, 0);
        assert_eq!(s.matches(), &[2]);
    }

    #[test]
    fn first_match_selected_from_anchor_going_down() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        // Anchor is at idx 3 (Docs update). First match with idx >= 3 in
        // ["login" matches 0,2,4] is idx 4.
        s.set_query("login".into(), &commits, 3);
        assert_eq!(s.current_match_idx(), Some(4));
    }

    #[test]
    fn anchor_itself_is_inclusive() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("login".into(), &commits, 2);
        assert_eq!(s.current_match_idx(), Some(2));
    }

    #[test]
    fn wraps_when_no_matches_below_anchor() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        // "alice" matches 0 and 3. Anchor at 4 (beyond last match) → wrap to 0.
        s.set_query("alice".into(), &commits, 4);
        assert_eq!(s.current_match_idx(), Some(0));
    }

    #[test]
    fn go_next_cycles_through_matches() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("login".into(), &commits, 0);
        assert_eq!(s.current_match_idx(), Some(0));
        assert_eq!(s.go_next(), Some(2));
        assert_eq!(s.go_next(), Some(4));
        assert_eq!(s.go_next(), Some(0)); // wraps
    }

    #[test]
    fn go_previous_cycles_through_matches() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("login".into(), &commits, 0);
        assert_eq!(s.go_previous(), Some(4)); // wraps from 0 → last
        assert_eq!(s.go_previous(), Some(2));
        assert_eq!(s.go_previous(), Some(0));
    }

    #[test]
    fn is_match_only_true_for_matching_indices() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("login".into(), &commits, 0);
        assert!(s.is_match(0));
        assert!(!s.is_match(1));
        assert!(s.is_match(2));
        assert!(!s.is_match(3));
        assert!(s.is_match(4));
    }

    #[test]
    fn no_matches_leaves_current_none() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("nothingmatches".into(), &commits, 0);
        assert!(s.matches().is_empty());
        assert_eq!(s.current_match_idx(), None);
        assert!(
            s.is_dimming_active(),
            "dimming stays on so UI shows 0/results"
        );
    }

    #[test]
    fn whitespace_only_query_treated_as_empty() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("   ".into(), &commits, 0);
        assert!(s.matches().is_empty());
        assert!(!s.is_dimming_active());
    }

    #[test]
    fn recompute_matches_after_repo_shift() {
        let commits = sample_commits();
        let mut s = CommitSearch::new();
        s.set_query("alice".into(), &commits, 0);
        assert_eq!(s.matches(), &[0, 3]);

        let mut shifted = commits.clone();
        shifted.insert(0, mk("fff666", "New dirty work", "Bob"));
        s.recompute_matches(&shifted, Some(0));
        assert_eq!(s.matches(), &[1, 4]);
        assert_eq!(s.current_match_idx(), Some(1));
    }
}
