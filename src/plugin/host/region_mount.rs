use git_leviathan_plugin_api::descriptor::region::{RegionDescriptor, RegionKind, REGIONS};

use crate::plugin::diagnostic::{DiagnosticSeverity, PluginDiagnostic, PluginSourceSpan};
use crate::plugin::resources::PluginId;
use crate::plugin::slots::{IsSlot, SlotRegistry};
use crate::plugin::ui::main_bar_slots::{PreparedSlot, PreparedSlotOp};
use crate::widgets::chrome::main_bar::MainBarRegistry;
use crate::widgets::chrome::repo_region::RepoRegionRegistry;
use crate::widgets::chrome::tab_bar_slots::TabBarRegistry;

use super::types::PluginHost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RegionSurfaceKind {
    Chrome,
    Content,
    Decoration,
    Menu,
    Virtual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderMount {
    MainBar,
    TabBar,
    Repository,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VirtualHandler {
    ContextMenu,
    GraphDecoration,
    DiffDecoration,
    Overlay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MountTarget {
    Render(RenderMount),
    Virtual(VirtualHandler),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RegionMountEntry {
    pub(crate) name: &'static str,
    pub(crate) kind: RegionSurfaceKind,
    target: MountTarget,
    pub(crate) context_provider: &'static str,
    pub(crate) capability: &'static str,
}

pub(crate) struct RegionMountRegistry;

pub(crate) static REGION_MOUNT_REGISTRY: RegionMountRegistry = RegionMountRegistry;

const ENTRIES: &[RegionMountEntry] = &[
    RegionMountEntry {
        name: "main_bar",
        kind: RegionSurfaceKind::Chrome,
        target: MountTarget::Render(RenderMount::MainBar),
        context_provider: "chrome.main_bar",
        capability: "ui:region:main_bar",
    },
    RegionMountEntry {
        name: "tab_bar",
        kind: RegionSurfaceKind::Chrome,
        target: MountTarget::Render(RenderMount::TabBar),
        context_provider: "chrome.tab_bar",
        capability: "ui:region:tab_bar",
    },
    RegionMountEntry {
        name: "repository",
        kind: RegionSurfaceKind::Content,
        target: MountTarget::Render(RenderMount::Repository),
        context_provider: "repository.active",
        capability: "ui:region:repository",
    },
    RegionMountEntry {
        name: "repository.graph.context_menu",
        kind: RegionSurfaceKind::Menu,
        target: MountTarget::Virtual(VirtualHandler::ContextMenu),
        context_provider: "repository.graph",
        capability: "ui:context_menu:repository.graph.context_menu",
    },
    RegionMountEntry {
        name: "repository.diff.context_menu",
        kind: RegionSurfaceKind::Menu,
        target: MountTarget::Virtual(VirtualHandler::ContextMenu),
        context_provider: "repository.diff",
        capability: "ui:context_menu:repository.diff.context_menu",
    },
    RegionMountEntry {
        name: "repository.graph.decorations",
        kind: RegionSurfaceKind::Decoration,
        target: MountTarget::Virtual(VirtualHandler::GraphDecoration),
        context_provider: "repository.graph.row",
        capability: "ui:decoration:graph",
    },
    RegionMountEntry {
        name: "repository.diff.decorations",
        kind: RegionSurfaceKind::Decoration,
        target: MountTarget::Virtual(VirtualHandler::DiffDecoration),
        context_provider: "repository.diff.line",
        capability: "ui:decoration:diff",
    },
    RegionMountEntry {
        name: "overlays",
        kind: RegionSurfaceKind::Virtual,
        target: MountTarget::Virtual(VirtualHandler::Overlay),
        context_provider: "app.overlay",
        capability: "ui:overlay",
    },
];

impl RegionMountRegistry {
    pub(crate) fn iter(&self) -> impl Iterator<Item = &'static RegionMountEntry> {
        ENTRIES.iter()
    }

    pub(crate) fn entry(&self, name: &str) -> Option<&'static RegionMountEntry> {
        ENTRIES.iter().find(|entry| entry.name == name)
    }

    pub(crate) fn is_virtual_context_menu_region(&self, name: &str) -> bool {
        self.entry(name)
            .is_some_and(|entry| entry.target == MountTarget::Virtual(VirtualHandler::ContextMenu))
    }

    pub(crate) fn apply_main_bar_slots(&self, host: &PluginHost, registry: &mut MainBarRegistry) {
        self.apply_render_mount(
            host,
            RenderMount::MainBar,
            registry,
            PreparedSlot::into_main_bar,
        );
    }

    pub(crate) fn apply_tab_bar_slots(&self, host: &PluginHost, registry: &mut TabBarRegistry) {
        self.apply_render_mount(
            host,
            RenderMount::TabBar,
            registry,
            PreparedSlot::into_tab_bar,
        );
    }

    pub(crate) fn apply_repo_region_slots(
        &self,
        host: &PluginHost,
        registry: &mut RepoRegionRegistry,
    ) {
        self.apply_render_mount(
            host,
            RenderMount::Repository,
            registry,
            PreparedSlot::into_repo_pane,
        );
    }

    fn apply_render_mount<T: IsSlot>(
        &self,
        host: &PluginHost,
        mount: RenderMount,
        registry: &mut SlotRegistry<T>,
        convert: impl Fn(PreparedSlot) -> T,
    ) {
        let entry = self
            .iter()
            .find(|entry| entry.target == MountTarget::Render(mount))
            .expect("render mount registered");
        let descriptor = REGIONS
            .get(entry.name)
            .expect("render descriptor registered");
        debug_assert_eq!(entry.kind, RegionSurfaceKind::from_descriptor(descriptor));
        debug_assert!(!entry.context_provider.is_empty());
        debug_assert!(!entry.capability.is_empty());
        apply_region_slots(host, registry, entry.name, convert);
    }
}

impl RegionSurfaceKind {
    fn from_descriptor(descriptor: &RegionDescriptor) -> Self {
        match &descriptor.kind {
            RegionKind::Chrome { .. } => Self::Chrome,
            RegionKind::Content { .. } => Self::Content,
        }
    }
}

fn apply_region_slots<T: IsSlot>(
    host: &PluginHost,
    registry: &mut SlotRegistry<T>,
    region_name: &str,
    convert: impl Fn(PreparedSlot) -> T,
) {
    for op in &host.slot_ops {
        match op {
            PreparedSlotOp::Add(p) if p.region == region_name => {
                registry.add(convert(p.clone()));
            }
            PreparedSlotOp::Replace { target, spec, .. }
                if target.region().as_str() == region_name =>
            {
                if !registry.replace(target, convert(spec.clone().mounted_at(target.clone()))) {
                    host.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(spec.plugin_id.clone()),
                            DiagnosticSeverity::Warning,
                            "schema.slot_replace_missing",
                            format!(
                                "leviathan.ui.slot.replace({region_name}, \"{}\") — no such slot",
                                target.slot_id()
                            ),
                        )
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.ui.slot.replace".into(),
                        }),
                    );
                }
            }
            PreparedSlotOp::Remove {
                requester_plugin_id,
                target,
            } if target.region().as_str() == region_name => {
                if !registry.remove(target) {
                    host.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(requester_plugin_id.clone()),
                            DiagnosticSeverity::Warning,
                            "schema.slot_remove_missing",
                            format!(
                                "leviathan.ui.slot.remove({region_name}, \"{}\") — no such slot",
                                target.slot_id()
                            ),
                        )
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.ui.slot.remove".into(),
                        }),
                    );
                }
            }
            _ => {}
        }
    }
    registry.apply_visibility_and_priority(
        |address| {
            address.region().as_str() == region_name
                && host.contribution_overrides.is_hidden(address)
        },
        |address| {
            if address.region().as_str() == region_name {
                host.contribution_overrides.priority(address)
            } else {
                None
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_region_descriptor_has_mount_or_virtual_handler() {
        for descriptor in REGIONS.iter() {
            let entry = REGION_MOUNT_REGISTRY
                .entry(descriptor.name)
                .unwrap_or_else(|| panic!("missing mount for {}", descriptor.name));
            assert_eq!(entry.kind, RegionSurfaceKind::from_descriptor(descriptor));
            assert_ne!(entry.context_provider, "");
            assert_ne!(entry.capability, "");
            match &descriptor.kind {
                RegionKind::Chrome { sections, .. } => assert!(!sections.is_empty()),
                RegionKind::Content { panes } => {
                    assert!(!panes.is_empty());
                    for pane in *panes {
                        assert!(!pane.sections.is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn virtual_entries_have_handlers() {
        for entry in REGION_MOUNT_REGISTRY.iter() {
            match entry.target {
                MountTarget::Render(_) | MountTarget::Virtual(_) => {}
            }
            assert_ne!(entry.context_provider, "");
            assert_ne!(entry.capability, "");
        }
    }
}
