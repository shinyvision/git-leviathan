//! Main-bar registry — alias over the generic `SlotRegistry`.

use crate::plugin::slots::SlotRegistry;

use super::slot::{MainBarSlot, Section};

pub type MainBarRegistry = SlotRegistry<MainBarSlot>;

/// Section-typed iterator. Wraps the generic `iter_container` with a
/// `Section` input so the view layer doesn't construct `Container`
/// values by hand.
pub fn iter_section<'a>(
    registry: &'a MainBarRegistry,
    section: Section,
) -> impl Iterator<Item = &'a MainBarSlot> + 'a {
    registry.iter_container(section.container())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use iced::widget::text;
    use iced::Element;

    fn slot(id: &str, section: Section, priority: i32) -> MainBarSlot {
        MainBarSlot::new(id, section, priority, |_ctx| -> Element<'_, Message> {
            text("").into()
        })
    }

    #[test]
    fn add_then_iter_yields_slot() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Center, 10));
        let ids: Vec<&str> = iter_section(&reg, Section::Center)
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, vec!["a"]);
    }

    #[test]
    fn iter_section_filters() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("a", Section::Left, 10));
        reg.add(slot("b", Section::Center, 10));
        let ids: Vec<&str> = iter_section(&reg, Section::Center)
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[test]
    fn iter_sorts_priority() {
        let mut reg = MainBarRegistry::new();
        reg.add(slot("c", Section::Center, 30));
        reg.add(slot("a", Section::Center, 10));
        reg.add(slot("b", Section::Center, 20));
        let ids: Vec<&str> = iter_section(&reg, Section::Center)
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }
}
