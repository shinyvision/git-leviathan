//! FLIP-style position tracker. Each frame, the tab bar reports current
//! per-tab x positions; this struct compares against last-known positions
//! and starts an eased animation for any tab that moved, so the tab visually
//! holds its old position and eases to the new one. The animation offset is
//! computed in pixels from real layout positions — not a constant slot
//! width — so variable-width tabs animate accurately.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Instant;

const DURATION_MS: f32 = 200.0;

#[derive(Debug, Clone, Copy)]
struct AnimEntry {
    started_at: Instant,
    from_px: f32,
}

#[derive(Debug)]
pub struct PositionTracker<K: Eq + Hash + Copy> {
    last_positions: HashMap<K, f32>,
    animations: HashMap<K, AnimEntry>,
}

impl<K: Eq + Hash + Copy> Default for PositionTracker<K> {
    fn default() -> Self {
        Self {
            last_positions: HashMap::new(),
            animations: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Copy> PositionTracker<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn settle(&mut self, current: &HashMap<K, f32>, now: Instant) {
        for (&k, &cur_x) in current {
            if let Some(&prev_x) = self.last_positions.get(&k) {
                let delta = prev_x - cur_x;
                if delta.abs() >= 0.5 {
                    let prior_offset = self.offset_for(k, now);
                    let new_from = prior_offset + delta;
                    if new_from.abs() < 0.5 {
                        self.animations.remove(&k);
                    } else {
                        self.animations.insert(
                            k,
                            AnimEntry {
                                started_at: now,
                                from_px: new_from,
                            },
                        );
                    }
                }
            }
            self.last_positions.insert(k, cur_x);
        }
        self.last_positions.retain(|k, _| current.contains_key(k));
        self.animations.retain(|k, _| current.contains_key(k));
    }

    pub fn offset_for(&self, key: K, now: Instant) -> f32 {
        let Some(a) = self.animations.get(&key) else {
            return 0.0;
        };
        let elapsed_ms = now.duration_since(a.started_at).as_secs_f32() * 1000.0;
        let t = (elapsed_ms / DURATION_MS).clamp(0.0, 1.0);
        let progress = ease_out_cubic(t);
        lerp(a.from_px, 0.0, progress)
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.animations.values().any(|a| {
            let elapsed_ms = now.duration_since(a.started_at).as_secs_f32() * 1000.0;
            elapsed_ms < DURATION_MS
        })
    }

    pub fn is_animating_key(&self, key: K, now: Instant) -> bool {
        self.animations.get(&key).is_some_and(|a| {
            let elapsed_ms = now.duration_since(a.started_at).as_secs_f32() * 1000.0;
            elapsed_ms < DURATION_MS
        })
    }
}

fn lerp(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress.clamp(0.0, 1.0)
}

fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn map<K: Eq + Hash + Copy>(pairs: &[(K, f32)]) -> HashMap<K, f32> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn fresh_tracker_has_no_offset() {
        let t: PositionTracker<u32> = PositionTracker::new();
        assert_eq!(t.offset_for(1, Instant::now()), 0.0);
        assert!(!t.is_animating(Instant::now()));
    }

    #[test]
    fn first_observation_records_without_animating() {
        let now = Instant::now();
        let mut t: PositionTracker<u32> = PositionTracker::new();
        t.settle(&map(&[(1, 100.0)]), now);
        assert_eq!(t.offset_for(1, now), 0.0);
        assert!(!t.is_animating(now));
    }

    #[test]
    fn position_change_starts_animation_with_actual_delta() {
        let now = Instant::now();
        let mut t: PositionTracker<u32> = PositionTracker::new();
        t.settle(&map(&[(1, 100.0)]), now);
        t.settle(&map(&[(1, 60.0)]), now);
        assert!((t.offset_for(1, now) - 40.0).abs() < 0.01);
        assert!(t.is_animating(now));
    }

    #[test]
    fn animation_decays_to_zero() {
        let now = Instant::now();
        let mut t: PositionTracker<u32> = PositionTracker::new();
        t.settle(&map(&[(1, 100.0)]), now);
        t.settle(&map(&[(1, 60.0)]), now);
        let mid = t.offset_for(1, at(now, 100));
        assert!(mid < 40.0 && mid > 0.0);
        let after = t.offset_for(1, at(now, 250));
        assert_eq!(after, 0.0);
        assert!(!t.is_animating(at(now, 250)));
    }

    #[test]
    fn second_move_during_animation_blends_additively() {
        let now = Instant::now();
        let mut t: PositionTracker<u32> = PositionTracker::new();
        t.settle(&map(&[(1, 100.0)]), now);
        t.settle(&map(&[(1, 60.0)]), now);
        let visual_before = t.offset_for(1, at(now, 100));
        t.settle(&map(&[(1, 30.0)]), at(now, 100));
        let visual_after = t.offset_for(1, at(now, 100));
        assert!((visual_after - (visual_before + 30.0)).abs() < 0.01);
    }

    #[test]
    fn opposing_moves_can_cancel() {
        let now = Instant::now();
        let mut t: PositionTracker<u32> = PositionTracker::new();
        t.settle(&map(&[(1, 100.0)]), now);
        t.settle(&map(&[(1, 60.0)]), now);
        t.settle(&map(&[(1, 100.0)]), now);
        assert_eq!(t.offset_for(1, now), 0.0);
        assert!(!t.is_animating(now));
    }

    #[test]
    fn keys_animate_independently() {
        let now = Instant::now();
        let mut t: PositionTracker<u32> = PositionTracker::new();
        t.settle(&map(&[(1, 100.0), (2, 200.0)]), now);
        t.settle(&map(&[(1, 60.0), (2, 220.0)]), now);
        assert!((t.offset_for(1, now) - 40.0).abs() < 0.01);
        assert!((t.offset_for(2, now) - (-20.0)).abs() < 0.01);
    }

    #[test]
    fn stale_key_dropped() {
        let now = Instant::now();
        let mut t: PositionTracker<u32> = PositionTracker::new();
        t.settle(&map(&[(1, 100.0), (2, 200.0)]), now);
        t.settle(&map(&[(1, 100.0)]), now);
        // Re-introducing 2 should start fresh — no leftover position.
        t.settle(&map(&[(1, 100.0), (2, 999.0)]), now);
        assert_eq!(t.offset_for(2, now), 0.0);
    }

    #[test]
    fn sub_pixel_change_ignored() {
        let now = Instant::now();
        let mut t: PositionTracker<u32> = PositionTracker::new();
        t.settle(&map(&[(1, 100.0)]), now);
        t.settle(&map(&[(1, 100.2)]), now);
        assert_eq!(t.offset_for(1, now), 0.0);
        assert!(!t.is_animating(now));
    }
}
