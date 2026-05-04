use std::fmt;

use crate::plugin::resources::PluginId;

pub const BUILTIN_PLUGIN_ID: &str = "builtin";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Container {
    Section(String),
    Pane { pane: String, section: String },
}

impl Container {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId(String);

impl RegionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for RegionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for RegionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for RegionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(String);

impl SlotId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SlotId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SlotId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for SlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotAddress {
    plugin_id: PluginId,
    region: RegionId,
    container: Container,
    slot_id: SlotId,
}

impl SlotAddress {
    pub fn new(
        plugin_id: impl Into<PluginId>,
        region: impl Into<RegionId>,
        container: Container,
        slot_id: impl Into<SlotId>,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            region: region.into(),
            container,
            slot_id: slot_id.into(),
        }
    }

    pub fn builtin(
        region: impl Into<RegionId>,
        container: Container,
        slot_id: impl Into<SlotId>,
    ) -> Self {
        Self::new(
            PluginId::from(BUILTIN_PLUGIN_ID),
            region,
            container,
            slot_id,
        )
    }

    pub fn plugin_id(&self) -> &PluginId {
        &self.plugin_id
    }

    pub fn region(&self) -> &RegionId {
        &self.region
    }

    pub fn container(&self) -> &Container {
        &self.container
    }

    pub fn slot_id(&self) -> &SlotId {
        &self.slot_id
    }

    pub fn is_builtin(&self) -> bool {
        self.plugin_id.as_str() == BUILTIN_PLUGIN_ID
    }

    pub fn display_handle(&self) -> String {
        format!("{}:{}:{}", self.region, self.container.key(), self.slot_id)
    }

    pub fn full_handle(&self) -> String {
        format!("{}:{}", self.plugin_id, self.display_handle())
    }
}

impl fmt::Display for SlotAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.full_handle())
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

    #[test]
    fn slot_address_full_handle_includes_owner() {
        let address = SlotAddress::new("p", "main_bar", Container::Section("left".into()), "x");
        assert_eq!(address.display_handle(), "main_bar:left:x");
        assert_eq!(address.full_handle(), "p:main_bar:left:x");
    }
}
