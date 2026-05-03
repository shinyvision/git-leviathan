//! Plugin overlay rendering.

use iced::{Element, Length};

use crate::message::Message;
use crate::plugin::bridge::widget_tree::{self, BuildCtx, DispatchScope};
use crate::plugin::host::PluginHost;

pub fn layers(host: &PluginHost) -> Vec<Element<'static, Message>> {
    let overlays = host.extension_overlays();
    let mut layers = Vec::with_capacity(overlays.len());
    for overlay in overlays.into_iter().rev() {
        let plugin_root = host
            .plugin_root(&overlay.plugin_id)
            .unwrap_or_else(|| std::path::Path::new(""));
        let ctx = BuildCtx {
            plugin_id: &overlay.plugin_id,
            scope: DispatchScope::Overlay {
                overlay_id: &overlay.id,
            },
            plugin_root,
            split_states: host.split_sizes(),
            active_drag: host.active_drag(),
        };
        layers.push(
            iced::widget::container(widget_tree::build(&overlay.widget, &ctx))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        );
    }
    layers
}
