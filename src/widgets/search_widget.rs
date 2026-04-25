//! Reusable search-widget UI + state.
//!
//! A `SearchWidget<M>` carries a query string, a list of match handles `M`
//! chosen by the host, and the cursor within that list. The host decides how
//! matches are computed and how to act on them — this module stays oblivious
//! to the domain (commit list, text buffer, anything else). The accompanying
//! `search_bar_view` renders the floating bar overlay (top-right, 5 px pad).
//!
//! For text buffers built on `text::TextCanvasData`, the helper
//! `find_text_matches` walks selectable rows and returns `TextMatch` handles
//! pointing at substring ranges.

use iced::{
    widget::{button, container, row, text, text_input, MouseArea, Space, Stack},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::{assets, theme, widgets::text::TextCanvasData};

/// Offset of the floating bar from the panel's top-right corner.
pub const EDGE_PADDING: f32 = 5.0;

const BAR_HEIGHT: f32 = 36.0;
const BAR_WIDTH: f32 = 380.0;
const BAR_BG: Color = Color {
    r: 0.282,
    g: 0.349,
    b: 0.659,
    a: 1.0,
};
const BAR_BG_HOVER: Color = Color {
    r: 0.322,
    g: 0.400,
    b: 0.745,
    a: 1.0,
};
const INPUT_BG: Color = Color {
    r: 0.224,
    g: 0.278,
    b: 0.525,
    a: 1.0,
};
const INPUT_PLACEHOLDER: Color = Color {
    r: 0.710,
    g: 0.757,
    b: 0.902,
    a: 1.0,
};
const DIVIDER: Color = Color {
    r: 0.710,
    g: 0.757,
    b: 0.902,
    a: 0.35,
};

/// Stable identity for a search widget instance. Each text buffer that can
/// host a search owns one — the id pins the iced `text_input` focus id so
/// the bar stays focused across re-renders without bouncing focus between
/// buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SearchWidgetId(pub &'static str);

pub fn input_id(id: SearchWidgetId) -> iced::widget::Id {
    iced::widget::Id::from(format!("search-widget-{}", id.0))
}

/// Messages emitted by the bar. Generic — the host wraps them in whatever
/// scope/route it needs.
#[derive(Debug, Clone)]
pub enum SearchWidgetMessage {
    InputChanged(String),
    /// Enter pressed in the input. The host reads the current Shift modifier
    /// to decide Next (plain Enter) vs Previous (Shift+Enter); kept separate
    /// because `text_input::on_submit` can't branch on modifiers at widget-
    /// construction time.
    Submit,
    Next,
    Previous,
    Close,
    /// Clicks that land on the bar background — host refocuses the input so a
    /// second click on the bar feels like "bring me back to typing".
    BarClicked,
}

/// Reusable search state. `M` is the host's match handle (e.g. `usize` for a
/// commit list, `TextMatch` for a text buffer).
#[derive(Debug, Clone)]
pub struct SearchWidget<M: Clone> {
    query: String,
    matches: Vec<M>,
    current_match: Option<usize>,
    needs_focus: bool,
    id: SearchWidgetId,
}

impl<M: Clone> SearchWidget<M> {
    pub fn new(id: SearchWidgetId) -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current_match: None,
            needs_focus: true,
            id,
        }
    }

    pub fn id(&self) -> SearchWidgetId {
        self.id
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    pub fn matches(&self) -> &[M] {
        &self.matches
    }
    pub fn current_match_index(&self) -> Option<usize> {
        self.current_match
    }
    pub fn current_match(&self) -> Option<M> {
        self.current_match
            .and_then(|i| self.matches.get(i).cloned())
    }

    pub fn is_dimming_active(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Replace the match list. `initial` picks which match to highlight —
    /// `None` keeps the cursor at 0 (or `None` if empty). Hosts that want
    /// "first match at or after anchor" semantics compute that index
    /// themselves and pass it in.
    pub fn set_matches(&mut self, matches: Vec<M>, initial: Option<usize>) {
        self.matches = matches;
        self.current_match = if self.matches.is_empty() {
            None
        } else {
            Some(initial.unwrap_or(0).min(self.matches.len() - 1))
        };
    }

    pub fn clear_matches(&mut self) {
        self.matches.clear();
        self.current_match = None;
    }

    pub fn go_next(&mut self) -> Option<M> {
        if self.matches.is_empty() {
            return None;
        }
        let next = match self.current_match {
            Some(i) => (i + 1) % self.matches.len(),
            None => 0,
        };
        self.current_match = Some(next);
        self.matches.get(next).cloned()
    }

    pub fn go_previous(&mut self) -> Option<M> {
        if self.matches.is_empty() {
            return None;
        }
        let prev = match self.current_match {
            Some(i) => (i + self.matches.len() - 1) % self.matches.len(),
            None => self.matches.len() - 1,
        };
        self.current_match = Some(prev);
        self.matches.get(prev).cloned()
    }

    pub fn needs_focus(&self) -> bool {
        self.needs_focus
    }
    pub fn take_needs_focus(&mut self) -> bool {
        let was = self.needs_focus;
        self.needs_focus = false;
        was
    }
    pub fn request_focus(&mut self) {
        self.needs_focus = true;
    }
}

/// Render the search bar. `on_message` routes generic widget messages into
/// the host's message type — the host wraps them in whatever scope tag it
/// needs for dispatch.
///
/// `query` does not need to outlive the returned element: `text_input`
/// copies the placeholder / value into its own owned state at construction
/// time, so the borrow ends when this function returns.
pub fn search_bar_view<'a, Msg: Clone + 'a>(
    id: SearchWidgetId,
    query: &str,
    current_idx: Option<usize>,
    total: usize,
    on_message: impl Fn(SearchWidgetMessage) -> Msg + Clone + 'a,
) -> Element<'a, Msg> {
    let magnifier =
        container(assets::icon(assets::SEARCH, 16.0, Color::WHITE)).padding(Padding::from([0, 4]));

    let on_input = on_message.clone();
    let on_submit = on_message.clone();
    let input = text_input("find", query)
        .id(input_id(id))
        .on_input(move |s| on_input(SearchWidgetMessage::InputChanged(s)))
        .on_submit(on_submit(SearchWidgetMessage::Submit))
        .size(theme::FONT_MD)
        .padding(theme::INPUT_PADDING)
        .width(Length::Fill)
        .style(bar_input_style);

    let counter = text(results_label(current_idx, total))
        .size(theme::FONT_SM)
        .style(|_: &Theme| text::Style {
            color: Some(Color::WHITE),
        });

    let up_btn = icon_button(
        assets::ARROW_UP,
        on_message.clone(),
        SearchWidgetMessage::Previous,
    );
    let down_btn = icon_button(
        assets::ARROW_DOWN,
        on_message.clone(),
        SearchWidgetMessage::Next,
    );
    let close_btn = icon_button(
        assets::CLOSE,
        on_message.clone(),
        SearchWidgetMessage::Close,
    );

    let bar_row = row![
        magnifier,
        container(input).width(Length::Fill),
        vertical_divider(),
        container(counter).padding(Padding::from([0, 2])),
        vertical_divider(),
        up_btn,
        down_btn,
        close_btn,
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .padding(Padding::from([0, 8]));

    let bar = container(bar_row)
        .width(Length::Fixed(BAR_WIDTH))
        .height(Length::Fixed(BAR_HEIGHT))
        .align_y(iced::alignment::Vertical::Center)
        .style(|_: &Theme| container::Style {
            background: Some(BAR_BG.into()),
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.35,
                },
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        });

    let bar_clicked = on_message(SearchWidgetMessage::BarClicked);
    let bar = MouseArea::new(bar).on_press(bar_clicked);

    container(bar)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Top)
        .padding(Padding {
            top: EDGE_PADDING,
            right: EDGE_PADDING,
            bottom: 0.0,
            left: 0.0,
        })
        .into()
}

/// Wraps a content element with the search bar as an overlay layer. When
/// `bar` is `None` the overlay becomes a zero-sized `Space` so the Stack
/// structure stays constant across open/close — keeping the child element's
/// own scroll/focus state intact.
pub fn overlay<'a, Msg: Clone + 'a>(
    content: Element<'a, Msg>,
    bar: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let overlay_el: Element<'a, Msg> = match bar {
        Some(b) => b,
        None => Space::new()
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into(),
    };
    Stack::with_children(vec![content, overlay_el])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn results_label(current_idx: Option<usize>, total: usize) -> String {
    if total == 0 {
        return "0 results".to_string();
    }
    let pos = current_idx.map(|i| i + 1).unwrap_or(1);
    format!("{} of {}", pos, total)
}

fn bar_input_style(_: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = if matches!(status, text_input::Status::Focused { .. }) {
        Color {
            a: 0.6,
            ..Color::WHITE
        }
    } else {
        Color::TRANSPARENT
    };
    text_input::Style {
        background: INPUT_BG.into(),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 4.0.into(),
        },
        icon: INPUT_PLACEHOLDER,
        placeholder: INPUT_PLACEHOLDER,
        value: Color::WHITE,
        selection: theme::ACCENT_BLUE,
    }
}

fn icon_button_style(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => Some(BAR_BG_HOVER.into()),
            _ => None,
        },
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        text_color: Color::WHITE,
        shadow: Default::default(),
        snap: false,
    }
}

fn icon_button<'a, Msg: Clone + 'a>(
    icon_bytes: &'static [u8],
    on_message: impl Fn(SearchWidgetMessage) -> Msg + Clone + 'a,
    msg: SearchWidgetMessage,
) -> Element<'a, Msg> {
    button(assets::icon(icon_bytes, 16.0, Color::WHITE))
        .on_press(on_message(msg))
        .padding(Padding::from([4, 6]))
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(28.0))
        .style(icon_button_style)
        .into()
}

fn vertical_divider<'a, Msg: 'a>() -> Element<'a, Msg> {
    container(
        Space::new()
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(20.0)),
    )
    .style(|_: &Theme| container::Style {
        background: Some(DIVIDER.into()),
        ..Default::default()
    })
    .into()
}

/// A match location within a row-based text buffer. `col_end` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMatch {
    pub row: usize,
    pub col_start: usize,
    pub col_end: usize,
}

/// Walks a `TextCanvasData`'s selectable rows and collects every
/// case-insensitive (ASCII-fold) substring match of `needle`. Empty or
/// whitespace-only needles return no matches.
///
/// Char indices are stable under `to_ascii_lowercase` (ASCII-only fold leaves
/// non-ASCII alone), so the returned `col_start`/`col_end` are valid indices
/// into the row's raw text for setting a `TextSelection`.
pub fn find_text_matches(data: &TextCanvasData, needle: &str) -> Vec<TextMatch> {
    let trimmed = needle.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let needle_lc = trimmed.to_ascii_lowercase();
    let needle_chars: Vec<char> = needle_lc.chars().collect();
    let n_len = needle_chars.len();
    let mut out = Vec::new();

    for (row_idx, row) in data.rows().iter().enumerate() {
        if !row.selectable() {
            continue;
        }
        let raw = row.raw_text();
        if raw.is_empty() {
            continue;
        }
        let row_lc = raw.to_ascii_lowercase();
        let row_chars: Vec<char> = row_lc.chars().collect();
        if row_chars.len() < n_len {
            continue;
        }
        let mut i = 0;
        while i + n_len <= row_chars.len() {
            if row_chars[i..i + n_len] == needle_chars[..] {
                out.push(TextMatch {
                    row: row_idx,
                    col_start: i,
                    col_end: i + n_len,
                });
                i += n_len;
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Index of the first match at or after `anchor_row` (wrapping to 0 when
/// none). `None` when `matches` is empty.
pub fn first_match_from_anchor(matches: &[TextMatch], anchor_row: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(
        matches
            .iter()
            .position(|m| m.row >= anchor_row)
            .unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::text::{CanvasRow, TextCanvasData};
    use iced::widget::canvas::Frame;
    use std::sync::Arc;

    #[derive(Debug)]
    struct StubRow(String);

    impl CanvasRow for StubRow {
        fn height(&self) -> f32 {
            20.0
        }
        fn selectable(&self) -> bool {
            true
        }
        fn char_count(&self) -> usize {
            self.0.chars().count()
        }
        fn raw_text(&self) -> String {
            self.0.clone()
        }
        fn draw_gutter(&self, _: &mut Frame, _: f32, _: f32) {}
        fn draw_content(&self, _: &mut Frame, _: f32, _: f32, _: f32) {}
    }

    fn data_from(lines: &[&str]) -> TextCanvasData {
        let rows: Vec<Arc<dyn CanvasRow>> = lines
            .iter()
            .map(|s| Arc::new(StubRow((*s).to_string())) as Arc<dyn CanvasRow>)
            .collect();
        TextCanvasData::from_rows(rows, 1000.0, 8.0, 40.0)
    }

    #[test]
    fn widget_cycles_next_and_previous() {
        let mut w: SearchWidget<usize> = SearchWidget::new(SearchWidgetId("t"));
        w.set_matches(vec![1, 3, 5], Some(0));
        assert_eq!(w.current_match(), Some(1));
        assert_eq!(w.go_next(), Some(3));
        assert_eq!(w.go_next(), Some(5));
        assert_eq!(w.go_next(), Some(1));
        assert_eq!(w.go_previous(), Some(5));
    }

    #[test]
    fn empty_needle_returns_no_matches() {
        let data = data_from(&["hello world"]);
        assert!(find_text_matches(&data, "").is_empty());
        assert!(find_text_matches(&data, "   ").is_empty());
    }

    #[test]
    fn finds_multiple_matches_per_row() {
        let data = data_from(&["foo bar foo", "nothing here"]);
        let matches = find_text_matches(&data, "foo");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].row, 0);
        assert_eq!(matches[0].col_start, 0);
        assert_eq!(matches[0].col_end, 3);
        assert_eq!(matches[1].col_start, 8);
        assert_eq!(matches[1].col_end, 11);
    }

    #[test]
    fn case_insensitive_ascii() {
        let data = data_from(&["Hello World", "HELLO"]);
        let matches = find_text_matches(&data, "hello");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].row, 0);
        assert_eq!(matches[1].row, 1);
    }

    #[test]
    fn first_match_from_anchor_wraps() {
        let matches = vec![
            TextMatch {
                row: 1,
                col_start: 0,
                col_end: 3,
            },
            TextMatch {
                row: 5,
                col_start: 0,
                col_end: 3,
            },
        ];
        assert_eq!(first_match_from_anchor(&matches, 0), Some(0));
        assert_eq!(first_match_from_anchor(&matches, 2), Some(1));
        assert_eq!(first_match_from_anchor(&matches, 10), Some(0));
    }
}
