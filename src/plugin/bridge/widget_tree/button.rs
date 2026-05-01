//! `{kind = "button", on_click?, text?, child?, width?, height?, style?}`
//! — fully configurable themed button.
//!
//! Content: either `child` (any widget) OR `text` (shortcut — equivalent to
//! `child = {kind = "text", value = "..."}` with `size = 14`). If both are
//! set, `child` wins.
//!
//! Padding is **not** a property of the button. Plugins wrap the child in a
//! `padding` widget (or rely on the child's own intrinsic size). This
//! keeps layout concerns orthogonal — the button owns its visual/clickable
//! rect + style, the padding widget owns spacing.
//!
//! `on_click` dispatches according to the current scope:
//! - Screen → `PluginMessage::Event { plugin_id, screen_id, event, value }`
//! - Slot → `PluginMessage::SlotClicked { plugin_id, region, container, slot_id }`
//!
//! Empty/absent `on_click` renders a non-interactive button (no on_press
//! handler; iced disables hover highlight automatically).
//!
//! Styling is controlled by the `style` sub-table:
//! ```text
//! style = {
//!   background       = "#RRGGBB",  -- base background, default none
//!   background_hover = "#RRGGBB",  -- hover background, default none
//!   text_color       = "#RRGGBB",  -- foreground, default theme TEXT_PRIMARY
//!   border           = { width=1, radius=4, color="#RRGGBB" },
//! }
//! ```

use iced::{
    widget::{button, text, Space},
    Element, Length, Padding, Theme,
};
use serde_json::Value;

use crate::message::Message;
use crate::plugin::message::PluginMessage;
use crate::theme;

use super::common::{parse_border, parse_color, parse_length};
use super::{BuildCtx, DispatchScope};

pub(super) fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let content: Element<'static, Message> = if let Some(child_node) = node.get("child") {
        super::build(child_node, ctx)
    } else if let Some(label) = node.get("text").and_then(Value::as_str) {
        text(label.to_string()).size(14.0).into()
    } else {
        Space::new().into()
    };

    let event = node
        .get("on_click")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let value = node.get("value").cloned().unwrap_or(Value::Null);
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
            }),
        })
    };

    let width = parse_length(node.get("width"));
    let height = parse_length(node.get("height"));

    let style_node = node.get("style");
    let background = style_node
        .and_then(|s| s.get("background"))
        .and_then(|v| parse_color(Some(v)));
    let background_hover = style_node
        .and_then(|s| s.get("background_hover"))
        .and_then(|v| parse_color(Some(v)));
    let text_color = style_node
        .and_then(|s| s.get("text_color"))
        .and_then(|v| parse_color(Some(v)))
        .unwrap_or(theme::TEXT_PRIMARY);
    let border = style_node
        .and_then(|s| s.get("border"))
        .map(|b| parse_border(Some(b)))
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

    // Zero intrinsic padding — the plugin must wrap `child` in a `padding`
    // widget if it wants breathing room. Iced's button default padding is
    // non-zero; force it to zero so Lua has total control.
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
