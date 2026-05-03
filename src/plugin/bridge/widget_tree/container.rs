//! `{kind = "container", child?, bg?, width?, height?,
//!   max_width?, max_height?, center_x?, center_y?}`.
//!
//! Also exposes `container_size_limits` — the `resizable_split` widget
//! reads per-pane `(min, max)` from container children at render time so
//! drag clamps against each pane's own bounds.

use iced::{
    widget::{container, Space},
    Background, Element, Length, Theme,
};

use crate::message::Message;
use crate::plugin::ui::widget_ast::{ContainerNode, WidgetAst, WidgetNode};

use super::common::{length_or, opt_color_to_iced};
use super::BuildCtx;

pub(super) fn build(node: &ContainerNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let child: Element<'static, Message> = match &node.child {
        Some(c) => super::build(c, ctx),
        None => Space::new().into(),
    };
    let bg = opt_color_to_iced(&node.bg);
    let mut cont = container(child);
    cont = cont.width(length_or(node.width, Length::Fill));
    cont = cont.height(length_or(node.height, Length::Fill));
    if let Some(max_w) = node.max_width {
        cont = cont.max_width(max_w);
    }
    if let Some(max_h) = node.max_height {
        cont = cont.max_height(max_h);
    }
    if node.center_x {
        cont = cont.center_x(Length::Fill);
    }
    if node.center_y {
        cont = cont.center_y(Length::Fill);
    }
    if let Some(color) = bg {
        cont = cont.style(move |_: &Theme| container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        });
    }
    cont.into()
}

/// Extract `(min, max)` size limits from a container child for the given
/// axis. Falls back to the global split defaults when the child isn't a
/// container or doesn't set its own limits.
pub fn container_size_limits(ast: &WidgetAst, is_vertical: bool) -> (f32, f32) {
    let default_min = crate::plugin::ui::split::MIN_PANEL_SIZE;
    let default_max = crate::plugin::ui::split::MAX_PANEL_SIZE;
    let WidgetNode::Container(c) = &ast.node else {
        return (default_min, default_max);
    };
    let (min, max) = if is_vertical {
        (c.min_height, c.max_height)
    } else {
        (c.min_width, c.max_width)
    };
    (min.unwrap_or(default_min), max.unwrap_or(default_max))
}
