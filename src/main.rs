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
const APP_ICON_PNG: &[u8] = include_bytes!("../packaging/icons/git-leviathan.png");

use app::App;

fn main() -> iced::Result {
    configure_wgpu_backend();
    configure_libgit2_caches();

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
            icon: iced::window::icon::from_file_data(APP_ICON_PNG, None).ok(),
            platform_specific: platform_specific(),
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}

#[cfg(target_os = "linux")]
fn platform_specific() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific {
        application_id: "git-leviathan".to_string(),
        ..Default::default()
    }
}

#[cfg(not(target_os = "linux"))]
fn platform_specific() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific::default()
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn configure_wgpu_backend() {
    if std::env::var_os("WGPU_BACKEND").is_none() {
        std::env::set_var("WGPU_BACKEND", "vulkan");
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
fn configure_wgpu_backend() {}

fn configure_libgit2_caches() {
    unsafe {
        let _ = git2::opts::set_mwindow_size(8 * 1024 * 1024);
        let _ = git2::opts::set_mwindow_mapped_limit(64 * 1024 * 1024);
    }
}
