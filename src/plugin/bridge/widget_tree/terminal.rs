use iced::{Element, Length};

use crate::message::Message;
use crate::plugin::terminal::{registry, TerminalId};
use crate::plugin::ui::widget_ast::TerminalNode;
use crate::widgets::terminal::TerminalView;

use super::common::{build_error_widget, length_or};
use super::BuildCtx;

pub(super) fn build(node: &TerminalNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let id = TerminalId::from(node.session);
    // The terminal widget both renders and (on keyboard focus) writes to the
    // PTY, so a plugin must own the session it names — otherwise a guessed id
    // would hijack another plugin's terminal. The shell API gates writes the
    // same way; this closes the widget-tree path.
    if !registry().owned_by_plugin(id, ctx.plugin_id) {
        return build_error_widget(
            "terminal.not_owned",
            "terminal session is not owned by this plugin",
        );
    }
    TerminalView::new(id, registry().clone())
        .width(length_or(node.width, Length::Fill))
        .height(length_or(node.height, Length::Fill))
        .font_size(node.font_size)
        .into()
}
