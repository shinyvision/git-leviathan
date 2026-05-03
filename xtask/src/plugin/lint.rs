use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use git_leviathan_plugin_api::capability::is_known_capability;
use git_leviathan_plugin_api::descriptor::api;
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
        for required in function.capabilities {
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
        let Some(desc) = WIDGETS.get(&kind) else {
            continue;
        };
        let allowed = desc
            .fields
            .iter()
            .map(|field| field.name)
            .chain(std::iter::once("kind"))
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
}
