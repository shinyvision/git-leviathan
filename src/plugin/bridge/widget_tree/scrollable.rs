//! `{kind = "scrollable", child, width?, height?, id?, scroll_y?}`.

use iced::{
    widget::{scrollable, Id, Space},
    Element, Length,
};

use crate::message::Message;
use crate::plugin::ui::effects::plugin_scrollable_id;
use crate::plugin::ui::widget_ast::ScrollableNode;

use super::common::length_or;
use super::BuildCtx;

pub(super) fn build(node: &ScrollableNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let child: Element<'static, Message> = match &node.child {
        Some(c) => super::build(c, ctx),
        None => Space::new().into(),
    };
    let w = length_or(node.width, Length::Fill);
    let h = length_or(node.height, Length::Fill);
    let mut scroll = scrollable(child).width(w).height(h);
    if let Some(id) = &node.id {
        scroll = scroll.id(Id::from(plugin_scrollable_id(
            ctx.plugin_id,
            &ctx.scope.storage_key(),
            id,
        )));
    }
    scroll.into()
}
