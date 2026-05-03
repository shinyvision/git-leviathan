//! `{kind = "icon", path, size?, color?}` — SVG glyph from the plugin's
//! sandbox root.

use iced::{widget::svg, Element, Length, Theme};

use crate::message::Message;
use crate::plugin::ui::widget_ast::IconNode;
use crate::theme;

use super::common::{error_text, is_safe_relative_path, opt_color_to_iced};
use super::BuildCtx;

pub(super) fn build(node: &IconNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    if !is_safe_relative_path(&node.path) {
        return error_text(format!("invalid icon path: {:?}", node.path));
    }
    let size = node.size;
    let color = opt_color_to_iced(&node.color).unwrap_or(theme::TEXT_PRIMARY);
    let resolved = ctx.plugin_root.join(&node.path);
    svg(svg::Handle::from_path(resolved))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_: &Theme, _: svg::Status| svg::Style { color: Some(color) })
        .into()
}
