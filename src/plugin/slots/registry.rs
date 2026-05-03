//! Generic slot store.
//!
//! `SlotRegistry<T>` holds zero-or-more `T` values addressable by
//! `(container, id)`. `T` must implement `IsSlot` so the registry can
//! read its addressing fields without owning the builder closure shape.
//! Each region instantiates with its own concrete slot type whose
//! builder uses the lifetime gymnastics that region needs.
//!
//! Within a container, slots iterate ascending by `priority`, with
//! insertion-order tiebreak (stable sort) — preserving the rule the
//! original `MainBarRegistry` documented at
//! `widgets/chrome/main_bar/registry.rs:6-9`.

use super::address::Container;

/// Addressing fields the registry reads. Mutating slots in-place is not
/// supported (replacements drop and reinsert), so reads are enough.
pub trait IsSlot {
    fn id(&self) -> &str;
    fn container(&self) -> &Container;
    fn priority(&self) -> i32;
}

/// Ordered collection. Linear scan on lookup; sized for tens of slots
/// per region, not thousands.
pub struct SlotRegistry<T: IsSlot> {
    slots: Vec<T>,
}

impl<T: IsSlot> Default for SlotRegistry<T> {
    fn default() -> Self {
        Self { slots: Vec::new() }
    }
}

impl<T: IsSlot> SlotRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace by id. Replacement preserves the original vec
    /// position (insertion-order tiebreak unchanged).
    pub fn add(&mut self, slot: T) {
        if let Some(existing) = self.slots.iter_mut().find(|s| s.id() == slot.id()) {
            *existing = slot;
        } else {
            self.slots.push(slot);
        }
    }

    /// Remove by id. Returns true if a slot was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let n = self.slots.len();
        self.slots.retain(|s| s.id() != id);
        self.slots.len() != n
    }

    /// Replace in place only if present. Returns true if replaced.
    pub fn replace(&mut self, id: &str, new: T) -> bool {
        if let Some(existing) = self.slots.iter_mut().find(|s| s.id() == id) {
            *existing = new;
            true
        } else {
            false
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.slots.iter().any(|s| s.id() == id)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Slots in `container`, ordered for rendering.
    pub fn iter_container<'a>(&'a self, container: Container) -> impl Iterator<Item = &'a T> + 'a {
        let mut picked: Vec<(usize, &T)> = self
            .slots
            .iter()
            .enumerate()
            .filter(move |(_, s)| s.container() == &container)
            .collect();
        picked.sort_by(|(ia, a), (ib, b)| a.priority().cmp(&b.priority()).then(ia.cmp(ib)));
        picked.into_iter().map(|(_, s)| s)
    }
}

#[cfg(test)]
mod tests {
    use super::super::address::Container;
    use super::*;

    struct TestSlot {
        id: String,
        container: Container,
        priority: i32,
    }

    impl IsSlot for TestSlot {
        fn id(&self) -> &str {
            &self.id
        }
        fn container(&self) -> &Container {
            &self.container
        }
        fn priority(&self) -> i32 {
            self.priority
        }
    }

    fn slot(id: &str, container: Container, priority: i32) -> TestSlot {
        TestSlot {
            id: id.into(),
            container,
            priority,
        }
    }

    fn left() -> Container {
        Container::Section("left".into())
    }
    fn center() -> Container {
        Container::Section("center".into())
    }

    #[test]
    fn add_then_iter_yields_slot() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        let ids: Vec<&str> = reg
            .iter_container(center())
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, vec!["a"]);
    }

    #[test]
    fn iter_container_filters() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", left(), 10));
        reg.add(slot("b", center(), 10));
        let ids: Vec<&str> = reg
            .iter_container(center())
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[test]
    fn iter_sorts_by_priority() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("c", center(), 30));
        reg.add(slot("a", center(), 10));
        reg.add(slot("b", center(), 20));
        let ids: Vec<&str> = reg
            .iter_container(center())
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn ties_break_by_insertion_order() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("first", center(), 10));
        reg.add(slot("second", center(), 10));
        reg.add(slot("third", center(), 10));
        let ids: Vec<&str> = reg
            .iter_container(center())
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, vec!["first", "second", "third"]);
    }

    #[test]
    fn remove_returns_true_when_present() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        assert!(reg.remove("a"));
        assert!(!reg.contains("a"));
    }

    #[test]
    fn remove_missing_returns_false() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        assert!(!reg.remove("nope"));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn add_duplicate_replaces_in_place() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        reg.add(slot("b", center(), 20));
        reg.add(slot("a", center(), 99));
        let ids: Vec<&str> = reg
            .iter_container(center())
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, vec!["b", "a"]);
    }

    #[test]
    fn replace_existing_returns_true() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        assert!(reg.replace("a", slot("a", left(), 5)));
        assert_eq!(reg.iter_container(center()).count(), 0);
        assert_eq!(reg.iter_container(left()).count(), 1);
    }

    #[test]
    fn replace_missing_returns_false() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        assert!(!reg.replace("ghost", slot("ghost", center(), 0)));
        assert!(reg.is_empty());
    }
}
