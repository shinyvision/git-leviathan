use iced::{
    widget::{button, column, container, progress_bar, row, scrollable, text, Space},
    Element, Length, Padding,
};
use serde_json::Value;

use crate::message::app::AppMessage;
use crate::message::Message;
use crate::plugin::ui::widget_ast::WidgetAst;
use crate::plugin::ui::widget_ast_semantic::{
    LayoutNode, SemanticItem, SemanticNode, SemanticOption,
};
use crate::theme;

use super::common::{opt_color_to_iced, ScopeDispatch};
use super::BuildCtx;

pub(super) fn build(
    ast: &WidgetAst,
    node: &SemanticNode,
    ctx: &BuildCtx<'_>,
) -> Element<'static, Message> {
    match node.kind.as_str() {
        "command_button" | "toolbar_button" => action_button(ast, node, ctx),
        "status_item" | "badge" | "tag" | "commit_ref" | "branch_ref" | "remote_ref" => {
            pill_for(node, label_for(ast, node))
        }
        "list" | "tree" | "menu" => list_like(node, ctx),
        "table" => table(node),
        "section" | "form" => section_like(ast, node, ctx),
        "checkbox" | "toggle" => checked_like(ast, node),
        "select" | "radio_group" => options_like(ast, node),
        "divider" => container(Space::new().height(Length::Fixed(1.0)))
            .height(Length::Fixed(1.0))
            .width(Length::Fill)
            .into(),
        "tooltip" | "popover" => section_like(ast, node, ctx),
        "empty_state" => container(text(label_for(ast, node)).size(13.0))
            .padding(Padding::from([12, 10]))
            .width(Length::Fill)
            .into(),
        "code" | "diff" => text(node.text.clone().unwrap_or_default())
            .size(12.0)
            .into(),
        "progress" => {
            let value = node.progress.unwrap_or(0.0).clamp(0.0, 1.0);
            column![
                text(label_for(ast, node)).size(12.0),
                container(progress_bar(0.0..=1.0, value)).width(Length::Fill)
            ]
            .spacing(spacing_for(node, 4.0))
            .into()
        }
        _ => text(label_for(ast, node)).into(),
    }
}

pub(super) fn build_layout(node: &LayoutNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    match node.kind.as_str() {
        "grid" => grid(node, ctx),
        "dock" | "split" => linear(node, ctx, node.direction.as_deref().unwrap_or("horizontal")),
        "tabs" => tabs(node, ctx),
        "virtual_list" => scrollable(linear(node, ctx, "vertical")).into(),
        _ => linear(node, ctx, "vertical"),
    }
}

fn action_button(
    ast: &WidgetAst,
    node: &SemanticNode,
    ctx: &BuildCtx<'_>,
) -> Element<'static, Message> {
    let content: Element<'static, Message> = if let Some(child) = &node.child {
        super::build(child, ctx)
    } else {
        text(label_for(ast, node)).size(13.0).into()
    };
    let mut btn = button(content).padding(Padding::from([4, 8]));
    if node.disabled || ast.meta.disabled_reason.is_some() {
        return btn.into();
    }
    if let Some(command) = &node.command {
        return btn
            .on_press(Message::App(AppMessage::InvokeCommand {
                id: command.clone(),
                args: node.value.clone(),
            }))
            .into();
    }
    if let Some(event) = &node.on_click {
        btn = btn.on_press(plugin_event(ctx, event.clone(), node.value.clone()));
    }
    btn.into()
}

fn section_like(
    ast: &WidgetAst,
    node: &SemanticNode,
    ctx: &BuildCtx<'_>,
) -> Element<'static, Message> {
    let mut children = Vec::new();
    let label = label_for(ast, node);
    if !label.is_empty() {
        children.push(text(label).size(14.0).into());
    }
    if let Some(child) = &node.child {
        children.push(super::build(child, ctx));
    }
    children.extend(node.children.iter().map(|child| super::build(child, ctx)));
    container(column(children).spacing(spacing_for(node, 6.0)))
        .padding(Padding::from([6, 0]))
        .width(Length::Fill)
        .into()
}

fn list_like(node: &SemanticNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let rows = node
        .items
        .iter()
        .map(|item| item_row(item, ctx, 0))
        .chain(node.children.iter().map(|child| super::build(child, ctx)))
        .collect::<Vec<_>>();
    column(rows).spacing(spacing_for(node, 3.0)).into()
}

fn item_row(item: &SemanticItem, ctx: &BuildCtx<'_>, depth: usize) -> Element<'static, Message> {
    let indent = Space::new().width(Length::Fixed((depth * 12) as f32));
    let mut content = Vec::new();
    content.push(row![indent, text(item.label.clone()).size(13.0)].into());
    if let Some(child) = &item.child {
        content.push(super::build(child, ctx));
    }
    content.extend(
        item.children
            .iter()
            .map(|child| item_row(child, ctx, depth + 1)),
    );
    column(content).spacing(2.0).into()
}

fn table(node: &SemanticNode) -> Element<'static, Message> {
    let mut rows = Vec::new();
    if !node.columns.is_empty() {
        rows.push(
            row(node
                .columns
                .iter()
                .map(|c| text(c.title.clone()).size(12.0).into())
                .collect::<Vec<_>>())
            .spacing(12.0)
            .into(),
        );
    }
    rows.extend(node.rows.iter().map(|r| {
        row(r
            .cells
            .iter()
            .map(|cell| text(value_label(cell)).size(12.0).into())
            .collect::<Vec<_>>())
        .spacing(12.0)
        .into()
    }));
    column(rows).spacing(spacing_for(node, 4.0)).into()
}

fn checked_like(ast: &WidgetAst, node: &SemanticNode) -> Element<'static, Message> {
    let mark = if node.checked { "[x]" } else { "[ ]" };
    let mut label = text(format!("{mark} {}", label_for(ast, node))).size(13.0);
    if let Some(color) = opt_color_to_iced(&node.color) {
        label =
            label.style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(color) });
    }
    label.into()
}

fn options_like(ast: &WidgetAst, node: &SemanticNode) -> Element<'static, Message> {
    let mut rows = vec![text(label_for(ast, node)).size(13.0).into()];
    rows.extend(node.options.iter().map(option_label));
    column(rows).spacing(spacing_for(node, 3.0)).into()
}

fn option_label(option: &SemanticOption) -> Element<'static, Message> {
    text(format!("• {}", option.label)).size(12.0).into()
}

fn grid(node: &LayoutNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let mut rows = Vec::new();
    let spacing = layout_spacing_for(node, 8.0);
    for chunk in node.children.chunks(node.columns.max(1)) {
        rows.push(
            row(chunk
                .iter()
                .map(|child| super::build(child, ctx))
                .collect::<Vec<_>>())
            .spacing(spacing)
            .into(),
        );
    }
    column(rows).spacing(spacing).into()
}

fn linear(node: &LayoutNode, ctx: &BuildCtx<'_>, direction: &str) -> Element<'static, Message> {
    let mut children = Vec::new();
    if let Some(child) = &node.child {
        children.push(super::build(child, ctx));
    }
    children.extend(node.children.iter().map(|child| super::build(child, ctx)));
    let spacing = layout_spacing_for(node, 8.0);
    if direction == "horizontal" {
        row(children).spacing(spacing).into()
    } else {
        column(children).spacing(spacing).into()
    }
}

fn tabs(node: &LayoutNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let active = node
        .tabs
        .iter()
        .find(|tab| tab.id == node.active)
        .or_else(|| node.tabs.first());
    let spacing = layout_spacing_for(node, 6.0);
    let tab_titles = row(node
        .tabs
        .iter()
        .map(|tab| text(tab.title.clone()).size(12.0).into())
        .collect::<Vec<_>>())
    .spacing(spacing);
    let body = active
        .and_then(|tab| tab.child.as_deref())
        .map(|child| super::build(child, ctx))
        .unwrap_or_else(|| Space::new().into());
    column![tab_titles, body].spacing(spacing).into()
}

fn pill_for(node: &SemanticNode, label: String) -> Element<'static, Message> {
    let mut label = text(label).size(12.0);
    if let Some(color) = opt_color_to_iced(&node.color) {
        label =
            label.style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(color) });
    }
    container(label)
        .padding(Padding::from([2, 6]))
        .style(|_: &iced::Theme| iced::widget::container::Style {
            text_color: Some(theme::TEXT_PRIMARY),
            background: Some(theme::BG_PANEL.into()),
            border: iced::Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn label_for(ast: &WidgetAst, node: &SemanticNode) -> String {
    node.label
        .clone()
        .or_else(|| node.title.clone())
        .or_else(|| node.text.clone())
        .or_else(|| ast.meta.label.clone())
        .unwrap_or_default()
}

fn spacing_for(node: &SemanticNode, default: f32) -> f32 {
    node.spacing.unwrap_or(default)
}

fn layout_spacing_for(node: &LayoutNode, default: f32) -> f32 {
    node.spacing.unwrap_or(default)
}

fn value_label(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn plugin_event(ctx: &BuildCtx<'_>, event: String, value: Value) -> Message {
    ScopeDispatch::from_ctx(ctx).publish(event, value)
}
