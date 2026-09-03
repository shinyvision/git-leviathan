//! Media diff view: old → new images, audio clips or videos with a
//! kind-specific toolbar, per-side captions, transport controls and an
//! optional side-by-side properties panel.

use std::sync::Arc;
use std::time::Instant;

use iced::{
    widget::{
        button, column, container, row, scrollable, slider, text, tooltip, MouseArea, Space,
    },
    Alignment, Border, Color, Element, Length, Padding, Theme,
};

use crate::{
    assets,
    message::Message,
    screens::repository::{
        panel_messages::{CenterAction, DiffPanelAction},
        panels::diff::{
            AbsentReason, CompareMode, DifferenceState, MediaAction, MediaDiffState,
            MediaSideState, TransportCommand,
        },
        state::FocusedPanel,
        RepositoryMessage,
    },
    services::media::{
        format_bytes, format_timecode_short, video::VideoPlayer, MediaKind, MediaSide,
    },
    style, theme,
    widgets::{
        media::{
            image_viewer::{
                image_viewer, ImageLayer, ImageViewerEvent, ImageViewerSpec, PaneContent,
            },
            timeline::{timeline, TimelineEvent, TimelineSpec, TransportSource},
            video_surface::{video_surface, VideoSurfaceEvent, VideoSurfaceSpec},
        },
        shared::{h_divider, horizontal_space, v_divider},
    },
};

use super::diff_header;

const TOOL_ICON: f32 = 15.0;
const CONTROL_ICON: f32 = 16.0;
const INFO_PANEL_WIDTH: f32 = 340.0;
const AUDIO_TIMELINE_HEIGHT: f32 = 170.0;
const VIDEO_TIMELINE_HEIGHT: f32 = 40.0;
const ANIMATION_TIMELINE_HEIGHT: f32 = 34.0;
const PLAYBACK_RATES: [f32; 6] = [0.25, 0.5, 1.0, 1.25, 1.5, 2.0];

pub(in crate::screens::repository) struct MediaViewModel<'a> {
    pub(in crate::screens::repository) file_path: &'a str,
    pub(in crate::screens::repository) state: &'a MediaDiffState,
}

fn msg(action: MediaAction) -> Message {
    Message::repo(RepositoryMessage::DiffPanel(DiffPanelAction::Media(action)))
}

fn transport(side: MediaSide, command: TransportCommand) -> Message {
    msg(MediaAction::Transport { side, command })
}

pub(in crate::screens::repository) fn media_center_view<'a>(
    model: MediaViewModel<'a>,
) -> Element<'a, Message> {
    let MediaViewModel { file_path, state } = model;
    let _span = crate::perf::Span::new("ui.media_view")
        .field("path", file_path)
        .field("kind", state.kind.label());

    let kind = state.effective_kind();
    let header = diff_header(file_path, None, Some(kind_badge(kind)));
    let toolbar = toolbar(state, kind);

    let body: Element<'a, Message> = match kind {
        MediaKind::Image => image_body(state),
        MediaKind::Audio => audio_body(state),
        MediaKind::Video => video_body(state),
    };

    let mut content = row![body].spacing(0).height(Length::Fill);
    if state.show_info {
        content = content.push(v_divider()).push(info_panel(state));
    }

    let full = column![header, toolbar, content]
        .spacing(0)
        .height(Length::Fill);

    MouseArea::new(
        container(full)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::panel_container),
    )
    .on_press(Message::repo(RepositoryMessage::Center(
        CenterAction::PanelFocused(FocusedPanel::Center),
    )))
    .into()
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

fn kind_badge<'a>(kind: MediaKind) -> Element<'a, Message> {
    let (icon, label) = match kind {
        MediaKind::Image => (assets::PHOTO, "IMAGE"),
        MediaKind::Audio => (assets::MUSIC, "AUDIO"),
        MediaKind::Video => (assets::MOVIE, "VIDEO"),
    };
    container(
        row![
            assets::icon(icon, 12.0, theme::TEXT_SECONDARY),
            text(label).size(theme::FONT_XS).style(style::secondary_text),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([2, 7]))
    .style(|_: &Theme| container::Style {
        background: Some(theme::BG_BASE.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn tool_button_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_: &Theme, status: button::Status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = if active {
            Some(theme::BG_SELECTED.into())
        } else if hovered {
            Some(theme::BG_HOVER.into())
        } else {
            None
        };
        button::Style {
            background,
            text_color: theme::TEXT_PRIMARY,
            border: Border {
                color: if active { theme::ACCENT_BLUE } else { Color::TRANSPARENT },
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: Default::default(),
            snap: false,
        }
    }
}

fn tooltip_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::BG_HEADER.into()),
        text_color: Some(theme::TEXT_PRIMARY),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn with_tip<'a>(
    content: impl Into<Element<'a, Message>>,
    tip: impl iced::widget::text::IntoFragment<'a>,
    position: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(tip).size(theme::FONT_XS)).padding(Padding::from([3, 7])),
        position,
    )
    .gap(4)
    .style(tooltip_style)
    .into()
}

/// Icon button with tooltip. `on_press: None` renders it disabled.
fn icon_button<'a>(
    icon: &'static [u8],
    tip: &'a str,
    active: bool,
    size: f32,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let color = if on_press.is_none() {
        theme::TEXT_MUTED
    } else if active {
        theme::ACCENT_BLUE
    } else {
        theme::TEXT_SECONDARY
    };
    let mut btn = button(assets::icon(icon, size, color))
        .style(tool_button_style(active))
        .padding(Padding::from([4, 5]));
    if let Some(m) = on_press {
        btn = btn.on_press(m);
    }
    with_tip(btn, tip, tooltip::Position::Bottom)
}

fn text_button<'a>(label: &'a str, tip: &'a str, active: bool, on_press: Option<Message>) -> Element<'a, Message> {
    let color = if on_press.is_none() {
        theme::TEXT_MUTED
    } else if active {
        theme::ACCENT_BLUE
    } else {
        theme::TEXT_SECONDARY
    };
    let mut btn = button(
        text(label)
            .size(theme::FONT_SM)
            .style(move |_: &Theme| text::Style { color: Some(color) }),
    )
    .style(tool_button_style(active))
    .padding(Padding::from([4, 8]));
    if let Some(m) = on_press {
        btn = btn.on_press(m);
    }
    with_tip(btn, tip, tooltip::Position::Bottom)
}

fn toolbar_separator<'a>() -> Element<'a, Message> {
    container(Space::new().width(Length::Fixed(1.0)).height(Length::Fixed(18.0)))
        .style(|_: &Theme| container::Style {
            background: Some(theme::BORDER.into()),
            ..Default::default()
        })
        .padding(Padding::from([0, 4]))
        .into()
}

fn slider_style(_: &Theme, status: slider::Status) -> slider::Style {
    let handle_color = match status {
        slider::Status::Hovered | slider::Status::Dragged => theme::ACCENT_BLUE,
        _ => theme::TEXT_SECONDARY,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (theme::ACCENT_BLUE.into(), theme::BORDER.into()),
            width: 4.0,
            border: Border {
                radius: 2.0.into(),
                ..Default::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 6.0 },
            background: handle_color.into(),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

fn toolbar<'a>(state: &'a MediaDiffState, kind: MediaKind) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = Vec::new();

    match kind {
        MediaKind::Image => {
            let both = state.both_images().is_some();
            let mode = state.image.mode;
            let mode_btn = |icon, tip, m: CompareMode| {
                let enabled = both || !m.needs_both_images();
                icon_button(
                    icon,
                    tip,
                    mode == m,
                    TOOL_ICON,
                    enabled.then(|| msg(MediaAction::SetCompareMode(m))),
                )
            };
            items.push(mode_btn(assets::COLUMNS, "Side by side", CompareMode::SideBySide));
            items.push(mode_btn(assets::SWIPE, "Swipe (drag the divider)", CompareMode::Swipe));
            items.push(mode_btn(assets::LAYERS, "Onion skin (blend old and new)", CompareMode::OnionSkin));
            items.push(mode_btn(assets::CONTRAST, "Difference (highlight changed pixels)", CompareMode::Difference));

            match mode {
                CompareMode::Swipe => {
                    items.push(toolbar_separator());
                    items.push(
                        slider(0.0..=1.0f32, state.image.swipe, |v| msg(MediaAction::SetSwipe(v)))
                            .step(0.005_f32)
                            .width(Length::Fixed(140.0))
                            .style(slider_style)
                            .into(),
                    );
                }
                CompareMode::OnionSkin => {
                    items.push(toolbar_separator());
                    items.push(
                        text("Old").size(theme::FONT_XS).style(style::secondary_text).into(),
                    );
                    items.push(
                        slider(0.0..=1.0f32, state.image.onion, |v| msg(MediaAction::SetOnion(v)))
                            .step(0.01_f32)
                            .width(Length::Fixed(140.0))
                            .style(slider_style)
                            .into(),
                    );
                    items.push(
                        text("New").size(theme::FONT_XS).style(style::secondary_text).into(),
                    );
                }
                _ => {}
            }

            items.push(toolbar_separator());
            let has_image = state.new.image().is_some() || state.old.image().is_some();
            items.push(icon_button(
                assets::ZOOM_OUT,
                "Zoom out (−)",
                false,
                TOOL_ICON,
                has_image.then(|| msg(MediaAction::ZoomOut)),
            ));
            let zoom_label = match state.image.effective_scale {
                Some(s) if s >= 0.1 => format!("{:.0}%", s * 100.0),
                Some(s) => format!("{:.1}%", s * 100.0),
                None => "—".to_string(),
            };
            items.push(
                container(
                    text(zoom_label)
                        .size(theme::FONT_SM)
                        .font(theme::MONO)
                        .style(style::primary_text),
                )
                .width(Length::Fixed(52.0))
                .align_x(iced::alignment::Horizontal::Center)
                .into(),
            );
            items.push(icon_button(
                assets::ZOOM_IN,
                "Zoom in (+)",
                false,
                TOOL_ICON,
                has_image.then(|| msg(MediaAction::ZoomIn)),
            ));
            items.push(icon_button(
                assets::FIT_SCREEN,
                "Fit to view (0)",
                state.image.view.is_fit(),
                TOOL_ICON,
                has_image.then(|| msg(MediaAction::ZoomFit)),
            ));
            items.push(icon_button(
                assets::ACTUAL_SIZE,
                "Actual size 1:1 (1)",
                state.image.view.scale == Some(1.0),
                TOOL_ICON,
                has_image.then(|| msg(MediaAction::ZoomActual)),
            ));
            items.push(toolbar_separator());
            items.push(icon_button(
                if state.image.linked_views {
                    assets::LINK
                } else {
                    assets::UNLINK
                },
                if state.image.linked_views {
                    "Views linked: zoom and pan move both sides"
                } else {
                    "Views unlinked: each side zooms independently"
                },
                state.image.linked_views,
                TOOL_ICON,
                (both && mode == CompareMode::SideBySide)
                    .then(|| msg(MediaAction::ToggleLinkedViews)),
            ));
            items.push(icon_button(
                assets::GRID,
                "Checkerboard behind transparent pixels (c)",
                state.image.checkerboard,
                TOOL_ICON,
                Some(msg(MediaAction::ToggleCheckerboard)),
            ));
            items.push(icon_button(
                assets::PIXEL_GRID,
                "Pixel grid at high zoom (g)",
                state.image.pixel_grid,
                TOOL_ICON,
                Some(msg(MediaAction::TogglePixelGrid)),
            ));
            items.push(text_button(
                "Nearest",
                "Nearest-neighbour sampling (crisp pixels); linear when off",
                state.image.nearest,
                Some(msg(MediaAction::ToggleNearest)),
            ));
            items.push(icon_button(
                assets::EYE_DROPPER,
                "Pixel inspector: hover to read coordinates and colour",
                state.image.inspector,
                TOOL_ICON,
                has_image.then(|| msg(MediaAction::ToggleInspector)),
            ));
        }
        MediaKind::Audio | MediaKind::Video => {
            items.push(icon_button(
                if state.linked_playback {
                    assets::LINK
                } else {
                    assets::UNLINK
                },
                if state.linked_playback {
                    "Playback linked: play, pause and seek both sides together"
                } else {
                    "Playback independent: starting one side pauses the other"
                },
                state.linked_playback,
                TOOL_ICON,
                Some(msg(MediaAction::ToggleLinkedPlayback)),
            ));
            items.push(toolbar_separator());
            let muted = state.muted || state.volume <= 0.0;
            items.push(icon_button(
                if muted { assets::VOLUME_OFF } else { assets::VOLUME },
                if state.muted { "Unmute (m)" } else { "Mute (m)" },
                false,
                TOOL_ICON,
                Some(msg(MediaAction::KeyTransport(TransportCommand::ToggleMute))),
            ));
            items.push(
                slider(0.0..=1.0f32, state.volume, |v| {
                    msg(MediaAction::KeyTransport(TransportCommand::SetVolume(v)))
                })
                .step(0.01_f32)
                .width(Length::Fixed(110.0))
                .style(slider_style)
                .into(),
            );
            items.push(
                container(
                    text(format!("{:.0}%", state.volume * 100.0))
                        .size(theme::FONT_XS)
                        .font(theme::MONO)
                        .style(style::secondary_text),
                )
                .width(Length::Fixed(38.0))
                .into(),
            );
            items.push(icon_button(
                assets::REPEAT,
                if state.looping { "Loop on (l)" } else { "Loop off (l)" },
                state.looping,
                TOOL_ICON,
                Some(msg(MediaAction::KeyTransport(TransportCommand::ToggleLoop))),
            ));
            if kind == MediaKind::Video {
                items.push(toolbar_separator());
                items.push(assets::icon(assets::SPEED, TOOL_ICON, theme::TEXT_SECONDARY));
                for rate in PLAYBACK_RATES {
                    let label = format_rate(rate);
                    let active = (state.rate - rate).abs() < 1e-3;
                    let mut btn = button(
                        text(label)
                            .size(theme::FONT_XS)
                            .font(theme::MONO)
                            .style(move |_: &Theme| text::Style {
                                color: Some(if active {
                                    theme::ACCENT_BLUE
                                } else {
                                    theme::TEXT_SECONDARY
                                }),
                            }),
                    )
                    .style(tool_button_style(active))
                    .padding(Padding::from([3, 5]));
                    btn = btn.on_press(msg(MediaAction::KeyTransport(TransportCommand::SetRate(rate))));
                    items.push(btn.into());
                }
            }
            if let crate::services::media::engine::EngineStatus::Unavailable(reason) =
                crate::services::media::engine::engine().status()
            {
                items.push(toolbar_separator());
                items.push(with_tip(
                    row![
                        assets::icon(assets::WARNING, 13.0, theme::ACCENT_ORANGE),
                        text("No audio output")
                            .size(theme::FONT_XS)
                            .style(|_: &Theme| text::Style {
                                color: Some(theme::ACCENT_ORANGE),
                            }),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                    format!("Playback runs silently: {reason}"),
                    tooltip::Position::Bottom,
                ));
            }
        }
    }

    let mut bar = row(items).spacing(2).align_y(Alignment::Center);
    bar = bar.push(horizontal_space());
    bar = bar.push(icon_button(
        assets::INFO,
        "Properties panel (i)",
        state.show_info,
        TOOL_ICON,
        Some(msg(MediaAction::ToggleInfo)),
    ));
    bar = bar.push(icon_button(
        assets::FILE_TEXT,
        "Show this file as a text diff instead",
        false,
        TOOL_ICON,
        Some(msg(MediaAction::ViewAsText)),
    ));

    container(bar.padding(Padding::from([4, 8])).width(Length::Fill))
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_TOOLBAR.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn format_rate(rate: f32) -> String {
    if (rate - rate.round()).abs() < 1e-3 {
        format!("{}×", rate.round() as i32)
    } else {
        format!("{rate}×")
    }
}

fn caption<'a>(side: MediaSide, summary: String, status: Option<String>, focused: bool) -> Element<'a, Message> {
    let chip = container(
        text(side.label().to_ascii_uppercase())
            .size(theme::FONT_XS)
            .style(move |_: &Theme| text::Style {
                color: Some(if side == MediaSide::Old {
                    theme::ACCENT_RED
                } else {
                    theme::ACCENT_GREEN
                }),
            }),
    )
    .padding(Padding::from([1, 6]))
    .style(move |_: &Theme| container::Style {
        background: Some(
            if side == MediaSide::Old {
                theme::DELETION_BG
            } else {
                theme::ADDITION_BG
            }
            .into(),
        ),
        border: Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut r = row![
        chip,
        text(summary).size(theme::FONT_SM).style(style::secondary_text),
        horizontal_space(),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some(status) = status {
        r = r.push(
            text(status)
                .size(theme::FONT_XS)
                .font(theme::MONO)
                .style(style::dim_text),
        );
    }
    container(r.padding(Padding::from([4, 10])).width(Length::Fill))
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(theme::BG_HEADER.into()),
            border: Border {
                color: if focused { theme::ACCENT_BLUE } else { theme::BORDER },
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn placeholder<'a>(icon: &'static [u8], title: String, detail: String, color: Color) -> Element<'a, Message> {
    container(
        column![
            assets::icon(icon, 34.0, color),
            text(title).size(theme::FONT_LG).style(style::primary_text),
            text(detail)
                .size(theme::FONT_SM)
                .style(style::secondary_text)
                .align_x(iced::alignment::Horizontal::Center),
        ]
        .spacing(8)
        .align_x(Alignment::Center)
        .max_width(380),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center)
    .padding(20)
    .style(|_: &Theme| container::Style {
        background: Some(theme::BG_PANEL.into()),
        ..Default::default()
    })
    .into()
}

fn side_placeholder<'a>(side: &'a MediaSideState) -> Option<Element<'a, Message>> {
    match side {
        MediaSideState::Loading => Some(placeholder(
            assets::REFRESH,
            "Decoding…".to_string(),
            "Reading the file contents from git.".to_string(),
            theme::TEXT_DIM,
        )),
        MediaSideState::Absent(reason) => Some(placeholder(
            match reason {
                AbsentReason::NoPreviousVersion => assets::PLUS,
                AbsentReason::Deleted => assets::MINUS,
            },
            reason.title().to_string(),
            reason.detail().to_string(),
            theme::TEXT_DIM,
        )),
        MediaSideState::Failed { message, size } => Some(placeholder(
            assets::WARNING,
            "Cannot be shown".to_string(),
            match size {
                Some(size) => format!("{message}\n({})", format_bytes(*size)),
                None => message.clone(),
            },
            theme::ACCENT_ORANGE,
        )),
        _ => None,
    }
}

fn side_summary(side: &MediaSideState) -> String {
    match side {
        MediaSideState::Image { image, .. } => {
            let info = &image.info;
            let mut parts = vec![
                format!("{} × {}", info.original_width, info.original_height),
                info.format.clone(),
                format_bytes(info.file_size),
            ];
            if image.is_animated() {
                parts.push(format!("{} frames", image.frame_count()));
            }
            parts.join("  ·  ")
        }
        MediaSideState::Audio(player) => {
            let clip = &player.clip;
            let mut parts = vec![
                format_timecode_short(clip.duration_secs()),
                clip.info.codec.clone(),
                format!("{} Hz", clip.info.source_sample_rate),
                match clip.info.source_channels {
                    1 => "mono".to_string(),
                    2 => "stereo".to_string(),
                    n => format!("{n} ch"),
                },
                format_bytes(clip.info.file_size),
            ];
            if clip.truncated {
                parts.push("preview truncated".to_string());
            }
            parts.join("  ·  ")
        }
        MediaSideState::Video(player) => {
            let info = player.info();
            [
                format_timecode_short(info.duration_secs),
                format!("{} × {}", info.width, info.height),
                format!("{:.3} fps", info.fps),
                info.codec.clone(),
                format_bytes(info.file_size),
            ]
            .join("  ·  ")
        }
        MediaSideState::Loading => "Loading…".to_string(),
        MediaSideState::Absent(_) => "—".to_string(),
        MediaSideState::Failed { size, .. } => size.map(format_bytes).unwrap_or_else(|| "—".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

fn image_body<'a>(state: &'a MediaDiffState) -> Element<'a, Message> {
    let mode = state.image.mode;
    match (mode, state.both_images()) {
        (CompareMode::SideBySide, _) | (_, None) => row![
            image_pane(state, MediaSide::Old),
            v_divider(),
            image_pane(state, MediaSide::New),
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        (CompareMode::Swipe, Some((old, new))) => {
            let content = PaneContent::Swipe {
                base: layer_for(state, MediaSide::Old, old),
                overlay: layer_for(state, MediaSide::New, new),
                position: state.image.swipe,
            };
            overlay_pane(
                state,
                content,
                format!(
                    "Old {}   |   New {}",
                    side_summary(&state.old),
                    side_summary(&state.new)
                ),
                Some("drag the divider".to_string()),
            )
        }
        (CompareMode::OnionSkin, Some((old, new))) => {
            let content = PaneContent::Onion {
                base: layer_for(state, MediaSide::Old, old),
                overlay: layer_for(state, MediaSide::New, new),
                opacity: state.image.onion,
            };
            overlay_pane(
                state,
                content,
                format!(
                    "Old {}   |   New {}",
                    side_summary(&state.old),
                    side_summary(&state.new)
                ),
                Some(format!("new at {:.0}%", state.image.onion * 100.0)),
            )
        }
        (CompareMode::Difference, Some(_)) => match &state.image.difference {
            DifferenceState::Ready(diff) => {
                let content = PaneContent::Difference {
                    handle: diff.handle.clone(),
                    width: diff.width,
                    height: diff.height,
                };
                let status = if diff.changed_pixels == 0 {
                    "identical pixels".to_string()
                } else {
                    let bounds = diff
                        .changed_bounds
                        .map(|(x, y, w, h)| format!("  ·  region {w} × {h} at ({x}, {y})"))
                        .unwrap_or_default();
                    format!(
                        "{:.2}% changed ({} px, max Δ {}){bounds}",
                        diff.changed_ratio() * 100.0,
                        diff.changed_pixels,
                        diff.max_delta
                    )
                };
                overlay_pane(
                    state,
                    content,
                    "Changed pixels highlighted yellow → red by magnitude".to_string(),
                    Some(status),
                )
            }
            DifferenceState::Computing | DifferenceState::NotComputed => placeholder(
                assets::CONTRAST,
                "Computing difference…".to_string(),
                "Comparing every pixel of both versions.".to_string(),
                theme::TEXT_DIM,
            ),
            DifferenceState::Failed(reason) => placeholder(
                assets::CONTRAST,
                "Difference unavailable".to_string(),
                reason.clone(),
                theme::ACCENT_ORANGE,
            ),
        },
    }
}

fn layer_for(
    state: &MediaDiffState,
    side: MediaSide,
    image: &Arc<crate::services::media::image::DecodedImage>,
) -> ImageLayer {
    let playback = match state.side(side) {
        MediaSideState::Image { playback, .. } => *playback,
        _ => Default::default(),
    };
    ImageLayer {
        image: image.clone(),
        playback,
    }
}

fn viewer_for<'a>(state: &'a MediaDiffState, side: MediaSide, content: PaneContent) -> Element<'a, Message> {
    let view = state.view_for(side);
    image_viewer(ImageViewerSpec {
        content,
        view,
        checkerboard: state.image.checkerboard,
        nearest: state.image.nearest,
        pixel_grid: state.image.pixel_grid,
        inspector: state.image.inspector,
        on_event: Box::new(move |event: ImageViewerEvent| msg(MediaAction::Viewer { side, event })),
    })
}

fn image_pane<'a>(state: &'a MediaDiffState, side: MediaSide) -> Element<'a, Message> {
    let side_state = state.side(side);
    let focused = state.focused_side == side && state.any_playing();
    let mut col = column![].spacing(0).width(Length::Fill).height(Length::Fill);

    match side_state {
        MediaSideState::Image { image, playback } => {
            let status = image.is_animated().then(|| {
                let frame = playback.frame_at(image, Instant::now());
                format!("frame {} / {}", frame + 1, image.frame_count())
            });
            col = col.push(caption(side, side_summary(side_state), status, focused));
            let content = PaneContent::Single(ImageLayer {
                image: image.clone(),
                playback: *playback,
            });
            col = col.push(
                container(viewer_for(state, side, content))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_: &Theme| container::Style {
                        background: Some(theme::BG_PANEL.into()),
                        ..Default::default()
                    }),
            );
            if image.is_animated() {
                col = col.push(animation_controls(side, image, *playback));
            }
        }
        other => {
            col = col.push(caption(side, side_summary(other), None, false));
            if let Some(ph) = side_placeholder(other) {
                col = col.push(ph);
            }
        }
    }

    MouseArea::new(col)
        .on_press(msg(MediaAction::FocusSide(side)))
        .into()
}

fn overlay_pane<'a>(
    state: &'a MediaDiffState,
    content: PaneContent,
    summary: String,
    status: Option<String>,
) -> Element<'a, Message> {
    let mut col = column![caption(MediaSide::New, summary, status, false)]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill);
    col = col.push(
        container(viewer_for(state, MediaSide::New, content))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::BG_PANEL.into()),
                ..Default::default()
            }),
    );
    // Animated overlays: drive both sides from the new side's controls.
    if let (
        MediaSideState::Image { image, playback },
        _,
    ) = (&state.new, &state.old)
    {
        if image.is_animated() {
            col = col.push(animation_controls(MediaSide::New, image, *playback));
        }
    }
    col.into()
}

fn animation_controls<'a>(
    side: MediaSide,
    image: &'a Arc<crate::services::media::image::DecodedImage>,
    playback: crate::widgets::media::image_viewer::AnimationPlayback,
) -> Element<'a, Message> {
    let playing = playback.playing && playback.anchor.is_some();
    let source: Arc<dyn TransportSource> = Arc::new(super::super::media::AnimationTransport {
        image: image.clone(),
        playback,
    });
    let controls = row![
        icon_button(
            if playing { assets::PAUSE } else { assets::PLAY },
            if playing { "Pause (space)" } else { "Play (space)" },
            false,
            CONTROL_ICON,
            Some(transport(side, TransportCommand::TogglePlay)),
        ),
        icon_button(
            assets::FRAME_PREV,
            "Previous frame (,)",
            false,
            CONTROL_ICON,
            Some(transport(side, TransportCommand::StepFrame(-1))),
        ),
        icon_button(
            assets::FRAME_NEXT,
            "Next frame (.)",
            false,
            CONTROL_ICON,
            Some(transport(side, TransportCommand::StepFrame(1))),
        ),
        timeline(TimelineSpec {
            source,
            waveform: None,
            height: ANIMATION_TIMELINE_HEIGHT,
            accent: theme::ACCENT_BLUE,
            on_event: Box::new(move |event: TimelineEvent| msg(MediaAction::Timeline { side, event })),
        }),
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    container(controls.padding(Padding::from([2, 6])))
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_HEADER.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

fn audio_body<'a>(state: &'a MediaDiffState) -> Element<'a, Message> {
    column![
        audio_block(state, MediaSide::Old),
        h_divider(),
        audio_block(state, MediaSide::New),
    ]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn audio_block<'a>(state: &'a MediaDiffState, side: MediaSide) -> Element<'a, Message> {
    let side_state = state.side(side);
    let focused = state.focused_side == side && state.any_playing();
    let mut col = column![].spacing(0).width(Length::Fill).height(Length::Fill);
    match side_state {
        MediaSideState::Audio(player) => {
            let status = if player.voice.is_playing() {
                Some("playing".to_string())
            } else if player.voice.has_ended() {
                Some("ended".to_string())
            } else {
                None
            };
            col = col.push(caption(side, side_summary(side_state), status, focused));
            let waveform = Arc::new(player.clip.waveform.clone());
            let source: Arc<dyn TransportSource> = player.clone();
            let tl = timeline(TimelineSpec {
                source,
                waveform: Some(waveform),
                height: AUDIO_TIMELINE_HEIGHT,
                accent: if side == MediaSide::Old {
                    theme::ACCENT_RED
                } else {
                    theme::ACCENT_GREEN
                },
                on_event: Box::new(move |event: TimelineEvent| msg(MediaAction::Timeline { side, event })),
            });
            col = col.push(
                container(
                    column![tl, transport_controls(side, player.voice.is_playing(), false)]
                        .spacing(4)
                        .width(Length::Fill),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Center)
                .padding(Padding::from([8, 12]))
                .style(|_: &Theme| container::Style {
                    background: Some(theme::BG_PANEL.into()),
                    ..Default::default()
                }),
            );
        }
        other => {
            col = col.push(caption(side, side_summary(other), None, false));
            if let Some(ph) = side_placeholder(other) {
                col = col.push(ph);
            }
        }
    }
    MouseArea::new(col)
        .on_press(msg(MediaAction::FocusSide(side)))
        .into()
}

fn transport_controls<'a>(side: MediaSide, playing: bool, with_frame_step: bool) -> Element<'a, Message> {
    let mut r = row![
        icon_button(
            assets::SKIP_BACK,
            "Back 5 s (←)",
            false,
            CONTROL_ICON,
            Some(transport(side, TransportCommand::SeekRelative(-super::super::media::SEEK_STEP_SECS))),
        ),
        icon_button(
            if playing { assets::PAUSE } else { assets::PLAY },
            if playing { "Pause (space)" } else { "Play (space)" },
            playing,
            CONTROL_ICON + 4.0,
            Some(transport(side, TransportCommand::TogglePlay)),
        ),
        icon_button(
            assets::SKIP_FORWARD,
            "Forward 5 s (→)",
            false,
            CONTROL_ICON,
            Some(transport(side, TransportCommand::SeekRelative(super::super::media::SEEK_STEP_SECS))),
        ),
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    if with_frame_step {
        r = r.push(toolbar_separator());
        r = r.push(icon_button(
            assets::FRAME_PREV,
            "Previous frame (,)",
            false,
            CONTROL_ICON,
            Some(transport(side, TransportCommand::StepFrame(-1))),
        ));
        r = r.push(icon_button(
            assets::FRAME_NEXT,
            "Next frame (.)",
            false,
            CONTROL_ICON,
            Some(transport(side, TransportCommand::StepFrame(1))),
        ));
    }
    r = r.push(horizontal_space());
    r = r.push(text_button(
        "Stop",
        "Stop and return to the start (Home)",
        false,
        Some(transport(side, TransportCommand::Stop)),
    ));
    container(r).width(Length::Fill).into()
}

// ---------------------------------------------------------------------------
// Video
// ---------------------------------------------------------------------------

fn video_body<'a>(state: &'a MediaDiffState) -> Element<'a, Message> {
    row![
        video_pane(state, MediaSide::Old),
        v_divider(),
        video_pane(state, MediaSide::New),
    ]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn video_pane<'a>(state: &'a MediaDiffState, side: MediaSide) -> Element<'a, Message> {
    let side_state = state.side(side);
    let focused = state.focused_side == side && state.any_playing();
    let mut col = column![].spacing(0).width(Length::Fill).height(Length::Fill);
    match side_state {
        MediaSideState::Video(player) => {
            let status = if player.is_buffering() {
                Some("buffering…".to_string())
            } else if player.is_playing() {
                Some(format!("playing {}", format_rate(state.rate)))
            } else if player.has_ended() {
                Some("ended".to_string())
            } else {
                None
            };
            col = col.push(caption(side, side_summary(side_state), status, focused));
            let surface_player: Arc<VideoPlayer> = player.clone();
            col = col.push(
                container(video_surface(VideoSurfaceSpec {
                    player: surface_player,
                    on_event: Box::new(move |event: VideoSurfaceEvent| {
                        msg(MediaAction::Surface { side, event })
                    }),
                }))
                .width(Length::Fill)
                .height(Length::Fill),
            );
            let source: Arc<dyn TransportSource> = player.clone();
            let tl = timeline(TimelineSpec {
                source,
                waveform: None,
                height: VIDEO_TIMELINE_HEIGHT,
                accent: if side == MediaSide::Old {
                    theme::ACCENT_RED
                } else {
                    theme::ACCENT_GREEN
                },
                on_event: Box::new(move |event: TimelineEvent| msg(MediaAction::Timeline { side, event })),
            });
            col = col.push(
                container(
                    column![tl, transport_controls(side, player.is_playing(), true)]
                        .spacing(2)
                        .width(Length::Fill),
                )
                .width(Length::Fill)
                .padding(Padding::from([4, 8]))
                .style(|_: &Theme| container::Style {
                    background: Some(theme::BG_HEADER.into()),
                    border: Border {
                        color: theme::BORDER,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
            );
        }
        other => {
            col = col.push(caption(side, side_summary(other), None, false));
            if let Some(ph) = side_placeholder(other) {
                col = col.push(ph);
            }
        }
    }
    MouseArea::new(col)
        .on_press(msg(MediaAction::FocusSide(side)))
        .into()
}

// ---------------------------------------------------------------------------
// Properties panel
// ---------------------------------------------------------------------------

fn info_panel<'a>(state: &'a MediaDiffState) -> Element<'a, Message> {
    let old_props = state.old.properties();
    let new_props = state.new.properties();

    // Union of keys, new-side order first, then old-only keys.
    let mut keys: Vec<String> = new_props.iter().map(|(k, _)| k.clone()).collect();
    for (k, _) in &old_props {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    let lookup = |props: &[(String, String)], key: &str| -> Option<String> {
        props.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };

    let mut rows: Vec<Element<'a, Message>> = Vec::with_capacity(keys.len() + 1);
    rows.push(
        row![
            text("Property").size(theme::FONT_XS).style(style::dim_text).width(Length::FillPortion(3)),
            text("Old").size(theme::FONT_XS).style(style::dim_text).width(Length::FillPortion(4)),
            text("New").size(theme::FONT_XS).style(style::dim_text).width(Length::FillPortion(4)),
        ]
        .spacing(6)
        .padding(Padding::from([2, 0]))
        .into(),
    );
    for (idx, key) in keys.iter().enumerate() {
        let old_v = lookup(&old_props, key);
        let new_v = lookup(&new_props, key);
        let changed = old_v.is_some() && new_v.is_some() && old_v != new_v;
        let cell = |value: Option<String>, highlight: bool| {
            let color = if highlight {
                theme::ACCENT_ORANGE
            } else if value.is_none() {
                theme::TEXT_MUTED
            } else {
                theme::TEXT_PRIMARY
            };
            text(value.unwrap_or_else(|| "—".to_string()))
                .size(theme::FONT_XS)
                .style(move |_: &Theme| text::Style { color: Some(color) })
                .width(Length::FillPortion(4))
        };
        let line = row![
            text(key.clone())
                .size(theme::FONT_XS)
                .style(style::secondary_text)
                .width(Length::FillPortion(3)),
            cell(old_v, changed),
            cell(new_v, changed),
        ]
        .spacing(6)
        .padding(Padding::from([3, 4]));
        let zebra = idx % 2 == 1;
        rows.push(
            container(line)
                .width(Length::Fill)
                .style(move |_: &Theme| container::Style {
                    background: if changed {
                        Some(
                            Color {
                                a: 0.10,
                                ..theme::ACCENT_ORANGE
                            }
                            .into(),
                        )
                    } else if zebra {
                        Some(theme::BG_BASE.into())
                    } else {
                        None
                    },
                    ..Default::default()
                })
                .into(),
        );
    }
    if keys.is_empty() {
        rows.push(
            text("No properties available yet.")
                .size(theme::FONT_SM)
                .style(style::dim_text)
                .into(),
        );
    }

    let title = row![
        assets::icon(assets::INFO, 13.0, theme::TEXT_SECONDARY),
        text("Properties").size(theme::FONT_SM).style(style::primary_text),
        horizontal_space(),
        icon_button(
            assets::CLOSE,
            "Close properties",
            false,
            13.0,
            Some(msg(MediaAction::ToggleInfo)),
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let list = scrollable(column(rows).spacing(0).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(crate::widgets::shared::scrollbar_style);

    container(
        column![title, list]
            .spacing(6)
            .padding(Padding::from([6, 10]))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fixed(INFO_PANEL_WIDTH))
    .height(Length::Fill)
    .style(|_: &Theme| container::Style {
        background: Some(theme::BG_SIDEBAR.into()),
        ..Default::default()
    })
    .into()
}

