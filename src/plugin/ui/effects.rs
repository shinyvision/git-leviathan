use std::cell::RefCell;
use std::rc::Rc;

use crate::plugin::ui::dialog::DialogRequest;
use crate::plugin::ui::widget_ast::{WidgetAst, WidgetNode};

#[derive(Debug, Clone, PartialEq)]
pub struct ScrollToRequest {
    pub id: String,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginUiEffect {
    OpenRepositoryDialog(DialogRequest),
    CloseRepositoryDialog {
        plugin_id: String,
        dialog_id: String,
    },
    FocusRepositoryDialogControl {
        dialog_id: String,
        control_id: String,
    },
    PressRepositoryDialogButton {
        dialog_id: String,
        button_id: String,
    },
}

#[derive(Clone, Default)]
pub struct PendingUiEffects {
    scroll_to: Rc<RefCell<Vec<ScrollToRequest>>>,
    effects: Rc<RefCell<Vec<PluginUiEffect>>>,
}

impl PendingUiEffects {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue_widget_scroll_positions(
        &self,
        plugin_id: &str,
        scope_key: &str,
        widget: &WidgetAst,
    ) {
        let mut requests = Vec::new();
        collect_scroll_positions(plugin_id, scope_key, widget, &mut requests);
        if !requests.is_empty() {
            self.scroll_to.borrow_mut().extend(requests);
        }
    }

    pub fn take_scroll_to(&self) -> Vec<ScrollToRequest> {
        std::mem::take(&mut *self.scroll_to.borrow_mut())
    }

    pub fn queue_open_repository_dialog(&self, request: DialogRequest) {
        self.effects
            .borrow_mut()
            .push(PluginUiEffect::OpenRepositoryDialog(request));
    }

    pub fn queue_close_repository_dialog(
        &self,
        plugin_id: impl Into<String>,
        dialog_id: impl Into<String>,
    ) {
        self.effects
            .borrow_mut()
            .push(PluginUiEffect::CloseRepositoryDialog {
                plugin_id: plugin_id.into(),
                dialog_id: dialog_id.into(),
            });
    }

    pub fn queue_focus_repository_dialog_control(
        &self,
        dialog_id: impl Into<String>,
        control_id: impl Into<String>,
    ) {
        self.effects
            .borrow_mut()
            .push(PluginUiEffect::FocusRepositoryDialogControl {
                dialog_id: dialog_id.into(),
                control_id: control_id.into(),
            });
    }

    pub fn queue_press_repository_dialog_button(
        &self,
        dialog_id: impl Into<String>,
        button_id: impl Into<String>,
    ) {
        self.effects
            .borrow_mut()
            .push(PluginUiEffect::PressRepositoryDialogButton {
                dialog_id: dialog_id.into(),
                button_id: button_id.into(),
            });
    }

    pub fn take_effects(&self) -> Vec<PluginUiEffect> {
        std::mem::take(&mut *self.effects.borrow_mut())
    }
}

pub fn plugin_scrollable_id(plugin_id: &str, scope_key: &str, scrollable_id: &str) -> String {
    format!("plugin:{plugin_id}:{scope_key}:scrollable:{scrollable_id}")
}

fn collect_scroll_positions(
    plugin_id: &str,
    scope_key: &str,
    ast: &WidgetAst,
    out: &mut Vec<ScrollToRequest>,
) {
    match &ast.node {
        WidgetNode::Button(node) => collect_optional_child(plugin_id, scope_key, &node.child, out),
        WidgetNode::Row(node) => collect_children(plugin_id, scope_key, &node.children, out),
        WidgetNode::Column(node) => collect_children(plugin_id, scope_key, &node.children, out),
        WidgetNode::Container(node) => {
            collect_optional_child(plugin_id, scope_key, &node.child, out)
        }
        WidgetNode::Padding(node) => collect_optional_child(plugin_id, scope_key, &node.child, out),
        WidgetNode::Scrollable(node) => {
            if let (Some(id), Some(scroll_y)) = (&node.id, node.scroll_y) {
                out.push(ScrollToRequest {
                    id: plugin_scrollable_id(plugin_id, scope_key, id),
                    y: scroll_y.max(0.0),
                });
            }
            collect_optional_child(plugin_id, scope_key, &node.child, out);
        }
        WidgetNode::MouseArea(node) => {
            collect_optional_child(plugin_id, scope_key, &node.child, out)
        }
        WidgetNode::Terminal(_)
        | WidgetNode::Text(_)
        | WidgetNode::TextInput(_)
        | WidgetNode::Space(_)
        | WidgetNode::Icon(_)
        | WidgetNode::Image(_)
        | WidgetNode::Tablist(_) => {}
        WidgetNode::ResizableSplit(node) => {
            collect_children(plugin_id, scope_key, &node.children, out);
        }
        WidgetNode::Semantic(node) => {
            collect_optional_child(plugin_id, scope_key, &node.child, out);
            collect_children(plugin_id, scope_key, &node.children, out);
        }
        WidgetNode::Layout(node) => {
            collect_optional_child(plugin_id, scope_key, &node.child, out);
        }
    }
}

fn collect_optional_child(
    plugin_id: &str,
    scope_key: &str,
    child: &Option<Box<WidgetAst>>,
    out: &mut Vec<ScrollToRequest>,
) {
    if let Some(child) = child {
        collect_scroll_positions(plugin_id, scope_key, child, out);
    }
}

fn collect_children(
    plugin_id: &str,
    scope_key: &str,
    children: &[WidgetAst],
    out: &mut Vec<ScrollToRequest>,
) {
    for child in children {
        collect_scroll_positions(plugin_id, scope_key, child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> DialogRequest {
        DialogRequest {
            plugin_id: "plugin.one".into(),
            dialog_id: "confirm".into(),
            text: "Continue?".into(),
            title: Some("Confirm".into()),
            data: Vec::new(),
            controls: Vec::new(),
            buttons: Vec::new(),
            dismissible: true,
            autofocus: None,
        }
    }

    #[test]
    fn queued_repository_dialog_request_drains_once() {
        let pending = PendingUiEffects::new();
        let request = request();

        pending.queue_open_repository_dialog(request.clone());

        assert_eq!(
            pending.take_effects(),
            vec![PluginUiEffect::OpenRepositoryDialog(request)]
        );
        assert!(pending.take_effects().is_empty());
    }
}
