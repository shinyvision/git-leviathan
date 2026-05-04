//! Plugin settings schema and persistence helpers.
//!
//! Settings are config-surface JSON, not state. The schema is stored
//! alongside values so devtools can render a settings surface without
//! entering the plugin Lua state.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsFile {
    #[serde(default)]
    pub schema: Value,
    #[serde(default)]
    pub values: Value,
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            schema: json!({}),
            values: json!({}),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsMetadata {
    pub path: String,
    pub schema: Value,
    pub values: Value,
    pub schema_keys: Vec<String>,
    pub value_keys: Vec<String>,
    pub valid: bool,
    pub errors: Vec<String>,
}

pub fn read_settings(path: &Path) -> Result<SettingsFile, String> {
    if !path.exists() {
        return Ok(SettingsFile::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    match serde_json::from_str::<SettingsFile>(&raw) {
        Ok(file) => Ok(file),
        Err(_) => {
            let backup = path.with_extension("json.corrupt");
            fs::rename(path, backup).map_err(|e| e.to_string())?;
            Ok(SettingsFile::default())
        }
    }
}

pub fn write_settings(path: &Path, file: &SettingsFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

pub fn defaults_for_schema(schema: &Value) -> Value {
    let mut out = Map::new();
    let Some(schema) = schema.as_object() else {
        return Value::Object(out);
    };
    for (key, spec) in schema {
        if let Some(default) = spec.get("default") {
            out.insert(key.clone(), default.clone());
        }
    }
    Value::Object(out)
}

pub fn with_defaults(schema: &Value, values: &Value) -> Value {
    let mut merged = defaults_for_schema(schema);
    if let (Some(target), Some(values)) = (merged.as_object_mut(), values.as_object()) {
        for (key, value) in values {
            target.insert(key.clone(), value.clone());
        }
    }
    merged
}

pub fn validate_settings(schema: &Value, values: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let Some(schema_obj) = schema.as_object() else {
        errors.push("settings schema must be an object".to_string());
        return Err(errors);
    };
    let Some(values_obj) = values.as_object() else {
        errors.push("settings values must be an object".to_string());
        return Err(errors);
    };

    for key in values_obj.keys() {
        if !schema_obj.contains_key(key) {
            errors.push(format!("unknown setting `{key}`"));
        }
    }

    for (key, spec) in schema_obj {
        let Some(value) = values_obj.get(key) else {
            continue;
        };
        let ty = spec.get("type").and_then(Value::as_str).unwrap_or("json");
        match ty {
            "string" if !value.is_string() => errors.push(format!("`{key}` must be a string")),
            "boolean" if !value.is_boolean() => errors.push(format!("`{key}` must be a boolean")),
            "integer" if value.as_i64().is_none() => {
                errors.push(format!("`{key}` must be an integer"))
            }
            "number" if value.as_f64().is_none() => {
                errors.push(format!("`{key}` must be a number"))
            }
            "string" | "boolean" | "integer" | "number" | "json" => {}
            other => errors.push(format!("`{key}` has unsupported schema type `{other}`")),
        }

        if let Some(allowed) = spec.get("enum").and_then(Value::as_array) {
            if !allowed.iter().any(|item| item == value) {
                errors.push(format!("`{key}` is not an allowed value"));
            }
        }
        if let (Some(min), Some(n)) = (spec.get("min").and_then(Value::as_f64), value.as_f64()) {
            if n < min {
                errors.push(format!("`{key}` must be >= {min}"));
            }
        }
        if let (Some(max), Some(n)) = (spec.get("max").and_then(Value::as_f64), value.as_f64()) {
            if n > max {
                errors.push(format!("`{key}` must be <= {max}"));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn metadata(path: &Path) -> SettingsMetadata {
    let mut file = read_settings(path).unwrap_or_default();
    let values = with_defaults(&file.schema, &file.values);
    file.values = values.clone();
    let validation = validate_settings(&file.schema, &values);
    let schema_keys = object_keys(&file.schema);
    let value_keys = object_keys(&file.values);
    SettingsMetadata {
        path: path.display().to_string(),
        schema: file.schema,
        values,
        schema_keys,
        value_keys,
        valid: validation.is_ok(),
        errors: validation.err().unwrap_or_default(),
    }
}

pub fn generated_form_widget(
    schema: &Value,
    values: &Value,
) -> Option<crate::plugin::ui::widget_ast::WidgetAst> {
    let schema = schema.as_object()?;
    if schema.is_empty() {
        return None;
    }
    let mut children = Vec::new();
    for (key, spec) in schema {
        let ty = spec.get("type").and_then(Value::as_str).unwrap_or("json");
        let label = spec
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or(key.as_str());
        let value = values.get(key).cloned().unwrap_or(Value::Null);
        children.push(setting_widget(key, label, ty, spec, value));
    }
    crate::plugin::ui::widget_ast::decode(&json!({
        "kind": "form",
        "title": "Settings",
        "children": children,
    }))
    .ok()
}

fn setting_widget(key: &str, label: &str, ty: &str, spec: &Value, value: Value) -> Value {
    if let Some(options) = spec.get("enum").and_then(Value::as_array) {
        return json!({
            "kind": "select",
            "id": key,
            "label": label,
            "selected": value,
            "options": options.iter().map(|item| json!({
                "value": item,
                "label": item.as_str().map(str::to_string).unwrap_or_else(|| item.to_string()),
            })).collect::<Vec<_>>(),
        });
    }
    match ty {
        "boolean" => json!({
            "kind": "toggle",
            "id": key,
            "label": label,
            "checked": value.as_bool().unwrap_or(false),
            "value": value,
        }),
        "string" | "integer" | "number" => json!({
            "kind": "text_input",
            "id": key,
            "placeholder": label,
            "value": scalar_text(&value),
            "on_input": format!("settings.{key}.input"),
        }),
        _ => json!({
            "kind": "code",
            "id": key,
            "label": label,
            "language": "json",
            "value_text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into()),
        }),
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn object_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}
