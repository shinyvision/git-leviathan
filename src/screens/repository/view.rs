//! Screen-level view composition. Builds the sidebar/center/detail row
//! (side-by-side on wide windows) or the stacked column layout (narrow
//! windows), the toolbar branch-action wiring, and the overlay layers stack.

use iced::{
    mouse,
    widget::{column, container, mouse_area, responsive, row, stack, MouseArea},
    Element, Length, Padding, Theme,
};

use crate::{
    core::CommitKind,
    message::Message,
    theme,
    widgets::{branch_label, chrome, shared::horizontal_space},
};

use super::panel_messages::CenterAction;
use super::panels::center::view as center_view;
use super::panels::detail::view as detail_view;
use super::panels::detail::DetailOrientation;
use super::panels::{self};
use super::state::EffectiveLayout;
use super::{RepositoryMessage, RepositoryScreen};

/// Padding added to every context-menu / backdrop so the menu's top-left sits
/// just inside the click point rather than flush against it.
const MENU_POSITION_INSET: f32 = 4.0;

/// Per-row vertical advance used to drop the reset submenu down to its parent
/// row in the commit context menu.
const CONTEXT_MENU_ROW_STRIDE: f32 = 21.0;

/// Index of the "Reset … to this commit" row inside the commit context menu
/// for a single-selection menu; `+1` when a multi-select "Squash" row is also
/// present. Tracks the row order built by `center_view::commit_context_menu`.
const RESET_ROW_BASE_INDEX: usize = 3;

pub(in crate::screens::repository) fn view_with_repo_region<'a>(
    screen: &'a RepositoryScreen,
    registry: &'a crate::widgets::chrome::repo_region::RepoRegionRegistry,
    chrome_registry: &'a crate::widgets::chrome::repo_region::RepoChromeRegistry,
) -> Element<'a, Message> {
    if screen.panels.diff.is_conflict_fullscreen() {
        let center_graph = screen.panels.center.view_with(
            &panels::center::CenterViewCtx {
                data: &screen.data,
                selection: &screen.data.selection,
                dirty_commit_message: &screen.panels.detail.dirty_commit_message,
                commit_search: screen.data.commit_search.as_ref(),
                branch_popout: &screen.data.branch_popout,
                window_width: None,
            },
            None,
            None,
        );
        let center = screen
            .panels
            .diff
            .view_or_passthrough(center_graph, screen.is_blocking_git_write_in_flight());
        return container(center)
            .style(|_: &Theme| container::Style {
                background: Some(theme::BG_BASE.into()),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let body =
        responsive(move |size| build_body_with_region(screen, registry, chrome_registry, size));

    container(body)
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_BASE.into()),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn build_body_with_region<'a>(
    screen: &'a RepositoryScreen,
    registry: &'a crate::widgets::chrome::repo_region::RepoRegionRegistry,
    chrome_registry: &'a crate::widgets::chrome::repo_region::RepoChromeRegistry,
    size: iced::Size,
) -> Element<'a, Message> {
    use crate::widgets::chrome::repo_region as rr;

    let layout = screen
        .data
        .resize
        .effective_layout(size.width, screen.data.snapshot.num_lanes());
    let (orientation, sidebar_width, detail_width) = match layout {
        EffectiveLayout::SideBySide { sidebar, detail } => {
            (DetailOrientation::Vertical, sidebar, detail)
        }
        EffectiveLayout::Stacked { sidebar } => (
            DetailOrientation::Horizontal,
            sidebar,
            screen.data.resize.detail_width,
        ),
    };

    let sidebar = screen.panels.sidebar.view(
        &panels::sidebar::SidebarViewCtx {
            sections: screen.data.snapshot.sidebar_sections(),
            commit_count: screen.data.snapshot.commits().len(),
            width: sidebar_width,
            is_resizing: screen.data.resize.sidebar_resizing,
        },
        rr::render_top(registry, rr::Pane::Sidebar),
        rr::render_bottom(registry, rr::Pane::Sidebar),
    );
    let sidebar = wrap_with_chrome(
        sidebar,
        chrome_registry.render_overlay(rr::ChromePane::Sidebar),
        false,
    );
    let center_graph = screen.panels.center.view_with(
        &panels::center::CenterViewCtx {
            data: &screen.data,
            selection: &screen.data.selection,
            dirty_commit_message: &screen.panels.detail.dirty_commit_message,
            commit_search: screen.data.commit_search.as_ref(),
            branch_popout: &screen.data.branch_popout,
            window_width: Some(size.width),
        },
        None,
        None,
    );
    let center_body = screen
        .panels
        .diff
        .view_or_passthrough(center_graph, screen.is_blocking_git_write_in_flight());
    let center = panels::center::wrap_with_slots(
        center_body,
        rr::render_top(registry, rr::Pane::Graph),
        rr::render_bottom(registry, rr::Pane::Graph),
    );
    let center_chrome_pane = if screen.panels.diff.is_active() {
        rr::ChromePane::Diff
    } else {
        rr::ChromePane::Graph
    };
    let center = wrap_with_chrome(
        center,
        chrome_registry.render_overlay(center_chrome_pane),
        true,
    );

    let detail = screen.panels.detail.view_with(
        &panels::detail::DetailViewCtx {
            data: &screen.data,
            selection: &screen.data.selection,
            active_diff_file_path: screen.panels.diff.active_diff_file_path(),
            merged_diff: screen.merged_diff.result(),
            orientation,
            width: detail_width,
        },
        rr::render_top(registry, rr::Pane::Details),
        rr::render_bottom(registry, rr::Pane::Details),
    );
    let detail = wrap_with_chrome(
        detail,
        chrome_registry.render_overlay(rr::ChromePane::Details),
        false,
    );

    let sidebar = pane_frame(
        sidebar,
        Length::Fixed(sidebar_width + theme::PANE_SPLITTER_SIZE),
    );
    let center = pane_frame(center, Length::Fill);

    match orientation {
        DetailOrientation::Vertical => {
            let detail = pane_frame(
                detail,
                Length::Fixed(detail_width + theme::PANE_SPLITTER_SIZE),
            );
            row![sidebar, center, detail]
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        }
        DetailOrientation::Horizontal => {
            let splitter = detail_height_splitter(screen.data.resize.detail_height_resizing);
            let detail = pane_frame(detail, Length::Fill);
            let detail_pane = container(detail)
                .width(Length::Fill)
                .height(Length::Fixed(screen.data.resize.detail_height));
            column![
                row![sidebar, center]
                    .height(Length::Fill)
                    .width(Length::Fill),
                splitter,
                detail_pane,
            ]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
        }
    }
}

fn pane_frame<'a>(pane: Element<'a, Message>, width: Length) -> Element<'a, Message> {
    container(pane).width(width).height(Length::Fill).into()
}

fn wrap_with_chrome<'a>(
    body: Element<'a, Message>,
    layers: Option<Vec<Element<'a, Message>>>,
    fill: bool,
) -> Element<'a, Message> {
    match layers {
        None => body,
        Some(layers) => {
            let mut children: Vec<Element<'a, Message>> = Vec::with_capacity(layers.len() + 1);
            children.push(body);
            for layer in layers {
                children.push(non_interactive(layer));
            }
            let stacked = stack(children);
            if fill {
                stacked.width(Length::Fill).height(Length::Fill).into()
            } else {
                stacked.into()
            }
        }
    }
}

fn non_interactive<'a>(layer: Element<'a, Message>) -> Element<'a, Message> {
    container(layer)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .into()
}

fn detail_height_splitter(is_resizing: bool) -> Element<'static, Message> {
    let handle = container(horizontal_space())
        .width(Length::Fill)
        .height(Length::Fixed(theme::PANE_SPLITTER_SIZE))
        .style(move |_: &Theme| container::Style {
            background: if is_resizing {
                Some(theme::ACCENT_BLUE.into())
            } else {
                Some(theme::BORDER.into())
            },
            ..Default::default()
        });
    mouse_area(handle)
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::DetailHeightResizeStarted,
        )))
        .interaction(mouse::Interaction::ResizingVertically)
        .into()
}

pub(in crate::screens::repository) fn toolbar<'a>(
    screen: &'a RepositoryScreen,
    ctx: &crate::screens::ToolbarCtx<'a>,
) -> Element<'a, Message> {
    let branch_action = if screen.panels.diff.is_active() {
        None
    } else {
        let idx = screen.data.selection.selected_commit();
        screen.data.selected_commit(idx).and_then(|commit| {
            if commit.kind == CommitKind::Commit {
                Some(Message::App(crate::message::AppMessage::InvokeCommand {
                    id: "branch.create".to_string(),
                    args: serde_json::json!({
                        "commit_idx": idx as i64,
                        "hash": commit.hash.clone(),
                    }),
                }))
            } else {
                None
            }
        })
    };
    let slot_ctx = chrome::SlotCtx::new(
        screen.data.snapshot.repo_name(),
        screen.data.snapshot.current_branch(),
        ctx.now,
        screen.data.animation.push_started_at(),
        screen.data.animation.pull_started_at(),
        branch_action,
    );
    let main_bar = chrome::main_bar_view(ctx.main_bar_registry, &slot_ctx);
    screen
        .overlay_manager
        .toolbar_overlay(main_bar, &screen.data)
}

pub(in crate::screens::repository) fn overlay_layers(
    screen: &RepositoryScreen,
) -> Vec<Element<'_, Message>> {
    let mut layers = Vec::new();

    if let Some(menu_state) = screen.data.branch_popout.active_context_menu() {
        let backdrop = MouseArea::new(
            container(horizontal_space())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CloseContextMenu,
        )))
        .on_right_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CloseContextMenu,
        )))
        .into();

        let menu = container(branch_label::branch_context_menu(
            menu_state,
            screen.data.snapshot.current_branch(),
        ))
        .padding(Padding {
            top: (menu_state.position.y + MENU_POSITION_INSET).max(0.0),
            left: (menu_state.position.x + MENU_POSITION_INSET).max(0.0),
            ..Default::default()
        })
        .into();

        layers.push(backdrop);
        layers.push(menu);
    } else if let Some(commit_menu_state) = screen.data.branch_popout.active_commit_context_menu() {
        let backdrop = MouseArea::new(
            container(horizontal_space())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CloseContextMenu,
        )))
        .on_right_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CloseContextMenu,
        )))
        .into();

        let menu = container(center_view::commit_context_menu(
            commit_menu_state,
            screen.data.snapshot.current_branch(),
            screen.data.snapshot.head_hash(),
        ))
        .padding(Padding {
            top: (commit_menu_state.position.y + MENU_POSITION_INSET).max(0.0),
            left: (commit_menu_state.position.x + MENU_POSITION_INSET).max(0.0),
            ..Default::default()
        })
        .into();

        layers.push(backdrop);
        layers.push(menu);

        if let Some(submenu_state) = screen.data.branch_popout.active_reset_submenu() {
            let parent_width = center_view::commit_context_menu_width(
                commit_menu_state,
                screen.data.snapshot.current_branch(),
                screen.data.snapshot.head_hash(),
            );
            let reset_row_idx = {
                let mut i = RESET_ROW_BASE_INDEX;
                if commit_menu_state.selected_indices.len() > 1 {
                    i += 1;
                }
                i
            };
            let submenu_inner = MouseArea::new(center_view::reset_submenu(submenu_state))
                .on_enter(Message::repo(RepositoryMessage::Center(
                    CenterAction::ResetSubmenuHoverChanged(true),
                )))
                .on_exit(Message::repo(RepositoryMessage::Center(
                    CenterAction::ResetSubmenuHoverChanged(false),
                )));
            let submenu = container(submenu_inner)
                .padding(Padding {
                    top: (submenu_state.position.y
                        + MENU_POSITION_INSET
                        + reset_row_idx as f32 * CONTEXT_MENU_ROW_STRIDE)
                        .max(0.0),
                    left: (submenu_state.position.x + MENU_POSITION_INSET + parent_width).max(0.0),
                    ..Default::default()
                })
                .into();
            layers.push(submenu);
        }
    } else if let Some(worktree_menu_state) =
        screen.data.branch_popout.active_worktree_context_menu()
    {
        let backdrop = MouseArea::new(
            container(horizontal_space())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CloseContextMenu,
        )))
        .on_right_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CloseContextMenu,
        )))
        .into();

        let items: Vec<crate::widgets::context_menu::ContextMenuItem> =
            vec![crate::widgets::context_menu::context_menu_item(
                "Remove worktree\u{2026}",
                Some(Message::repo(RepositoryMessage::OverlayPanel(
                    super::panel_messages::OverlayPanelAction::WorktreeRemoveRequested {
                        path: worktree_menu_state.path.clone(),
                        branch_name: worktree_menu_state.branch_name.clone(),
                        is_active: worktree_menu_state.is_active,
                    },
                ))),
            )];
        let menu = container(crate::widgets::context_menu::ContextMenu::new(items))
            .padding(Padding {
                top: (worktree_menu_state.position.y + MENU_POSITION_INSET).max(0.0),
                left: (worktree_menu_state.position.x + MENU_POSITION_INSET).max(0.0),
                ..Default::default()
            })
            .into();

        layers.push(backdrop);
        layers.push(menu);
    } else if let Some(dirty_menu_state) =
        screen.data.branch_popout.active_dirty_file_context_menu()
    {
        let backdrop = MouseArea::new(
            container(horizontal_space())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CloseContextMenu,
        )))
        .on_right_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CloseContextMenu,
        )))
        .into();

        let menu = container(detail_view::dirty_file_context_menu(dirty_menu_state))
            .padding(Padding {
                top: (dirty_menu_state.position.y + MENU_POSITION_INSET).max(0.0),
                left: (dirty_menu_state.position.x + MENU_POSITION_INSET).max(0.0),
                ..Default::default()
            })
            .into();

        layers.push(backdrop);
        layers.push(menu);
    }

    layers.extend(
        screen
            .overlay_manager
            .overlay_layers(screen.data.resize.sidebar_width),
    );

    layers
}
