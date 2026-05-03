//! Repository tab bar — slot-driven composer.
//!
//! The bar is a `row![left, centre, fill, right]` strip wrapped in a
//! toolbar-styled container; every visual piece is a slot in
//! `TabBarRegistry`. Built-ins (`builtin.plus_button` / `builtin.tab_list`
//! / `builtin.version_label`) are registered at app startup; plugins
//! contribute or replace by id via `leviathan.ui.regions.*`.
//! Drag-reorder, click selection, close button — all live
//! inside the `builtin.tab_list` slot's `TabBar` widget.

use iced::{
    widget::{container, row},
    Alignment, Element, Length,
};

use crate::{
    message::Message,
    plugin::tab_snapshot::TabsSnapshot,
    style, theme,
    widgets::chrome::tab_bar_slots::{self, TabBarCtx, TabBarRegistry},
};

pub fn tab_bar_view<'a>(
    registry: &'a TabBarRegistry,
    snapshot: &'a TabsSnapshot,
) -> Element<'a, Message> {
    let ctx = TabBarCtx::with_tabs(snapshot);

    let left: Vec<Element<'a, Message>> =
        tab_bar_slots::iter_section(registry, tab_bar_slots::Section::Left)
            .map(|slot| (slot.builder)(&ctx))
            .collect();
    let centre: Vec<Element<'a, Message>> =
        tab_bar_slots::iter_section(registry, tab_bar_slots::Section::Center)
            .map(|slot| (slot.builder)(&ctx))
            .collect();
    let right: Vec<Element<'a, Message>> =
        tab_bar_slots::iter_section(registry, tab_bar_slots::Section::Right)
            .map(|slot| (slot.builder)(&ctx))
            .collect();

    // Centre row is the only Length::Fill in the strip — its TabBar
    // child fills it and pushes the right section to the screen edge.
    // Adding another Fill (e.g. an explicit `horizontal_space()`) would
    // split the leftover width 50/50 and leave a visible seam.
    let bar = row![
        row(left).align_y(Alignment::Center),
        row(centre).align_y(Alignment::Center).width(Length::Fill),
        row(right).align_y(Alignment::Center),
    ]
    .align_y(Alignment::Center);

    container(bar)
        .height(Length::Fixed(theme::TAB_HEIGHT as f32))
        .width(Length::Fill)
        .style(style::toolbar_container)
        .into()
}
