use std::time::{Duration, Instant};

use iced::advanced::text::Renderer as _;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::Renderer as _;
use iced::advanced::{layout, renderer, Clipboard, Layout, Shell, Widget};
use iced::{
    alignment, keyboard, mouse, window, Color, Element, Event, Font, Length, Pixels, Point,
    Rectangle, Renderer, Size, Theme,
};

use crate::plugin::terminal::{TerminalId, TerminalRegistry, TerminalSnapshot};

const FONT_SIZE: f32 = 13.0;
const CELL_W_SCALE: f32 = 0.62;
const CELL_H_SCALE: f32 = 1.35;
const CURSOR_BLINK_MS: u64 = 500;

#[derive(Debug, Clone, Copy)]
struct TerminalState {
    focused: bool,
    blink_epoch: Instant,
    last_redraw: Instant,
}

impl Default for TerminalState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            focused: false,
            blink_epoch: now,
            last_redraw: now,
        }
    }
}

pub struct TerminalView {
    id: TerminalId,
    registry: TerminalRegistry,
    width: Length,
    height: Length,
    font_size: f32,
}

impl TerminalView {
    pub fn new(id: TerminalId, registry: TerminalRegistry) -> Self {
        Self {
            id,
            registry,
            width: Length::Fill,
            height: Length::Fill,
            font_size: FONT_SIZE,
        }
    }

    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    pub fn font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size.clamp(8.0, 24.0);
        self
    }
}

impl<Message> Widget<Message, Theme, Renderer> for TerminalView {
    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<TerminalState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(TerminalState::default())
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<TerminalState>();
        let bounds = layout.bounds();
        resize_to_bounds(&self.registry, self.id, bounds, self.font_size);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let focused = cursor.is_over(bounds);
                if state.focused != focused {
                    state.focused = focused;
                    shell.request_redraw();
                }
                if focused {
                    shell.capture_event();
                }
            }
            Event::Keyboard(key_event) if state.focused => {
                let application_cursor = self.registry.application_cursor(self.id);
                if let Some(bytes) = key_event_bytes(key_event, application_cursor) {
                    let _ = self.registry.write(self.id, &bytes);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                state.last_redraw = *now;
                if state.focused {
                    shell.request_redraw_at(*now + Duration::from_millis(CURSOR_BLINK_MS));
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            terminal_bg(),
        );

        let snapshot = self.registry.snapshot(self.id);
        if snapshot.missing {
            draw_message(
                renderer,
                bounds,
                viewport,
                "terminal session closed",
                self.font_size,
            );
            return;
        }

        draw_terminal(renderer, bounds, viewport, &snapshot, self.font_size);
        let state = tree.state.downcast_ref::<TerminalState>();
        if state.focused
            && snapshot.cursor_visible
            && cursor_visible(state.blink_epoch, state.last_redraw)
        {
            draw_cursor(renderer, bounds, &snapshot, self.font_size);
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }
}

fn draw_terminal(
    renderer: &mut Renderer,
    bounds: Rectangle,
    viewport: &Rectangle,
    snapshot: &TerminalSnapshot,
    font_size: f32,
) {
    let cell_w = cell_width(font_size);
    let cell_h = cell_height(font_size);
    for cell in &snapshot.cells {
        let x = bounds.x + f32::from(cell.col) * cell_w;
        let y = bounds.y + f32::from(cell.row) * cell_h;
        if x >= bounds.x + bounds.width || y >= bounds.y + bounds.height {
            continue;
        }
        let mut fg = vt_color(cell.fg, text_fg());
        let mut bg = vt_color(cell.bg, terminal_bg());
        if cell.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if bg.a > 0.0 && bg != terminal_bg() {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x,
                        y,
                        width: cell_w,
                        height: cell_h,
                    },
                    ..renderer::Quad::default()
                },
                bg,
            );
        }
        if !cell.text.is_empty() {
            renderer.fill_text(
                iced::advanced::text::Text {
                    content: cell.text.clone(),
                    font: Font::MONOSPACE,
                    size: Pixels(font_size),
                    line_height: iced::advanced::text::LineHeight::Absolute(Pixels(cell_h)),
                    bounds: Size::new(cell_w * 2.0, cell_h),
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    shaping: iced::advanced::text::Shaping::Basic,
                    wrapping: iced::advanced::text::Wrapping::None,
                },
                Point::new(x, y),
                if cell.bold { brighten(fg) } else { fg },
                *viewport,
            );
        }
    }
}

fn draw_cursor(
    renderer: &mut Renderer,
    bounds: Rectangle,
    snapshot: &TerminalSnapshot,
    font_size: f32,
) {
    let cell_w = cell_width(font_size);
    let cell_h = cell_height(font_size);
    let x = bounds.x + f32::from(snapshot.cursor_col) * cell_w;
    let y = bounds.y + f32::from(snapshot.cursor_row) * cell_h;
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x,
                y,
                width: cell_w,
                height: cell_h,
            },
            ..renderer::Quad::default()
        },
        Color {
            r: 0.8,
            g: 0.85,
            b: 1.0,
            a: 0.35,
        },
    );
}

fn draw_message(
    renderer: &mut Renderer,
    bounds: Rectangle,
    viewport: &Rectangle,
    value: &str,
    font_size: f32,
) {
    renderer.fill_text(
        iced::advanced::text::Text {
            content: value.to_string(),
            font: Font::MONOSPACE,
            size: Pixels(font_size),
            line_height: iced::advanced::text::LineHeight::Absolute(Pixels(cell_height(font_size))),
            bounds: bounds.size(),
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Top,
            shaping: iced::advanced::text::Shaping::Basic,
            wrapping: iced::advanced::text::Wrapping::None,
        },
        Point::new(bounds.x + 8.0, bounds.y + 8.0),
        text_dim(),
        *viewport,
    );
}

fn resize_to_bounds(
    registry: &TerminalRegistry,
    id: TerminalId,
    bounds: Rectangle,
    font_size: f32,
) {
    if bounds.width < 1.0 || bounds.height < 1.0 {
        return;
    }
    let cols = (bounds.width / cell_width(font_size)).floor().max(2.0) as u16;
    let rows = (bounds.height / cell_height(font_size)).floor().max(2.0) as u16;
    let _ = registry.resize(
        id,
        rows,
        cols,
        bounds.width.round().clamp(0.0, f32::from(u16::MAX)) as u16,
        bounds.height.round().clamp(0.0, f32::from(u16::MAX)) as u16,
    );
}

fn key_event_bytes(event: &keyboard::Event, application_cursor: bool) -> Option<Vec<u8>> {
    let keyboard::Event::KeyPressed {
        key,
        modifiers,
        text,
        ..
    } = event
    else {
        return None;
    };

    if modifiers.control() {
        return ctrl_bytes(key);
    }

    let text_bytes = || text.as_ref().map(|s| s.as_bytes().to_vec());
    let mut bytes = match key.as_ref() {
        keyboard::Key::Named(named) => {
            named_key_bytes(named, application_cursor).or_else(text_bytes)
        }
        _ => text.as_ref().map(|s| s.as_bytes().to_vec()),
    }?;
    if modifiers.alt() && !bytes.starts_with(b"\x1b") {
        let mut prefixed = vec![0x1b];
        prefixed.extend(bytes);
        bytes = prefixed;
    }
    Some(bytes)
}

fn named_key_bytes(named: keyboard::key::Named, application_cursor: bool) -> Option<Vec<u8>> {
    use keyboard::key::Named;
    let value = match named {
        Named::Enter => "\r",
        Named::Tab => "\t",
        Named::Escape => "\x1b",
        Named::Backspace => "\x7f",
        Named::ArrowUp if application_cursor => "\x1bOA",
        Named::ArrowDown if application_cursor => "\x1bOB",
        Named::ArrowRight if application_cursor => "\x1bOC",
        Named::ArrowLeft if application_cursor => "\x1bOD",
        Named::ArrowUp => "\x1b[A",
        Named::ArrowDown => "\x1b[B",
        Named::ArrowRight => "\x1b[C",
        Named::ArrowLeft => "\x1b[D",
        Named::Home => "\x1b[H",
        Named::End => "\x1b[F",
        Named::PageUp => "\x1b[5~",
        Named::PageDown => "\x1b[6~",
        Named::Delete => "\x1b[3~",
        Named::Insert => "\x1b[2~",
        _ => return None,
    };
    Some(value.as_bytes().to_vec())
}

fn ctrl_bytes(key: &keyboard::Key) -> Option<Vec<u8>> {
    match key.as_ref() {
        keyboard::Key::Character(s) => {
            let ch = s.chars().next()?.to_ascii_lowercase();
            let byte = match ch {
                'a'..='z' => (ch as u8) - b'a' + 1,
                '@' | ' ' => 0,
                '[' => 0x1b,
                '\\' => 0x1c,
                ']' => 0x1d,
                '^' => 0x1e,
                '_' => 0x1f,
                '?' => 0x7f,
                _ => return None,
            };
            Some(vec![byte])
        }
        keyboard::Key::Named(keyboard::key::Named::Space) => Some(vec![0]),
        keyboard::Key::Named(keyboard::key::Named::Backspace) => Some(vec![0x7f]),
        keyboard::Key::Named(keyboard::key::Named::Escape) => Some(vec![0x1b]),
        _ => None,
    }
}

fn cursor_visible(epoch: Instant, now: Instant) -> bool {
    (now.checked_duration_since(epoch)
        .unwrap_or_default()
        .as_millis()
        / u128::from(CURSOR_BLINK_MS))
    .is_multiple_of(2)
}

fn cell_width(font_size: f32) -> f32 {
    (font_size * CELL_W_SCALE).max(5.0)
}

fn cell_height(font_size: f32) -> f32 {
    (font_size * CELL_H_SCALE).max(10.0)
}

fn terminal_bg() -> Color {
    Color::from_rgb8(8, 10, 15)
}

fn text_fg() -> Color {
    Color::from_rgb8(225, 229, 244)
}

fn text_dim() -> Color {
    Color::from_rgb8(88, 93, 110)
}

fn brighten(color: Color) -> Color {
    Color {
        r: (color.r * 1.2).min(1.0),
        g: (color.g * 1.2).min(1.0),
        b: (color.b * 1.2).min(1.0),
        a: color.a,
    }
}

fn vt_color(color: vt100::Color, default: Color) -> Color {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color::from_rgb8(r, g, b),
        vt100::Color::Idx(idx) => palette_color(idx, default),
    }
}

fn palette_color(idx: u8, default: Color) -> Color {
    const ANSI: [(u8, u8, u8); 16] = [
        (17, 19, 27),
        (255, 107, 107),
        (114, 213, 114),
        (255, 209, 102),
        (106, 169, 255),
        (199, 146, 234),
        (94, 223, 255),
        (225, 229, 244),
        (88, 93, 110),
        (255, 143, 143),
        (155, 233, 155),
        (255, 224, 138),
        (149, 194, 255),
        (221, 179, 255),
        (159, 240, 255),
        (255, 255, 255),
    ];
    if let Some((r, g, b)) = ANSI.get(idx as usize) {
        Color::from_rgb8(*r, *g, *b)
    } else if (16..=231).contains(&idx) {
        let n = idx - 16;
        let r = n / 36;
        let g = (n / 6) % 6;
        let b = n % 6;
        Color::from_rgb8(cube(r), cube(g), cube(b))
    } else if (232..=255).contains(&idx) {
        let level = 8 + (idx - 232) * 10;
        Color::from_rgb8(level, level, level)
    } else {
        default
    }
}

fn cube(value: u8) -> u8 {
    if value == 0 {
        0
    } else {
        55 + value * 40
    }
}

impl<'a, Message: 'a> From<TerminalView> for Element<'a, Message> {
    fn from(view: TerminalView) -> Self {
        Element::new(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{key, Key, Location, Modifiers};

    fn key_event(key: Key, text: Option<&str>, modifiers: Modifiers) -> keyboard::Event {
        keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: key::Physical::Code(key::Code::Space),
            location: Location::Standard,
            modifiers,
            text: text.map(Into::into),
            repeat: false,
        }
    }

    #[test]
    fn named_space_falls_back_to_text_input() {
        let event = key_event(
            Key::Named(keyboard::key::Named::Space),
            Some(" "),
            Modifiers::NONE,
        );
        assert_eq!(key_event_bytes(&event, false), Some(vec![b' ']));
    }

    #[test]
    fn ctrl_space_still_sends_nul() {
        let event = key_event(
            Key::Named(keyboard::key::Named::Space),
            Some(" "),
            Modifiers::CTRL,
        );
        assert_eq!(key_event_bytes(&event, false), Some(vec![0]));
    }

    #[test]
    fn ctrl_d_sends_eot() {
        let event = key_event(Key::Character("d".into()), Some("d"), Modifiers::CTRL);
        assert_eq!(key_event_bytes(&event, false), Some(vec![4]));
    }
}
