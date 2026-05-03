use iced::{
    keyboard,
    widget::{button, column, container, text},
    Alignment, Element, Length, Task,
};

use crate::{
    message::{AppMessage, Message},
    screens::screen_trait::Screen,
};

pub struct BlankScreen;

#[derive(Debug, Clone)]
pub enum BlankMessage {
    KeyPressed(keyboard::Key, keyboard::Modifiers),
}

impl BlankScreen {
    pub fn new() -> Self {
        Self
    }
}

impl Screen for BlankScreen {
    type Message = BlankMessage;

    fn update(&mut self, msg: BlankMessage) -> Task<Message> {
        match msg {
            BlankMessage::KeyPressed(key, modifiers) => {
                let _ = (key, modifiers);
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let content: Element<Message> = container(
            column![
                text("No repositories open").size(24),
                button("Open Repository")
                    .on_press(Message::App(AppMessage::OpenRepoDialog))
                    .padding(10),
            ]
            .spacing(16)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .into();

        content
    }
}
