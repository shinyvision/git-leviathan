use iced::{widget::svg, Color, Element, Length, Theme};

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
pub const ARROWS_SORT: &[u8] = include_bytes!("icons/arrows-sort.svg");
pub const REFRESH: &[u8] = include_bytes!("icons/refresh.svg");
pub const PLUS: &[u8] = include_bytes!("icons/plus-filled.svg");
pub const MINUS: &[u8] = include_bytes!("icons/minus.svg");

pub const COPY: &[u8] = include_bytes!("icons/copy.svg");
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
