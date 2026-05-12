use iced::{
    alignment,
    widget::{column, container, responsive, row, text, Space},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::{
    message::Message,
    plugin::keymap::{render_chord, KeymapPrefixHint, Keystroke},
    theme,
};

use super::App;

const OVERLAY_MAX_HEIGHT: f32 = 256.0;
const HEADER_HEIGHT: f32 = 34.0;
const FOOTER_HEIGHT: f32 = 22.0;
const ROW_HEIGHT: f32 = 28.0;
const PANEL_PAD_Y: f32 = 10.0;
const MIN_COLUMN_WIDTH: f32 = 280.0;
const MAX_COLUMNS: usize = 5;

#[derive(Debug, Default)]
pub(super) struct KeyChordState {
    context: Option<String>,
    buffer: Vec<Keystroke>,
}

impl KeyChordState {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn is_active(&self) -> bool {
        !self.buffer.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.context = None;
        self.buffer.clear();
    }

    pub(super) fn reset_for_context(&mut self, context: String) {
        self.context = Some(context);
        self.buffer.clear();
    }

    pub(super) fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    pub(super) fn buffer(&self) -> &[Keystroke] {
        &self.buffer
    }

    pub(super) fn push(&mut self, stroke: Keystroke) {
        self.buffer.push(stroke);
    }

    pub(super) fn replace_with(&mut self, context: String, stroke: Keystroke) {
        self.context = Some(context);
        self.buffer.clear();
        self.buffer.push(stroke);
    }
}

pub(super) fn layer(app: &App) -> Option<Element<'static, Message>> {
    if !app.key_chord.is_active() {
        return None;
    }
    let context = app.key_chord.context()?.to_string();
    let prefix = app.key_chord.buffer().to_vec();
    let prefix_label = render_chord(&prefix);
    let hints = app
        .plugin_host
        .keymap_registry()
        .borrow()
        .prefix_hints(&context, &prefix);
    if hints.is_empty() {
        return None;
    }

    Some(render_overlay(prefix_label, context, hints))
}

fn render_overlay(
    prefix: String,
    context: String,
    hints: Vec<KeymapPrefixHint>,
) -> Element<'static, Message> {
    responsive(move |size| {
        let columns = ((size.width - 32.0) / MIN_COLUMN_WIDTH)
            .floor()
            .max(1.0)
            .min(MAX_COLUMNS as f32) as usize;
        let rows_per_column =
            ((OVERLAY_MAX_HEIGHT - HEADER_HEIGHT - FOOTER_HEIGHT - PANEL_PAD_Y * 2.0) / ROW_HEIGHT)
                .floor()
                .max(1.0) as usize;
        let capacity = rows_per_column.saturating_mul(columns).max(1);
        let visible_count = hints.len().min(capacity);
        let hidden_count = hints.len().saturating_sub(visible_count);

        let mut cols: Vec<Element<Message>> = Vec::new();
        for chunk in hints[..visible_count].chunks(rows_per_column) {
            let rows = chunk
                .iter()
                .cloned()
                .map(hint_row)
                .collect::<Vec<Element<Message>>>();
            cols.push(column(rows).spacing(0).width(Length::FillPortion(1)).into());
        }

        let header = row![
            label_text("prefix", theme::TEXT_DIM),
            label_text(&prefix, theme::ACCENT_ORANGE),
            label_text("context", theme::TEXT_DIM),
            label_text(&context, theme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            label_text("Esc close", theme::TEXT_DIM),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center)
        .height(Length::Fixed(HEADER_HEIGHT));

        let footer = row![
            if hidden_count > 0 {
                label_text(&format!("+{hidden_count} hidden"), theme::TEXT_SECONDARY)
            } else {
                label_text("", theme::TEXT_DIM)
            },
            Space::new().width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center)
        .height(Length::Fixed(FOOTER_HEIGHT));

        let body = row(cols).spacing(20);

        let panel = container(
            column![header, body, footer]
                .spacing(0)
                .padding(Padding {
                    top: PANEL_PAD_Y,
                    right: 16.0,
                    bottom: PANEL_PAD_Y,
                    left: 16.0,
                })
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .max_height(OVERLAY_MAX_HEIGHT)
        .clip(true)
        .style(panel_style);

        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(alignment::Vertical::Bottom)
            .into()
    })
    .into()
}

fn hint_row(hint: KeymapPrefixHint) -> Element<'static, Message> {
    let key_color = if hint.is_group {
        theme::ACCENT_BLUE
    } else {
        theme::ACCENT_ORANGE
    };
    let description = if hint.description.is_empty() {
        hint.command.clone()
    } else {
        hint.description.clone()
    };
    let meta = hint_meta(&hint);

    row![
        container(mono_text(&hint.key, theme::TEXT_PRIMARY))
            .width(Length::Fixed(56.0))
            .align_x(alignment::Horizontal::Right),
        mono_text(">", theme::TEXT_DIM),
        container(mono_text(&description, key_color)).width(Length::FillPortion(3)),
        container(mono_text(&meta, theme::TEXT_DIM)).width(Length::FillPortion(2)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .height(Length::Fixed(ROW_HEIGHT))
    .into()
}

fn hint_meta(hint: &KeymapPrefixHint) -> String {
    match (hint.child_count, hint.command.is_empty()) {
        (0, false) if !hint.plugin_id.is_empty() => format!("{} {}", hint.plugin_id, hint.command),
        (0, false) => hint.command.clone(),
        (1, false) => format!("{} +1", hint.command),
        (n, false) => format!("{} +{n}", hint.command),
        (1, true) => "+1 keymap".to_string(),
        (n, true) => format!("+{n} keymaps"),
    }
}

fn label_text(value: &str, color: Color) -> Element<'static, Message> {
    mono_text(value, color)
}

fn mono_text(value: &str, color: Color) -> Element<'static, Message> {
    text(value.to_string())
        .font(theme::MONO)
        .size(theme::FONT_SM)
        .color(color)
        .into()
}

fn panel_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::BG_HEADER.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        shadow: iced::Shadow {
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.40,
            },
            offset: iced::Vector::new(0.0, -2.0),
            blur_radius: 10.0,
        },
        ..Default::default()
    }
}
