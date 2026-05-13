use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use git_leviathan_plugin_api::api_version::plugin_api_compatibility_error;
use git_leviathan_plugin_api::capability::is_known_capability;
use git_leviathan_plugin_api::descriptor::api;
use git_leviathan_plugin_api::descriptor::extension_point::extension_point;
use git_leviathan_plugin_api::descriptor::region::REGIONS;
use git_leviathan_plugin_api::descriptor::widget::WIDGETS;
use git_leviathan_plugin_api::manifest::PluginManifest;
use mlua::Lua;

use super::{has_docs, has_tests, lua_source_files, path_label, validate_plugin_id};

#[derive(Debug)]
pub(super) struct LintReport {
    pub(super) diagnostics: Vec<Diagnostic>,
}

impl LintReport {
    pub(super) fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.level == Level::Error)
    }

    fn push(&mut self, level: Level, path: impl Into<String>, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            level,
            path: path.into(),
            message: message.into(),
        });
    }
}

#[derive(Debug)]
pub(super) struct Diagnostic {
    pub(super) level: Level,
    pub(super) path: String,
    pub(super) message: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Level {
    Error,
    Warning,
}

pub(super) fn lint_path(path: &Path) -> LintReport {
    let mut report = LintReport {
        diagnostics: Vec::new(),
    };

    if !path.is_dir() {
        report.push(
            Level::Error,
            path.display().to_string(),
            "plugin path must be a directory",
        );
        return report;
    }

    let manifest_path = path.join("plugin.toml");
    let manifest_raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(e) => {
            report.push(
                Level::Error,
                manifest_path.display().to_string(),
                format!("failed to read manifest: {e}"),
            );
            return report;
        }
    };

    let manifest_value: Option<toml::Value> = match toml::from_str(&manifest_raw) {
        Ok(value) => Some(value),
        Err(e) => {
            report.push(
                Level::Error,
                manifest_path.display().to_string(),
                format!("manifest TOML is invalid: {e}"),
            );
            None
        }
    };

    let manifest: Option<PluginManifest> = match toml::from_str::<PluginManifest>(&manifest_raw) {
        Ok(manifest) => {
            if let Err(e) = validate_plugin_id(&manifest.id) {
                report.push(Level::Error, "plugin.toml", e.to_string());
            }
            if let Some(message) = plugin_api_compatibility_error(manifest.api_version) {
                report.push(Level::Error, "plugin.toml", message);
            }
            Some(manifest)
        }
        Err(e) => {
            report.push(
                Level::Error,
                "plugin.toml",
                format!("manifest schema error: {e}"),
            );
            None
        }
    };

    if let Some(value) = &manifest_value {
        lint_manifest_capabilities(value, &mut report);
    }

    if !path.join("init.lua").is_file() {
        report.push(Level::Error, "init.lua", "missing plugin entry point");
    }

    if !has_docs(path) {
        report.push(
            Level::Error,
            path.display().to_string(),
            "missing docs: add README.md, docs/*.md, or doc/*.md",
        );
    }
    if !has_tests(path) {
        report.push(
            Level::Error,
            path.display().to_string(),
            "missing tests: add tests/*.lua or tests/*.md",
        );
    }

    let source_files = match lua_source_files(path, false) {
        Ok(files) => files,
        Err(e) => {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("failed to scan Lua files: {e}"),
            );
            Vec::new()
        }
    };

    let declared_caps: BTreeSet<String> = manifest
        .as_ref()
        .map(|m| {
            m.capabilities
                .iter()
                .cloned()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    let provides = manifest
        .as_ref()
        .map(|m| {
            m.provides_services
                .iter()
                .map(|s| s.key())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let consumes = manifest
        .as_ref()
        .map(|m| {
            m.consumes_services
                .iter()
                .map(|s| s.key())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    for file in source_files {
        match fs::read_to_string(&file) {
            Ok(source) => {
                lint_lua_syntax(&file, &source, &mut report);
                lint_unknown_api_calls(&file, &source, &mut report);
                lint_undeclared_capabilities(&file, &source, &declared_caps, &mut report);
                lint_widgets(&file, &source, &mut report);
                lint_decoration_specs(&file, &source, &mut report);
                lint_slot_specs(&file, &source, &declared_caps, &mut report);
                lint_contribute_specs(&file, &source, &declared_caps, &mut report);
                lint_command_ids(&file, &source, &declared_caps, &mut report);
                lint_context_fields(&file, &source, &mut report);
                lint_services(&file, &source, &provides, &consumes, &mut report);
            }
            Err(e) => report.push(
                Level::Error,
                file.display().to_string(),
                format!("failed to read Lua source: {e}"),
            ),
        }
    }

    report
}

fn lint_manifest_capabilities(value: &toml::Value, report: &mut LintReport) {
    let Some(capabilities) = value.get("capabilities") else {
        return;
    };
    let Some(items) = capabilities.as_array() else {
        report.push(
            Level::Error,
            "plugin.toml",
            "capabilities must be an array of strings",
        );
        return;
    };
    for item in items {
        match item.as_str() {
            Some(capability) if is_known_capability(capability) => {}
            Some(capability) => report.push(
                Level::Error,
                "plugin.toml",
                format!("unknown capability `{capability}`"),
            ),
            None => report.push(
                Level::Error,
                "plugin.toml",
                "capabilities entries must be strings",
            ),
        }
    }
}

fn lint_lua_syntax(path: &Path, source: &str, report: &mut LintReport) {
    let lua = Lua::new();
    if let Err(e) = lua.load(source).set_name(path_label(path)).into_function() {
        report.push(
            Level::Error,
            path.display().to_string(),
            format!("Lua syntax error: {e}"),
        );
    }
}

fn lint_unknown_api_calls(path: &Path, source: &str, report: &mut LintReport) {
    let known = known_function_paths();
    for call in detect_leviathan_calls(source) {
        if !known.contains(call.as_str()) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("unknown Leviathan API call `{call}`"),
            );
        }
    }
}

fn lint_undeclared_capabilities(
    path: &Path,
    source: &str,
    declared: &BTreeSet<String>,
    report: &mut LintReport,
) {
    for function in api::all_functions() {
        if function.capabilities.is_empty() || !source_calls_path(source, function.path) {
            continue;
        }
        if matches!(
            function.path,
            "leviathan.command.invoke" | "leviathan.log" | "leviathan.ui.contribute"
        ) {
            continue;
        }
        for required in function.capabilities {
            if required.contains("<region>")
                || required.contains("<container>")
                || required.contains("<id>")
            {
                continue;
            }
            if !capability_declared(required, declared) {
                report.push(
                    Level::Error,
                    path.display().to_string(),
                    format!(
                        "`{}` requires undeclared capability `{}`",
                        function.path, required
                    ),
                );
            }
        }
    }
}

fn lint_services(
    path: &Path,
    source: &str,
    provides: &BTreeSet<String>,
    consumes: &BTreeSet<String>,
    report: &mut LintReport,
) {
    for service in detect_service_calls(source, "leviathan.services.register") {
        if !provides.contains(&service) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("service provider `{service}` is missing from provides_services"),
            );
        }
    }
    for service in detect_service_calls(source, "leviathan.services.get") {
        if !consumes.contains(&service) && !provides.contains(&service) {
            report.push(
                Level::Warning,
                path.display().to_string(),
                format!("service consumer `{service}` is missing from consumes_services"),
            );
        }
    }
}

fn lint_widgets(path: &Path, source: &str, report: &mut LintReport) {
    for table in find_widget_tables(source) {
        let Some(kind) = top_level_string_field(&table.body, "kind") else {
            continue;
        };
        if is_decoration_table(&table.body, &kind) {
            continue;
        }
        let Some(desc) = WIDGETS.get(&kind) else {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("unknown widget kind `{kind}`"),
            );
            continue;
        };
        let allowed = desc
            .fields
            .iter()
            .map(|field| field.name)
            .chain(std::iter::once("kind"))
            .chain(["id", "width", "height"])
            .collect::<BTreeSet<_>>();
        for field in top_level_fields(&table.body) {
            if !allowed.contains(field.as_str()) {
                report.push(
                    Level::Error,
                    path.display().to_string(),
                    format!("widget `{kind}` has invalid field `{field}`"),
                );
            }
        }
    }
}

fn lint_slot_specs(
    path: &Path,
    source: &str,
    declared: &BTreeSet<String>,
    report: &mut LintReport,
) {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for table in find_call_tables(source, "leviathan.ui.slot.add") {
        let Some(region) = top_level_string_field(&table.body, "region") else {
            continue;
        };
        let pane = top_level_string_field(&table.body, "pane");
        let section = top_level_string_field(&table.body, "section");
        let id = top_level_string_field(&table.body, "id").unwrap_or_default();
        lint_region_address(path, &region, pane.as_deref(), section.as_deref(), report);
        let required = format!("ui:region:{region}");
        if !capability_declared(&required, declared) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("slot `{id}` requires undeclared capability `{required}`"),
            );
        }
        if !id.is_empty() {
            let key = format!(
                "{}:{}:{}:{}",
                region,
                pane.as_deref().unwrap_or(""),
                section.as_deref().unwrap_or(""),
                id
            );
            let count = seen.entry(key.clone()).or_default();
            *count += 1;
            if *count == 2 {
                report.push(
                    Level::Error,
                    path.display().to_string(),
                    format!("slot id collision at `{key}`"),
                );
            }
        }
    }

    for table in find_call_tables(source, "leviathan.ui.slot.replace") {
        let Some(region) = top_level_string_field(&table.body, "region") else {
            continue;
        };
        let pane = top_level_string_field(&table.body, "pane");
        let section = top_level_string_field(&table.body, "section");
        lint_region_address(path, &region, pane.as_deref(), section.as_deref(), report);
        let required = format!("ui:region:{region}");
        if !capability_declared(&required, declared) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("slot replace requires undeclared capability `{required}`"),
            );
        }
    }
}

fn lint_contribute_specs(
    path: &Path,
    source: &str,
    declared: &BTreeSet<String>,
    report: &mut LintReport,
) {
    for point_id in detect_string_first_arg(source, "leviathan.ui.contribute") {
        let Some(point) = extension_point(&point_id) else {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("unknown extension point `{point_id}`"),
            );
            continue;
        };
        for required in point.capabilities {
            if !capability_declared(required, declared) {
                report.push(
                    Level::Error,
                    path.display().to_string(),
                    format!(
                        "ui contribution `{point_id}` requires undeclared capability `{required}`"
                    ),
                );
            }
        }
    }
}

fn lint_region_address(
    path: &Path,
    region: &str,
    pane: Option<&str>,
    section: Option<&str>,
    report: &mut LintReport,
) {
    match REGIONS.get(region) {
        Some(desc) => {
            if let Err(e) = desc.validate_address(pane, section) {
                report.push(Level::Error, path.display().to_string(), e);
            }
        }
        None => report.push(
            Level::Error,
            path.display().to_string(),
            format!("unknown UI region `{region}`"),
        ),
    }
}

fn lint_command_ids(
    path: &Path,
    source: &str,
    declared: &BTreeSet<String>,
    report: &mut LintReport,
) {
    let mut known = built_in_command_ids();
    known.extend(detect_string_first_arg(source, "leviathan.command.create"));

    for id in detect_string_first_arg(source, "leviathan.command.invoke") {
        if !known.contains(&id) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("unknown command id `{id}`"),
            );
        }
        let required = format!("command:invoke:{id}");
        if !capability_declared(&required, declared) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("command invoke requires undeclared capability `{required}`"),
            );
        }
    }
    for id in detect_keymap_command_ids(source) {
        if !known.contains(&id) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("unknown command id `{id}`"),
            );
        }
    }
    for table in find_call_tables(source, "leviathan.ui.context_menu") {
        if let Some(command) = top_level_string_field(&table.body, "command") {
            if !known.contains(&command) {
                report.push(
                    Level::Error,
                    path.display().to_string(),
                    format!("unknown command id `{command}`"),
                );
            }
        }
    }
}

fn lint_context_fields(path: &Path, source: &str, report: &mut LintReport) {
    let allowed = context_field_map();
    for access in detect_context_field_accesses(source) {
        let Some(fields) = allowed.get(access.prefix.as_str()) else {
            continue;
        };
        if !fields.contains(access.field.as_str()) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!(
                    "invalid context field `{}` on `{}`",
                    access.field, access.prefix
                ),
            );
        }
    }
}

fn is_decoration_table(body: &str, kind: &str) -> bool {
    let fields = top_level_fields(body).into_iter().collect::<BTreeSet<_>>();
    match kind {
        "badge" => ["text", "fg", "bg"]
            .iter()
            .any(|field| fields.contains(*field)),
        "icon" => fields.contains("glyph"),
        "marker" => fields.contains("shape"),
        "lane" => fields.contains("index"),
        "line_hint" => fields.contains("severity") || fields.contains("file"),
        "hunk_badge" => fields.contains("hunk_id"),
        "line_gutter" => fields.contains("glyph") || fields.contains("file"),
        _ => false,
    }
}

fn lint_decoration_specs(path: &Path, source: &str, report: &mut LintReport) {
    for table in find_call_argument_tables(source, "leviathan.ui.graph_decoration") {
        lint_decoration_fields(path, "graph", &table.body, report);
    }
    for table in find_call_argument_tables(source, "leviathan.ui.diff_decoration") {
        lint_decoration_fields(path, "diff", &table.body, report);
    }
}

fn lint_decoration_fields(path: &Path, family: &str, body: &str, report: &mut LintReport) {
    let Some(kind) = top_level_string_field(body, "kind") else {
        report.push(
            Level::Error,
            path.display().to_string(),
            format!("{family} decoration is missing string field `kind`"),
        );
        return;
    };

    let (allowed, required): (&[&str], &[&str]) = match (family, kind.as_str()) {
        ("graph", "badge") => (&["id", "kind", "text", "fg", "bg"], &["kind", "text"]),
        ("graph", "icon") => (&["id", "kind", "glyph", "color"], &["kind", "glyph"]),
        ("graph", "marker") => (
            &["id", "kind", "shape", "color"],
            &["kind", "shape", "color"],
        ),
        ("graph", "lane") => (
            &["id", "kind", "index", "color"],
            &["kind", "index", "color"],
        ),
        ("diff", "line_hint") => (
            &["id", "kind", "severity", "text", "file", "line"],
            &["kind", "severity", "text", "file", "line"],
        ),
        ("diff", "hunk_badge") => (
            &["id", "kind", "hunk_id", "label", "color"],
            &["kind", "hunk_id", "label"],
        ),
        ("diff", "line_gutter") => (
            &["id", "kind", "file", "line", "glyph", "color"],
            &["kind", "file", "line", "glyph"],
        ),
        _ => {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("unknown {family} decoration kind `{kind}`"),
            );
            return;
        }
    };

    let fields = top_level_fields(body).into_iter().collect::<BTreeSet<_>>();
    for field in &fields {
        if !allowed.contains(&field.as_str()) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("{family} decoration `{kind}` has invalid field `{field}`"),
            );
        }
    }
    for field in required {
        if !fields.contains(*field) {
            report.push(
                Level::Error,
                path.display().to_string(),
                format!("{family} decoration `{kind}` is missing field `{field}`"),
            );
        }
    }
}

#[derive(Debug)]
struct LuaTable {
    body: String,
}

fn find_widget_tables(source: &str) -> Vec<LuaTable> {
    let mut tables = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = matching_brace(source, i) {
                let body = &source[i + 1..end];
                if top_level_string_field(body, "kind").is_some() {
                    tables.push(LuaTable {
                        body: body.to_string(),
                    });
                }
                i = end;
            }
        }
        i += 1;
    }
    tables
}

fn find_call_tables(source: &str, path: &str) -> Vec<LuaTable> {
    let mut tables = Vec::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(path) {
        let start = offset + found + path.len();
        let bytes = source.as_bytes();
        let mut i = start;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'(' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
        if i < bytes.len() && bytes[i] == b'{' {
            if let Some(end) = matching_brace(source, i) {
                tables.push(LuaTable {
                    body: source[i + 1..end].to_string(),
                });
                offset = end + 1;
                continue;
            }
        }
        offset = start;
    }
    tables
}

fn find_call_argument_tables(source: &str, path: &str) -> Vec<LuaTable> {
    let mut tables = Vec::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(path) {
        let start = offset + found + path.len();
        let bytes = source.as_bytes();
        let mut i = start;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let call_end = if i < bytes.len() && bytes[i] == b'(' {
            matching_paren(source, i).unwrap_or(source.len())
        } else {
            source.len()
        };
        let mut quote = None;
        while i < call_end {
            let b = bytes[i];
            if let Some(q) = quote {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
                i += 1;
                continue;
            }
            match b {
                b'\'' | b'"' => quote = Some(b),
                b'{' => {
                    if let Some(end) = matching_brace(source, i) {
                        tables.push(LuaTable {
                            body: source[i + 1..end].to_string(),
                        });
                        i = end;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        offset = call_end.saturating_add(1);
    }
    tables
}

fn matching_brace(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = start;
    let mut quote = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => quote = Some(b),
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn matching_paren(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = start;
    let mut quote = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => quote = Some(b),
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn top_level_fields(body: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut quote = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => quote = Some(b),
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'_' | b'a'..=b'z' | b'A'..=b'Z' if depth == 0 => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && matches!(bytes[i], b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')
                {
                    i += 1;
                }
                let name = &body[start..i];
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'=' {
                    fields.push(name.to_string());
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    fields
}

fn top_level_string_field(body: &str, field: &str) -> Option<String> {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut quote = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => quote = Some(b),
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'_' | b'a'..=b'z' | b'A'..=b'Z' if depth == 0 => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && matches!(bytes[i], b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')
                {
                    i += 1;
                }
                if &body[start..i] != field {
                    continue;
                }
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j >= bytes.len() || bytes[j] != b'=' {
                    continue;
                }
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                return parse_lua_string(&body[j..]).map(|(value, _)| value);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn known_function_paths() -> BTreeSet<&'static str> {
    api::all_functions().map(|function| function.path).collect()
}

pub(super) fn detect_leviathan_calls(source: &str) -> BTreeSet<String> {
    let mut calls = BTreeSet::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while let Some(offset) = source[i..].find("leviathan.") {
        let start = i + offset;
        let mut end = start + "leviathan".len();
        while end < bytes.len() {
            if bytes[end] == b'.' {
                end += 1;
                if end >= bytes.len() || !is_ident_start(bytes[end]) {
                    break;
                }
                end += 1;
                while end < bytes.len() && is_ident_continue(bytes[end]) {
                    end += 1;
                }
            } else {
                break;
            }
        }
        let mut j = end;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < bytes.len() && (bytes[j] == b'(' || bytes[j] == b'{') {
            calls.insert(source[start..end].to_string());
        }
        i = end;
    }
    calls
}

fn source_calls_path(source: &str, path: &str) -> bool {
    let Some(mut i) = source.find(path) else {
        return false;
    };
    while i < source.len() {
        let after = i + path.len();
        let mut j = after;
        let bytes = source.as_bytes();
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < bytes.len() && (bytes[j] == b'(' || bytes[j] == b'{') {
            return true;
        }
        let Some(next) = source[after..].find(path) else {
            return false;
        };
        i = after + next;
    }
    false
}

fn is_ident_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic()
}

fn is_ident_continue(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit()
}

fn capability_declared(required: &str, declared: &BTreeSet<String>) -> bool {
    if required.ends_with(":*") {
        let prefix = required.trim_end_matches('*');
        return declared.iter().any(|cap| cap.starts_with(prefix));
    }
    match required {
        "ui:region:*" => declared.iter().any(|cap| cap.starts_with("ui:region:")),
        "fs:read" => declared.iter().any(|cap| cap.starts_with("fs:read")),
        "fs:write:*" => declared.iter().any(|cap| cap.starts_with("fs:write")),
        "fs:watch" => declared.iter().any(|cap| cap.starts_with("fs:watch")),
        "env" => declared
            .iter()
            .any(|cap| cap == "env" || cap.starts_with("env:")),
        required => declared.contains(required),
    }
}

fn detect_service_calls(source: &str, path: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(path) {
        let start = offset + found + path.len();
        let Some(paren) = source[start..].find('(').map(|p| start + p + 1) else {
            break;
        };
        let rest = source[paren..].trim_start();
        let Some((name, consumed)) = parse_lua_string(rest) else {
            offset = paren;
            continue;
        };
        let tail = rest[consumed..].trim_start();
        let service = if name.contains('@') {
            name
        } else if let Some(version) = tail
            .strip_prefix(',')
            .and_then(|s| s.trim_start().split(|c: char| !c.is_ascii_digit()).next())
            .filter(|s| !s.is_empty())
        {
            format!("{name}@{version}")
        } else {
            offset = paren;
            continue;
        };
        out.insert(service);
        offset = paren;
    }
    out
}

fn detect_string_first_arg(source: &str, path: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(path) {
        let start = offset + found + path.len();
        let Some(paren) = source[start..].find('(').map(|p| start + p + 1) else {
            break;
        };
        let rest = source[paren..].trim_start();
        if let Some((name, _)) = parse_lua_string(rest) {
            out.insert(name);
        }
        offset = paren;
    }
    out
}

fn detect_keymap_command_ids(source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find("leviathan.keymap.set") {
        let start = offset + found + "leviathan.keymap.set".len();
        let Some(paren) = source[start..].find('(').map(|p| start + p + 1) else {
            break;
        };
        let mut rest = source[paren..].trim_start();
        for arg_index in 0..3 {
            let Some((value, consumed)) = parse_lua_string(rest) else {
                break;
            };
            if arg_index == 2 {
                out.insert(value);
                break;
            }
            rest = rest[consumed..].trim_start();
            let Some(tail) = rest.strip_prefix(',') else {
                break;
            };
            rest = tail.trim_start();
        }
        offset = paren;
    }
    out
}

fn built_in_command_ids() -> BTreeSet<String> {
    [
        "repository.open",
        "repository.refresh",
        "repository.fetch",
        "repository.pull",
        "repository.push",
        "repository.open_search",
        "repository.jump_top",
        "repository.jump_bottom",
        "repository.stage_selected_file",
        "repository.unstage_selected_file",
        "repository.discard_selected_file",
        "repository.focus_panel",
        "branch.merge",
        "git.checkout",
        "git.create_branch",
        "git.delete_branch",
        "git.create_tag",
        "git.delete_tag",
        "git.commit",
        "git.stash_push",
        "git.stash_pop",
        "git.reset",
        "git.fetch",
        "git.push",
        "git.merge",
        "git.rebase",
        "git.stage",
        "git.blame",
        "git.discard",
        "diff.copy_path",
        "ui.dock.open",
        "ui.dock.close",
        "ui.dock.move",
        "ui.dock.reset_layout",
        "plugin.reload",
        "plugin.disable",
        "plugin.enable",
        "plugin.open_log",
        "plugin.inspect_ui_tree",
        "plugin.inspect_ui_context",
        "plugin.inspect_dock_layout",
        "plugin.run_health_check",
        "plugin.clear_state",
        "plugin.export_diagnostic_bundle",
        "plugin.show_capability_audit",
        "plugin.show_runtime_path",
        "plugin_ui.toggle_contribution",
        "plugin_ui.reset_layout",
        "plugin_ui.inspect_contribution",
        "command_palette.open",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug)]
struct ContextAccess {
    prefix: String,
    field: String,
}

fn detect_context_field_accesses(source: &str) -> Vec<ContextAccess> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < bytes.len() && is_ident_continue(bytes[i]) {
            i += 1;
        }
        let ident = &source[start..i];
        if ident != "ctx" && ident != "context" {
            continue;
        }
        let mut parts = vec![ident.to_string()];
        while i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            if i >= bytes.len() || !is_ident_start(bytes[i]) {
                break;
            }
            let field_start = i;
            i += 1;
            while i < bytes.len() && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let field = source[field_start..i].to_string();
            let prefix = parts.join(".");
            out.push(ContextAccess {
                prefix,
                field: field.clone(),
            });
            parts.push(field);
        }
    }
    out
}

fn context_field_map() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    let mut map = BTreeMap::new();
    map.insert(
        "ctx",
        [
            "version",
            "plugin_id",
            "generation_id",
            "type",
            "surface",
            "features",
            "theme",
            "repository",
            "tab",
            "selection",
            "focus",
            "viewport",
            "payload",
            "schema",
            "values",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "context",
        [
            "version",
            "plugin_id",
            "generation_id",
            "type",
            "surface",
            "features",
            "theme",
            "repository",
            "tab",
            "selection",
            "focus",
            "viewport",
            "payload",
            "schema",
            "values",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "ctx.repository",
        [
            "is_open",
            "name",
            "workdir_path",
            "current_branch_name",
            "head_hash",
            "default_remote_name",
            "has_remote",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "ctx.tab",
        ["is_open", "id", "path", "name", "index", "count"]
            .into_iter()
            .collect(),
    );
    map.insert(
        "ctx.selection",
        [
            "available",
            "kind",
            "selected_commit_id",
            "selected_file_path",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "ctx.focus",
        [
            "surface",
            "kind",
            "region",
            "pane",
            "section",
            "plugin_id",
            "screen_id",
            "overlay_id",
            "reason",
            "matches_surface",
            "matches_region",
            "matches_pane",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "ctx.viewport",
        ["known", "width", "height"].into_iter().collect(),
    );
    map.insert(
        "ctx.theme",
        ["name", "colors", "dimensions", "fonts"]
            .into_iter()
            .collect(),
    );
    map.insert(
        "context.repository",
        [
            "is_open",
            "name",
            "workdir_path",
            "current_branch_name",
            "head_hash",
            "default_remote_name",
            "has_remote",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "context.tab",
        ["is_open", "id", "path", "name", "index", "count"]
            .into_iter()
            .collect(),
    );
    map.insert(
        "context.selection",
        [
            "available",
            "kind",
            "selected_commit_id",
            "selected_file_path",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "context.focus",
        [
            "surface",
            "kind",
            "region",
            "pane",
            "section",
            "plugin_id",
            "screen_id",
            "overlay_id",
            "reason",
            "matches_surface",
            "matches_region",
            "matches_pane",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "context.viewport",
        ["known", "width", "height"].into_iter().collect(),
    );
    map.insert(
        "context.theme",
        ["name", "colors", "dimensions", "fonts"]
            .into_iter()
            .collect(),
    );
    map
}

fn parse_lua_string(source: &str) -> Option<(String, usize)> {
    let quote = source.as_bytes().first().copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for (i, ch) in source[1..].char_indices() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch as u8 == quote {
            return Some((out, i + 2));
        } else {
            out.push(ch);
        }
    }
    None
}

pub(super) fn print_lint_report(report: &LintReport) {
    if report.diagnostics.is_empty() {
        println!("lint ok");
        return;
    }
    for diagnostic in &report.diagnostics {
        let level = match diagnostic.level {
            Level::Error => "error",
            Level::Warning => "warning",
        };
        println!("{level}: {}: {}", diagnostic.path, diagnostic.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unknown_api_calls() {
        let calls = detect_leviathan_calls("leviathan.fs.read_file('x')\nleviathan.nope()");
        assert!(calls.contains("leviathan.fs.read_file"));
        assert!(calls.contains("leviathan.nope"));
    }

    #[test]
    fn validates_widget_fields() {
        let mut report = LintReport {
            diagnostics: Vec::new(),
        };
        lint_widgets(
            Path::new("init.lua"),
            r#"{ kind = "text", value = "ok", padding = 4 }"#,
            &mut report,
        );
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("invalid field `padding`")));
    }

    #[test]
    fn validates_unknown_widget_kind() {
        let mut report = LintReport {
            diagnostics: Vec::new(),
        };
        lint_widgets(
            Path::new("init.lua"),
            r#"{ kind = "wat", value = "ok" }"#,
            &mut report,
        );
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unknown widget kind `wat`")));
    }

    #[test]
    fn validates_decoration_specs_separately_from_widgets() {
        let mut report = LintReport {
            diagnostics: Vec::new(),
        };
        lint_widgets(
            Path::new("init.lua"),
            r##"leviathan.ui.graph_decoration("HEAD", { kind = "badge", text = "HEAD", fg = "#fff", bg = "#000" })"##,
            &mut report,
        );
        lint_decoration_specs(
            Path::new("init.lua"),
            r##"
            leviathan.ui.graph_decoration("HEAD", { kind = "badge", label = "HEAD", color = "#fff" })
            leviathan.ui.diff_decoration({ kind = "line_gutter", file = "src/main.rs", line = 3, glyph = "!" })
            "##,
            &mut report,
        );
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("missing field `text`")));
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("invalid field `label`")));
        assert!(!report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("widget `badge` has invalid field `fg`")));
    }

    #[test]
    fn validates_slot_region_capability_and_collision() {
        let mut report = LintReport {
            diagnostics: Vec::new(),
        };
        lint_slot_specs(
            Path::new("init.lua"),
            r#"
            leviathan.ui.slot.add{ region = "missing", section = "left", id = "a", widget = { kind = "text" } }
            leviathan.ui.slot.add{ region = "main_bar", section = "left", id = "dup", widget = { kind = "text" } }
            leviathan.ui.slot.add{ region = "main_bar", section = "left", id = "dup", widget = { kind = "text" } }
            "#,
            &BTreeSet::new(),
            &mut report,
        );
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unknown UI region `missing`")));
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("ui:region:main_bar")));
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("slot id collision")));
    }

    #[test]
    fn validates_contribute_capability_by_extension_point() {
        let mut missing = LintReport {
            diagnostics: Vec::new(),
        };
        lint_contribute_specs(
            Path::new("init.lua"),
            r#"leviathan.ui.contribute("repository.graph.chrome", { id = "focus", widget = function() return nil end })"#,
            &BTreeSet::new(),
            &mut missing,
        );
        assert!(missing
            .diagnostics
            .iter()
            .any(|d| d.message.contains("ui:chrome:repository.graph.chrome")));
        assert!(!missing
            .diagnostics
            .iter()
            .any(|d| d.message.contains("ui:decoration:graph")));

        let mut declared = BTreeSet::new();
        declared.insert("ui:chrome:repository.graph.chrome".to_string());
        let mut ok = LintReport {
            diagnostics: Vec::new(),
        };
        lint_contribute_specs(
            Path::new("init.lua"),
            r#"leviathan.ui.contribute("repository.graph.chrome", { id = "focus", widget = function() return nil end })"#,
            &declared,
            &mut ok,
        );
        assert!(ok.diagnostics.is_empty());
    }

    #[test]
    fn lint_rejects_incompatible_manifest_api_version() {
        let tmp =
            std::env::temp_dir().join(format!("git-leviathan-xtask-lint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("plugin.toml"),
            r#"
id = "future"
name = "Future"
version = "0.1.0"
api_version = "1.1"
"#,
        )
        .unwrap();
        std::fs::write(tmp.join("init.lua"), "").unwrap();
        std::fs::write(tmp.join("README.md"), "# Future\n").unwrap();
        std::fs::create_dir_all(tmp.join("tests")).unwrap();
        std::fs::write(tmp.join("tests").join("smoke.lua"), "").unwrap();

        let report = lint_path(&tmp);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("supported plugin api versions: 1.0")));
        assert!(report.has_errors());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn validates_unknown_command_ids_and_context_fields() {
        let mut report = LintReport {
            diagnostics: Vec::new(),
        };
        lint_command_ids(
            Path::new("init.lua"),
            r#"leviathan.command.create("local.ok", { run = function() end })
               leviathan.command.invoke("missing.cmd")
               leviathan.keymap.set("global", "x", "local.ok")
               leviathan.keymap.set("repository", "l", "repository.focus_panel")"#,
            &BTreeSet::new(),
            &mut report,
        );
        lint_context_fields(
            Path::new("init.lua"),
            "return ctx.repository.nope .. tostring(ctx.focus.matches_pane)",
            &mut report,
        );
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unknown command id `missing.cmd`")));
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("invalid context field `nope`")));
    }
}
