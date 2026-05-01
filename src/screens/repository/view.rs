//! Screen-level view composition. Builds the sidebar/center/detail row
//! (side-by-side on wide windows) or the stacked column layout (narrow
//! windows), the toolbar branch-action wiring, and the overlay layers stack.

use iced::{
    mouse,
    widget::{column, container, mouse_area, responsive, row, MouseArea},
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

pub(in crate::screens::repository) fn view_with_repo_region<'a>(
    screen: &'a RepositoryScreen,
    registry: &'a crate::widgets::chrome::repo_region::RepoRegionRegistry,
) -> Element<'a, Message> {
    if screen.panels.diff.is_conflict_fullscreen() {
        let center_graph = screen.panels.center.view_with(&panels::center::CenterViewCtx {
            data: &screen.data,
            selection: &screen.data.selection,
            dirty_commit_message: &screen.panels.detail.dirty_commit_message,
            commit_search: screen.data.commit_search.as_ref(),
            branch_popout: &screen.data.branch_popout,
        });
        let center = screen.panels.diff.view_or_passthrough(center_graph);
        return container(center)
            .style(|_: &Theme| container::Style {
                background: Some(theme::BG_BASE.into()),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let body = responsive(move |size| build_body_with_region(screen, registry, size));

    container(body)
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_BASE.into()),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn wrap_pane<'a>(
    pane: crate::widgets::chrome::repo_region::Pane,
    body: Element<'a, Message>,
    registry: &'a crate::widgets::chrome::repo_region::RepoRegionRegistry,
) -> Element<'a, Message> {
    use crate::widgets::chrome::repo_region as rr;

    let top_empty = rr::is_empty(registry, pane, rr::Section::Top);
    let bottom_empty = rr::is_empty(registry, pane, rr::Section::Bottom);
    if top_empty && bottom_empty {
        return body;
    }

    let pane_ctx = rr::RepoPaneCtx::new();
    let mut col = column![].spacing(0).width(Length::Fill).height(Length::Fill);
    if !top_empty {
        let items: Vec<Element<'a, Message>> = rr::iter(registry, pane, rr::Section::Top)
            .map(|s| (s.builder)(&pane_ctx))
            .collect();
        col = col.push(iced::widget::column(items).spacing(0));
    }
    col = col.push(body);
    if !bottom_empty {
        let items: Vec<Element<'a, Message>> = rr::iter(registry, pane, rr::Section::Bottom)
            .map(|s| (s.builder)(&pane_ctx))
            .collect();
        col = col.push(iced::widget::column(items).spacing(0));
    }
    col.into()
}

fn build_body_with_region<'a>(
    screen: &'a RepositoryScreen,
    registry: &'a crate::widgets::chrome::repo_region::RepoRegionRegistry,
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

    let sidebar_body = screen.panels.sidebar.view(&panels::sidebar::SidebarViewCtx {
        sections: screen.data.snapshot.sidebar_sections(),
        commit_count: screen.data.snapshot.commits().len(),
        width: sidebar_width,
        is_resizing: screen.data.resize.sidebar_resizing,
        active_worktree_path: screen.fleet.active_path(),
    });
    let center_graph = screen.panels.center.view_with(&panels::center::CenterViewCtx {
        data: &screen.data,
        selection: &screen.data.selection,
        dirty_commit_message: &screen.panels.detail.dirty_commit_message,
        commit_search: screen.data.commit_search.as_ref(),
        branch_popout: &screen.data.branch_popout,
    });
    let center_body = screen.panels.diff.view_or_passthrough(center_graph);

    let detail_body = screen
        .panels
        .detail
        .view_with(&panels::detail::DetailViewCtx {
            data: &screen.data,
            selection: &screen.data.selection,
            active_diff_file_path: screen.panels.diff.active_diff_file_path(),
            merged_diff: screen.merged_diff.result(),
            orientation,
            width: detail_width,
        });

    let sidebar = wrap_pane(rr::Pane::Sidebar, sidebar_body, registry);
    let center = wrap_pane(rr::Pane::Graph, center_body, registry);
    let detail = wrap_pane(rr::Pane::Details, detail_body, registry);

    match orientation {
        DetailOrientation::Vertical => row![sidebar, center, detail].height(Length::Fill).into(),
        DetailOrientation::Horizontal => {
            let splitter = detail_height_splitter(screen.data.resize.detail_height_resizing);
            let detail_pane = container(detail)
                .width(Length::Fill)
                .height(Length::Fixed(screen.data.resize.detail_height));
            column![
                row![sidebar, center].height(Length::Fill).width(Length::Fill),
                splitter,
                detail_pane,
            ]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
        }
    }
}

fn detail_height_splitter(is_resizing: bool) -> Element<'static, Message> {
    let handle = container(horizontal_space())
        .width(Length::Fill)
        .height(Length::Fixed(5.0))
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
                Some(Message::repo(RepositoryMessage::Center(
                    CenterAction::CreateBranchHereRequested {
                        commit_idx: idx,
                        commit_hash: commit.hash.clone(),
                    },
                )))
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
    screen.overlay_manager.toolbar_overlay(main_bar, &screen.data)
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
            top: (menu_state.position.y + 4.0).max(0.0),
            left: (menu_state.position.x + 4.0).max(0.0),
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
            top: (commit_menu_state.position.y + 4.0).max(0.0),
            left: (commit_menu_state.position.x + 4.0).max(0.0),
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
                let mut i = 3usize;
                if commit_menu_state.selected_indices.len() > 1 {
                    i += 1;
                }
                i
            };
            let row_stride: f32 = 21.0;
            let submenu_inner = MouseArea::new(center_view::reset_submenu(submenu_state))
                .on_enter(Message::repo(RepositoryMessage::Center(
                    CenterAction::ResetSubmenuHoverChanged(true),
                )))
                .on_exit(Message::repo(RepositoryMessage::Center(
                    CenterAction::ResetSubmenuHoverChanged(false),
                )));
            let submenu = container(submenu_inner)
                .padding(Padding {
                    top: (submenu_state.position.y + 4.0 + reset_row_idx as f32 * row_stride)
                        .max(0.0),
                    left: (submenu_state.position.x + 4.0 + parent_width).max(0.0),
                    ..Default::default()
                })
                .into();
            layers.push(submenu);
        }
    } else if let Some(worktree_menu_state) = screen.data.branch_popout.active_worktree_context_menu() {
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
                top: (worktree_menu_state.position.y + 4.0).max(0.0),
                left: (worktree_menu_state.position.x + 4.0).max(0.0),
                ..Default::default()
            })
            .into();

        layers.push(backdrop);
        layers.push(menu);
    } else if let Some(dirty_menu_state) = screen.data.branch_popout.active_dirty_file_context_menu() {
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
                top: (dirty_menu_state.position.y + 4.0).max(0.0),
                left: (dirty_menu_state.position.x + 4.0).max(0.0),
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
