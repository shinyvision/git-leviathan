mod app;
mod assets;
mod config;
mod core;
mod message;
mod screens;
mod services;
mod style;
mod theme;
mod toast;
mod utils;
mod view_model;
mod widgets;

const JETBRAINS_MONO_REG: &[u8] = include_bytes!("assets/fonts/JetBrainsMono-Regular.ttf");
const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("assets/fonts/JetBrainsMono-Bold.ttf");

use app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title(|_: &App| "Git Leviathan".to_string())
        .subscription(App::subscription)
        .theme(App::theme)
        .antialiasing(true)
        .font(JETBRAINS_MONO_REG)
        .font(JETBRAINS_MONO_BOLD)
        .window(iced::window::Settings {
            size: iced::Size::new(1400.0, 900.0),
            min_size: Some(iced::Size::new(900.0, 600.0)),
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "git-leviathan".to_string(),
                ..Default::default()
            },
            ..Default::default()
        })
        .run()
}
