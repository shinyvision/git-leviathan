use iced::{keyboard, Element, Subscription, Task};

use crate::message::Message;
use crate::plugin::host::PluginHost;
use crate::plugin::message::PluginMessage;
use crate::screens::{Screen, ToolbarCtx};
use crate::widgets::chrome;

#[derive(Debug, Clone)]
pub struct PluginScreenSummary {
    pub plugin_id: String,
    pub screen_id: String,
    pub title: String,
    pub breadcrumbs: Vec<String>,
    pub bind_repository: bool,
}

#[derive(Debug, Clone)]
pub struct PluginScreen {
    plugin_id: String,
    screen_id: String,
    title: String,
    breadcrumbs: Vec<String>,
    bound_repo_path: Option<String>,
    focused: bool,
}

impl PluginScreen {
    pub fn new(summary: PluginScreenSummary, bound_repo_path: Option<String>) -> Self {
        Self {
            plugin_id: summary.plugin_id,
            screen_id: summary.screen_id,
            title: summary.title,
            breadcrumbs: summary.breadcrumbs,
            bound_repo_path,
            focused: false,
        }
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn screen_id(&self) -> &str {
        &self.screen_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn bound_repo_path(&self) -> Option<&str> {
        self.bound_repo_path.as_deref()
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn view_with_host<'a>(&'a self, host: &'a PluginHost) -> Element<'a, Message> {
        crate::plugin::ui::screen::view_for(host, &self.plugin_id, &self.screen_id)
    }

    pub fn subscription_with_host(&self, host: &PluginHost) -> Subscription<Message> {
        crate::plugin::ui::screen::subscription(host)
    }

    pub fn handle_key_pressed(
        &self,
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> PluginMessage {
        PluginMessage::Event {
            plugin_id: self.plugin_id.clone(),
            screen_id: self.screen_id.clone(),
            event: "key".to_string(),
            value: serde_json::json!({
                "key": format!("{key:?}"),
                "ctrl": modifiers.control(),
                "shift": modifiers.shift(),
                "alt": modifiers.alt(),
                "logo": modifiers.logo(),
                "command": modifiers.command(),
            }),
        }
    }

    pub fn can_close(&self, host: &PluginHost) -> bool {
        host.can_close_screen(&self.plugin_id, &self.screen_id)
    }
}

impl Screen for PluginScreen {
    type Message = PluginMessage;

    fn update(&mut self, msg: PluginMessage) -> Task<Message> {
        Task::done(Message::Plugin(msg))
    }

    fn view(&self) -> Element<'_, Message> {
        iced::widget::text("").into()
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn breadcrumbs(&self) -> &[String] {
        &self.breadcrumbs
    }

    fn has_focus(&self) -> bool {
        self.focused
    }

    fn toolbar<'a>(&'a self, ctx: &ToolbarCtx<'a>) -> Option<Element<'a, Message>> {
        let slot_ctx = chrome::SlotCtx::new(&self.title, "", ctx.now, None, None, None);
        Some(chrome::main_bar_view(ctx.main_bar_registry, &slot_ctx))
    }
}
