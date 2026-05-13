use super::*;
use crate::{
    core::TabId,
    services::{DefaultPresenter, GitRepositoryGateway},
};
use std::{path::PathBuf, sync::Arc};

fn sample_toolbar_dialog(owner_scope: &str) -> Dialog {
    let owner = if owner_scope == "plugin" {
        DialogOwner::plugin("owner")
    } else {
        DialogOwner::native("owner")
    };
    Dialog {
        id: DialogId("sample".into()),
        owner,
        message: dialog::model::DialogMessage {
            title: None,
            text: "Sample".into(),
        },
        data: Vec::new(),
        controls: vec![
            dialog::model::DialogControl {
                id: DialogControlId("name".into()),
                label: None,
                text_input: Some(dialog::model::DialogTextInput {
                    placeholder: "Name".into(),
                    value: String::new(),
                    submit_button_id: None,
                    width: None,
                }),
                dropdown: None,
            },
            dialog::model::DialogControl {
                id: DialogControlId("choice".into()),
                label: None,
                text_input: None,
                dropdown: Some(dialog::model::DialogDropdown {
                    placeholder: "Choice".into(),
                    options: vec![dialog::model::DialogDropdownOption {
                        id: "one".into(),
                        text: "One".into(),
                    }],
                    selected_option_id: None,
                    open: false,
                    width: None,
                    leading_icon: None,
                }),
            },
        ],
        buttons: vec![dialog::model::DialogButton {
            id: DialogButtonId("ok".into()),
            text: "OK".into(),
            style: dialog::model::DialogButtonStyle("primary".into()),
            keys: Vec::new(),
            closes_dialog: true,
            enabled: true,
        }],
        dismissible: true,
        autofocus: None,
    }
}

fn text_input_value(manager: &OverlayManager, control_id: &str) -> String {
    manager
        .toolbar_dialog
        .as_ref()
        .unwrap()
        .controls
        .iter()
        .find(|control| control.id == DialogControlId(control_id.into()))
        .unwrap()
        .text_input
        .as_ref()
        .unwrap()
        .value
        .clone()
}

fn dropdown_state(manager: &OverlayManager, control_id: &str) -> (bool, Option<String>) {
    let dropdown = manager
        .toolbar_dialog
        .as_ref()
        .unwrap()
        .controls
        .iter()
        .find(|control| control.id == DialogControlId(control_id.into()))
        .unwrap()
        .dropdown
        .as_ref()
        .unwrap();
    (dropdown.open, dropdown.selected_option_id.clone())
}

fn key(raw: &str) -> dialog::model::DialogKey {
    dialog::model::DialogKey(raw.into())
}

fn dialog_ctx<'a>(operations: &'a mut OperationCoordinator) -> DialogCtx<'a> {
    let gateway = GitRepositoryGateway::from_path(".");
    DialogCtx {
        repository: gateway.clone(),
        primary_repository: gateway,
        presenter: Arc::new(DefaultPresenter::new()),
        tab_id: TabId(1),
        active_path: PathBuf::from("."),
        operations,
    }
}

#[test]
fn new_manager_has_no_active_dialog_and_no_slide() {
    let m = OverlayManager::new();
    assert!(m.toolbar_dialog.is_none());
    assert_eq!(m.toolbar_slide_offset, 0.0);
    assert!(!m.is_animating());
    assert!(!m.is_text_input_active());
}

#[test]
fn native_dialog_input_event_updates_local_dialog_state() {
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(sample_toolbar_dialog("native"));

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId("sample".into()),
        DialogEvent::InputChanged {
            control_id: DialogControlId("name".into()),
            value: "main".into(),
        },
        None,
    );

    assert_eq!(text_input_value(&m, "name"), "main");
}

#[test]
fn plugin_dialog_dropdown_events_update_local_dialog_state() {
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(sample_toolbar_dialog("plugin"));

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId("sample".into()),
        DialogEvent::DropdownToggled {
            control_id: DialogControlId("choice".into()),
        },
        None,
    );
    assert_eq!(dropdown_state(&m, "choice"), (true, None));

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId("sample".into()),
        DialogEvent::DropdownChanged {
            control_id: DialogControlId("choice".into()),
            option_id: "one".into(),
        },
        None,
    );
    assert_eq!(dropdown_state(&m, "choice"), (false, Some("one".into())));
}

#[test]
fn dialog_button_press_closes_when_button_closes_dialog() {
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(sample_toolbar_dialog("native"));

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId("sample".into()),
        DialogEvent::ButtonPressed {
            button_id: DialogButtonId("ok".into()),
        },
        None,
    );

    assert!(m.toolbar_dialog.is_none());
}

#[test]
fn dialog_button_press_keeps_dialog_when_button_does_not_close() {
    let mut dialog = sample_toolbar_dialog("native");
    dialog.buttons[0].closes_dialog = false;
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId("sample".into()),
        DialogEvent::ButtonPressed {
            button_id: DialogButtonId("ok".into()),
        },
        None,
    );

    assert!(m.toolbar_dialog.is_some());
}

#[test]
fn dialog_button_press_ignores_disabled_button() {
    let mut dialog = sample_toolbar_dialog("native");
    dialog.buttons[0].enabled = false;
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId("sample".into()),
        DialogEvent::ButtonPressed {
            button_id: DialogButtonId("ok".into()),
        },
        None,
    );

    assert!(m.toolbar_dialog.is_some());
}

#[test]
fn plugin_dialog_button_press_routes_callback_and_closes_dialog() {
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(sample_toolbar_dialog("plugin"));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId("sample".into()),
            button_id: DialogButtonId("ok".into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(
        dispatch,
        DialogDispatch::PluginDialogButtonPressed {
            ref plugin_id,
            ref dialog_id,
            ref button_id,
        } if plugin_id == "owner" && dialog_id == "sample" && button_id == "ok"
    ));
    assert!(m.toolbar_dialog.is_none());
}

#[test]
fn plugin_dialog_declared_y_and_n_keys_route_button_callbacks() {
    for (key_name, button_id) in [("y", "ok"), ("n", "cancel")] {
        let mut dialog = sample_toolbar_dialog("plugin");
        dialog.buttons[0].keys = vec![key("y")];
        dialog.buttons[0].closes_dialog = false;
        dialog.buttons.push(dialog::model::DialogButton {
            id: DialogButtonId("cancel".into()),
            text: "Cancel".into(),
            style: dialog::model::DialogButtonStyle("secondary".into()),
            keys: vec![key("n")],
            closes_dialog: false,
            enabled: true,
        });
        let mut m = OverlayManager::new();
        m.toolbar_dialog = Some(dialog);

        let action = m
            .toolbar_dialog_key_action(&keyboard::Key::Character(key_name.into()))
            .expect("declared key should trigger button");
        let mut operations = OperationCoordinator::new();
        let dispatch = m.dispatch(action, dialog_ctx(&mut operations));

        assert!(matches!(
            dispatch,
            DialogDispatch::PluginDialogButtonPressed {
                button_id: ref routed_button_id,
                ..
            } if routed_button_id == button_id
        ));
        assert!(m.toolbar_dialog.is_some());
    }
}

#[test]
fn plugin_dialog_bound_escape_key_routes_button_callback() {
    let mut dialog = sample_toolbar_dialog("plugin");
    dialog.buttons[0].keys = vec![key("Esc")];
    dialog.dismissible = true;
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    let action = m
        .toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Escape))
        .expect("Esc-bound button should trigger");
    let mut operations = OperationCoordinator::new();
    let dispatch = m.dispatch(action, dialog_ctx(&mut operations));

    assert!(matches!(
        dispatch,
        DialogDispatch::PluginDialogButtonPressed {
            ref button_id,
            ..
        } if button_id == "ok"
    ));
    assert!(m.toolbar_dialog.is_none());
}

#[test]
fn plugin_dialog_unbound_escape_dismisses_without_callback() {
    let mut dialog = sample_toolbar_dialog("plugin");
    dialog.dismissible = true;
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    let action = m
        .toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Escape))
        .expect("dismissible dialog should handle unbound Escape");
    let mut operations = OperationCoordinator::new();
    let dispatch = m.dispatch(action, dialog_ctx(&mut operations));

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_none());
}

#[test]
fn native_dialog_button_press_never_routes_plugin_callback() {
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(sample_toolbar_dialog("native"));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId("sample".into()),
            button_id: DialogButtonId("ok".into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_none());
}

#[test]
fn push_behind_force_push_requested_opens_force_push_toolbar_dialog() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(push_behind::dialog(push_behind::State {
        branch_name: "main".into(),
        remote_name: "origin".into(),
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(push_behind::DIALOG_ID.into()),
            button_id: DialogButtonId(push_behind::FORCE_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::RestoreCenterListScroll));
    assert!(m.toolbar_dialog.as_ref().is_some_and(force_push::is_dialog));
    assert_eq!(m.toolbar_slide_offset, OVERLAY_ENTER_OFFSET);
    assert!(!operations.is_writing());
}

#[test]
fn force_push_cancel_key_routes_to_native_cancel_button() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(force_push::dialog(force_push::State {
        branch_name: "main".into(),
        remote_name: "origin".into(),
    }));

    let action = m
        .toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Escape))
        .expect("Escape should trigger force-push cancel");

    assert!(matches!(
        &action,
        OverlayPanelAction::DialogButtonPressed {
            button_id: DialogButtonId(ref id),
            ..
        } if id == force_push::CANCEL_BUTTON_ID
    ));

    let mut operations = OperationCoordinator::new();
    let dispatch = m.dispatch(action, dialog_ctx(&mut operations));

    assert!(matches!(dispatch, DialogDispatch::CancelClosed));
    assert!(m.toolbar_dialog.is_none());
    assert!(!operations.is_writing());
}

#[test]
fn force_push_confirm_button_starts_native_force_push() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(force_push::dialog(force_push::State {
        branch_name: "main".into(),
        remote_name: "origin".into(),
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(force_push::DIALOG_ID.into()),
            button_id: DialogButtonId(force_push::CONFIRM_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_none());
    assert!(operations.is_writing());
}

#[test]
fn migrated_simple_confirmations_open_as_toolbar_dialogs() {
    let dialogs = vec![
        stash_delete::dialog(stash_delete::State {
            stash_index: 0,
            display_name: "WIP".into(),
        }),
        delete_tag::dialog(delete_tag::State {
            tag_name: "v1.0.0".into(),
            tag_remote_names: vec!["origin".into()],
        }),
        cherry_pick_confirm::dialog(cherry_pick_confirm::State {
            commit_hash: "abc123".into(),
        }),
        revert_confirm::dialog(revert_confirm::State {
            commit_hash: "def456".into(),
        }),
        remove_worktree::dialog(remove_worktree::State {
            path: PathBuf::from("/tmp/worktree"),
            branch_name: "feature".into(),
            is_active: false,
        }),
        discard::dialog(discard::State {
            target: discard::Target::File("src/main.rs".into()),
        }),
    ];

    for dialog in dialogs {
        let mut m = OverlayManager::new();
        m.open_toolbar_dialog(dialog);

        assert!(m.toolbar_dialog.is_some());
        assert_eq!(m.toolbar_slide_offset, OVERLAY_ENTER_OFFSET);
    }
}

#[test]
fn migrated_simple_confirmation_escape_routes_to_cancel_button() {
    let dialogs = vec![
        stash_delete::dialog(stash_delete::State {
            stash_index: 0,
            display_name: "WIP".into(),
        }),
        delete_tag::dialog(delete_tag::State {
            tag_name: "v1.0.0".into(),
            tag_remote_names: Vec::new(),
        }),
        cherry_pick_confirm::dialog(cherry_pick_confirm::State {
            commit_hash: "abc123".into(),
        }),
        revert_confirm::dialog(revert_confirm::State {
            commit_hash: "def456".into(),
        }),
        remove_worktree::dialog(remove_worktree::State {
            path: PathBuf::from("/tmp/worktree"),
            branch_name: "feature".into(),
            is_active: false,
        }),
        discard::dialog(discard::State {
            target: discard::Target::All,
        }),
    ];

    for dialog in dialogs {
        let mut m = OverlayManager::new();
        m.open_toolbar_dialog(dialog);
        let action = m
            .toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Escape))
            .expect("Escape should trigger cancel");

        assert!(matches!(
            &action,
            OverlayPanelAction::DialogButtonPressed {
                button_id: DialogButtonId(ref id),
                ..
            } if id == "cancel"
        ));

        let mut operations = OperationCoordinator::new();
        let dispatch = m.dispatch(action, dialog_ctx(&mut operations));

        assert!(matches!(dispatch, DialogDispatch::CancelClosed));
        assert!(m.toolbar_dialog.is_none());
        assert!(!operations.is_writing());
    }
}

#[test]
fn stash_delete_confirm_button_starts_native_drop_stash() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(stash_delete::dialog(stash_delete::State {
        stash_index: 2,
        display_name: "WIP".into(),
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(stash_delete::DIALOG_ID.into()),
            button_id: DialogButtonId(stash_delete::CONFIRM_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_none());
    assert!(operations.is_writing());
}

#[test]
fn delete_tag_confirm_button_starts_native_delete_and_keeps_dialog_data() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(delete_tag::dialog(delete_tag::State {
        tag_name: "v1.0.0".into(),
        tag_remote_names: vec!["origin".into(), "upstream".into()],
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(delete_tag::DIALOG_ID.into()),
            button_id: DialogButtonId(delete_tag::CONFIRM_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert_eq!(
        m.delete_tag_remote_names(),
        vec!["origin".to_string(), "upstream".to_string()]
    );
    assert!(m.toolbar_dialog.is_some());
    assert!(operations.is_writing());
}

#[test]
fn cherry_pick_and_revert_confirm_buttons_start_native_operations() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(cherry_pick_confirm::dialog(cherry_pick_confirm::State {
        commit_hash: "abc123".into(),
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(cherry_pick_confirm::DIALOG_ID.into()),
            button_id: DialogButtonId(cherry_pick_confirm::IMMEDIATE_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_none());
    assert!(operations.is_writing());

    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(revert_confirm::dialog(revert_confirm::State {
        commit_hash: "def456".into(),
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(revert_confirm::DIALOG_ID.into()),
            button_id: DialogButtonId(revert_confirm::IN_PLACE_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_none());
    assert!(operations.is_writing());
}

#[test]
fn remove_worktree_and_discard_confirm_buttons_route_native_operations() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(remove_worktree::dialog(remove_worktree::State {
        path: PathBuf::from("/tmp/worktree"),
        branch_name: "feature".into(),
        is_active: false,
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(remove_worktree::DIALOG_ID.into()),
            button_id: DialogButtonId(remove_worktree::CONFIRM_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_none());
    assert!(operations.is_writing());

    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(discard::dialog(discard::State {
        target: discard::Target::Files {
            paths: vec!["src/main.rs".into(), "src/lib.rs".into()],
            count: 2,
        },
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(discard::DIALOG_ID.into()),
            button_id: DialogButtonId(discard::CONFIRM_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_some());
    assert!(operations.is_writing());
}

#[test]
fn toolbar_dialog_key_action_triggers_matching_enabled_button() {
    let mut dialog = sample_toolbar_dialog("native");
    dialog.buttons[0].keys = vec![key("Y")];
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    let action = m
        .toolbar_dialog_key_action(&keyboard::Key::Character("y".into()))
        .expect("declared key should trigger button");

    assert!(matches!(
        action,
        OverlayPanelAction::DialogButtonPressed {
            button_id: DialogButtonId(ref id),
            ..
        } if id == "ok"
    ));
}

#[test]
fn toolbar_dialog_key_action_ignores_disabled_button_keys() {
    let mut dialog = sample_toolbar_dialog("native");
    dialog.buttons[0].keys = vec![key("y")];
    dialog.buttons[0].enabled = false;
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    let action = m.toolbar_dialog_key_action(&keyboard::Key::Character("y".into()));

    assert!(action.is_none());
}

#[test]
fn toolbar_dialog_key_action_ignores_undeclared_keys() {
    let mut dialog = sample_toolbar_dialog("native");
    dialog.buttons[0].keys = vec![key("y")];
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    let action = m.toolbar_dialog_key_action(&keyboard::Key::Character("n".into()));

    assert!(action.is_none());
}

#[test]
fn toolbar_dialog_escape_prefers_bound_button() {
    let mut dialog = sample_toolbar_dialog("native");
    dialog.buttons[0].keys = vec![key("Esc")];
    dialog.buttons[0].closes_dialog = false;
    dialog.dismissible = true;
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    let action = m
        .toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Escape))
        .expect("Esc-bound button should trigger");

    assert!(matches!(
        action,
        OverlayPanelAction::DialogButtonPressed {
            button_id: DialogButtonId(ref id),
            ..
        } if id == "ok"
    ));
}

#[test]
fn toolbar_dialog_escape_dismisses_when_unbound_and_dismissible() {
    let mut dialog = sample_toolbar_dialog("native");
    dialog.dismissible = true;
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    let action = m
        .toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Escape))
        .expect("dismissible dialog should handle unbound Escape");

    assert!(matches!(
        action,
        OverlayPanelAction::DialogDismissed {
            dialog_id: DialogId(ref id)
        } if id == "sample"
    ));
}

#[test]
fn toolbar_dialog_escape_falls_through_when_unbound_and_not_dismissible() {
    let mut dialog = sample_toolbar_dialog("native");
    dialog.dismissible = false;
    let mut m = OverlayManager::new();
    m.toolbar_dialog = Some(dialog);

    let action = m.toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Escape));

    assert!(action.is_none());
}

#[test]
fn is_text_input_active_true_when_rename_branch_open() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(rename_branch::dialog(rename_branch::State {
        branch_name: "old".into(),
        is_remote: false,
        new_branch_input: String::new(),
        remote_name: None,
        remote_ref: None,
    }));
    assert!(m.is_text_input_active());
}

#[test]
fn is_text_input_active_true_when_create_branch_open() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(create_branch::dialog(
        create_branch::State {
            commit_hash: "deadbeef".into(),
            branch_name_input: String::new(),
        },
        &[],
    ));
    assert!(m.is_text_input_active());
}

#[test]
fn is_text_input_active_false_for_confirmation_only_dialogs() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(delete_branch::dialog(delete_branch::State {
        branch_name: "feature".into(),
        is_remote: false,
        has_remote: false,
        remote_name: None,
        remote_ref: None,
    }));
    assert!(!m.is_text_input_active());

    m = OverlayManager::new();
    m.open_toolbar_dialog(stash_delete::dialog(stash_delete::State {
        stash_index: 0,
        display_name: "WIP".into(),
    }));
    assert!(!m.is_text_input_active());
}

#[test]
fn migrated_complex_dialogs_open_as_toolbar_dialogs() {
    let dialogs = vec![
        delete_branch::dialog(delete_branch::State {
            branch_name: "feature".into(),
            is_remote: false,
            has_remote: true,
            remote_name: Some("origin".into()),
            remote_ref: Some("origin/feature".into()),
        }),
        create_branch::dialog(
            create_branch::State {
                commit_hash: "abc123".into(),
                branch_name_input: String::new(),
            },
            &[],
        ),
        rename_branch::dialog(rename_branch::State {
            branch_name: "old".into(),
            is_remote: false,
            new_branch_input: "old".into(),
            remote_name: None,
            remote_ref: None,
        }),
        create_tag::dialog(create_tag::State {
            commit_hash: "abc123".into(),
            tag_name_input: String::new(),
        }),
        conflict_checkout::dialog(conflict_checkout::State {
            branch_name: "main".into(),
            remote_ref: "origin/main".into(),
            new_branch_input: String::new(),
        }),
        set_upstream::dialog(set_upstream::State::new(
            "topic".into(),
            "origin".into(),
            vec!["origin".into()],
        )),
        push_behind::dialog(push_behind::State {
            branch_name: "main".into(),
            remote_name: "origin".into(),
        }),
        modify_delete_conflict::dialog(modify_delete_conflict::State {
            path: "file.txt".into(),
        }),
    ];

    for dialog in dialogs {
        let mut m = OverlayManager::new();
        m.open_toolbar_dialog(dialog);

        assert!(m.toolbar_dialog.is_some());
        assert_eq!(m.toolbar_slide_offset, OVERLAY_ENTER_OFFSET);
    }
}

#[test]
fn migrated_input_dialog_updates_validation_and_enter_submit() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(create_branch::dialog(
        create_branch::State {
            commit_hash: "abc123".into(),
            branch_name_input: String::new(),
        },
        &["existing".into()],
    ));

    let action = m.toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Enter));
    assert!(
        action.is_none(),
        "empty branch name should keep Enter disabled"
    );

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId(create_branch::DIALOG_ID.into()),
        DialogEvent::InputChanged {
            control_id: DialogControlId("branch_name".into()),
            value: "topic".into(),
        },
        None,
    );

    let action = m
        .toolbar_dialog_key_action(&keyboard::Key::Named(keyboard::key::Named::Enter))
        .expect("valid branch name should enable Enter");

    assert!(matches!(
        action,
        OverlayPanelAction::DialogButtonPressed {
            button_id: DialogButtonId(ref id),
            ..
        } if id == create_branch::CONFIRM_BUTTON_ID
    ));
}

#[test]
fn migrated_set_upstream_dropdown_updates_state_and_submit_enabled() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(set_upstream::dialog(set_upstream::State::new(
        "topic".into(),
        "origin".into(),
        vec!["origin".into(), "fork".into()],
    )));

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId(set_upstream::DIALOG_ID.into()),
        DialogEvent::DropdownToggled {
            control_id: DialogControlId("remote".into()),
        },
        None,
    );
    assert_eq!(dropdown_state(&m, "remote"), (true, Some("origin".into())));

    m.dispatch_toolbar_dialog_event_with_ctx(
        DialogId(set_upstream::DIALOG_ID.into()),
        DialogEvent::DropdownChanged {
            control_id: DialogControlId("remote".into()),
            option_id: "fork".into(),
        },
        None,
    );
    assert_eq!(dropdown_state(&m, "remote"), (false, Some("fork".into())));
    assert!(m
        .toolbar_dialog
        .as_ref()
        .unwrap()
        .buttons
        .iter()
        .any(|button| button.id.0 == set_upstream::CONFIRM_BUTTON_ID && button.enabled));
}

#[test]
fn migrated_complex_confirm_buttons_route_native_operations() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(create_tag::dialog(create_tag::State {
        commit_hash: "abc123".into(),
        tag_name_input: "v1".into(),
    }));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(create_tag::DIALOG_ID.into()),
            button_id: DialogButtonId(create_tag::CONFIRM_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(operations.is_writing());

    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(modify_delete_conflict::dialog(
        modify_delete_conflict::State {
            path: "file.txt".into(),
        },
    ));
    let mut operations = OperationCoordinator::new();

    let dispatch = m.dispatch(
        OverlayPanelAction::DialogButtonPressed {
            dialog_id: DialogId(modify_delete_conflict::DIALOG_ID.into()),
            button_id: DialogButtonId(modify_delete_conflict::KEEP_MODIFIED_BUTTON_ID.into()),
        },
        dialog_ctx(&mut operations),
    );

    assert!(matches!(dispatch, DialogDispatch::Task(_)));
    assert!(m.toolbar_dialog.is_none());
    assert!(operations.is_writing());
}

#[test]
fn is_animating_true_when_toolbar_slide_offset_positive() {
    let m = OverlayManager::new();
    assert!(!m.is_animating());
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(stash_delete::dialog(stash_delete::State {
        stash_index: 0,
        display_name: "WIP".into(),
    }));
    assert!(m.is_animating());
}

#[test]
fn tick_animation_decays_toolbar_slide_offset_toward_zero() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(stash_delete::dialog(stash_delete::State {
        stash_index: 0,
        display_name: "WIP".into(),
    }));
    // toolbar_slide_offset was OVERLAY_ENTER_OFFSET after open.
    let before = m.toolbar_slide_offset;
    let task = m.tick_animation(1.0);
    assert!((before - m.toolbar_slide_offset - OVERLAY_SLIDE_SPEED_PX_PER_MS).abs() < 1e-4);
    assert!(task.is_none());
}

#[test]
fn tick_animation_clamps_to_zero_on_large_delta() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(stash_delete::dialog(stash_delete::State {
        stash_index: 0,
        display_name: "WIP".into(),
    }));
    m.tick_animation(1000.0);
    assert_eq!(m.toolbar_slide_offset, 0.0);
    assert!(!m.is_animating());
}

#[test]
fn tick_animation_emits_focus_once_when_create_branch_needs_focus() {
    let mut m = OverlayManager::new();
    m.open_toolbar_dialog(create_branch::dialog(
        create_branch::State {
            commit_hash: "cafebabe".into(),
            branch_name_input: String::new(),
        },
        &[],
    ));
    // Drive toolbar_slide_offset to 0 in one big tick → first tick emits focus.
    let first = m.tick_animation(1000.0);
    assert!(first.is_some(), "focus task expected on animation end");
    assert_eq!(m.toolbar_slide_offset, 0.0);

    // Second tick finds slide already at 0 → no re-emit.
    let second = m.tick_animation(16.0);
    assert!(
        second.is_none(),
        "focus must not re-fire once animation already finished"
    );
}
