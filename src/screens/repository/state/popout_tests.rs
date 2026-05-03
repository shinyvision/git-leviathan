use super::{BranchPopoutController, CommitContextMenuState, ContextMenuState};
use crate::core::{Commit, CommitKind};
use iced::{Point, Rectangle};

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
    Rectangle {
        x,
        y,
        width,
        height,
    }
}

fn open_popout(controller: &mut BranchPopoutController) {
    assert!(controller.open(7, "testhash".to_string()));
    controller.pointer_moved(Point::new(20.0, 20.0));
    controller.update_trigger_bounds(
        Some(rect(10.0, 10.0, 40.0, 20.0)),
        Some(Point::new(20.0, 20.0)),
    );
    controller.update_panel_bounds(
        Some(rect(10.0, 10.0, 140.0, 80.0)),
        Some(Point::new(20.0, 20.0)),
    );
    assert_eq!(controller.active().map(|state| state.commit_idx), Some(7));
}

fn context_menu_state(branch_name: &str) -> ContextMenuState {
    ContextMenuState {
        branch_name: branch_name.to_string(),
        tag_remote_names: Vec::new(),
        default_remote_name: None,
        is_remote: false,
        has_remote: false,
        is_tag: false,
        remote_name: None,
        remote_branch_name: None,
        can_fast_forward: false,
        position: Point::ORIGIN,
    }
}

fn commit_context_menu_state() -> CommitContextMenuState {
    CommitContextMenuState {
        commit_idx: 5,
        commit_hash: "abc123".to_string(),
        position: Point::ORIGIN,
        stash_index: None,
        stash_display_name: None,
        selected_indices: vec![5],
        selected_hashes: vec!["abc123".to_string()],
    }
}

fn sample_commit(hash: &str) -> Commit {
    Commit {
        kind: CommitKind::Commit,
        hash: hash.to_string(),
        short_hash: hash.chars().take(7).collect(),
        message: "message".to_string(),
        author: "author".to_string(),
        date: "today".to_string(),
        parent_hashes: vec![],
        is_merge_in_progress: false,
        conflicted_files: vec![],
        staged_files: vec![],
        unstaged_files: vec![],
    }
}

#[test]
fn opening_context_menu_preserves_active_popout() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);

    controller.open_context_menu(context_menu_state("feature/foo"));

    assert_eq!(controller.active().map(|state| state.commit_idx), Some(7));
    assert_eq!(
        controller
            .active_context_menu()
            .map(|state| state.branch_name.as_str()),
        Some("feature/foo")
    );
}

#[test]
fn context_menu_prevents_popout_from_closing_on_pointer_exit() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);
    controller.open_context_menu(context_menu_state("feature/foo"));

    controller.pointer_moved(Point::new(500.0, 500.0));

    assert_eq!(controller.active().map(|state| state.commit_idx), Some(7));
    assert!(controller.active_context_menu().is_some());
}

#[test]
fn closing_context_menu_also_closes_popout() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);
    controller.open_context_menu(context_menu_state("feature/foo"));

    controller.close_context_menu();

    assert!(controller.active().is_none());
    assert!(controller.active_context_menu().is_none());
}

#[test]
fn pointer_leaving_window_keeps_popout_open_while_context_menu_is_visible() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);
    controller.open_context_menu(context_menu_state("feature/foo"));

    controller.pointer_left_window();

    assert_eq!(controller.active().map(|state| state.commit_idx), Some(7));
    assert!(controller.active_context_menu().is_some());
}

#[test]
fn opening_commit_context_menu_does_not_close_active_popout() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);
    controller.open_commit_context_menu(commit_context_menu_state());

    assert_eq!(
        controller
            .active_commit_context_menu()
            .map(|s| s.commit_idx),
        Some(5)
    );
    assert!(controller.active_context_menu().is_none());
    // Note: The commit context menu does not automatically close the active popout;
    // that is handled at the RepositoryScreen level when processing CommitRightClicked message.
    assert_eq!(controller.active().map(|s| s.commit_idx), Some(7));
}

#[test]
fn commit_context_menu_prevents_popout_from_closing_on_pointer_exit() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);
    controller.open_commit_context_menu(commit_context_menu_state());

    controller.pointer_moved(Point::new(500.0, 500.0));

    assert_eq!(
        controller
            .active_commit_context_menu()
            .map(|s| s.commit_idx),
        Some(5)
    );
}

#[test]
fn patch_commit_indices_or_noop_keeps_popout_open_when_hash_absent() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);
    // None of these commits match "testhash", but the popout must survive.
    let commits = vec![sample_commit("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")];

    controller.patch_commit_indices_or_noop(&commits);

    assert!(
        controller.active().is_some(),
        "fetch-path index patch must not close open popout"
    );
}

#[test]
fn patch_commit_indices_or_noop_updates_index_when_hash_present() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);
    let commits = vec![
        sample_commit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        sample_commit("testhash"),
    ];

    controller.patch_commit_indices_or_noop(&commits);

    assert_eq!(
        controller.active().map(|s| s.commit_idx),
        Some(1),
        "popout's commit_idx must migrate to the hash's new position"
    );
}

#[test]
fn sync_commit_indices_closes_popout_when_hash_absent() {
    let mut controller = BranchPopoutController::default();
    open_popout(&mut controller);
    let commits = vec![sample_commit("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")];

    controller.sync_commit_indices(&commits);

    assert!(controller.active().is_none());
}
