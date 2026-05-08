use iced::{
    alignment::{Horizontal, Vertical},
    keyboard,
    widget::{button, column, container, image, responsive, row, text, Stack},
    Alignment, Background, Border, Color, ContentFit, Element, Length, Padding, Shadow, Task,
    Theme, Vector,
};

use crate::{
    assets,
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
        responsive(blank_view).into()
    }
}

fn blank_view(size: iced::Size) -> Element<'static, Message> {
    let background = image(assets::blank_background_handle())
        .width(Length::Fill)
        .height(Length::Fill)
        .content_fit(ContentFit::Cover);

    let scale = responsive_scale(size);
    let horizontal_padding = (size.width * 0.04).clamp(20.0, 44.0);
    let available_width = (size.width - horizontal_padding * 2.0).max(180.0);

    let logo_size = (270.0 * scale).clamp(150.0, 310.0);
    let title_size = (58.0 * scale).clamp(28.0, 60.0);
    let subtitle_size = (26.0 * scale).clamp(14.0, 27.0);
    let button_width = (390.0 * scale).clamp(220.0, 420.0).min(available_width);
    let button_height = (70.0 * scale).clamp(46.0, 74.0);
    let button_text_size = (28.0 * scale).clamp(16.0, 28.0);
    let button_icon_size = (30.0 * scale).clamp(17.0, 30.0);
    let button_spacing = (18.0 * scale).clamp(10.0, 18.0);
    let content_spacing = (30.0 * scale).clamp(16.0, 32.0);
    let copy_spacing = (10.0 * scale).clamp(6.0, 10.0);
    let bottom_padding = (size.height * 0.145 * scale.clamp(0.75, 1.05)).clamp(52.0, 170.0);
    let (logo_center_x, logo_center_y) =
        background_cover_point(size, BACKGROUND_CIRCLE_CENTER_X, BACKGROUND_CIRCLE_CENTER_Y);
    let logo_left = (logo_center_x - logo_size / 2.0).clamp(0.0, (size.width - logo_size).max(0.0));
    let logo_top = (logo_center_y - logo_size / 2.0).clamp(0.0, (size.height - logo_size).max(0.0));

    let logo = image(assets::app_logo_handle())
        .width(Length::Fixed(logo_size))
        .height(Length::Fixed(logo_size))
        .content_fit(ContentFit::Contain);

    let title = row![
        text("Welcome to ").size(title_size).color(Color::WHITE),
        text("Git Leviathan!").size(title_size).color(WARM_TITLE),
    ]
    .spacing(0)
    .align_y(Alignment::Center);

    let copy = column![
        text("You currently don't have any repositories open.")
            .size(subtitle_size)
            .color(SUBTITLE),
        text("Click the big blue button to start!")
            .size(subtitle_size)
            .color(SUBTITLE),
    ]
    .spacing(copy_spacing)
    .align_x(Alignment::Center);

    let button_label = row![
        assets::icon(assets::FOLDER, button_icon_size, Color::WHITE),
        text("Open Repository")
            .size(button_text_size)
            .color(Color::WHITE),
    ]
    .spacing(button_spacing)
    .align_y(Alignment::Center);

    let open_button = button(
        container(button_label)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center),
    )
    .width(Length::Fixed(button_width))
    .height(Length::Fixed(button_height))
    .padding(Padding::ZERO)
    .style(open_button_style)
    .on_press(Message::App(AppMessage::OpenRepoDialog));

    let logo_layer = container(logo)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: logo_top,
            right: 0.0,
            bottom: 0.0,
            left: logo_left,
        })
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top);

    let content = container(
        column![title, copy, open_button]
            .spacing(content_spacing)
            .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(Padding {
        top: 0.0,
        right: horizontal_padding,
        bottom: bottom_padding,
        left: horizontal_padding,
    })
    .align_x(Horizontal::Center)
    .align_y(Vertical::Bottom);

    Stack::with_children(vec![background.into(), logo_layer.into(), content.into()])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn responsive_scale(size: iced::Size) -> f32 {
    const DESIGN_WIDTH: f32 = 1672.0;
    const DESIGN_HEIGHT: f32 = 941.0;

    let width_ratio = size.width / DESIGN_WIDTH;
    let height_ratio = size.height / DESIGN_HEIGHT;
    let blended = width_ratio * 0.58 + height_ratio * 0.42;
    let limiting_axis = width_ratio.min(height_ratio);

    (blended * 0.72 + limiting_axis * 0.28).clamp(0.54, 1.08)
}

fn background_cover_point(size: iced::Size, source_x: f32, source_y: f32) -> (f32, f32) {
    let scale = (size.width / BACKGROUND_WIDTH).max(size.height / BACKGROUND_HEIGHT);
    let rendered_width = BACKGROUND_WIDTH * scale;
    let rendered_height = BACKGROUND_HEIGHT * scale;
    let offset_x = (size.width - rendered_width) / 2.0;
    let offset_y = (size.height - rendered_height) / 2.0;

    (offset_x + source_x * scale, offset_y + source_y * scale)
}

const BACKGROUND_WIDTH: f32 = 1672.0;
const BACKGROUND_HEIGHT: f32 = 941.0;
const BACKGROUND_CIRCLE_CENTER_X: f32 = BACKGROUND_WIDTH / 2.0;
const BACKGROUND_CIRCLE_CENTER_Y: f32 = 304.0;

fn open_button_style(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => BUTTON_HOVER,
        button::Status::Pressed => BUTTON_PRESSED,
        button::Status::Disabled => BUTTON_BASE,
        button::Status::Active => BUTTON_BASE,
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: Border {
            color: BUTTON_BORDER,
            width: 1.5,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: BUTTON_GLOW,
            offset: Vector::new(0.0, 0.0),
            blur_radius: 24.0,
        },
        snap: false,
    }
}

const WARM_TITLE: Color = Color {
    r: 1.0,
    g: 0.58,
    b: 0.34,
    a: 1.0,
};

const SUBTITLE: Color = Color {
    r: 0.67,
    g: 0.70,
    b: 0.80,
    a: 1.0,
};

const BUTTON_BASE: Color = Color {
    r: 0.04,
    g: 0.39,
    b: 0.98,
    a: 0.96,
};

const BUTTON_HOVER: Color = Color {
    r: 0.10,
    g: 0.50,
    b: 1.0,
    a: 1.0,
};

const BUTTON_PRESSED: Color = Color {
    r: 0.03,
    g: 0.29,
    b: 0.78,
    a: 1.0,
};

const BUTTON_BORDER: Color = Color {
    r: 0.36,
    g: 0.72,
    b: 1.0,
    a: 0.88,
};

const BUTTON_GLOW: Color = Color {
    r: 0.02,
    g: 0.36,
    b: 1.0,
    a: 0.48,
};
