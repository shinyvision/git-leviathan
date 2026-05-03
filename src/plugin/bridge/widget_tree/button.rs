//! `{kind = "button", on_click?, text?, child?, width?, height?, style?}`
//! — fully configurable themed button. Routes clicks per dispatch scope.

use iced::{
    widget::{button, text, Space},
    Element, Length, Padding, Theme,
};

use crate::message::Message;
use crate::plugin::message::PluginMessage;
use crate::plugin::ui::widget_ast::ButtonNode;
use crate::theme;

use super::common::{border_to_iced, length_explicit, opt_color_to_iced};
use super::{BuildCtx, DispatchScope};

pub(super) fn build(node: &ButtonNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let content: Element<'static, Message> = if let Some(child_ast) = &node.child {
        super::build(child_ast, ctx)
    } else if let Some(label) = &node.text {
        text(label.clone()).size(14.0).into()
    } else {
        Space::new().into()
    };

    let event = node.on_click.clone().unwrap_or_default();
    let value = node.value.clone();
    let plugin_id = ctx.plugin_id.to_string();
    let on_press: Option<Message> = if event.is_empty() {
        None
    } else {
        Some(match ctx.scope {
            DispatchScope::Screen { screen_id } => Message::Plugin(PluginMessage::Event {
                plugin_id,
                screen_id: screen_id.to_string(),
                event,
                value,
            }),
            DispatchScope::Slot {
                region,
                container,
                slot_id,
            } => Message::Plugin(PluginMessage::SlotClicked {
                plugin_id,
                region: region.to_string(),
                container: container.to_string(),
                slot_id: slot_id.to_string(),
                event,
                value,
            }),
            DispatchScope::Overlay { overlay_id } => Message::Plugin(PluginMessage::OverlayEvent {
                plugin_id,
                overlay_id: overlay_id.to_string(),
                event,
                value,
            }),
        })
    };

    let width = length_explicit(node.width);
    let height = length_explicit(node.height);

    let background = opt_color_to_iced(&node.style.background);
    let background_hover = opt_color_to_iced(&node.style.background_hover);
    let text_color = opt_color_to_iced(&node.style.text_color).unwrap_or(theme::TEXT_PRIMARY);
    let border = node
        .style
        .border
        .as_ref()
        .map(border_to_iced)
        .unwrap_or_default();

    let style_fn = move |_: &Theme, status: button::Status| {
        let bg = match status {
            button::Status::Hovered => background_hover.or(background),
            _ => background,
        };
        button::Style {
            background: bg.map(Into::into),
            text_color,
            border,
            shadow: Default::default(),
            snap: false,
        }
    };

    let mut btn = button(content).padding(Padding::ZERO).style(style_fn);
    if let Some(w) = width {
        btn = btn.width(w);
    }
    if let Some(h) = height {
        btn = btn.height(h);
    } else {
        btn = btn.height(Length::Shrink);
    }
    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }
    btn.into()
}
