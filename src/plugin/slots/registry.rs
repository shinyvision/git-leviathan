use std::cell::RefCell;

use super::address::{Container, SlotAddress};

pub trait IsSlot {
    fn address(&self) -> &SlotAddress;
    fn display_id(&self) -> &str;
    fn priority(&self) -> i32;
    fn set_priority(&mut self, priority: i32);

    fn container(&self) -> &Container {
        self.address().container()
    }
}

pub struct SlotRegistry<T: IsSlot> {
    slots: Vec<T>,
    /// Indices into `slots`, sorted by `(priority, insertion index)`.
    /// Rebuilt only when `slots` mutates so per-frame `iter_container`
    /// neither allocates nor sorts. `None` means "stale, rebuild on read".
    sorted: RefCell<Option<Vec<usize>>>,
}

impl<T: IsSlot> Default for SlotRegistry<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            sorted: RefCell::new(None),
        }
    }
}

impl<T: IsSlot> SlotRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    fn invalidate(&mut self) {
        *self.sorted.get_mut() = None;
    }

    pub fn add(&mut self, slot: T) -> bool {
        self.invalidate();
        if let Some(existing) = self
            .slots
            .iter_mut()
            .find(|s| s.address() == slot.address())
        {
            *existing = slot;
            true
        } else {
            self.slots.push(slot);
            false
        }
    }

    pub fn remove(&mut self, address: &SlotAddress) -> bool {
        self.invalidate();
        let n = self.slots.len();
        self.slots.retain(|s| s.address() != address);
        self.slots.len() != n
    }

    pub fn replace(&mut self, address: &SlotAddress, new: T) -> bool {
        self.invalidate();
        if let Some(existing) = self.slots.iter_mut().find(|s| s.address() == address) {
            *existing = new;
            true
        } else {
            false
        }
    }

    pub fn contains(&self, address: &SlotAddress) -> bool {
        self.slots.iter().any(|s| s.address() == address)
    }

    pub fn contains_display_id(&self, id: &str) -> bool {
        self.slots.iter().any(|s| s.display_id() == id)
    }

    pub fn apply_visibility_and_priority(
        &mut self,
        is_hidden: impl Fn(&SlotAddress) -> bool,
        priority_for: impl Fn(&SlotAddress) -> Option<i32>,
    ) {
        self.invalidate();
        self.slots.retain(|slot| !is_hidden(slot.address()));
        for slot in &mut self.slots {
            if let Some(priority) = priority_for(slot.address()) {
                slot.set_priority(priority);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn ensure_sorted(&self) {
        if self.sorted.borrow().is_some() {
            return;
        }
        let mut order: Vec<usize> = (0..self.slots.len()).collect();
        order.sort_by(|&ia, &ib| {
            self.slots[ia]
                .priority()
                .cmp(&self.slots[ib].priority())
                .then(ia.cmp(&ib))
        });
        *self.sorted.borrow_mut() = Some(order);
    }

    pub fn iter_container<'a>(&'a self, container: Container) -> impl Iterator<Item = &'a T> + 'a {
        self.ensure_sorted();
        let picked: Vec<&T> = self
            .sorted
            .borrow()
            .as_ref()
            .expect("ensure_sorted populated the cache")
            .iter()
            .map(|&i| &self.slots[i])
            .filter(|s| s.container() == &container)
            .collect();
        picked.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::super::address::Container;
    use super::*;

    struct TestSlot {
        id: String,
        address: SlotAddress,
        priority: i32,
    }

    impl IsSlot for TestSlot {
        fn address(&self) -> &SlotAddress {
            &self.address
        }
        fn display_id(&self) -> &str {
            &self.id
        }
        fn priority(&self) -> i32 {
            self.priority
        }
        fn set_priority(&mut self, priority: i32) {
            self.priority = priority;
        }
    }

    fn slot(id: &str, container: Container, priority: i32) -> TestSlot {
        TestSlot {
            id: id.into(),
            address: SlotAddress::new("test", "main_bar", container, id),
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
            .map(|s| s.display_id())
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
            .map(|s| s.display_id())
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
            .map(|s| s.display_id())
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
            .map(|s| s.display_id())
            .collect();
        assert_eq!(ids, vec!["first", "second", "third"]);
    }

    #[test]
    fn remove_returns_true_when_present() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        let address = SlotAddress::new("test", "main_bar", center(), "a");
        assert!(reg.remove(&address));
        assert!(!reg.contains(&address));
    }

    #[test]
    fn remove_missing_returns_false() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        let address = SlotAddress::new("test", "main_bar", center(), "nope");
        assert!(!reg.remove(&address));
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
            .map(|s| s.display_id())
            .collect();
        assert_eq!(ids, vec!["b", "a"]);
    }

    #[test]
    fn replace_existing_returns_true() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        let address = SlotAddress::new("test", "main_bar", center(), "a");
        assert!(reg.replace(&address, slot("a", center(), 5)));
        assert_eq!(reg.iter_container(center()).count(), 1);
    }

    #[test]
    fn replace_missing_returns_false() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        let address = SlotAddress::new("test", "main_bar", center(), "ghost");
        assert!(!reg.replace(&address, slot("ghost", center(), 0)));
        assert!(reg.is_empty());
    }

    #[test]
    fn same_display_id_in_different_containers_does_not_collide() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("same", left(), 10));
        reg.add(slot("same", center(), 10));
        assert_eq!(reg.iter_container(left()).count(), 1);
        assert_eq!(reg.iter_container(center()).count(), 1);
    }

    #[test]
    fn mutation_after_read_reflects_in_next_iter() {
        let mut reg: SlotRegistry<TestSlot> = SlotRegistry::new();
        reg.add(slot("a", center(), 10));
        reg.add(slot("b", center(), 20));
        let first: Vec<&str> = reg
            .iter_container(center())
            .map(|s| s.display_id())
            .collect();
        assert_eq!(first, vec!["a", "b"]);

        reg.add(slot("c", center(), 5));
        let second: Vec<&str> = reg
            .iter_container(center())
            .map(|s| s.display_id())
            .collect();
        assert_eq!(second, vec!["c", "a", "b"]);

        reg.apply_visibility_and_priority(|addr| addr.slot_id().as_str() == "a", |_| None);
        let third: Vec<&str> = reg
            .iter_container(center())
            .map(|s| s.display_id())
            .collect();
        assert_eq!(third, vec!["c", "b"]);
    }
}
