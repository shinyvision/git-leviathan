//! Built-in main-bar slots.
//!
//! Each slot is what used to be a hardcoded chunk of the monolithic
//! `main_bar_view`. The visual output is *bit-for-bit preserved*: padding,
//! spacing, colors, dim-on-disabled behaviour all match the pre-refactor
//! code. The only structural change is that each item now lives behind a
//! named, addressable `MainBarSlot`.
//!
//! ID convention: `builtin.<name>`. Priorities are spaced by 10 so plugins
//! can insert between them without needing to rebalance.
//!
//! ## Priority map
//!
//! | Section | Priority | ID                          |
//! | ------- | -------- | --------------------------- |
//! | Left    | 10       | `builtin.repo_info`         |
//! | Left    | 20       | `builtin.separator_chevron` |
//! | Left    | 30       | `builtin.branch_info`       |
//! | Left    | 40       | `builtin.fetch_indicator`   |
//! | Center  | 10       | `builtin.pull`              |
//! | Center  | 20       | `builtin.push`              |
//! | Center  | 30       | `builtin.branch`            |
//! | Center  | 40       | `builtin.stash`             |
//! | Center  | 50       | `builtin.pop`               |
//! | Right   | 10       | `builtin.search`            |

use iced::{
    widget::{button, column, container, text},
    Color, Element, Length, Padding, Theme,
};

use crate::{
    assets,
    message::Message,
    screens::repository::{
        panel_messages::CenterAction as CenterAct, RepositoryMessage,
    },
    style as shared_style, theme,
    widgets::primitives::spinner,
};

use super::{
    slot::{MainBarSlot, Section},
    style,
};

/// Blend `c` halfway toward the base background — the exact same dimming
/// the monolith used for disabled toolbar icons/labels.
fn dim(c: Color, enabled: bool) -> Color {
    if enabled {
        return c;
    }
    let bg = theme::BG_BASE;
    Color {
        r: c.r * 0.5 + bg.r * 0.5,
        g: c.g * 0.5 + bg.g * 0.5,
        b: c.b * 0.5 + bg.b * 0.5,
        a: c.a,
    }
}

/// Wrap a spinner in the 2px inset container the monolith used for pull/push.
fn spinner_inset<'a>(
    started_at: std::time::Instant,
    color: Color,
) -> Element<'a, Message> {
    container(spinner::spinner(started_at, color, 16.0))
        .padding(Padding {
            top: 2.0,
            bottom: 2.0,
            left: 2.0,
            right: 2.0,
        })
        .into()
}

/// Shared "icon-over-label" toolbar button used by pull/push/branch/stash/pop.
/// `on_press = None` renders a visually-disabled button (no press handler).
fn action_button<'a>(
    icon: Element<'a, Message>,
    label: &'static str,
    label_color: Color,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let col = column![
        icon,
        text(label)
            .size(theme::FONT_XS)
            .style(move |_: &Theme| iced::widget::text::Style {
                color: Some(label_color),
            }),
    ]
    .spacing(3)
    .align_x(iced::Alignment::Center);

    let mut btn = button(col)
        .style(style::action_btn)
        .padding(Padding::from([6, 12]))
        .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32));
    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }
    btn.into()
}

fn repo_info_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.repo_info", Section::Left, 10, |ctx| {
        let repo_col = column![
            text("repository")
                .size(theme::FONT_XS)
                .style(shared_style::dim_text),
            text(ctx.repo_name)
                .size(theme::FONT_SM)
                .style(shared_style::primary_text),
        ];
        container(repo_col).padding(Padding::from([0, 10])).into()
    })
}

fn separator_chevron_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.separator_chevron", Section::Left, 20, |_ctx| {
        assets::icon(assets::CHEVRON_RIGHT, 20.0, theme::BORDER)
    })
}

fn branch_info_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.branch_info", Section::Left, 30, |ctx| {
        let branch_col = column![
            text("branch")
                .size(theme::FONT_XS)
                .style(shared_style::dim_text),
            text(ctx.branch_name)
                .size(theme::FONT_SM)
                .style(|_: &Theme| iced::widget::text::Style {
                    color: Some(theme::TEXT_ACTIVE_BRANCH),
                }),
        ];
        container(branch_col).padding(Padding::from([0, 10])).into()
    })
}

fn fetch_indicator_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.fetch_indicator", Section::Left, 40, |ctx| {
        let inner: Element<'_, Message> = match ctx.fetch_started_at {
            Some(started_at) => spinner::spinner(started_at, theme::TEXT_DIM, 13.0).into(),
            None => assets::tab_icon(assets::REFRESH, theme::TEXT_DIM),
        };
        container(inner).padding(Padding::from([0, 6])).into()
    })
}

fn pull_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.pull", Section::Center, 10, |ctx| {
        let enabled = !ctx.busy;
        let icon_color = dim(theme::TEXT_SECONDARY, enabled);
        let label_color = dim(theme::TEXT_DIM, enabled);
        let icon: Element<'_, Message> = match ctx.pull_started_at {
            Some(started_at) => spinner_inset(started_at, icon_color),
            None => assets::toolbar_icon(assets::PULL, icon_color),
        };
        let on_press = enabled.then(|| Message::repo(RepositoryMessage::PullRequested));
        action_button(icon, "Pull", label_color, on_press)
    })
}

fn push_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.push", Section::Center, 20, |ctx| {
        let enabled = !ctx.busy;
        let icon_color = dim(theme::TEXT_SECONDARY, enabled);
        let label_color = dim(theme::TEXT_DIM, enabled);
        let icon: Element<'_, Message> = match ctx.push_started_at {
            Some(started_at) => spinner_inset(started_at, icon_color),
            None => assets::toolbar_icon(assets::PUSH, icon_color),
        };
        let on_press = enabled.then(|| Message::repo(RepositoryMessage::PushRequested));
        action_button(icon, "Push", label_color, on_press)
    })
}

fn branch_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.branch", Section::Center, 30, |ctx| {
        let enabled = ctx.branch_action.is_some();
        let icon_color = dim(theme::TEXT_SECONDARY, enabled);
        let label_color = dim(theme::TEXT_DIM, enabled);
        let icon = assets::toolbar_icon(assets::BRANCH, icon_color);
        action_button(icon, "Branch", label_color, ctx.branch_action.clone())
    })
}

fn stash_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.stash", Section::Center, 40, |_ctx| {
        let icon = assets::toolbar_icon(assets::STASH, theme::TEXT_SECONDARY);
        // Stash/Pop use the static dim_text style, not the dynamic dim()
        // function — matching the monolith exactly.
        let col = column![
            icon,
            text("Stash").size(theme::FONT_XS).style(shared_style::dim_text),
        ]
        .spacing(3)
        .align_x(iced::Alignment::Center);
        button(col)
            .style(style::action_btn)
            .padding(Padding::from([6, 12]))
            .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32))
            .on_press(Message::repo(RepositoryMessage::Center(
                CenterAct::StashCreateRequested,
            )))
            .into()
    })
}

fn pop_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.pop", Section::Center, 50, |_ctx| {
        let icon = assets::toolbar_icon(assets::POP, theme::TEXT_SECONDARY);
        let col = column![
            icon,
            text("Pop").size(theme::FONT_XS).style(shared_style::dim_text),
        ]
        .spacing(3)
        .align_x(iced::Alignment::Center);
        button(col)
            .style(style::action_btn)
            .padding(Padding::from([6, 12]))
            .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32))
            .on_press(Message::repo(RepositoryMessage::Center(
                CenterAct::StashPopRequested { stash_index: 0 },
            )))
            .into()
    })
}

fn search_slot() -> MainBarSlot {
    MainBarSlot::new("builtin.search", Section::Right, 10, |_ctx| {
        let size = 32.0_f32;
        button(
            container(assets::icon(assets::SEARCH, 16.0, theme::TEXT_SECONDARY))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .padding(0)
        .style(style::search_btn)
        .on_press(Message::repo(RepositoryMessage::OpenCommitSearch))
        .into()
    })
}

/// Register all built-in slots on a fresh or existing registry. Safe to
/// call on an already-populated registry: existing ids are replaced.
pub fn register_all(registry: &mut super::MainBarRegistry) {
    registry.add(repo_info_slot());
    registry.add(separator_chevron_slot());
    registry.add(branch_info_slot());
    registry.add(fetch_indicator_slot());
    registry.add(pull_slot());
    registry.add(push_slot());
    registry.add(branch_slot());
    registry.add(stash_slot());
    registry.add(pop_slot());
    registry.add(search_slot());
}
