use std::sync::LazyLock;

use iced::{
    widget::{image, svg},
    Color, Element, Length, Theme,
};

pub const BLANK_BACKGROUND: &[u8] = include_bytes!("images/git_leviathan_background.png");
pub const APP_LOGO: &[u8] = include_bytes!("../../packaging/icons/git-leviathan.png");

static BLANK_BACKGROUND_HANDLE: LazyLock<image::Handle> =
    LazyLock::new(|| image::Handle::from_bytes(BLANK_BACKGROUND));
static APP_LOGO_HANDLE: LazyLock<image::Handle> =
    LazyLock::new(|| image::Handle::from_bytes(APP_LOGO));

pub fn blank_background_handle() -> image::Handle {
    BLANK_BACKGROUND_HANDLE.clone()
}

pub fn app_logo_handle() -> image::Handle {
    APP_LOGO_HANDLE.clone()
}

pub const UNDO: &[u8] = include_bytes!("icons/arrow-back-up.svg");

pub const PULL: &[u8] = include_bytes!("icons/arrow-bar-to-down.svg");
pub const PUSH: &[u8] = include_bytes!("icons/arrow-bar-up.svg");
pub const ARROW_UP: &[u8] = include_bytes!("icons/arrow-up.svg");
pub const ARROW_DOWN: &[u8] = include_bytes!("icons/arrow-down.svg");
pub const BRANCH: &[u8] = include_bytes!("icons/git-branch.svg");
pub const STASH: &[u8] = include_bytes!("icons/archive.svg");
pub const POP: &[u8] = include_bytes!("icons/package-import.svg");

pub const FOLDER: &[u8] = include_bytes!("icons/folder.svg");
pub const CHEVRON_DOWN: &[u8] = include_bytes!("icons/chevron-down.svg");
pub const CHEVRON_RIGHT: &[u8] = include_bytes!("icons/chevron-right.svg");
pub const CLOSE: &[u8] = include_bytes!("icons/x.svg");

pub const SEARCH: &[u8] = include_bytes!("icons/search.svg");
pub const LAPTOP: &[u8] = include_bytes!("icons/device-laptop.svg");
pub const CLOUD: &[u8] = include_bytes!("icons/cloud.svg");
pub const STACK: &[u8] = include_bytes!("icons/stack.svg");
pub const TREE: &[u8] = include_bytes!("icons/tree.svg");

pub const TAG: &[u8] = include_bytes!("icons/tag.svg");
pub const CHECK: &[u8] = include_bytes!("icons/check.svg");

pub const PENCIL: &[u8] = include_bytes!("icons/pencil-filled.svg");
pub const REFRESH: &[u8] = include_bytes!("icons/refresh.svg");
pub const PLUS: &[u8] = include_bytes!("icons/plus-filled.svg");
pub const MINUS: &[u8] = include_bytes!("icons/minus.svg");

pub const COPY: &[u8] = include_bytes!("icons/copy.svg");

// Media diff viewer.
pub const PLAY: &[u8] = include_bytes!("icons/player-play.svg");
pub const PAUSE: &[u8] = include_bytes!("icons/player-pause.svg");
pub const SKIP_BACK: &[u8] = include_bytes!("icons/player-skip-back.svg");
pub const SKIP_FORWARD: &[u8] = include_bytes!("icons/player-skip-forward.svg");
pub const FRAME_PREV: &[u8] = include_bytes!("icons/player-track-prev.svg");
pub const FRAME_NEXT: &[u8] = include_bytes!("icons/player-track-next.svg");
pub const VOLUME: &[u8] = include_bytes!("icons/volume.svg");
pub const VOLUME_OFF: &[u8] = include_bytes!("icons/volume-off.svg");
pub const REPEAT: &[u8] = include_bytes!("icons/repeat.svg");
pub const ZOOM_IN: &[u8] = include_bytes!("icons/zoom-in.svg");
pub const ZOOM_OUT: &[u8] = include_bytes!("icons/zoom-out.svg");
pub const FIT_SCREEN: &[u8] = include_bytes!("icons/arrows-maximize.svg");
pub const ACTUAL_SIZE: &[u8] = include_bytes!("icons/aspect-ratio.svg");
pub const LINK: &[u8] = include_bytes!("icons/link.svg");
pub const UNLINK: &[u8] = include_bytes!("icons/unlink.svg");
pub const INFO: &[u8] = include_bytes!("icons/info-circle.svg");
pub const FILE_TEXT: &[u8] = include_bytes!("icons/file-text.svg");
pub const GRID: &[u8] = include_bytes!("icons/grid-pattern.svg");
pub const COLUMNS: &[u8] = include_bytes!("icons/columns-2.svg");
pub const SWIPE: &[u8] = include_bytes!("icons/arrows-horizontal.svg");
pub const LAYERS: &[u8] = include_bytes!("icons/layers-intersect.svg");
pub const CONTRAST: &[u8] = include_bytes!("icons/contrast.svg");
pub const PHOTO: &[u8] = include_bytes!("icons/photo.svg");
pub const MUSIC: &[u8] = include_bytes!("icons/music.svg");
pub const MOVIE: &[u8] = include_bytes!("icons/movie.svg");
pub const EYE_DROPPER: &[u8] = include_bytes!("icons/eye-dropper.svg");
pub const PIXEL_GRID: &[u8] = include_bytes!("icons/focus-2.svg");
pub const WARNING: &[u8] = include_bytes!("icons/alert-triangle.svg");
pub const SPEED: &[u8] = include_bytes!("icons/gauge.svg");
pub const EXTERNAL_LINK: &[u8] = include_bytes!("icons/external-link.svg");

pub fn icon<'a, Message: 'a>(data: &'static [u8], size: f32, color: Color) -> Element<'a, Message> {
    svg(svg::Handle::from_memory(data))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_: &Theme, _: svg::Status| svg::Style { color: Some(color) })
        .into()
}

pub fn toolbar_icon<'a, Message: 'a>(data: &'static [u8], color: Color) -> Element<'a, Message> {
    icon(data, 20.0, color)
}

pub fn sidebar_icon<'a, Message: 'a>(data: &'static [u8], color: Color) -> Element<'a, Message> {
    icon(data, 14.0, color)
}

pub fn tab_icon<'a, Message: 'a>(data: &'static [u8], color: Color) -> Element<'a, Message> {
    icon(data, 13.0, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_screen_image_handles_are_stable() {
        assert_eq!(blank_background_handle(), blank_background_handle());
        assert_eq!(app_logo_handle(), app_logo_handle());
    }
}
