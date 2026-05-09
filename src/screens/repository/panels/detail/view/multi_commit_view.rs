//! Multi-commit merged-diff panel.
//!
//! Shown when 2+ commits are selected: lists the selected commits in a
//! scrollable top section and renders the merged diff's file list below.

use iced::{
    widget::{column, container, responsive, row, scrollable, text},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::{
    assets,
    core::Commit,
    message::Message,
    services::git::MergedCommitDiffResult,
    services::text_measurement::{cached_measure_width, FontFamily},
    style, theme,
    utils::initials,
    widgets::shared::{h_divider, horizontal_space, scrollbar_style, v_divider},
};

use super::super::state::DetailViewModel;
use super::super::DetailOrientation;
use super::{file_list_status, file_list_view, FileListClickTarget};
use crate::screens::repository::panel_messages::DetailFileListKind;

pub(super) fn multi_commit_detail_panel_content<'a>(
    screen: DetailViewModel<'a>,
) -> Element<'a, Message> {
    if matches!(screen.orientation, DetailOrientation::Horizontal) {
        return multi_commit_detail_panel_horizontal(screen);
    }

    let width = screen.width;
    let count = screen.multi_commits.len();
    let multi_commits = screen.multi_commits;
    let merged_diff = screen.merged_diff;
    let active_diff_file_path = screen.active_diff_file_path;

    let multi_commit_row_height: f32 = 42.0;
    let commit_list_natural_height = count as f32 * multi_commit_row_height;

    let commit_and_file_sections = responsive(move |size| {
        let max_commit_height = size.height / 3.0;
        let commit_height = commit_list_natural_height.min(max_commit_height);
        let remaining_height = (size.height - commit_height).max(0.0);

        let commit_list =
            scrollable_commit_rows(&multi_commits, size.width, Length::Fixed(commit_height));

        let cl_container = container(commit_list)
            .width(Length::Fill)
            .padding(Padding::from([0, 10]));

        let (stats_elem, files_elem) = merged_stats_and_files(
            merged_diff,
            active_diff_file_path,
            screen.merged_file_list_scroll_y,
        );

        column![
            cl_container,
            h_divider(),
            stats_elem,
            h_divider(),
            container(files_elem).height(Length::Fixed(remaining_height)),
        ]
        .spacing(0)
        .height(Length::Fill)
        .into()
    });

    let detail_col = column![
        count_bar(count),
        h_divider(),
        heading(count),
        h_divider(),
        commit_and_file_sections,
    ]
    .spacing(0)
    .height(Length::Fill);

    container(detail_col)
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(style::panel_container)
        .into()
}

fn multi_commit_detail_panel_horizontal<'a>(screen: DetailViewModel<'a>) -> Element<'a, Message> {
    let count = screen.multi_commits.len();
    let multi_commits = screen.multi_commits;
    let merged_diff = screen.merged_diff;
    let active_diff_file_path = screen.active_diff_file_path;

    let commit_list =
        responsive(move |size| scrollable_commit_rows(&multi_commits, size.width, Length::Fill));

    let commit_list_container = container(commit_list)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::from([0, 10]));

    let left_col = column![
        count_bar(count),
        h_divider(),
        heading(count),
        h_divider(),
        commit_list_container
    ]
    .spacing(0)
    .height(Length::Fill)
    .width(Length::FillPortion(3));

    let (stats_elem, files_elem) = merged_stats_and_files(
        merged_diff,
        active_diff_file_path,
        screen.merged_file_list_scroll_y,
    );

    let right_col = column![stats_elem, h_divider(), files_elem]
        .spacing(0)
        .height(Length::Fill)
        .width(Length::FillPortion(2));

    container(row![left_col, v_divider(), right_col].spacing(0))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::panel_container)
        .into()
}

fn count_bar<'a>(count: usize) -> Element<'a, Message> {
    container(
        text(format!("{} commits selected", count))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
    )
    .padding(Padding::from([6, 10]))
    .width(Length::Fill)
    .align_x(iced::alignment::Horizontal::Center)
    .into()
}

fn heading<'a>(count: usize) -> Element<'a, Message> {
    container(
        text(format!("Viewing merged diff of {} commits", count))
            .size(theme::FONT_MD)
            .style(style::primary_text),
    )
    .padding(Padding {
        top: 10.0,
        right: 10.0,
        bottom: 6.0,
        left: 10.0,
    })
    .into()
}

fn scrollable_commit_rows<'a>(
    multi_commits: &[&'a Commit],
    available_width: f32,
    height: Length,
) -> Element<'a, Message> {
    let hide_meta = available_width < 400.0;
    // Row overhead: outer container padding (10 left + 20 right, incl. scrollbar gap)
    //   + row padding (4 + 4) + spacing between 4 children (8 * 3)
    //   + avatar (24) + fixed hash column estimate (60).
    let summary_avail = (available_width - 5.0 - 8.0 - 24.0 - 24.0 - 60.0 - 5.0).max(50.0);
    let commit_rows: Vec<Element<Message>> = multi_commits
        .iter()
        .map(|commit| multi_commit_row(commit, hide_meta, summary_avail))
        .collect();

    scrollable(
        container(column(commit_rows).spacing(0).width(Length::Fill))
            .width(Length::Fill)
            .padding(Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 0.0,
            }),
    )
    .height(height)
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::new().width(5).scroller_width(5),
    ))
    .style(scrollbar_style)
    .into()
}

fn merged_stats_and_files<'a>(
    merged_diff: Option<&'a MergedCommitDiffResult>,
    active_diff_file_path: Option<&'a str>,
    scroll_y: f32,
) -> (Element<'a, Message>, Element<'a, Message>) {
    let Some(merged) = merged_diff else {
        let loading = container(
            text("Loading merged diff…")
                .size(theme::FONT_SM)
                .style(style::dim_text),
        )
        .padding(Padding::from([6, 10]))
        .width(Length::Fill)
        .height(Length::Fill);

        return (
            container(horizontal_space())
                .height(Length::Fixed(0.0))
                .into(),
            loading.into(),
        );
    };

    let stats = row![
        assets::sidebar_icon(assets::PENCIL, theme::TEXT_SECONDARY),
        text(format!("{} modified", merged.modified_count))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
        text(format!("  + {} added", merged.added_count))
            .size(theme::FONT_SM)
            .style(|_: &Theme| text::Style {
                color: Some(theme::ACCENT_GREEN),
            }),
        horizontal_space(),
    ]
    .padding(Padding::from([6, 10]))
    .spacing(4)
    .align_y(iced::Alignment::Center);

    let files = if merged.files.is_empty() {
        file_list_status("No file changes")
    } else {
        file_list_view(
            &merged.files,
            FileListClickTarget::Merged,
            DetailFileListKind::Merged,
            scroll_y,
            active_diff_file_path,
        )
    };

    (stats.into(), files)
}

fn truncate_summary(message: &str, available_width: f32) -> String {
    let font_size = theme::FONT_SM;
    let full_width = cached_measure_width(message, FontFamily::Default, font_size);
    if full_width <= available_width {
        return message.to_string();
    }
    let ellipsis_width = cached_measure_width("…", FontFamily::Default, font_size);
    let available_for_text = available_width - ellipsis_width;
    if available_for_text <= 0.0 {
        return "…".to_string();
    }
    let mut low = 0usize;
    let mut high = message.len();
    let mut best_byte = 0usize;
    while low < high {
        let mid = (low + high).div_ceil(2);
        let mut boundary = mid.min(message.len());
        while boundary > 0 && !message.is_char_boundary(boundary) {
            boundary -= 1;
        }
        let substr = &message[..boundary];
        let w = cached_measure_width(substr, FontFamily::Default, font_size);
        if w <= available_for_text {
            best_byte = boundary;
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if best_byte == 0 {
        "…".to_string()
    } else {
        format!("{}…", &message[..best_byte])
    }
}

fn multi_commit_row<'a>(
    commit: &'a Commit,
    hide_meta: bool,
    summary_avail: f32,
) -> Element<'a, Message> {
    let avatar = container(
        text(initials(&commit.author))
            .size(theme::FONT_XS)
            .style(style::white_text),
    )
    .width(Length::Fixed(24.0))
    .height(Length::Fixed(24.0))
    .style(|_: &Theme| container::Style {
        background: Some(Color::from_rgb(0.4, 0.3, 0.7).into()),
        border: Border {
            radius: 12.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center);

    let (summary_raw, _) =
        crate::screens::repository::panels::detail::split_commit_message(&commit.message);
    let summary = truncate_summary(&summary_raw, summary_avail);

    let middle: Element<'a, Message> = if hide_meta {
        let date_only = commit
            .date
            .split(" @ ")
            .next()
            .unwrap_or(&commit.date)
            .to_string();
        column![
            text(summary)
                .size(theme::FONT_SM)
                .style(style::primary_text),
            text(date_only)
                .size(theme::FONT_XS)
                .style(style::secondary_text),
        ]
        .spacing(2)
        .into()
    } else {
        column![
            text(summary)
                .size(theme::FONT_SM)
                .style(style::primary_text),
            text(format!("{} by {}", commit.date, commit.author))
                .size(theme::FONT_XS)
                .style(style::secondary_text),
        ]
        .spacing(2)
        .into()
    };

    let hash = text(commit.short_hash.clone())
        .size(theme::FONT_XS)
        .style(|_: &Theme| text::Style {
            color: Some(theme::ACCENT_BLUE),
        });

    row![avatar, middle, horizontal_space(), hash]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .padding(Padding::from([6, 4]))
        .into()
}
