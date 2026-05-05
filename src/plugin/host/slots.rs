use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use mlua::RegistryKey;

use crate::plugin::api::{RawSlotOp, RawSlotSpec, WidgetSource};
use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::{GenerationId, PluginId};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};
use crate::plugin::slots::{SlotAddress, BUILTIN_PLUGIN_ID};
use crate::plugin::ui::invalidation::DynamicWidgetTelemetry;
use crate::plugin::ui::main_bar_slots::{
    parse_container, DynamicAstCache, DynamicWidgetRegistration, PreparedSlot, PreparedSlotOp,
    SlotWidget,
};

pub(super) fn op_belongs_to(op: &PreparedSlotOp, plugin_id: &str) -> bool {
    match op {
        PreparedSlotOp::Add(p) => p.plugin_id == plugin_id,
        PreparedSlotOp::Replace { spec, .. } => spec.plugin_id == plugin_id,
        PreparedSlotOp::Remove {
            requester_plugin_id,
            ..
        } => requester_plugin_id == plugin_id,
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
        PreparedSlotOp::Replace { target, spec, .. } => {
            spec.plugin_id == plugin_id && target.display_handle() == handle
        }
        PreparedSlotOp::Remove { .. } => false,
    }
}

pub(crate) fn validate_raw_slot_op(
    diagnostics: &DiagnosticStore,
    generation_id: GenerationId,
    live_ops: &[PreparedSlotOp],
    plugin_id: &str,
    op: &RawSlotOp,
) -> bool {
    match op {
        RawSlotOp::Add(raw) => {
            if raw.id.starts_with("builtin.") {
                slot_diagnostic(
                    diagnostics,
                    generation_id,
                    plugin_id,
                    "schema.slot_builtin_id_reserved",
                    format!(
                        "slot id `{}` is reserved; use leviathan.ui.slot.replace for built-ins",
                        raw.id
                    ),
                    "leviathan.ui.slot.add",
                );
                return false;
            }
            let address = add_address(plugin_id, raw);
            let active = effective_slots(live_ops);
            if active.contains_key(&address) {
                slot_diagnostic(
                    diagnostics,
                    generation_id,
                    plugin_id,
                    "schema.slot_duplicate_address",
                    format!("duplicate slot address `{}`", address.display_handle()),
                    "leviathan.ui.slot.add",
                );
            }
            for (existing, owner) in &active {
                if owner == plugin_id
                    && existing.region().as_str() == raw.region
                    && existing.slot_id().as_str() == raw.id
                    && existing.container() != &parse_container(&raw.container)
                {
                    slot_diagnostic(
                        diagnostics,
                        generation_id,
                        plugin_id,
                        "schema.slot_id_reused",
                        format!(
                            "slot id `{}` reused in `{}` and `{}`",
                            raw.id,
                            existing.container().key(),
                            raw.container
                        ),
                        "leviathan.ui.slot.add",
                    );
                    break;
                }
            }
            true
        }
        RawSlotOp::Remove {
            target_plugin_id,
            region,
            container,
            id,
            ..
        } => validate_mutation_target(
            diagnostics,
            generation_id,
            live_ops,
            MutationTarget {
                plugin_id,
                target_plugin_id: target_plugin_id.as_deref(),
                region,
                container,
                id,
                op: "remove",
            },
        ),
        RawSlotOp::Replace {
            target_plugin_id,
            region,
            container,
            id,
            ..
        } => validate_mutation_target(
            diagnostics,
            generation_id,
            live_ops,
            MutationTarget {
                plugin_id,
                target_plugin_id: target_plugin_id.as_deref(),
                region,
                container,
                id,
                op: "replace",
            },
        ),
    }
}

pub(crate) fn prepare_op(
    plugin_id: &str,
    plugin_root: &Path,
    op: RawSlotOp,
    ledger: &ResourceLedger,
    handlers: &mut HashMap<SlotAddress, RegistryKey>,
    dynamic_widgets: &mut HashMap<SlotAddress, DynamicWidgetRegistration>,
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
            target_plugin_id,
            region,
            container,
            id,
            ..
        } => Ok(PreparedSlotOp::Remove {
            requester_plugin_id: plugin_id.to_string(),
            target: target_address(
                plugin_id,
                target_plugin_id.as_deref(),
                &region,
                &container,
                &id,
            ),
        }),
        RawSlotOp::Replace {
            target_plugin_id,
            region,
            container,
            id,
            spec,
            ..
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
                requester_plugin_id: plugin_id.to_string(),
                target: target_address(
                    plugin_id,
                    target_plugin_id.as_deref(),
                    &region,
                    &container,
                    &id,
                ),
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
    handlers: &mut HashMap<SlotAddress, RegistryKey>,
    dynamic_widgets: &mut HashMap<SlotAddress, DynamicWidgetRegistration>,
) -> Result<PreparedSlot, String> {
    let RawSlotSpec {
        id,
        region,
        container,
        priority,
        widget,
        depends_on,
        on_click,
        source_location,
    } = raw;
    let container_parsed = parse_container(&container);
    let address = SlotAddress::new(
        plugin_id,
        region.clone(),
        container_parsed.clone(),
        id.clone(),
    );
    if let Some(key) = on_click {
        handlers.insert(address.clone(), key);
    }
    let widget = match widget {
        WidgetSource::Static(ast) => SlotWidget::Static(ast),
        WidgetSource::Dynamic { key, call } => {
            let cache: DynamicAstCache = Rc::new(RefCell::new(None));
            dynamic_widgets.insert(
                address.clone(),
                DynamicWidgetRegistration {
                    key,
                    cache: Rc::clone(&cache),
                    call,
                    dependencies: depends_on.clone(),
                    telemetry: Rc::new(RefCell::new(DynamicWidgetTelemetry::default())),
                },
            );
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
        address: address.clone(),
        mount_address: address,
        region,
        container: container_parsed,
        priority,
        widget,
        plugin_root: plugin_root.to_path_buf(),
    })
}

fn add_address(plugin_id: &str, raw: &RawSlotSpec) -> SlotAddress {
    SlotAddress::new(
        plugin_id,
        raw.region.clone(),
        parse_container(&raw.container),
        raw.id.clone(),
    )
}

fn target_address(
    requester_plugin_id: &str,
    target_plugin_id: Option<&str>,
    region: &str,
    container: &str,
    id: &str,
) -> SlotAddress {
    let owner = target_plugin_id
        .map(str::to_string)
        .unwrap_or_else(|| default_target_plugin_id(requester_plugin_id, id));
    SlotAddress::new(
        owner,
        region.to_string(),
        parse_container(container),
        id.to_string(),
    )
}

fn default_target_plugin_id(requester_plugin_id: &str, id: &str) -> String {
    if id.starts_with("builtin.") {
        BUILTIN_PLUGIN_ID.to_string()
    } else {
        requester_plugin_id.to_string()
    }
}

struct MutationTarget<'a> {
    plugin_id: &'a str,
    target_plugin_id: Option<&'a str>,
    region: &'a str,
    container: &'a str,
    id: &'a str,
    op: &'a str,
}

fn validate_mutation_target(
    diagnostics: &DiagnosticStore,
    generation_id: GenerationId,
    live_ops: &[PreparedSlotOp],
    request: MutationTarget<'_>,
) -> bool {
    let target = target_address(
        request.plugin_id,
        request.target_plugin_id,
        request.region,
        request.container,
        request.id,
    );
    if target.is_builtin() {
        if crate::plugin::slots::builtins::get(request.region, request.container, request.id)
            .is_some()
        {
            return true;
        }
        slot_diagnostic(
            diagnostics,
            generation_id,
            request.plugin_id,
            missing_code(request.op),
            format!(
                "{}_slot target missing `{}:{}:{}`",
                request.op, request.region, request.container, request.id
            ),
            mutation_api_name(request.op),
        );
        return false;
    }
    let active = effective_slots(live_ops);
    if let Some(owner) = active.get(&target) {
        if target.plugin_id().as_str() == request.plugin_id || request.target_plugin_id.is_some() {
            return true;
        }
        slot_diagnostic(
            diagnostics,
            generation_id,
            request.plugin_id,
            "schema.slot_mutation_unauthorized",
            format!(
                "{}_slot denied for `{}:{}:{}` owned by `{owner}`",
                request.op, request.region, request.container, request.id
            ),
            mutation_api_name(request.op),
        );
        return false;
    }
    let same_target_owner = active
        .iter()
        .find(|(address, _)| {
            address.region().as_str() == request.region
                && address.container().key() == request.container
                && address.slot_id().as_str() == request.id
        })
        .map(|(_, owner)| owner.clone());
    match same_target_owner {
        Some(owner) if owner != request.plugin_id => {
            slot_diagnostic(
                diagnostics,
                generation_id,
                request.plugin_id,
                "schema.slot_mutation_unauthorized",
                format!(
                    "{}_slot denied for `{}:{}:{}` owned by `{owner}`",
                    request.op, request.region, request.container, request.id
                ),
                mutation_api_name(request.op),
            );
            false
        }
        _ => {
            slot_diagnostic(
                diagnostics,
                generation_id,
                request.plugin_id,
                missing_code(request.op),
                format!(
                    "{}_slot target missing `{}:{}:{}`",
                    request.op, request.region, request.container, request.id
                ),
                mutation_api_name(request.op),
            );
            false
        }
    }
}

fn effective_slots(ops: &[PreparedSlotOp]) -> HashMap<SlotAddress, String> {
    let mut active = HashMap::new();
    for op in ops {
        match op {
            PreparedSlotOp::Add(p) => {
                active.insert(p.mount_address.clone(), p.plugin_id.clone());
            }
            PreparedSlotOp::Replace { target, spec, .. } => {
                active.insert(target.clone(), spec.plugin_id.clone());
            }
            PreparedSlotOp::Remove { target, .. } => {
                active.remove(target);
            }
        }
    }
    active
}

fn slot_diagnostic(
    diagnostics: &DiagnosticStore,
    generation_id: GenerationId,
    plugin_id: &str,
    code: &'static str,
    message: String,
    api_name: &'static str,
) {
    diagnostics.record(
        PluginDiagnostic::new(
            PluginId::from(plugin_id.to_string()),
            DiagnosticSeverity::Warning,
            code,
            message,
        )
        .with_generation(generation_id)
        .with_source(PluginSourceSpan::ApiFunction {
            name: api_name.into(),
        }),
    );
}

fn mutation_api_name(op: &str) -> &'static str {
    match op {
        "replace" => "leviathan.ui.slot.replace",
        _ => "leviathan.ui.slot.remove",
    }
}

fn missing_code(op: &str) -> &'static str {
    match op {
        "replace" => "schema.slot_replace_missing",
        _ => "schema.slot_remove_missing",
    }
}
