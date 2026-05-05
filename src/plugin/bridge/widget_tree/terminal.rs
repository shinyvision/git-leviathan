use iced::{Element, Length};

use crate::message::Message;
use crate::plugin::terminal::{registry, TerminalId};
use crate::plugin::ui::widget_ast::TerminalNode;
use crate::widgets::terminal::TerminalView;

use super::common::length_or;

pub(super) fn build(node: &TerminalNode) -> Element<'static, Message> {
    TerminalView::new(TerminalId::from(node.session), registry().clone())
        .width(length_or(node.width, Length::Fill))
        .height(length_or(node.height, Length::Fill))
        .font_size(node.font_size)
        .into()
}
