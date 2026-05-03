//! Slot addressing types.
//!
//! A slot is uniquely identified by `(region, container, id)`. The region
//! identifies the UI surface, the container locates a subdivision inside the
//! region (a section for chrome regions, or a pane+section pair for content
//! regions), and the id is the plugin-supplied stable handle.

use std::fmt;

/// Where inside a region the slot sits.
///
/// Chrome regions use `Section` directly. Content regions use
/// `Pane { pane, section }` so a pane can host its own subdivisions
/// (`top`, `bottom`) without polluting the region-level enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Container {
    /// Direct section (e.g. main_bar's left/center/right; tab_bar's left/right).
    Section(String),
    /// Pane + section (e.g. repository.sidebar.top).
    Pane { pane: String, section: String },
}

impl Container {
    /// Compose a string the registry can use as a `HashMap` key. Stable;
    /// `regions.add_slot` errors will reference these.
    pub fn key(&self) -> String {
        match self {
            Container::Section(s) => s.clone(),
            Container::Pane { pane, section } => format!("{pane}.{section}"),
        }
    }
}

impl fmt::Display for Container {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_section_key() {
        assert_eq!(Container::Section("left".into()).key(), "left");
    }

    #[test]
    fn container_pane_key() {
        assert_eq!(
            Container::Pane {
                pane: "sidebar".into(),
                section: "top".into()
            }
            .key(),
            "sidebar.top"
        );
    }
}
