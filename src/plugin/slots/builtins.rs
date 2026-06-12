use super::{Container, SlotAddress, BUILTIN_PLUGIN_ID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinSlotDescriptor {
    pub region: &'static str,
    pub container: &'static str,
    pub id: &'static str,
    pub priority: i32,
    pub replace_capability: &'static str,
    pub remove_capability: &'static str,
}

impl BuiltinSlotDescriptor {
    pub fn address(self) -> SlotAddress {
        SlotAddress::new(
            BUILTIN_PLUGIN_ID,
            self.region,
            Container::parse(self.container),
            self.id,
        )
    }
}

pub const BUILTIN_SLOTS: &[BuiltinSlotDescriptor] = &[
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "left",
        id: "builtin.repo_info",
        priority: 10,
        replace_capability: "ui:replace:main_bar:left:builtin.repo_info",
        remove_capability: "ui:remove:main_bar:left:builtin.repo_info",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "left",
        id: "builtin.separator_chevron",
        priority: 20,
        replace_capability: "ui:replace:main_bar:left:builtin.separator_chevron",
        remove_capability: "ui:remove:main_bar:left:builtin.separator_chevron",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "left",
        id: "builtin.branch_info",
        priority: 30,
        replace_capability: "ui:replace:main_bar:left:builtin.branch_info",
        remove_capability: "ui:remove:main_bar:left:builtin.branch_info",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "left",
        id: "builtin.fetch_indicator",
        priority: 40,
        replace_capability: "ui:replace:main_bar:left:builtin.fetch_indicator",
        remove_capability: "ui:remove:main_bar:left:builtin.fetch_indicator",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "center",
        id: "builtin.pull",
        priority: 10,
        replace_capability: "ui:replace:main_bar:center:builtin.pull",
        remove_capability: "ui:remove:main_bar:center:builtin.pull",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "center",
        id: "builtin.push",
        priority: 20,
        replace_capability: "ui:replace:main_bar:center:builtin.push",
        remove_capability: "ui:remove:main_bar:center:builtin.push",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "center",
        id: "builtin.branch",
        priority: 30,
        replace_capability: "ui:replace:main_bar:center:builtin.branch",
        remove_capability: "ui:remove:main_bar:center:builtin.branch",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "center",
        id: "builtin.stash",
        priority: 40,
        replace_capability: "ui:replace:main_bar:center:builtin.stash",
        remove_capability: "ui:remove:main_bar:center:builtin.stash",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "center",
        id: "builtin.pop",
        priority: 50,
        replace_capability: "ui:replace:main_bar:center:builtin.pop",
        remove_capability: "ui:remove:main_bar:center:builtin.pop",
    },
    BuiltinSlotDescriptor {
        region: "main_bar",
        container: "right",
        id: "builtin.search",
        priority: 10,
        replace_capability: "ui:replace:main_bar:right:builtin.search",
        remove_capability: "ui:remove:main_bar:right:builtin.search",
    },
    BuiltinSlotDescriptor {
        region: "tab_bar",
        container: "left",
        id: "builtin.plus_button",
        priority: 10,
        replace_capability: "ui:replace:tab_bar:left:builtin.plus_button",
        remove_capability: "ui:remove:tab_bar:left:builtin.plus_button",
    },
    BuiltinSlotDescriptor {
        region: "tab_bar",
        container: "center",
        id: "builtin.tab_list",
        priority: 10,
        replace_capability: "ui:replace:tab_bar:center:builtin.tab_list",
        remove_capability: "ui:remove:tab_bar:center:builtin.tab_list",
    },
    BuiltinSlotDescriptor {
        region: "tab_bar",
        container: "right",
        id: "builtin.version_label",
        priority: 10,
        replace_capability: "ui:replace:tab_bar:right:builtin.version_label",
        remove_capability: "ui:remove:tab_bar:right:builtin.version_label",
    },
];

pub fn get(region: &str, container: &str, id: &str) -> Option<&'static BuiltinSlotDescriptor> {
    BUILTIN_SLOTS
        .iter()
        .find(|slot| slot.region == region && slot.container == container && slot.id == id)
}
