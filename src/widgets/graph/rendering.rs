//! Pure painting primitives for the commit graph: geometry math (curve
//! sampling, dotted-line placement), the per-row drawing loop, and commit-dot
//! decorations. Independent of Iced's `Program` trait; callers (`program.rs`)
//! invoke `draw_graph_rows` into an existing canvas `Frame`.
use iced::{
    alignment,
    widget::canvas::{self, path::Arc, Frame, Path, Stroke},
    Color, Pixels, Point, Radians, Size,
};

use crate::theme::{AVATAR_RADIUS, BG_SELECTED, LANE_WIDTH, ROW_H};
use crate::view_model::GraphRow;
use crate::widgets::palette::{self, LANE_COUNT};

use super::lane_painter::{
    build_h_to_v_into, build_v_to_h_into, merge_connector_sample, v_to_h_sample,
};

pub const LINE_WIDTH: f32 = 2.0;
const DOT_SIZE: f32 = 1.0;
const DOT_SPACING: f32 = 2.5;
const WIP_CIRCLE_RADIUS: f32 = AVATAR_RADIUS + 1.5;
const MERGE_RADIUS: f32 = AVATAR_RADIUS * 0.5;
const STASH_HALF: f32 = AVATAR_RADIUS;
const STASH_ICON_SPAN: f32 = AVATAR_RADIUS * 1.4 + 5.0;

fn lane_x(lane: f32) -> f32 {
    lane * LANE_WIDTH + LANE_WIDTH / 2.0 + 3.0
}

fn lane_color(idx: usize) -> Color {
    palette::lane_color(idx)
}

fn stroke(color: Color) -> Stroke<'static> {
    Stroke::default().with_color(color).with_width(LINE_WIDTH)
}

/// Greedily chain polylines whose endpoints meet so dot placement can run as
/// one continuous walk. Out of order / disjoint sub-paths (e.g. two different
/// stashes on lanes sharing a color) each form their own chain.
fn chain_polylines(polylines: &[Vec<Point>]) -> Vec<Vec<Point>> {
    const EPS: f32 = 0.5;
    let mut chains: Vec<Vec<Point>> = Vec::new();
    for polyline in polylines {
        if polyline.len() < 2 {
            continue;
        }
        let start = polyline[0];
        let appended = chains.iter_mut().rev().find_map(|chain| {
            let tail = *chain.last().unwrap();
            ((tail.x - start.x).abs() < EPS && (tail.y - start.y).abs() < EPS).then_some(chain)
        });
        if let Some(chain) = appended {
            chain.extend_from_slice(&polyline[1..]);
        } else {
            chains.push(polyline.clone());
        }
    }
    chains
}

/// Walk a connected polyline and emit axis-aligned dots at constant
/// arc-length intervals. First dot lands at the chain's starting point,
/// subsequent dots every `DOT_SIZE + DOT_SPACING` units.
///
/// Uses an absolute target-offset cursor instead of incrementally decrementing
/// a `remaining` counter per sub-segment — the incremental form lost dots when
/// chains passed through many tiny sampled sub-segments because floating-point
/// drift pushed the cursor just past the emit threshold.
fn place_dots_along_chain(points: &[Point], out: &mut Vec<Point>) {
    if points.len() < 2 {
        return;
    }
    let step = DOT_SIZE + DOT_SPACING;
    let total_len: f32 = points
        .windows(2)
        .map(|pair| {
            let dx = pair[1].x - pair[0].x;
            let dy = pair[1].y - pair[0].y;
            (dx * dx + dy * dy).sqrt()
        })
        .sum();
    if total_len == 0.0 {
        return;
    }
    let dot_count = (total_len / step).floor() as usize + 1;

    let mut emitted = 0usize;
    let mut walked = 0.0f32;
    let mut next_target = 0.0f32;
    const EPSILON: f32 = 1e-2;

    for pair in points.windows(2) {
        let dx = pair[1].x - pair[0].x;
        let dy = pair[1].y - pair[0].y;
        let seg_len = (dx * dx + dy * dy).sqrt();
        let seg_end = walked + seg_len;
        while emitted < dot_count && next_target <= seg_end + EPSILON {
            let s = if seg_len > 0.0 {
                ((next_target - walked) / seg_len).clamp(0.0, 1.0)
            } else {
                0.0
            };
            out.push(Point::new(pair[0].x + s * dx, pair[0].y + s * dy));
            emitted += 1;
            next_target = emitted as f32 * step;
        }
        walked = seg_end;
    }
}

fn v_to_h_polyline(x1: f32, y1: f32, x2: f32, y2: f32) -> Vec<Point> {
    // Straight segments only need two endpoints — sampling them into 40 tiny
    // sub-segments lets floating-point accumulation drift far enough across a
    // long chain that the dot-placement cursor misses an emit boundary.
    if x1 == x2 || y1 == y2 {
        return vec![Point::new(x1, y1), Point::new(x2, y2)];
    }
    sample_curve(40, |t| v_to_h_sample(x1, y1, x2, y2, t))
}

fn h_to_v_polyline(x1: f32, y1: f32, x2: f32, y2: f32) -> Vec<Point> {
    if x1 == x2 || y1 == y2 {
        return vec![Point::new(x1, y1), Point::new(x2, y2)];
    }
    sample_curve(40, |t| merge_connector_sample(x1, x2, y1, y2, t))
}

fn sample_curve(steps: usize, mut f: impl FnMut(f32) -> (f32, f32)) -> Vec<Point> {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let (x, y) = f(t);
            Point::new(x, y)
        })
        .collect()
}

fn draw_dots(frame: &mut Frame, dots: &[Point], color: Color) {
    if dots.is_empty() {
        return;
    }
    let path = Path::new(|b| {
        for p in dots {
            b.rectangle(*p, Size::new(DOT_SIZE, DOT_SIZE));
        }
    });
    frame.fill(&path, color);
}

/// Regular commit: outer ring + dark fill + author initials.
fn draw_avatar(frame: &mut Frame, cx: f32, cy: f32, lane_col: Color, initials: &str) {
    let outer = Path::circle(Point::new(cx, cy), AVATAR_RADIUS + 1.5);
    frame.fill(&outer, lane_col);

    let bg_col = Color {
        r: lane_col.r * 0.38,
        g: lane_col.g * 0.38,
        b: lane_col.b * 0.38,
        a: 1.0,
    };
    frame.fill(&Path::circle(Point::new(cx, cy), AVATAR_RADIUS), bg_col);

    frame.fill_text(canvas::Text {
        content: initials.to_string(),
        position: Point::new(cx, cy),
        color: Color::WHITE,
        size: Pixels(9.0),
        align_x: iced::advanced::text::Alignment::Center,
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

/// WIP (dirty) row: dotted circle outline — no fill, dashes in lane color.
fn draw_wip_circle(frame: &mut Frame, cx: f32, cy: f32, lane_col: Color) {
    use std::f32::consts::TAU;

    const NUM_DASHES: usize = 20;
    let period = TAU / NUM_DASHES as f32;
    let dash_len = period * 0.25;
    let path = Path::new(|b| {
        for i in 0..NUM_DASHES {
            let start = period * i as f32;
            b.arc(Arc {
                center: Point::new(cx, cy),
                radius: WIP_CIRCLE_RADIUS,
                start_angle: Radians(start),
                end_angle: Radians(start + dash_len),
            });
        }
    });
    frame.stroke(&path, stroke(lane_col));
}

fn draw_stash_marker(frame: &mut Frame, cx: f32, cy: f32, lane_col: Color) {
    let left = cx - STASH_HALF;
    let right = cx + STASH_HALF;
    let top = cy - STASH_HALF;
    let bottom = cy + STASH_HALF;

    // Distribute dots per-side with step adjusted so each edge hosts the same
    // count and each corner hosts exactly one dot. The generic open-chain
    // walker overruns on a closed perimeter when total length isn't a clean
    // multiple of step, producing a stray dot stacked near the start and an
    // extra dot on whichever edge the walk closes on.
    let side = 2.0 * STASH_HALF;
    let desired_step = DOT_SIZE + DOT_SPACING;
    let dots_per_side = (side / desired_step).round().max(1.0) as usize;
    let step = side / dots_per_side as f32;

    let mut dots = Vec::with_capacity(dots_per_side * 4);
    for i in 0..dots_per_side {
        let t = i as f32 * step;
        dots.push(Point::new(left + t, top));
        dots.push(Point::new(right, top + t));
        dots.push(Point::new(right - t, bottom));
        dots.push(Point::new(left, bottom - t));
    }
    draw_dots(frame, &dots, lane_col);

    // Tabler "stack" icon — two paths traced from assets/icons/stack.svg
    // (viewBox 0..24). Drawn as separate strokes so lyon treats them as
    // disjoint sub-paths.
    let scale = STASH_ICON_SPAN / 30.0;
    let map = |x: f32, y: f32| Point::new(cx + (x - 12.0) * scale, cy + (y - 12.0) * scale);
    let icon_stroke = Stroke::default().with_color(lane_col).with_width(1.0);

    let top_diamond = Path::new(|b| {
        b.move_to(map(12.0, 6.0));
        b.line_to(map(4.0, 10.0));
        b.line_to(map(12.0, 14.0));
        b.line_to(map(20.0, 10.0));
        b.close();
    });
    frame.stroke(&top_diamond, icon_stroke);

    let bottom_v = Path::new(|b| {
        b.move_to(map(4.0, 14.0));
        b.line_to(map(12.0, 18.0));
        b.line_to(map(20.0, 14.0));
    });
    frame.stroke(&bottom_v, icon_stroke);
}

/// Merge commit: solid filled circle — no initials, no dark inner fill.
fn draw_merge_dot(frame: &mut Frame, cx: f32, cy: f32, lane_col: Color) {
    // Dark outer ring so the dot visually "pops" on the lines it sits on.
    let ring_col = Color {
        r: lane_col.r * 0.5,
        g: lane_col.g * 0.5,
        b: lane_col.b * 0.5,
        a: 1.0,
    };
    frame.fill(
        &Path::circle(Point::new(cx, cy), MERGE_RADIUS + 1.5),
        ring_col,
    );
    frame.fill(&Path::circle(Point::new(cx, cy), MERGE_RADIUS), lane_col);
}

pub fn draw_selected_rows(frame: &mut Frame, selected_local_indices: &[usize], width: f32) {
    for &local_idx in selected_local_indices {
        let row_y = local_idx as f32 * ROW_H;
        frame.fill_rectangle(Point::new(0.0, row_y), Size::new(width, ROW_H), BG_SELECTED);
    }
}

pub fn draw_graph_rows(
    frame: &mut Frame,
    rows: &[GraphRow],
    start_index: usize,
    total_rows: usize,
) {
    let num_colors = LANE_COUNT;

    // Accumulate segments grouped by color to minimise frame.stroke() calls.
    // v_to_h: upper/lower curves (rounded_v_to_h geometry).
    // h_to_v: merge connector curves (rounded_h_to_v geometry).
    // dotted_h_to_v: pending dirty merge connector curves.
    // horiz:  last-row connector horizontal arms.
    let mut v_to_h: Vec<Vec<(f32, f32, f32, f32)>> = vec![vec![]; num_colors];
    let mut h_to_v: Vec<Vec<(f32, f32, f32, f32)>> = vec![vec![]; num_colors];
    let mut horiz: Vec<Vec<(f32, f32, f32)>> = vec![vec![]; num_colors]; // (x1, x2, y)
                                                                         // All dotted sub-paths per color, in traversal order. Adjacent entries
                                                                         // that share endpoints are chained into a single continuous polyline
                                                                         // before dots are placed (so spacing stays uniform across row/curve
                                                                         // boundaries).
    let mut dotted_polylines: Vec<Vec<Vec<Point>>> = vec![vec![]; num_colors];

    for (local_idx, row) in rows.iter().enumerate() {
        let global_idx = start_index + local_idx;
        let row_y = (start_index + local_idx) as f32 * ROW_H;
        let mid_y = row_y + ROW_H / 2.0;

        // Upper half — corner curves arriving at the commit.
        // Skipped for the first row: no commits exist above it.
        if global_idx > 0 {
            for seg in &row.upper {
                let ci = seg.from_lane as usize % num_colors;
                v_to_h[ci].push((lane_x(seg.from_lane), row_y, lane_x(seg.to_lane), mid_y));
            }
            for seg in &row.dotted_upper {
                let ci = seg.from_lane as usize % num_colors;
                let end_y = if row.is_stash && seg.to_lane == row.commit_lane as f32 {
                    mid_y - STASH_HALF
                } else {
                    mid_y
                };
                dotted_polylines[ci].push(v_to_h_polyline(
                    lane_x(seg.from_lane),
                    row_y,
                    lane_x(seg.to_lane),
                    end_y,
                ));
            }
        }

        // Lower half — stubs departing from the commit.
        // Skipped for connector-destination lanes that had no straight pass-through.
        // Skipped entirely for the last row.
        if global_idx + 1 < total_rows {
            for seg in &row.lower {
                let is_connector_endpoint =
                    row.connectors.iter().any(|c| c.to_lane == seg.from_lane);
                if is_connector_endpoint {
                    let had_straight_passthrough = row
                        .upper
                        .iter()
                        .chain(row.dotted_upper.iter())
                        .any(|u| u.from_lane == seg.from_lane && u.to_lane == seg.from_lane);
                    if !had_straight_passthrough {
                        continue;
                    }
                }
                let ci = seg.from_lane as usize % num_colors;
                v_to_h[ci].push((
                    lane_x(seg.from_lane),
                    mid_y,
                    lane_x(seg.to_lane),
                    row_y + ROW_H,
                ));
            }
            for seg in &row.dotted_lower {
                let is_connector_endpoint = row
                    .dotted_connectors
                    .iter()
                    .any(|c| c.to_lane == seg.from_lane);
                if is_connector_endpoint {
                    let had_straight_passthrough = row
                        .upper
                        .iter()
                        .chain(row.dotted_upper.iter())
                        .any(|u| u.from_lane == seg.from_lane && u.to_lane == seg.from_lane);
                    if !had_straight_passthrough {
                        continue;
                    }
                }
                let ci = seg.from_lane as usize % num_colors;
                let start_y = if row.is_stash && seg.from_lane == row.commit_lane as f32 {
                    mid_y + STASH_HALF + 4.0
                } else if row.has_commit || row.is_stash {
                    mid_y
                } else {
                    mid_y + WIP_CIRCLE_RADIUS + LINE_WIDTH
                };
                dotted_polylines[ci].push(v_to_h_polyline(
                    lane_x(seg.from_lane),
                    start_y,
                    lane_x(seg.to_lane),
                    row_y + ROW_H,
                ));
            }
        }

        // Connectors — merge arm from commit dot to each secondary-parent lane.
        for conn in &row.connectors {
            let commit_x = lane_x(conn.from_lane);
            let slot_x = lane_x(conn.to_lane);
            let ci = conn.to_lane as usize % num_colors;
            if global_idx + 1 < total_rows {
                h_to_v[ci].push((commit_x, mid_y, slot_x, row_y + ROW_H));
            } else {
                horiz[ci].push((commit_x, slot_x, mid_y));
            }
        }

        for conn in &row.dotted_connectors {
            let commit_x = lane_x(conn.from_lane);
            let slot_x = lane_x(conn.to_lane);
            let ci = conn.to_lane as usize % num_colors;
            if global_idx + 1 < total_rows {
                dotted_polylines[ci].push(h_to_v_polyline(commit_x, mid_y, slot_x, row_y + ROW_H));
            } else {
                dotted_polylines[ci]
                    .push(vec![Point::new(commit_x, mid_y), Point::new(slot_x, mid_y)]);
            }
        }
    }

    // Dotted (WIP / stash) lanes first — solid lanes drawn second so they
    // occlude dotted lanes where both occupy the same space.
    //
    // Dotted lines are rendered as explicit axis-aligned square dots placed
    // at constant arc-length spacing along each sub-path. Lyon's built-in
    // `LineDash` resets phase at every sub-path boundary and rotates dashes
    // around curves, both of which produce visibly uneven spacing where
    // short corners meet long straights. Manual placement keeps spacing and
    // orientation uniform.
    #[allow(clippy::needless_range_loop)] // `ci` flows to lane_color(ci) independent of the slice.
    for ci in 0..num_colors {
        if dotted_polylines[ci].is_empty() {
            continue;
        }
        let color = lane_color(ci);
        let mut dots: Vec<Point> = Vec::new();
        for chain in chain_polylines(&dotted_polylines[ci]) {
            place_dots_along_chain(&chain, &mut dots);
        }
        if !dots.is_empty() {
            draw_dots(frame, &dots, color);
        }
    }

    // Solid lanes second — drawn on top of dotted so solid occludes dotted
    // where both occupy the same lane space. Per color, all sub-paths are
    // batched into a single Path + single frame.stroke so the backend emits
    // one draw call per color instead of one per segment — crucial for
    // drivers (Nvidia) with non-trivial per-draw overhead.
    for ci in 0..num_colors {
        if v_to_h[ci].is_empty() && h_to_v[ci].is_empty() && horiz[ci].is_empty() {
            continue;
        }
        let color = lane_color(ci);
        let batched = Path::new(|b| {
            for &(x1, y1, x2, y2) in &v_to_h[ci] {
                build_v_to_h_into(b, x1, y1, x2, y2);
            }
            for &(x1, y1, x2, y2) in &h_to_v[ci] {
                build_h_to_v_into(b, x1, y1, x2, y2);
            }
            for &(x1, x2, y) in &horiz[ci] {
                b.move_to(Point::new(x1, y));
                b.line_to(Point::new(x2, y));
            }
        });
        frame.stroke(&batched, stroke(color));
    }

    // Commit dots — drawn after all lines so they sit on top.
    for (local_idx, row) in rows.iter().enumerate() {
        let row_y = (start_index + local_idx) as f32 * ROW_H;
        let mid_y = row_y + ROW_H / 2.0;
        let dot_x = lane_x(row.commit_lane as f32);
        let lane_col = lane_color(row.commit_lane);
        if row.is_stash {
            draw_stash_marker(frame, dot_x, mid_y, lane_col);
        } else if !row.has_commit {
            draw_wip_circle(frame, dot_x + 1.0, mid_y, lane_col);
        } else if row.is_merge {
            draw_merge_dot(frame, dot_x, mid_y, lane_col);
        } else {
            draw_avatar(frame, dot_x, mid_y, lane_col, &row.author_initials);
        }
    }
}

