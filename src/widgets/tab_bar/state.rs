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

    pub fn on_cursor_moved(&mut self, cursor: Point, slot_bounds: &[(f32, f32)]) -> bool {
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
        let Some(slot_idx) = pick_slot(cursor.x, slot_bounds) else {
            return false;
        };
        let Some(order) = self.working_order.as_mut() else {
            return false;
        };
        if order.get(slot_idx).copied() == Some(dragged) {
            return false;
        }
        let Some(from) = order.iter().position(|k| *k == dragged) else {
            return false;
        };
        let to = slot_idx.min(order.len().saturating_sub(1));
        if from == to {
            return false;
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

pub fn pick_slot(cursor_x: f32, slots: &[(f32, f32)]) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, &(left, right)) in slots.iter().enumerate() {
        let center = (left + right) * 0.5;
        let dist = (cursor_x - center).abs();
        match best {
            None => best = Some((i, dist)),
            Some((_, best_dist)) if dist < best_dist => best = Some((i, dist)),
            _ => {}
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn pick_slot_empty_returns_none() {
        let slots: [(f32, f32); 0] = [];
        assert_eq!(pick_slot(0.0, &slots), None);
    }

    #[test]
    fn pick_slot_picks_closest_center() {
        let slots = [(0.0, 100.0), (100.0, 200.0)];
        assert_eq!(pick_slot(50.0, &slots), Some(0));
        assert_eq!(pick_slot(150.0, &slots), Some(1));
    }

    #[test]
    fn pick_slot_past_right_picks_last() {
        let slots = [(0.0, 100.0), (100.0, 200.0)];
        assert_eq!(pick_slot(500.0, &slots), Some(1));
    }

    #[test]
    fn pick_slot_before_left_picks_first() {
        let slots = [(50.0, 150.0), (150.0, 250.0)];
        assert_eq!(pick_slot(-100.0, &slots), Some(0));
    }

    #[test]
    fn pick_slot_equidistant_picks_lower_index() {
        let slots = [(0.0, 100.0), (100.0, 200.0)];
        assert_eq!(pick_slot(100.0, &slots), Some(0));
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
        s.on_cursor_moved(pt(12.0, 11.0), &SLOTS_2);
        assert!(!s.is_dragging());
        assert_eq!(s.on_released(), DragOutcome::Click(1));
    }

    #[test]
    fn move_past_threshold_starts_drag() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(30.0, 10.0), &SLOTS_2);
        assert!(s.is_dragging());
        // Released without reorder (cursor still in slot 0 = where tab 1 is).
        assert_eq!(s.on_released(), DragOutcome::None);
    }

    #[test]
    fn drag_and_hover_swaps_working_order() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2, 3]);
        s.on_cursor_moved(pt(40.0, 10.0), &SLOTS_3);
        // cursor at x=40 stays in slot 0, dragged tab already there: no swap.
        assert_eq!(s.working_order(), Some(&[1u32, 2, 3][..]));
        // cursor at x=130 — slot 2.
        s.on_cursor_moved(pt(130.0, 10.0), &SLOTS_3);
        assert_eq!(s.working_order(), Some(&[2u32, 3, 1][..]));
        let outcome = s.on_released();
        assert_eq!(outcome, DragOutcome::Reorder(vec![2, 3, 1]));
    }

    #[test]
    fn cursor_held_in_target_slot_does_not_flap() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2, 3]);
        // Drag 1 into slot 2. Order becomes [2, 3, 1].
        s.on_cursor_moved(pt(130.0, 10.0), &SLOTS_3);
        assert_eq!(s.working_order(), Some(&[2u32, 3, 1][..]));
        // Cursor stays in slot 2; layout (children in caller order) unchanged.
        // Bug pre-fix: pick by tab key would say "target = tab 3 (now at slot
        // 2)" → swap back. With pick-by-slot, slot 2 already holds dragged →
        // no-op, regardless of how many cursor events arrive.
        for _ in 0..10 {
            assert!(!s.on_cursor_moved(pt(130.0, 10.0), &SLOTS_3));
        }
        assert_eq!(s.working_order(), Some(&[2u32, 3, 1][..]));
    }

    #[test]
    fn drag_then_release_without_swap_returns_none() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(30.0, 10.0), &SLOTS_2);
        assert_eq!(s.on_released(), DragOutcome::None);
    }

    #[test]
    fn release_clears_state() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(40.0, 10.0), &SLOTS_2);
        s.on_released();
        assert!(!s.is_dragging());
        assert!(s.cursor().is_none());
        assert!(s.grab_offset().is_none());
        assert!(s.working_order().is_none());
    }

    #[test]
    fn cursor_moved_without_press_is_noop() {
        let mut s: DragState<u32> = DragState::new();
        let changed = s.on_cursor_moved(pt(40.0, 10.0), &SLOTS_2);
        assert!(!changed);
    }

    #[test]
    fn reorder_round_trip_returns_none() {
        let mut s: DragState<u32> = DragState::new();
        s.on_press(1, pt(10.0, 10.0), 0.0, &[1, 2]);
        s.on_cursor_moved(pt(80.0, 10.0), &SLOTS_2);
        assert_eq!(s.working_order(), Some(&[2u32, 1][..]));
        s.on_cursor_moved(pt(10.0, 10.0), &SLOTS_2);
        assert_eq!(s.working_order(), Some(&[1u32, 2][..]));
        assert_eq!(s.on_released(), DragOutcome::None);
    }
}
