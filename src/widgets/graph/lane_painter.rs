use iced::{widget::canvas::path::Builder, Point};

use crate::theme::TURN_RADIUS;

/// Write a v→h segment into an existing path builder (same geometry as `rounded_v_to_h`).
/// Starts a new sub-path with `move_to` so multiple calls can share one `Path`.
pub(crate) fn build_v_to_h_into(b: &mut Builder, x1: f32, y1: f32, x2: f32, y2: f32) {
    let end = Point::new(x2, y2);
    b.move_to(Point::new(x1, y1));
    let horiz_dist = (x2 - x1).abs();
    let vert_dist = (y2 - y1).abs();
    if horiz_dist == 0.0 || vert_dist == 0.0 {
        b.line_to(end);
    } else {
        let corner_radius = TURN_RADIUS.min(horiz_dist).min(vert_dist);
        let dir = if x2 > x1 { 1.0 } else { -1.0 };
        let curve_start_y = y2 - corner_radius;
        let curve_end_x = x1 + dir * corner_radius;
        if curve_start_y - y1 > 0.0 {
            b.line_to(Point::new(x1, curve_start_y));
        }
        b.quadratic_curve_to(Point::new(x1, y2), Point::new(curve_end_x, y2));
        if (x2 - curve_end_x).abs() > 0.0 {
            b.line_to(end);
        }
    }
}

/// Write an h→v segment into an existing path builder (same geometry as `rounded_h_to_v`).
/// Starts a new sub-path with `move_to` so multiple calls can share one `Path`.
pub(crate) fn build_h_to_v_into(b: &mut Builder, x1: f32, y1: f32, x2: f32, y2: f32) {
    let end = Point::new(x2, y2);
    b.move_to(Point::new(x1, y1));
    let horiz_dist = (x2 - x1).abs();
    let vert_dist = (y2 - y1).abs();
    if horiz_dist == 0.0 || vert_dist == 0.0 {
        b.line_to(end);
    } else {
        let corner_radius = TURN_RADIUS.min(horiz_dist).min(vert_dist);
        let dir = if x2 > x1 { 1.0 } else { -1.0 };
        let curve_start_x = x2 - dir * corner_radius;
        if (curve_start_x - x1).abs() > 0.0 {
            b.line_to(Point::new(curve_start_x, y1));
        }
        b.quadratic_curve_to(Point::new(x2, y1), Point::new(x2, y2));
        if vert_dist > 0.0 {
            b.line_to(end);
        }
    }
}

/// Sample a point on the `rounded_v_to_h` curve at t ∈ [0, 1].
pub fn v_to_h_sample(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> (f32, f32) {
    let horiz_dist = (x2 - x1).abs();
    let vert_dist = (y2 - y1).abs();
    if horiz_dist < 0.0 || vert_dist < 0.0 {
        let x = x1 + (x2 - x1) * t;
        let y = y1 + (y2 - y1) * t;
        return (x, y);
    }

    let corner_radius = TURN_RADIUS.min(horiz_dist).min(vert_dist);
    let dir = if x2 > x1 { 1.0 } else { -1.0 };
    let curve_start_y = y2 - corner_radius;
    let curve_end_x = x1 + dir * corner_radius;
    let arm_len = (curve_start_y - y1).abs();
    let curve_len = corner_radius * std::f32::consts::SQRT_2;
    let leg_len = (x2 - curve_end_x).abs();
    let total_len = arm_len + curve_len + leg_len;
    let arm_frac = if total_len > 0.0 {
        arm_len / total_len
    } else {
        0.0
    };
    let curve_frac = if total_len > 0.0 {
        curve_len / total_len
    } else {
        0.0
    };

    if t <= arm_frac {
        let s = if arm_frac > 0.0 { t / arm_frac } else { 0.0 };
        let y = y1 + s * (curve_start_y - y1);
        (x1, y)
    } else if t <= arm_frac + curve_frac {
        let s = if curve_frac > 0.0 {
            (t - arm_frac) / curve_frac
        } else {
            0.0
        };
        let u = 1.0 - s;
        let x =
            u * u * u * x1 + 3.0 * u * u * s * x1 + 3.0 * u * s * s * x1 + s * s * s * curve_end_x;
        let y = u * u * u * curve_start_y
            + 3.0 * u * u * s * y2
            + 3.0 * u * s * s * y2
            + s * s * s * y2;
        (x, y)
    } else {
        let leg_frac = 1.0 - arm_frac - curve_frac;
        let s = if leg_frac > 0.0 {
            (t - arm_frac - curve_frac) / leg_frac
        } else {
            1.0
        };
        let x = curve_end_x + s * (x2 - curve_end_x);
        (x, y2)
    }
}

/// Sample a point on the merge connector path at t ∈ [0, 1].
/// Uses the same corner logic as `build_h_to_v_into`.
pub fn merge_connector_sample(
    commit_x: f32,
    slot_x: f32,
    mid_y: f32,
    row_bottom: f32,
    t: f32,
) -> (f32, f32) {
    let horiz_dist = (slot_x - commit_x).abs();
    let vert_dist = row_bottom - mid_y;
    if horiz_dist == 0.0 {
        let x = commit_x + (slot_x - commit_x) * t;
        let y = mid_y + (row_bottom - mid_y) * t;
        return (x, y);
    }
    let corner_radius = TURN_RADIUS.min(horiz_dist).min(vert_dist);
    let dir = if slot_x > commit_x { 1.0 } else { -1.0 };
    let curve_start_x = slot_x - dir * corner_radius;

    let arm_len = (curve_start_x - commit_x).abs();
    let curve_len = corner_radius * std::f32::consts::SQRT_2;
    let total_len = arm_len + curve_len;
    let arm_frac = if total_len > 0.0 {
        arm_len / total_len
    } else {
        0.0
    };

    if t <= arm_frac {
        let s = if arm_frac > 0.0 { t / arm_frac } else { 0.0 };
        let x = commit_x + s * (curve_start_x - commit_x);
        (x, mid_y)
    } else {
        let s = if arm_frac < 1.0 {
            (t - arm_frac) / (1.0 - arm_frac)
        } else {
            0.0
        };
        let u = 1.0 - s;
        let x = u * u * u * curve_start_x
            + 3.0 * u * u * s * slot_x
            + 3.0 * u * s * s * slot_x
            + s * s * s * slot_x;
        let y = u * u * u * mid_y
            + 3.0 * u * u * s * mid_y
            + 3.0 * u * s * s * mid_y
            + s * s * s * row_bottom;
        (x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_connector_sample, v_to_h_sample};
    use crate::theme::TURN_RADIUS;

    fn samples(commit_x: f32, slot_x: f32, mid_y: f32, row_bottom: f32) -> Vec<(f32, f32)> {
        (0..=40)
            .map(|i| merge_connector_sample(commit_x, slot_x, mid_y, row_bottom, i as f32 / 40.0))
            .collect()
    }

    fn v_samples(x1: f32, y1: f32, x2: f32, y2: f32) -> Vec<(f32, f32)> {
        (0..=40)
            .map(|i| v_to_h_sample(x1, y1, x2, y2, i as f32 / 40.0))
            .collect()
    }

    fn all_in_box(pts: &[(f32, f32)], x_lo: f32, x_hi: f32, y_lo: f32, y_hi: f32) -> bool {
        pts.iter().all(|&(x, y)| {
            x >= x_lo - 0.01 && x <= x_hi + 0.01 && y >= y_lo - 0.01 && y <= y_hi + 0.01
        })
    }

    #[test]
    fn v_to_h_left_stays_inside_lane_corridor() {
        let pts = v_samples(42.0, 68.0, 16.0, 85.0);
        assert!(all_in_box(&pts, 16.0, 42.0, 68.0, 85.0));
    }

    #[test]
    fn v_to_h_right_stays_inside_lane_corridor() {
        let pts = v_samples(16.0, 68.0, 42.0, 85.0);
        assert!(all_in_box(&pts, 16.0, 42.0, 68.0, 85.0));
    }

    #[test]
    fn v_to_h_departs_vertically() {
        let (x0, y0) = v_to_h_sample(42.0, 68.0, 16.0, 85.0, 0.0);
        let (x1, y1) = v_to_h_sample(42.0, 68.0, 16.0, 85.0, 0.02);
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        assert!(
            dx < dy * 0.05,
            "Should depart vertically: dx={dx:.4} dy={dy:.4}"
        );
    }

    #[test]
    fn v_to_h_arrives_horizontally() {
        let (x0, y0) = v_to_h_sample(42.0, 68.0, 16.0, 85.0, 0.98);
        let (x1, y1) = v_to_h_sample(42.0, 68.0, 16.0, 85.0, 1.0);
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        assert!(
            dy < dx * 0.05,
            "Should arrive horizontally: dx={dx:.4} dy={dy:.4}"
        );
    }

    #[test]
    fn v_to_h_endpoints_are_exact() {
        let (x, y) = v_to_h_sample(42.0, 68.0, 16.0, 85.0, 0.0);
        assert!((x - 42.0).abs() < 0.001 && (y - 68.0).abs() < 0.001);
        let (x, y) = v_to_h_sample(42.0, 68.0, 16.0, 85.0, 1.0);
        assert!((x - 16.0).abs() < 0.001 && (y - 85.0).abs() < 0.001);
    }

    #[test]
    fn v_to_h_only_rounds_the_corner_box() {
        let pts = v_samples(16.0, 68.0, 42.0, 85.0);
        let corner_radius = TURN_RADIUS.min(42.0 - 16.0).min(85.0 - 68.0);

        for &(x, y) in &pts {
            if y < 85.0 - corner_radius - 0.1 {
                assert!(
                    (x - 16.0).abs() < 0.1,
                    "vertical arm drifted horizontally too early: x={x:.2} y={y:.2}"
                );
            }
            if x > 16.0 + corner_radius + 0.1 {
                assert!(
                    (y - 85.0).abs() < 0.1,
                    "horizontal leg drifted vertically too late: x={x:.2} y={y:.2}"
                );
            }
        }
    }

    #[test]
    fn endpoints_are_exact() {
        let (x, y) = merge_connector_sample(16.0, 42.0, 68.0, 85.0, 0.0);
        assert!((x - 16.0).abs() < 0.001 && (y - 68.0).abs() < 0.001);
        let (x, y) = merge_connector_sample(16.0, 42.0, 68.0, 85.0, 1.0);
        assert!((x - 42.0).abs() < 0.001 && (y - 85.0).abs() < 0.001);
    }

    #[test]
    fn departs_horizontally() {
        // At t≈0 we are on the straight arm — must move purely horizontally.
        let (x0, y0) = merge_connector_sample(16.0, 42.0, 68.0, 85.0, 0.0);
        let (x1, y1) = merge_connector_sample(16.0, 42.0, 68.0, 85.0, 0.02);
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        assert!(
            dy < dx * 0.05,
            "should depart horizontally: dx={dx:.4} dy={dy:.4}"
        );
    }

    #[test]
    fn arrives_vertically() {
        // At t≈1 horizontal speed must be negligible vs vertical.
        let (x0, y0) = merge_connector_sample(16.0, 42.0, 68.0, 85.0, 0.98);
        let (x1, y1) = merge_connector_sample(16.0, 42.0, 68.0, 85.0, 1.0);
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        assert!(
            dx < dy * 0.05,
            "should arrive vertically: dx={dx:.4} dy={dy:.4}"
        );
    }

    #[test]
    fn stays_within_corridor_slot_right() {
        let pts = samples(16.0, 42.0, 68.0, 85.0);
        assert!(all_in_box(&pts, 16.0, 42.0, 68.0, 85.0));
    }

    #[test]
    fn stays_within_corridor_slot_left() {
        let pts = samples(42.0, 16.0, 68.0, 85.0);
        assert!(all_in_box(&pts, 16.0, 42.0, 68.0, 85.0));
    }

    /// The curve's horizontal footprint must never exceed vert_dist (= row_bottom - mid_y).
    /// This is what makes the corner the same size as the bottom V→H curve (just rotated).
    #[test]
    fn corner_width_capped_to_vert_dist() {
        // Wide connector: horiz_dist=52 > vert_dist=17. Corner must only use 17px wide.
        let commit_x = 10.0;
        let slot_x = 62.0; // horiz_dist = 52
        let mid_y = 68.0;
        let row_bottom = 85.0; // vert_dist = 17
        let vert_dist = row_bottom - mid_y;
        let pts = samples(commit_x, slot_x, mid_y, row_bottom);
        // Every point must have x ≤ slot_x (rightward connector).
        // The first point to dip below mid_y must be within vert_dist of slot_x.
        for &(x, y) in &pts {
            if y > mid_y + 0.1 {
                assert!(
                    (x - slot_x).abs() <= vert_dist + 0.1,
                    "curved portion x={x:.2} is too far from slot_x={slot_x:.2} (limit={vert_dist:.1})"
                );
            }
        }
    }
}
