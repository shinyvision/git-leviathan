//! Slot addressing types.
//!
//! A slot is uniquely identified by `(region, container, id)`. The region
//! identifies the UI surface, the container locates a subdivision inside the
//! region (a section for chrome regions, or a pane+section pair for content
//! regions), and the id is the plugin-supplied stable handle.

use std::fmt;

/// A region of the UI that hosts slots. New variants are added as more
/// surfaces become extensible. Stringly-typed strings come in from the Lua
/// API and are parsed into `RegionId` at the host boundary.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionId {
    MainBar,
    TabBar,
    Repository,
}

#[allow(dead_code)]
impl RegionId {
    pub fn as_str(self) -> &'static str {
        match self {
            RegionId::MainBar => "main_bar",
            RegionId::TabBar => "tab_bar",
            RegionId::Repository => "repository",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "main_bar" => Ok(RegionId::MainBar),
            "tab_bar" => Ok(RegionId::TabBar),
            "repository" => Ok(RegionId::Repository),
            other => Err(format!(
                "unknown region: {other:?} (want main_bar/tab_bar/repository)"
            )),
        }
    }
}

/// Where inside a region the slot sits.
///
/// Chrome regions use `Section` directly. Content regions use
/// `Pane { pane, section }` so a pane can host its own subdivisions
/// (`top`, `bottom`) without polluting the region-level enum.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Container {
    /// Direct section (e.g. main_bar's left/center/right; tab_bar's left/right).
    Section(String),
    /// Pane + section (e.g. repository.sidebar.top).
    Pane { pane: String, section: String },
}

#[allow(dead_code)]
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
    fn region_parse_known() {
        assert_eq!(RegionId::parse("main_bar").unwrap(), RegionId::MainBar);
        assert_eq!(RegionId::parse("tab_bar").unwrap(), RegionId::TabBar);
        assert_eq!(RegionId::parse("repository").unwrap(), RegionId::Repository);
    }

    #[test]
    fn region_parse_unknown() {
        assert!(RegionId::parse("nope").is_err());
    }

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
