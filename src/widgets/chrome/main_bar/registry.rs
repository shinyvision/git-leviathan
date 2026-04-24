//! Ordered, id-addressable store of `MainBarSlot`s.
//!
//! Plugins (and the built-ins themselves) contribute to the main bar by
//! inserting slots here. The view layer walks the registry at render time.
//!
//! ## Ordering
//!
//! Within a section, slots render in ascending `priority`. Ties resolve by
//! insertion order (stable sort).
//!
//! ## Identity
//!
//! Slot `id` is the stable handle: plugins address slots by id to remove or
//! replace them. Built-ins use the `builtin.*` namespace; plugins use
//! `plugin.<plugin_id>.<button_id>` by convention.

use super::slot::{MainBarSlot, Section};

/// Ordered collection of slots. Not a map: insertion order is meaningful
/// as a stable-sort tiebreaker, and slot ids are small/few enough that a
/// linear scan on lookup is not a hot path.
#[derive(Default)]
pub struct MainBarRegistry {
    slots: Vec<MainBarSlot>,
}

// The mutation API (`remove`, `replace`, `contains`, `len`, `is_empty`) is the
// seam that plugins will drive once the hook system is wired up. Nothing in
// the binary calls it yet — keep it exposed and suppress the dead-code lint.
#[allow(dead_code)]
impl MainBarRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a slot, replacing any existing slot with the same id.
    ///
    /// Replacement preserves the original slot's *position in the vec*,
    /// which matters only as a tiebreaker when priorities collide. The
    /// replacement's own `priority` wins for ordering purposes.
    pub fn add(&mut self, slot: MainBarSlot) {
        if let Some(existing) = self.slots.iter_mut().find(|s| s.id == slot.id) {
            *existing = slot;
        } else {
            self.slots.push(slot);
        }
    }

    /// Remove the slot with `id`. Returns `true` if a slot was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.slots.len();
        self.slots.retain(|s| s.id != id);
        self.slots.len() != len_before
    }

    /// Replace the slot with `id`. Returns `true` if a slot was found and
    /// replaced. Unlike [`add`](Self::add), this does *not* insert when the
    /// id is missing — use when you want to fail loudly on a typo.
    pub fn replace(&mut self, id: &str, new: MainBarSlot) -> bool {
        if let Some(existing) = self.slots.iter_mut().find(|s| s.id == id) {
            *existing = new;
            true
        } else {
            false
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.slots.iter().any(|s| s.id == id)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Slots belonging to `section`, ordered for rendering: ascending by
    /// `priority`, with insertion-order tiebreaking.
    pub fn iter_section(&self, section: Section) -> impl Iterator<Item = &MainBarSlot> {
        let mut picked: Vec<(usize, &MainBarSlot)> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.section == section)
            .collect();
        picked.sort_by(|(ia, a), (ib, b)| a.priority.cmp(&b.priority).then(ia.cmp(ib)));
        picked.into_iter().map(|(_, s)| s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use iced::widget::text;
    use iced::Element;

    fn noop_builder(
    ) -> impl for<'ctx, 'data> Fn(&'ctx super::super::slot::SlotCtx<'data>) -> Element<'data, Message>
           + 'static {
        // Cheap non-borrowing element — we only care about registry mechanics.
        |_ctx| -> Element<'_, Message> { text("").into() }
    }

    fn slot(id: &str, section: Section, priority: i32) -> MainBarSlot {
        MainBarSlot::new(id, section, priority, noop_builder())
    }

    #[test]
    fn add_then_iter_yields_slot() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Center, 10));
        let ids: Vec<&str> = reg.iter_section(Section::Center).map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a"]);
    }

    #[test]
    fn iter_section_filters_by_section() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Left, 10));
        reg.add(slot("b", Section::Center, 10));
        reg.add(slot("c", Section::Right, 10));
        let ids: Vec<&str> = reg.iter_section(Section::Center).map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[test]
    fn iter_section_sorts_by_priority_ascending() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("c", Section::Center, 30));
        reg.add(slot("a", Section::Center, 10));
        reg.add(slot("b", Section::Center, 20));
        let ids: Vec<&str> = reg.iter_section(Section::Center).map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn iter_section_ties_break_by_insertion_order() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("first", Section::Center, 10));
        reg.add(slot("second", Section::Center, 10));
        reg.add(slot("third", Section::Center, 10));
        let ids: Vec<&str> = reg.iter_section(Section::Center).map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["first", "second", "third"]);
    }

    #[test]
    fn remove_by_id_returns_true_and_drops_slot() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Center, 10));
        reg.add(slot("b", Section::Center, 20));
        assert!(reg.remove("a"));
        assert!(!reg.contains("a"));
        assert!(reg.contains("b"));
    }

    #[test]
    fn remove_missing_returns_false() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Center, 10));
        assert!(!reg.remove("nope"));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn add_with_duplicate_id_replaces_in_place() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Center, 10));
        reg.add(slot("b", Section::Center, 20));
        // Replace "a" with a higher priority — should re-order.
        reg.add(slot("a", Section::Center, 99));
        let ids: Vec<&str> = reg.iter_section(Section::Center).map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"]);
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn replace_existing_returns_true() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Center, 10));
        assert!(reg.replace("a", slot("a", Section::Right, 5)));
        // Section change should move the slot from Center to Right.
        assert_eq!(reg.iter_section(Section::Center).count(), 0);
        assert_eq!(reg.iter_section(Section::Right).count(), 1);
    }

    #[test]
    fn replace_missing_returns_false_and_does_not_insert() {
        let mut reg = MainBarRegistry::new();
        assert!(!reg.replace("ghost", slot("ghost", Section::Center, 10)));
        assert!(reg.is_empty());
    }

    #[test]
    fn insertion_tiebreak_is_stable_across_replacements() {
        // Replacing a slot preserves its vec position; priority is still
        // what orders it. When a later-inserted slot with the same priority
        // is added after replacement, the replaced one still wins the tie.
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Center, 10));
        reg.add(slot("b", Section::Center, 10));
        reg.add(slot("a", Section::Center, 10)); // in-place replace
        let ids: Vec<&str> = reg.iter_section(Section::Center).map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }
}
