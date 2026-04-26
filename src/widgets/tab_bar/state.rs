//! Drag-reorder state machine. Pure: no widget tree, no rendering. Owned by
//! the TabBar widget via its tree state. Click vs drag is decided by a small
//! pixel threshold; while dragging, hovers over other tabs swap them in the
//! visual `working_order`. Release returns either a click or — if the order
//! changed — the new order for the caller to persist.

use std::hash::Hash;

use iced::Point;

const DRAG_THRESHOLD: f32 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub enum DragOutcome<K> {
    None,
    Click(K),
    Reorder(Vec<K>),
}

#[derive(Debug)]
pub struct DragState<K: Copy + Eq + Hash> {
    press_origin: Option<(K, Point)>,
    dragging: Option<K>,
    grab_offset: Option<f32>,
    cursor: Option<Point>,
    working_order: Option<Vec<K>>,
    initial_order: Option<Vec<K>>,
}

impl<K: Copy + Eq + Hash> Default for DragState<K> {
    fn default() -> Self {
        Self {
            press_origin: None,
            dragging: None,
            grab_offset: None,
            cursor: None,
            working_order: None,
            initial_order: None,
        }
    }
}

impl<K: Copy + Eq + Hash> DragState<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_press(&mut self, key: K, cursor: Point, tab_left_x: f32, current_order: &[K]) {
        if !current_order.contains(&key) {
            return;
        }
        self.press_origin = Some((key, cursor));
        self.grab_offset = Some(cursor.x - tab_left_x);
        self.cursor = Some(cursor);
        self.working_order = Some(current_order.to_vec());
        self.initial_order = Some(current_order.to_vec());
    }

    pub fn on_cursor_moved(
        &mut self,
        cursor: Point,
        slot_bounds: &[(f32, f32)],
        is_animating: impl Fn(K) -> bool,
    ) -> bool {
        if self.press_origin.is_none() && self.dragging.is_none() {
            return false;
        }
        self.cursor = Some(cursor);

        if self.dragging.is_none() {
            if let Some((key, origin)) = self.press_origin {
                let dx = cursor.x - origin.x;
                let dy = cursor.y - origin.y;
                if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD {
                    self.dragging = Some(key);
                }
            }
        }
        let Some(dragged) = self.dragging else {
            return false;
        };
        let Some(order) = self.working_order.as_mut() else {
            return false;
        };
        let Some(from) = order.iter().position(|k| *k == dragged) else {
            return false;
        };
        let Some(grab) = self.grab_offset else {
            return false;
        };
        let Some(&(l, r)) = slot_bounds.get(from) else {
            return false;
        };
        let dragged_w = r - l;
        let dragged_center = cursor.x - grab + dragged_w * 0.5;

        let mut to = from;
        while to + 1 < slot_bounds.len() {
            let (nl, nr) = slot_bounds[to + 1];
            if dragged_center >= (nl + nr) * 0.5 {
                to += 1;
            } else {
                break;
            }
        }
        while to > 0 {
            let (nl, nr) = slot_bounds[to - 1];
            if dragged_center <= (nl + nr) * 0.5 {
                to -= 1;
            } else {
                break;
            }
        }
        if from == to {
            return false;
        }
        let lo = from.min(to);
        let hi = from.max(to);
        for k in &order[lo..=hi] {
            if *k != dragged && is_animating(*k) {
                return false;
            }
        }
        let entry = order.remove(from);
        order.insert(to, entry);
        true
    }

    pub fn on_released(&mut self) -> DragOutcome<K> {
        let press = self.press_origin.take();
        let was_dragging = self.dragging.take().is_some();
        let final_order = self.working_order.take();
        let initial = self.initial_order.take();
        self.grab_offset = None;
        self.cursor = None;

        if !was_dragging {
            return press
                .map(|(k, _)| DragOutcome::Click(k))
                .unwrap_or(DragOutcome::None);
        }
        match (final_order, initial) {
            (Some(final_order), Some(initial)) if final_order != initial => {
                DragOutcome::Reorder(final_order)
            }
            _ => DragOutcome::None,
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    pub fn dragging_key(&self) -> Option<K> {
        self.dragging
    }

    pub fn cursor(&self) -> Option<Point> {
        self.cursor
    }

    pub fn grab_offset(&self) -> Option<f32> {
        self.grab_offset
    }

    pub fn working_order(&self) -> Option<&[K]> {
        self.working_order.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn press_records_origin_and_offset() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(40.0, 0.0), 10.0, &[1, 2]);
        assert_eq!(s.cursor().map(|p| p.x), Some(40.0));
        assert_eq!(s.grab_offset(), Some(30.0));
        assert!(!s.is_dragging());
        assert_eq!(s.working_order(), Some(&[1u32, 2][..]));
    }

    #[test]
    fn press_on_unknown_key_is_noop() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(99, pt(40.0, 0.0), 10.0, &[1, 2]);
        assert_eq!(s.working_order(), None);
        assert!(s.cursor().is_none());
    }

    const SLOTS_3: [(f32, f32); 3] = [(0.0, 50.0), (50.0, 100.0), (100.0, 150.0)];
    const SLOTS_2: [(f32, f32); 2] = [(0.0, 50.0), (50.0, 100.0)];

    #[test]
    fn small_move_stays_a_click() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(12.0, 11.0), &SLOTS_2, |_| false);
        assert!(!s.is_dragging());
        assert_eq!(s.on_released(), DragOutcome::Click(1));
    }

    #[test]
    fn move_past_threshold_starts_drag() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(30.0, 10.0), &SLOTS_2, |_| false);
        assert!(s.is_dragging());
        // Released without reorder (cursor still in slot 0 = where tab 1 is).
        assert_eq!(s.on_released(), DragOutcome::None);
    }

    #[test]
    fn drag_and_hover_swaps_working_order() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2, 3]);
        s.on_cursor_moved(pt(40.0, 10.0), &SLOTS_3, |_| false);
        // cursor at x=40 stays in slot 0, dragged tab already there: no swap.
        assert_eq!(s.working_order(), Some(&[1u32, 2, 3][..]));
        // cursor at x=130 — slot 2.
        s.on_cursor_moved(pt(130.0, 10.0), &SLOTS_3, |_| false);
        assert_eq!(s.working_order(), Some(&[2u32, 3, 1][..]));
        let outcome = s.on_released();
        assert_eq!(outcome, DragOutcome::Reorder(vec![2, 3, 1]));
    }

    #[test]
    fn cursor_held_in_target_slot_does_not_flap() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2, 3]);
        // Drag 1 into slot 2. Order becomes [2, 3, 1].
        s.on_cursor_moved(pt(130.0, 10.0), &SLOTS_3, |_| false);
        assert_eq!(s.working_order(), Some(&[2u32, 3, 1][..]));
        // Cursor stays in slot 2; layout (children in caller order) unchanged.
        // Bug pre-fix: pick by tab key would say "target = tab 3 (now at slot
        // 2)" → swap back. With pick-by-slot, slot 2 already holds dragged →
        // no-op, regardless of how many cursor events arrive.
        for _ in 0..10 {
            assert!(!s.on_cursor_moved(pt(130.0, 10.0), &SLOTS_3, |_| false));
        }
        assert_eq!(s.working_order(), Some(&[2u32, 3, 1][..]));
    }

    #[test]
    fn drag_then_release_without_swap_returns_none() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(30.0, 10.0), &SLOTS_2, |_| false);
        assert_eq!(s.on_released(), DragOutcome::None);
    }

    #[test]
    fn release_clears_state() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(40.0, 10.0), &SLOTS_2, |_| false);
        s.on_released();
        assert!(!s.is_dragging());
        assert!(s.cursor().is_none());
        assert!(s.grab_offset().is_none());
        assert!(s.working_order().is_none());
    }

    #[test]
    fn cursor_moved_without_press_is_noop() {
        let mut s: DragState<u32> = DragState::new();
        let changed = s.on_cursor_moved(pt(40.0, 10.0), &SLOTS_2, |_| false);
        assert!(!changed);
    }

    #[test]
    fn swap_blocked_while_displaced_tab_still_animating() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2, 3]);
        // Cursor moves into slot 1: would displace tab 2. Tab 2 mid-animation → block.
        let changed = s.on_cursor_moved(pt(80.0, 10.0), &SLOTS_3, |k| k == 2);
        assert!(!changed);
        assert_eq!(s.working_order(), Some(&[1u32, 2, 3][..]));
        // Animation finishes → swap allowed.
        let changed = s.on_cursor_moved(pt(80.0, 10.0), &SLOTS_3, |_| false);
        assert!(changed);
        assert_eq!(s.working_order(), Some(&[2u32, 1, 3][..]));
    }

    #[test]
    fn small_over_large_does_not_flap() {
        // Small tab (W=20) at index 0, large tab (W=200) at index 1.
        // After swap, working layout becomes [Large(0..200), Small(200..220)].
        // Cursor held static near old large center: must NOT flap back.
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        // Grab offset = 10. Cursor at 120: dragged_center = 120 - 10 + 10 = 120.
        // Initial slots [(0,20),(20,220)]. Slot 1 center = 120. 120>=120 → swap.
        let slots_pre = [(0.0, 20.0), (20.0, 220.0)];
        assert!(s.on_cursor_moved(pt(120.0, 10.0), &slots_pre, |_| false));
        assert_eq!(s.working_order(), Some(&[2u32, 1][..]));
        // Post-swap slots [Large(0..200), Small(200..220)].
        let slots_post = [(0.0, 200.0), (200.0, 220.0)];
        for _ in 0..10 {
            assert!(!s.on_cursor_moved(pt(120.0, 10.0), &slots_post, |_| false));
        }
        assert_eq!(s.working_order(), Some(&[2u32, 1][..]));
    }

    #[test]
    fn reorder_round_trip_returns_none() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(80.0, 10.0), &SLOTS_2, |_| false);
        assert_eq!(s.working_order(), Some(&[2u32, 1][..]));
        s.on_cursor_moved(pt(10.0, 10.0), &SLOTS_2, |_| false);
        assert_eq!(s.working_order(), Some(&[1u32, 2][..]));
        assert_eq!(s.on_released(), DragOutcome::None);
    }
}
