//! View composition for the main bar.
//!
//! Walks a [`MainBarRegistry`], renders each slot in its section, and glues
//! the sections together with the exact layout the pre-refactor monolith
//! produced:
//!
//! - **Left**: flat row, spacing 0, no outer padding.
//! - **Center**: flat row, spacing 0.
//! - **Right**: row with 6px spacing, wrapped in `container(..).padding([0, 10])`.
//! - **Outer**: `row![left, hspace, center, hspace, right]`, TOOLBAR_HEIGHT high.
//!
//! Per-section layout (spacing, padding) is intentionally *not* a property
//! of the slot or the section — the section enum carries only identity.
//! Layout belongs to the view; slots belong to the registry.

use iced::{
    widget::{container, row},
    Element, Length, Padding,
};

use crate::{message::Message, style as shared_style, theme, widgets::shared::horizontal_space};

use super::{
    registry::{iter_section, MainBarRegistry},
    slot::{Section, SlotCtx},
};

pub fn main_bar_view<'data>(
    registry: &MainBarRegistry,
    ctx: &SlotCtx<'data>,
) -> Element<'data, Message> {
    let left = render_section(registry, ctx, Section::Left, 0.0);
    let center = render_section(registry, ctx, Section::Center, 0.0);
    let right_row = render_section(registry, ctx, Section::Right, 6.0);
    let right = container(right_row).padding(Padding::from([0, 10]));

    let bar = row![left, horizontal_space(), center, horizontal_space(), right,]
        .align_y(iced::Alignment::Center)
        .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32));

    container(bar)
        .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32))
        .width(Length::Fill)
        .style(shared_style::toolbar_container)
        .into()
}

fn render_section<'data>(
    registry: &MainBarRegistry,
    ctx: &SlotCtx<'data>,
    section: Section,
    spacing: f32,
) -> Element<'data, Message> {
    let items: Vec<Element<'data, Message>> = iter_section(registry, section)
        .map(|slot| (slot.builder)(ctx))
        .collect();
    row(items)
        .spacing(spacing)
        .align_y(iced::Alignment::Center)
        .into()
}
