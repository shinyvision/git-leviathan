//! `canvas::Program` implementation for the commit graph.
//!
//! Handles tile cache invalidation, selection-cache sync, mouse hit testing,
//! and viewport-driven tile re-draw. Drawing primitives live in
//! `rendering.rs`; cache state lives in `cache.rs`.
use iced::{
    mouse,
    widget::canvas::{self, Canvas, Geometry, Program},
    Element, Length, Rectangle, Renderer, Theme,
};

use crate::core::RepoVersion;
use crate::message::Message;
use crate::screens::repository::{panel_messages::CenterAction, RepositoryMessage};
use crate::theme::ROW_H;
use crate::view_model::GraphRow;

use super::cache::{FullGraphState, TILE_ROWS};
use super::rendering::{draw_graph_rows, draw_selected_rows};

#[derive(Debug, Clone)]
pub struct FullGraphProgram<'a> {
    pub rows: &'a [GraphRow],
    pub selected_indices: Vec<usize>,
    pub graph_revision: RepoVersion,
    pub visible_row_start: usize,
    pub visible_row_end: usize,
}

impl Program<Message> for FullGraphProgram<'_> {
    type State = FullGraphState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let row_count = self.rows.len();
        let selected_in_range: Vec<usize> = self
            .selected_indices
            .iter()
            .copied()
            .filter(|&idx| idx < row_count)
            .collect();

        if state.last_graph_revision.get() != self.graph_revision {
            state.tiles.borrow_mut().iter().for_each(|t| t.clear());
            state.selection_cache.clear();
            state.last_graph_revision.set(self.graph_revision);
        }

        let prev_row_count = state.last_row_count.get();
        if row_count < prev_row_count {
            let needed_tiles = row_count.div_ceil(TILE_ROWS);
            let mut tiles = state.tiles.borrow_mut();
            tiles.truncate(needed_tiles);
            for t in tiles.iter() {
                t.clear();
            }
        }
        state.last_row_count.set(row_count);

        let needed_tiles = row_count.div_ceil(TILE_ROWS);
        {
            let mut tiles = state.tiles.borrow_mut();
            while tiles.len() < needed_tiles {
                tiles.push(canvas::Cache::default());
            }
        }

        if *state.last_selected_indices.borrow() != selected_in_range {
            state.selection_cache.clear();
            *state.last_selected_indices.borrow_mut() = selected_in_range.clone();
        }

        let mut geoms: Vec<Geometry> = Vec::new();

        let selection = state
            .selection_cache
            .draw(renderer, bounds.size(), |frame| {
                draw_selected_rows(frame, &selected_in_range, bounds.width);
            });
        geoms.push(selection);

        let tile_from = self.visible_row_start / TILE_ROWS;
        let tile_to = self.visible_row_end.div_ceil(TILE_ROWS).min(needed_tiles);

        for tile_idx in tile_from..tile_to {
            let tile_start = tile_idx * TILE_ROWS;
            let tile_end = ((tile_idx + 1) * TILE_ROWS).min(row_count);
            let tile_bounds = Rectangle {
                x: 0.0,
                y: tile_start as f32 * ROW_H,
                width: bounds.width,
                height: (tile_end - tile_start) as f32 * ROW_H,
            };
            let tiles = state.tiles.borrow();
            let geom = tiles[tile_idx].draw_with_bounds(renderer, tile_bounds, |frame| {
                draw_graph_rows(
                    frame,
                    &self.rows[tile_start..tile_end],
                    tile_start,
                    row_count,
                );
            });
            geoms.push(geom);
        }

        geoms
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        use iced::mouse::Button;

        if let canvas::Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                let row = (pos.y / ROW_H) as usize;
                if row < self.rows.len() {
                    return Some(
                        iced::widget::Action::publish(Message::repo(RepositoryMessage::Center(
                            CenterAction::CommitSelected(row),
                        )))
                        .and_capture(),
                    );
                }
            }
        }

        if let canvas::Event::Mouse(mouse::Event::ButtonPressed(Button::Right)) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                let row = (pos.y / ROW_H) as usize;
                if row < self.rows.len() {
                    return Some(
                        iced::widget::Action::publish(Message::repo(RepositoryMessage::Center(
                            CenterAction::CommitRightClicked { commit_idx: row },
                        )))
                        .and_capture(),
                    );
                }
            }
        }
        None
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(pos) = cursor.position_in(bounds) {
            let row = (pos.y / ROW_H) as usize;
            if row < self.rows.len() {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::default()
    }
}

pub fn full_graph_widget<'a>(
    rows: &'a [GraphRow],
    selected_indices: Vec<usize>,
    num_lanes: usize,
    graph_revision: RepoVersion,
    visible_row_start: usize,
    visible_row_end: usize,
) -> Element<'a, Message> {
    let width = super::graph_column_width(num_lanes);
    let height = rows.len() as f32 * ROW_H;
    Canvas::new(FullGraphProgram {
        rows,
        selected_indices,
        graph_revision,
        visible_row_start,
        visible_row_end,
    })
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .into()
}
