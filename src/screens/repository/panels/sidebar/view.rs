use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, MouseArea},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::{
    assets,
    message::Message,
    style, theme,
    view_model::{Branch, SidebarSection, SidebarSectionKind, SidebarStash},
    widgets::primitives::hoverable::{HoverStatus, Hoverable, HoverableSwap},
    widgets::shared::{horizontal_space, scrollbar_style},
};

use super::super::super::{overlays, panel_messages::OverlayPanelAction, RepositoryMessage};
use super::{SidebarAction, SidebarPanel, SidebarViewCtx};

pub(in crate::screens::repository) fn filter_input_id() -> iced::widget::Id {
    iced::widget::Id::new("sidebar-filter-input")
}

fn matches_filter(needle: &str, hay: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

pub(super) fn view<'a>(panel: &'a SidebarPanel, ctx: &SidebarViewCtx<'a>) -> Element<'a, Message> {
    let sidebar_width = ctx.width;
    let is_resizing = ctx.is_resizing;
    let filter_query = panel.filter_query();
    let filter_input = text_input("Filter (Ctrl + Alt + f)", filter_query)
        .id(filter_input_id())
        .on_input(|s| sidebar_msg(SidebarAction::FilterChanged(s)))
        .size(theme::FONT_SM)
        .padding(Padding::from([3, 6]))
        .width(Length::Fill)
        .style(filter_input_style);

    let mut sections: Vec<Element<Message>> = vec![container(filter_input)
        .padding(Padding::from([6, 8]))
        .width(Length::Fill)
        .into()];

    for (i, section) in ctx.sections.iter().enumerate() {
        let expanded = panel.is_section_expanded(section.kind);
        sections.push(sidebar_section_view(
            section,
            i,
            sidebar_width,
            expanded,
            filter_query,
            ctx,
        ));
    }

    let sidebar_col = column(sections).width(Length::Fill).spacing(0);
    let scrolled = scrollable(sidebar_col)
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(5).scroller_width(5),
        ))
        .style(scrollbar_style);

    let viewing_text = if ctx.commit_count == 0 {
        "Loading…".to_string()
    } else {
        format!("Showing {} commits", ctx.commit_count)
    };
    let viewing_bar = container(
        text(viewing_text)
            .size(theme::FONT_SM)
            .style(style::secondary_text),
    )
    .padding(Padding::from([3, 8]))
    .width(Length::Fill)
    .style(style::header_container);

    let full_sidebar = column![viewing_bar, scrolled]
        .spacing(0)
        .height(Length::Fill);

    let sidebar_container = container(full_sidebar)
        .width(Length::Fixed(sidebar_width))
        .height(Length::Fill)
        .style(style::sidebar_container);

    let resize_handle = resize_handle_view(is_resizing, sidebar_width);

    MouseArea::new(row![sidebar_container, resize_handle])
        .on_press(sidebar_msg(SidebarAction::Focused))
        .into()
}

fn sidebar_msg(action: SidebarAction) -> Message {
    Message::repo(RepositoryMessage::Sidebar(action))
}

fn resize_handle_view(is_resizing: bool, effective_width: f32) -> Element<'static, Message> {
    use iced::widget::mouse_area;

    let handle = container(horizontal_space())
        .width(Length::Fixed(5.0))
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: if is_resizing {
                Some(theme::ACCENT_BLUE.into())
            } else {
                None
            },
            ..Default::default()
        });

    mouse_area(handle)
        .on_press(sidebar_msg(SidebarAction::ResizeStarted {
            effective_width,
        }))
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

/// Describes the trailing slot of a sidebar section header (to the right of
/// the section title, before the collapse indicator). Open enum: add variants
/// as new section affordances land.
enum HeaderTrailing {
    CountOnly,
    HoverAddButton(OverlayPanelAction),
}

fn trailing_for(kind: SidebarSectionKind, ctx: &SidebarViewCtx<'_>) -> HeaderTrailing {
    match kind {
        SidebarSectionKind::Remote => {
            HeaderTrailing::HoverAddButton(OverlayPanelAction::AddRemoteOpen)
        }
        SidebarSectionKind::Worktrees => {
            let (available_refs, default_dir_prefix) = build_create_worktree_inputs(ctx);
            HeaderTrailing::HoverAddButton(OverlayPanelAction::CreateWorktreeOpen {
                available_refs,
                default_dir_prefix,
            })
        }
        _ => HeaderTrailing::CountOnly,
    }
}

fn build_create_worktree_inputs(
    ctx: &SidebarViewCtx<'_>,
) -> (Vec<super::super::super::overlays::create_worktree::RefChoice>, String) {
    use super::super::super::overlays::create_worktree::RefChoice;

    let mut refs: Vec<RefChoice> = Vec::new();
    for section in ctx.sections {
        match section.kind {
            SidebarSectionKind::Local => {
                for b in &section.branches {
                    refs.push(RefChoice::LocalBranch(b.name.clone()));
                }
            }
            SidebarSectionKind::Remote => {
                for remote_node in &section.branches {
                    for branch in &remote_node.children {
                        refs.push(RefChoice::RemoteBranch {
                            remote: remote_node.name.clone(),
                            branch: branch.name.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    let default_dir_prefix = derive_default_dir_prefix(ctx);
    (refs, default_dir_prefix)
}

fn derive_default_dir_prefix(ctx: &SidebarViewCtx<'_>) -> String {
    let active = ctx.active_worktree_path;
    let parent = active.parent().map(|p| p.to_path_buf());
    let name = active.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match (parent, name) {
        (Some(p), n) if !n.is_empty() => format!("{}/{}.worktrees", p.to_string_lossy(), n),
        _ => String::new(),
    }
}

fn filter_input_style(_: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = if matches!(status, text_input::Status::Focused { .. }) {
        theme::ACCENT_BLUE
    } else {
        theme::BORDER
    };
    text_input::Style {
        background: theme::BG_HEADER.into(),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: theme::TEXT_DIM,
        placeholder: theme::TEXT_DIM,
        value: theme::TEXT_PRIMARY,
        selection: theme::ACCENT_BLUE,
    }
}

/// Whether a top-level branch in a given section should appear under the
/// active filter. Remote-section nodes (depth 0) are kept when any child
/// branch matches; leaf branches in other sections match on their own name.
fn branch_passes_filter(branch: &Branch, kind: SidebarSectionKind, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    match kind {
        SidebarSectionKind::Remote => branch
            .children
            .iter()
            .any(|c| matches_filter(needle, &c.name)),
        _ => matches_filter(needle, &branch.name),
    }
}

/// Badge count after applying the active filter. Empty needle → the
/// pre-computed `section.count` from the projection.
fn filtered_section_count(section: &SidebarSection, needle: &str) -> u32 {
    if needle.is_empty() {
        return section.count;
    }
    match section.kind {
        SidebarSectionKind::Remote => section
            .branches
            .iter()
            .map(|node| {
                node.children
                    .iter()
                    .filter(|c| matches_filter(needle, &c.name))
                    .count() as u32
            })
            .sum(),
        SidebarSectionKind::Stashes => section
            .stashes
            .iter()
            .filter(|s| matches_filter(needle, &s.display_name))
            .count() as u32,
        _ => section
            .branches
            .iter()
            .filter(|b| matches_filter(needle, &b.name))
            .count() as u32,
    }
}

fn sidebar_section_view<'a>(
    section: &'a SidebarSection,
    idx: usize,
    sidebar_width: f32,
    is_expanded: bool,
    filter_query: &str,
    ctx: &SidebarViewCtx<'_>,
) -> Element<'a, Message> {
    let chevron_icon = if is_expanded {
        assets::CHEVRON_DOWN
    } else {
        assets::CHEVRON_RIGHT
    };

    let section_icon = match section.kind {
        SidebarSectionKind::Local => assets::LAPTOP,
        SidebarSectionKind::Remote => assets::CLOUD,
        SidebarSectionKind::Worktrees => assets::TREE,
        SidebarSectionKind::Stashes => assets::STACK,
        SidebarSectionKind::Tags => assets::TAG,
    };

    let toggle_msg = sidebar_msg(SidebarAction::SectionToggled(idx));

    let badge_count = filtered_section_count(section, filter_query);
    let header_inner = sidebar_section_header(
        section,
        chevron_icon,
        section_icon,
        badge_count,
        trailing_for(section.kind, ctx),
    );
    let header: Element<Message> = MouseArea::new(
        container(header_inner)
            .width(Length::Fill)
            .height(Length::Shrink),
    )
    .on_press(toggle_msg)
    .into();

    let mut col_items: Vec<Element<Message>> = vec![header];

    if is_expanded {
        let needle = filter_query;
        for branch in &section.branches {
            if !branch_passes_filter(branch, section.kind, needle) {
                continue;
            }
            col_items.push(branch_tree_item(branch, 0, section.kind, None, needle));
        }
        for stash in &section.stashes {
            if !matches_filter(needle, &stash.display_name) {
                continue;
            }
            col_items.push(stash_tree_item(stash, sidebar_width));
        }
    }

    column(col_items).spacing(0).width(Length::Fill).into()
}

const SECTION_HEADER_HEIGHT: f32 = 28.0;

fn count_badge<'a>(count: u32) -> Element<'a, Message> {
    container(
        text(format!("{}", count))
            .size(theme::FONT_XS)
            .style(style::dim_text),
    )
    .padding(Padding::from([1, 5]))
    .style(|_: &Theme| container::Style {
        background: Some(theme::BG_BASE.into()),
        border: Border { radius: 8.0.into(), ..Default::default() },
        ..Default::default()
    })
    .into()
}

fn sidebar_section_header<'a>(
    section: &'a SidebarSection,
    chevron_icon: &'static [u8],
    icon: &'static [u8],
    count: u32,
    trailing: HeaderTrailing,
) -> Element<'a, Message> {
    match trailing {
        HeaderTrailing::CountOnly => simple_header(section, chevron_icon, icon, count),
        HeaderTrailing::HoverAddButton(action) => {
            hover_add_button_header(section, chevron_icon, icon, count, action)
        }
    }
}

fn simple_header<'a>(
    section: &'a SidebarSection,
    chevron_icon: &'static [u8],
    icon: &'static [u8],
    count: u32,
) -> Element<'a, Message> {
    container(
        row![
            assets::sidebar_icon(chevron_icon, theme::TEXT_DIM),
            assets::sidebar_icon(icon, theme::TEXT_SECONDARY),
            text(section.title()).size(theme::FONT_SM).style(style::secondary_text),
            horizontal_space(),
            count_badge(count),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(SECTION_HEADER_HEIGHT))
    .padding(Padding::from([0, 8]))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

fn hover_add_button_header<'a>(
    section: &'a SidebarSection,
    chevron_icon: &'static [u8],
    icon: &'static [u8],
    count: u32,
    action: OverlayPanelAction,
) -> Element<'a, Message> {
    let add_btn = button(
        container(assets::icon(assets::PLUS, 12.0, Color::WHITE))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center),
    )
    .on_press(Message::repo(RepositoryMessage::OverlayPanel(action.clone())))
    .padding(Padding::from(0))
    .style(|_: &Theme, status: button::Status| button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => {
                Some(overlays::CREATE_BUTTON.hover_background.into())
            }
            _ => Some(overlays::CREATE_BUTTON.background.into()),
        },
        border: Border {
            color: overlays::CREATE_BUTTON.border,
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: overlays::CREATE_BUTTON.text,
        shadow: Default::default(),
        snap: false,
    })
    .width(Length::Fixed(20.0))
    .height(Length::Fixed(20.0));

    let base_row = |trailing: Element<'a, Message>| -> iced::widget::Row<'a, Message> {
        row![
            assets::sidebar_icon(chevron_icon, theme::TEXT_DIM),
            assets::sidebar_icon(icon, theme::TEXT_SECONDARY),
            text(section.title()).size(theme::FONT_SM).style(style::secondary_text),
            horizontal_space(),
            trailing,
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
    };

    let row_h = Length::Fixed(SECTION_HEADER_HEIGHT);
    let idle = container(base_row(count_badge(count)))
        .width(Length::Fill)
        .height(row_h)
        .padding(Padding::from([0, 8]))
        .align_y(iced::alignment::Vertical::Center);
    let hover = container(base_row(add_btn.into()))
        .width(Length::Fill)
        .height(row_h)
        .padding(Padding::from([0, 8]))
        .align_y(iced::alignment::Vertical::Center);

    HoverableSwap::new(
        idle,
        hover,
        |_: &Theme| container::Style::default(),
        |_: &Theme| container::Style::default(),
    )
    .into()
}

fn stash_tree_item(stash: &SidebarStash, sidebar_width: f32) -> Element<'_, Message> {
    let available = (sidebar_width - 53.0).max(0.0);
    let display = crate::services::cached_truncate_name(&stash.display_name, available);

    let inner_row = row![
        horizontal_space().width(Length::Fixed(4.0)),
        assets::sidebar_icon(assets::STASH, theme::TEXT_DIM),
        text(display)
            .size(theme::FONT_SM)
            .style(style::primary_text)
            .wrapping(iced::widget::text::Wrapping::None),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    let content = container(inner_row)
        .padding(Padding::from([3, 6]))
        .width(Length::Fill);

    let interactive = Hoverable::new(content, |_: &Theme, status: HoverStatus| container::Style {
        background: if status.is_hovered() {
            Some(theme::BG_HOVER.into())
        } else {
            None
        },
        ..Default::default()
    });

    let hash = stash.hash.clone();
    MouseArea::new(interactive)
        .on_press(sidebar_msg(SidebarAction::StashPressed { hash }))
        .into()
}

fn branch_tree_item<'a>(
    branch: &'a Branch,
    depth: u16,
    section_kind: SidebarSectionKind,
    remote_name: Option<&'a str>,
    needle: &str,
) -> Element<'a, Message> {
    let indent = depth as f32 * 10.0;
    let is_current = branch.is_current;
    let name_color = if is_current {
        theme::TEXT_ACTIVE_BRANCH
    } else {
        theme::TEXT_PRIMARY
    };

    let icon_elem: Element<Message> = if is_current {
        assets::sidebar_icon(assets::CHECK, theme::TEXT_ACTIVE_BRANCH)
    } else if matches!(section_kind, SidebarSectionKind::Remote) && depth == 0 {
        assets::sidebar_icon(assets::CLOUD, theme::TEXT_DIM)
    } else if branch.worktree_path.is_some() {
        assets::sidebar_icon(assets::TREE, theme::TEXT_DIM)
    } else {
        assets::sidebar_icon(assets::BRANCH, theme::TEXT_DIM)
    };

    let inner_row = row![
        horizontal_space().width(Length::Fixed(indent + 4.0)),
        icon_elem,
        text(&branch.name)
            .size(theme::FONT_SM)
            .style(move |_: &Theme| text::Style {
                color: Some(name_color)
            }),
        horizontal_space(),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center);

    let row_bg = if is_current {
        Some(
            Color {
                r: 0.12,
                g: 0.28,
                b: 0.15,
                a: 0.35,
            }
            .into(),
        )
    } else {
        None
    };

    let content = container(inner_row)
        .padding(Padding::from([3, 6]))
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: row_bg,
            ..Default::default()
        });

    let row_with_border: Element<Message> = if is_current {
        let line_height = crate::theme::FONT_SM + 6.0;
        row![
            container(horizontal_space())
                .width(Length::Fixed(3.0))
                .height(Length::Fixed(line_height))
                .style(|_: &Theme| container::Style {
                    background: Some(theme::TEXT_ACTIVE_BRANCH.into()),
                    ..Default::default()
                }),
            content,
        ]
        .into()
    } else {
        content.into()
    };

    let is_remote = matches!(section_kind, SidebarSectionKind::Remote);
    let is_tag = matches!(section_kind, SidebarSectionKind::Tags);
    let is_remote_name_node = is_remote && depth == 0;
    let branch_name = branch.name.clone();

    let row_with_hover: Element<Message> = if is_remote_name_node {
        row_with_border
    } else {
        Hoverable::new(row_with_border, move |_: &Theme, status: HoverStatus| {
            container::Style {
                background: if status.is_hovered() {
                    Some(theme::BG_HOVER.into())
                } else {
                    None
                },
                ..Default::default()
            }
        })
        .into()
    };

    // For a remote-section node at depth>=1, its parent's name IS the remote
    // name (e.g. "origin"). Depth-0 under Remote is the remote itself.
    let menu_remote_name = if is_remote && depth > 0 {
        remote_name.map(|s| s.to_string())
    } else {
        None
    };

    let is_worktree_entry = matches!(section_kind, SidebarSectionKind::Worktrees)
        && branch.worktree_path.is_some();

    let press_msg = if is_worktree_entry {
        sidebar_msg(SidebarAction::WorktreeEntryPressed {
            path: branch.worktree_path.clone().expect("worktree_path present"),
        })
    } else {
        sidebar_msg(SidebarAction::BranchPressed {
            branch_name,
            is_remote,
        })
    };

    let right_msg = if is_worktree_entry {
        sidebar_msg(SidebarAction::WorktreeEntryRightClicked {
            path: branch.worktree_path.clone().expect("worktree_path present"),
            branch_name: branch.name.clone(),
        })
    } else {
        sidebar_msg(SidebarAction::BranchRightClicked {
            branch_name: branch.name.clone(),
            is_remote,
            is_tag,
            remote_name: menu_remote_name,
        })
    };

    let row_with_menu = MouseArea::new(row_with_hover)
        .on_press(press_msg)
        .on_right_press(right_msg);

    let mut items: Vec<Element<Message>> = vec![row_with_menu.into()];

    let show_children = is_remote && depth == 0;
    if show_children {
        let child_remote_name = if is_remote && depth == 0 {
            Some(branch.name.as_str())
        } else {
            remote_name
        };
        for child in &branch.children {
            if !needle.is_empty() && !matches_filter(needle, &child.name) {
                continue;
            }
            items.push(branch_tree_item(
                child,
                depth + 1,
                section_kind,
                child_remote_name,
                needle,
            ));
        }
    }

    column(items).spacing(0).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn derive_default_dir_prefix_composes_parent_name_dot_worktrees() {
        let sections: Vec<crate::view_model::SidebarSection> = Vec::new();
        let active = Path::new("/home/rachel/projects/git_leviathan");
        let ctx = SidebarViewCtx {
            sections: &sections,
            commit_count: 0,
            width: 240.0,
            is_resizing: false,
            active_worktree_path: active,
        };
        assert_eq!(
            derive_default_dir_prefix(&ctx),
            "/home/rachel/projects/git_leviathan.worktrees".to_string(),
        );
    }

    #[test]
    fn derive_default_dir_prefix_empty_when_no_parent() {
        let sections: Vec<crate::view_model::SidebarSection> = Vec::new();
        let active = Path::new("/");
        let ctx = SidebarViewCtx {
            sections: &sections,
            commit_count: 0,
            width: 240.0,
            is_resizing: false,
            active_worktree_path: active,
        };
        assert_eq!(derive_default_dir_prefix(&ctx), "".to_string());
    }
}
