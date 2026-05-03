//! `{kind = "column", children, spacing?, width?, height?, align_x?}`.
//! Defaults: width=fill, height=fill.

use iced::{widget::column, Element, Length};

use crate::message::Message;
use crate::plugin::ui::widget_ast::ColumnNode;

use super::common::{align_x_to_iced, length_or};
use super::BuildCtx;

pub(super) fn build(node: &ColumnNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let children: Vec<Element<'static, Message>> =
        node.children.iter().map(|c| super::build(c, ctx)).collect();
    let w = length_or(node.width, Length::Fill);
    let h = length_or(node.height, Length::Fill);
    let mut col = column(children).spacing(node.spacing).width(w).height(h);
    if let Some(align) = node.align_x {
        col = col.align_x(align_x_to_iced(align));
    }
    col.into()
}
