//! `{kind = "text_input", placeholder, value, on_input, on_submit?, width?,
//! height?, autofocus?, style?}` — single-line plugin input. `on_input` emits
//! the new string value as its payload; `on_submit` emits `Null`.

use iced::{
    widget::{container, text_input, Id},
    Border, Element, Length, Theme,
};
use serde_json::Value;

use crate::message::Message;
use crate::plugin::message::PluginMessage;
use crate::plugin::ui::widget_ast::{TextInputNode, WidgetAst};
use crate::theme;

use super::common::{border_to_iced, length_explicit, opt_color_to_iced};
use super::{BuildCtx, DispatchScope};

pub(super) fn build(
    ast: &WidgetAst,
    node: &TextInputNode,
    ctx: &BuildCtx<'_>,
) -> Element<'static, Message> {
    let dispatch = Dispatch::from_ctx(ctx);
    let on_input = node.on_input.clone();
    let on_submit = node.on_submit.clone();

    let background = opt_color_to_iced(&node.style.background).unwrap_or(theme::BG_HEADER);
    let text_color = opt_color_to_iced(&node.style.text_color).unwrap_or(theme::TEXT_PRIMARY);
    let placeholder_color =
        opt_color_to_iced(&node.style.placeholder_color).unwrap_or(theme::TEXT_DIM);
    let border = node
        .style
        .border
        .as_ref()
        .map(border_to_iced)
        .unwrap_or(Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 3.0.into(),
        });

    let style_fn = move |_: &Theme, status: text_input::Status| {
        let focused = matches!(status, text_input::Status::Focused { .. });
        text_input::Style {
            background: background.into(),
            border: Border {
                color: if focused {
                    theme::ACCENT_BLUE
                } else {
                    border.color
                },
                width: border.width,
                radius: border.radius,
            },
            icon: placeholder_color,
            placeholder: placeholder_color,
            value: text_color,
            selection: theme::ACCENT_BLUE,
        }
    };

    let input_dispatch = dispatch.clone();
    let mut input = text_input(&node.placeholder, &node.value)
        .on_input(move |value| input_dispatch.publish(on_input.clone(), Value::String(value)))
        .on_submit_maybe(on_submit.map(|event| dispatch.publish(event, Value::Null)))
        .size(theme::FONT_SM)
        .padding(theme::INPUT_PADDING)
        .style(style_fn);

    input = input.id(plugin_text_input_id(
        ctx.plugin_id,
        &ctx.scope.storage_key(),
        &ast.node_id.value,
    ));

    if let Some(width) = length_explicit(node.width) {
        input = input.width(width);
    }

    if let Some(height) = length_explicit(node.height) {
        container(input).height(height).into()
    } else {
        container(input).height(Length::Shrink).into()
    }
}

pub(crate) fn plugin_text_input_id(plugin_id: &str, scope_key: &str, node_id: &str) -> Id {
    Id::from(format!(
        "plugin:{plugin_id}:{scope_key}:text_input:{node_id}"
    ))
}

#[derive(Clone)]
enum Dispatch {
    Screen {
        plugin_id: String,
        screen_id: String,
    },
    Slot {
        plugin_id: String,
        region: String,
        container: String,
        slot_id: String,
    },
    Overlay {
        plugin_id: String,
        overlay_id: String,
    },
}

impl Dispatch {
    fn from_ctx(ctx: &BuildCtx<'_>) -> Self {
        let plugin_id = ctx.plugin_id.to_string();
        match ctx.scope {
            DispatchScope::Screen { screen_id } => Self::Screen {
                plugin_id,
                screen_id: screen_id.to_string(),
            },
            DispatchScope::Slot {
                region,
                container,
                slot_id,
            } => Self::Slot {
                plugin_id,
                region: region.to_string(),
                container: container.to_string(),
                slot_id: slot_id.to_string(),
            },
            DispatchScope::Overlay { overlay_id } => Self::Overlay {
                plugin_id,
                overlay_id: overlay_id.to_string(),
            },
        }
    }

    fn publish(&self, event: String, value: Value) -> Message {
        match self.clone() {
            Self::Screen {
                plugin_id,
                screen_id,
            } => Message::Plugin(PluginMessage::Event {
                plugin_id,
                screen_id,
                event,
                value,
            }),
            Self::Slot {
                plugin_id,
                region,
                container,
                slot_id,
            } => Message::Plugin(PluginMessage::SlotClicked {
                plugin_id,
                region,
                container,
                slot_id,
                event,
                value,
            }),
            Self::Overlay {
                plugin_id,
                overlay_id,
            } => Message::Plugin(PluginMessage::OverlayEvent {
                plugin_id,
                overlay_id,
                event,
                value,
            }),
        }
    }
}
