//! `{kind = "row", children, spacing?, width?, height?, align_y?}`.
//! Defaults: width=fill, height=shrink.

use iced::{widget::row, Element, Length};

use crate::message::Message;
use crate::plugin::ui::widget_ast::RowNode;

use super::common::{align_y_to_iced, length_or};
use super::BuildCtx;

pub(super) fn build(node: &RowNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let children: Vec<Element<'static, Message>> =
        node.children.iter().map(|c| super::build(c, ctx)).collect();
    let w = length_or(node.width, Length::Fill);
    let h = length_or(node.height, Length::Shrink);
    let mut r = row(children).spacing(node.spacing).width(w).height(h);
    if let Some(align) = node.align_y {
        r = r.align_y(align_y_to_iced(align));
    }
    r.into()
}
