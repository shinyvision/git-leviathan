//! Plugin screen render + subscription helpers.
//!
//! A "plugin screen" is not a separate `Screen` trait impl — it's rendered
//! directly from the host's cached widget tree whenever `active_screen()` is
//! `Some`. Keeping it out of the trait avoids inventing a second Message type
//! and lets `App::view` stay a single match on the active-screen enum.

use iced::widget::{container, text};
use iced::{event, mouse, Element, Event, Length, Subscription, Theme};

use crate::message::Message;
use crate::plugin::bridge::widget_tree::{self, BuildCtx, DispatchScope};
use crate::plugin::host::PluginHost;
use crate::plugin::message::PluginMessage;
use crate::theme;

pub fn view(host: &PluginHost) -> Element<'_, Message> {
    let Some((plugin_id, screen_id)) = host.active_screen() else {
        return fallback("no active plugin screen".to_string());
    };
    let Some(ast) = host.widget_tree() else {
        return fallback(format!("plugin {plugin_id}/{screen_id}: empty widget tree"));
    };
    let plugin_root = host
        .plugin_root(plugin_id)
        .unwrap_or_else(|| std::path::Path::new(""));
    let ctx = BuildCtx {
        plugin_id,
        scope: DispatchScope::Screen { screen_id },
        plugin_root,
        split_states: host.split_sizes(),
        active_drag: host.active_drag(),
    };
    let root = widget_tree::build(ast, &ctx);
    container(root)
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_BASE.into()),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub fn subscription(host: &PluginHost) -> Subscription<Message> {
    if !host.is_dragging_split() {
        return Subscription::none();
    }
    event::listen_with(|event, _status, _window| match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::Plugin(PluginMessage::SplitDragged {
                x: position.x,
                y: position.y,
            }))
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::Plugin(PluginMessage::SplitDragReleased))
        }
        _ => None,
    })
}

fn fallback<'a>(msg: String) -> Element<'a, Message> {
    container(text(msg).size(14.0))
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_BASE.into()),
            ..Default::default()
        })
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
