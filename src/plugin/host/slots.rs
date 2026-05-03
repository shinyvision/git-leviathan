use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use mlua::RegistryKey;

use crate::plugin::api::{RawSlotOp, RawSlotSpec, WidgetSource};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};
use crate::plugin::ui::main_bar_slots::{
    parse_container, DynamicAstCache, PreparedSlot, PreparedSlotOp, SlotWidget,
};

/// True when this slot op was registered by `plugin_id`. Used by
/// reload to park / restore plugin-owned host state.
pub(super) fn op_belongs_to(op: &PreparedSlotOp, plugin_id: &str) -> bool {
    match op {
        PreparedSlotOp::Add(p) => p.plugin_id == plugin_id,
        PreparedSlotOp::Replace { spec, .. } => spec.plugin_id == plugin_id,
        PreparedSlotOp::Remove { .. } => false,
    }
}

fn prepared_slot_handle(region: &str, container: &str, id: &str) -> String {
    format!("{region}:{container}:{id}")
}

pub(super) fn op_matches_slot_resource(op: &PreparedSlotOp, plugin_id: &str, handle: &str) -> bool {
    match op {
        PreparedSlotOp::Add(p) => {
            p.plugin_id == plugin_id
                && prepared_slot_handle(&p.region, &p.container.key(), &p.id) == handle
        }
        PreparedSlotOp::Replace {
            region,
            container,
            id,
            spec,
        } => {
            spec.plugin_id == plugin_id
                && prepared_slot_handle(region, &container.key(), id) == handle
        }
        PreparedSlotOp::Remove { .. } => false,
    }
}

pub(crate) fn prepare_op(
    plugin_id: &str,
    plugin_root: &Path,
    op: RawSlotOp,
    ledger: &ResourceLedger,
    handlers: &mut HashMap<String, RegistryKey>,
    dynamic_widgets: &mut HashMap<String, (RegistryKey, DynamicAstCache)>,
) -> Result<PreparedSlotOp, String> {
    match op {
        RawSlotOp::Add(raw) => {
            let prepared = prepare_slot(
                plugin_id,
                plugin_root,
                raw,
                ledger,
                handlers,
                dynamic_widgets,
            )?;
            Ok(PreparedSlotOp::Add(prepared))
        }
        RawSlotOp::Remove {
            region,
            container,
            id,
        } => Ok(PreparedSlotOp::Remove {
            region,
            container: parse_container(&container),
            id,
        }),
        RawSlotOp::Replace {
            region,
            container,
            id,
            spec,
        } => {
            let prepared = prepare_slot(
                plugin_id,
                plugin_root,
                spec,
                ledger,
                handlers,
                dynamic_widgets,
            )?;
            Ok(PreparedSlotOp::Replace {
                region,
                container: parse_container(&container),
                id,
                spec: prepared,
            })
        }
    }
}

fn prepare_slot(
    plugin_id: &str,
    plugin_root: &Path,
    raw: RawSlotSpec,
    ledger: &ResourceLedger,
    handlers: &mut HashMap<String, RegistryKey>,
    dynamic_widgets: &mut HashMap<String, (RegistryKey, DynamicAstCache)>,
) -> Result<PreparedSlot, String> {
    let RawSlotSpec {
        id,
        region,
        container,
        priority,
        widget,
        on_click,
        source_location,
    } = raw;
    let container_parsed = parse_container(&container);
    if let Some(key) = on_click {
        let handler_key = format!("{region}:{}:{id}", container_parsed.key());
        handlers.insert(handler_key, key);
    }
    let widget = match widget {
        WidgetSource::Static(ast) => SlotWidget::Static(ast),
        WidgetSource::Dynamic(key) => {
            let cache: DynamicAstCache = Rc::new(RefCell::new(None));
            dynamic_widgets.insert(id.clone(), (key, Rc::clone(&cache)));
            ledger.record(
                PluginResourceKind::DynamicWidgetCache,
                format!("{region}:{}:{id}", container_parsed.key()),
                source_location.clone(),
            );
            SlotWidget::Dynamic(cache)
        }
    };
    Ok(PreparedSlot {
        plugin_id: plugin_id.to_string(),
        id,
        region,
        container: container_parsed,
        priority,
        widget,
        plugin_root: plugin_root.to_path_buf(),
    })
}
