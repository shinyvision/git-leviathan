//! Built-in tab-bar slots.
//!
//! Each visual element in the default tab bar is registered as a named
//! slot — same `builtin.<name>` convention as `main_bar/builtins.rs` —
//! so plugins can replace any of them via
//! `leviathan.ui.tab_bar.replace("builtin.tab_list", { ... })`.
//!
//! ## Priority map
//!
//! | Section | Priority | ID                       |
//! | ------- | -------- | ------------------------ |
//! | Left    | 10       | `builtin.plus_button`    |
//! | Center  | 10       | `builtin.tab_list`       |
//! | Right   | 10       | `builtin.version_label`  |
//!
//! `builtin.tab_list` carries the entire native tab strip: drag-reorder,
//! hover, click, close button, folder icon. Plugins replacing it should
//! use the `tablist` widget kind to keep that behaviour.

use iced::{
    widget::{button, container},
    Border, Element, Length, Padding, Theme,
};

use std::collections::HashMap;

use crate::{
    assets,
    core::TabId,
    message::{AppMessage, Message},
    plugin::tab_snapshot::TabRegistryOp,
    style, theme,
    widgets::tab_bar::{TabBar, TabItem},
};

use crate::widgets::tab_bar::parts::{close_button, folder_icon};
use super::registry::{Section, TabBarCtx, TabBarRegistry, TabBarSlot};

pub fn register_all(registry: &mut TabBarRegistry) {
    registry.add(plus_button_slot());
    registry.add(tab_list_slot());
    registry.add(version_label_slot());
}

fn plus_button_slot() -> TabBarSlot {
    TabBarSlot::new("builtin.plus_button", Section::Left, 10, |_ctx| plus_button())
}

fn tab_list_slot() -> TabBarSlot {
    TabBarSlot::new("builtin.tab_list", Section::Center, 10, |ctx: &TabBarCtx<'_>| {
        let Some(snap) = ctx.tabs else {
            return iced::widget::Space::new().into();
        };
        let active = snap.active_id.unwrap_or(TabId(u64::MAX));
        let path_for: HashMap<TabId, String> =
            snap.tabs.iter().map(|t| (t.id, t.path.clone())).collect();
        let items: Vec<TabItem<'_, TabId, Message>> = snap
            .tabs
            .iter()
            .map(|t| {
                let is_active = Some(t.id) == snap.active_id;
                let close_path = t.path.clone();
                TabItem::new(t.id, t.name.clone())
                    .leading(folder_icon(is_active))
                    .trailing(close_button(Message::App(AppMessage::TabRegistryOp(
                        TabRegistryOp::Remove(close_path),
                    ))))
            })
            .collect();
        let select_paths = path_for.clone();
        let reorder_paths = path_for;
        TabBar::new(items, active)
            .on_select(move |id| {
                let path = select_paths.get(&id).cloned().unwrap_or_default();
                Message::App(AppMessage::TabRegistryOp(TabRegistryOp::Select(path)))
            })
            .on_reorder(move |order| {
                let paths: Vec<String> = order
                    .into_iter()
                    .filter_map(|id| reorder_paths.get(&id).cloned())
                    .collect();
                Message::App(AppMessage::TabRegistryOp(TabRegistryOp::Reorder(paths)))
            })
            .into()
    })
}

fn version_label_slot() -> TabBarSlot {
    TabBarSlot::new("builtin.version_label", Section::Right, 10, |_ctx| version_label())
}

fn plus_button<'a>() -> Element<'a, Message> {
    button(
        container(assets::tab_icon(assets::PLUS, theme::TEXT_DIM))
            .align_y(iced::alignment::Vertical::Center)
            .height(Length::Fill),
    )
    .style(|_: &Theme, status: button::Status| button::Style {
        background: match status {
            button::Status::Hovered => Some(theme::BG_HOVER.into()),
            _ => None,
        },
        text_color: theme::TEXT_DIM,
        border: Border::default(),
        shadow: Default::default(),
        snap: false,
    })
    .padding(Padding::from([0, 12]))
    .height(Length::Fixed(theme::TAB_HEIGHT as f32))
    .on_press(Message::App(AppMessage::OpenRepoDialog))
    .into()
}

fn version_label<'a>() -> Element<'a, Message> {
    container(
        iced::widget::text(format!("v{}", env!("CARGO_PKG_VERSION")))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
    )
    .padding(Padding::from([0, 12]))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
