//! Deterministic property-style tests for `widget_ast`.
//!
//! Goal: prove that *no* widget input can panic the host. The
//! decoder must either produce a valid `WidgetAst` (which the renderer
//! consumes without panicking) or return a structured
//! `WidgetDecodeError` we can attribute to a widget path.
//!
//! No proptest/arbitrary dep — a tiny seeded LCG produces ~1500 random
//! `serde_json::Value` trees up to depth 8. The renderer is exercised
//! inside `catch_unwind` so any panic (which would manifest as a real
//! test failure normally) is captured and reported.

use std::collections::HashMap;
use std::panic;

use serde_json::{json, Value};

use crate::plugin::bridge::widget_tree::{self, BuildCtx, DispatchScope};
use crate::plugin::ui::widget_ast::{decode, decode_with_limits, WidgetLimits};

/// Seeded linear congruential generator. Same numbers everywhere; no
/// `rand` dep. Constants from "Numerical Recipes" — they're for test
/// noise, not crypto, so quality only needs to be "uniform enough".
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }
    fn range(&mut self, max: u32) -> u32 {
        if max == 0 {
            0
        } else {
            (self.next() as u32) % max
        }
    }
    fn boolean(&mut self) -> bool {
        self.next() & 1 == 0
    }
    fn float(&mut self, max: f64) -> f64 {
        let n = self.next() as f64 / u64::MAX as f64;
        n * max
    }
    fn pick<'a, T>(&mut self, slice: &'a [T]) -> &'a T {
        &slice[self.range(slice.len() as u32) as usize]
    }
}

const KINDS: &[&str] = &[
    "text",
    "button",
    "row",
    "column",
    "container",
    "padding",
    "space",
    "icon",
    "image",
    "scrollable",
    "mouse_area",
    "tablist",
    "resizable_split",
    "command_button",
    "badge",
    "list",
    "table",
    "checkbox",
    "select",
    "progress",
    "grid",
    "tabs",
    "virtual_list",
    // Mix in a couple of typos to exercise the unknown_kind path.
    "rwo",
    "buttn",
    "",
];

const STRINGS: &[&str] = &[
    "hello",
    "go",
    "icons/x.svg",
    "fill",
    "shrink",
    "left",
    "center",
    "vertical",
    "",
];

const COLORS: &[&str] = &["#ff0000", "#00ff00", "#abcdef", "not-a-color", ""];

fn random_length(rng: &mut Lcg) -> Value {
    match rng.range(4) {
        0 => Value::Null,
        1 => json!(rng.float(800.0)),
        2 => Value::String("fill".into()),
        _ => Value::String("shrink".into()),
    }
}

fn random_spacing(rng: &mut Lcg) -> Value {
    if rng.boolean() {
        json!({ "token": "space.2" })
    } else {
        json!(rng.float(24.0))
    }
}

/// Produce a random tree node up to `depth_remaining` deep.
fn random_node(rng: &mut Lcg, depth_remaining: u32) -> Value {
    let kind = rng.pick(KINDS).to_string();
    let mut obj = serde_json::Map::new();
    obj.insert("kind".into(), Value::String(kind.clone()));
    // Optional id (sometimes a string, sometimes a wrong type — exercises
    // the id type-mismatch branch of the decoder).
    if rng.boolean() {
        obj.insert(
            "id".into(),
            match rng.range(4) {
                0 => Value::String(format!("id-{}", rng.next() % 1000)),
                1 => Value::Null,
                2 => Value::Number(7.into()),
                _ => json!(true),
            },
        );
    }
    match kind.as_str() {
        "text" => {
            obj.insert("value".into(), Value::String(rng.pick(STRINGS).to_string()));
            obj.insert("size".into(), json!(rng.float(40.0)));
            if rng.boolean() {
                obj.insert(
                    "color".into(),
                    if rng.boolean() {
                        json!({ "token": "text.primary" })
                    } else {
                        Value::String(rng.pick(COLORS).to_string())
                    },
                );
            }
        }
        "button" => {
            if rng.boolean() {
                obj.insert("text".into(), Value::String(rng.pick(STRINGS).to_string()));
            }
            if rng.boolean() && depth_remaining > 0 {
                obj.insert("child".into(), random_node(rng, depth_remaining - 1));
            }
            if rng.boolean() {
                obj.insert("on_click".into(), Value::String("e".into()));
            }
            obj.insert("width".into(), random_length(rng));
        }
        "row" | "column" | "resizable_split" => {
            let n = rng.range(4) as usize;
            let mut children = Vec::with_capacity(n);
            for _ in 0..n {
                if depth_remaining == 0 {
                    children.push(json!({ "kind": "space" }));
                } else {
                    children.push(random_node(rng, depth_remaining - 1));
                }
            }
            obj.insert("children".into(), Value::Array(children));
            obj.insert("spacing".into(), random_spacing(rng));
            if kind == "resizable_split" {
                obj.insert(
                    "direction".into(),
                    Value::String(
                        match rng.range(3) {
                            0 => "horizontal",
                            1 => "vertical",
                            _ => "diagonal",
                        }
                        .into(),
                    ),
                );
            }
        }
        "container" | "scrollable" | "padding" | "mouse_area" => {
            if rng.boolean() && depth_remaining > 0 {
                obj.insert("child".into(), random_node(rng, depth_remaining - 1));
            }
            if kind == "padding" {
                obj.insert("top".into(), json!(rng.float(20.0)));
                obj.insert("left".into(), json!(rng.float(20.0)));
            }
            if kind == "container" && rng.boolean() {
                obj.insert("bg".into(), Value::String(rng.pick(COLORS).to_string()));
            }
            if kind == "mouse_area" {
                obj.insert("on_click".into(), Value::String("e".into()));
                obj.insert("value".into(), json!({ "n": 1 }));
            }
        }
        "space" => {
            obj.insert("width".into(), random_length(rng));
            obj.insert("height".into(), random_length(rng));
        }
        "icon" | "image" => {
            if rng.boolean() {
                obj.insert("path".into(), Value::String(rng.pick(STRINGS).to_string()));
            } else {
                obj.insert(
                    "asset".into(),
                    json!({ "kind": "svg", "path": "icons/x.svg", "handle": "asset:svg:icons/x.svg" }),
                );
            }
            obj.insert("size".into(), json!(rng.float(64.0)));
        }
        "command_button" | "badge" | "checkbox" | "progress" => {
            obj.insert("label".into(), Value::String(rng.pick(STRINGS).to_string()));
            obj.insert("command".into(), Value::String("repository.fetch".into()));
            obj.insert("progress".into(), json!(rng.float(1.0)));
        }
        "list" => {
            obj.insert(
                "items".into(),
                json!([{ "label": "one" }, { "label": "two", "children": [{ "label": "child" }] }]),
            );
        }
        "table" => {
            obj.insert("columns".into(), json!([{ "id": "name", "title": "Name" }]));
            obj.insert("rows".into(), json!([{ "cells": ["a"] }]));
        }
        "select" => {
            obj.insert(
                "options".into(),
                json!([{ "label": "main", "value": "main" }]),
            );
            obj.insert("selected".into(), json!("main"));
        }
        "grid" | "virtual_list" => {
            obj.insert(
                "children".into(),
                json!([{ "kind": "text", "value": "row" }]),
            );
        }
        "tabs" => {
            obj.insert(
                "tabs".into(),
                json!([{ "id": "a", "title": "A", "child": { "kind": "text", "value": "A" } }]),
            );
            obj.insert("active".into(), json!("a"));
        }
        "tablist" => {
            let n = rng.range(4) as usize;
            let mut tabs = Vec::with_capacity(n);
            for i in 0..n {
                tabs.push(json!({
                    "id": format!("tab-{i}"),
                    "name": rng.pick(STRINGS).to_string(),
                }));
            }
            obj.insert("tabs".into(), Value::Array(tabs));
            obj.insert("active".into(), json!("tab-0"));
            obj.insert("orderable".into(), Value::Bool(rng.boolean()));
        }
        // Bogus kinds with random fields — catch the unknown_kind path.
        _ => {
            obj.insert("value".into(), Value::String("x".into()));
            obj.insert("children".into(), Value::Array(Vec::new()));
        }
    }
    Value::Object(obj)
}

#[test]
fn fuzz_random_trees_never_panic_renderer() {
    let splits: HashMap<String, Vec<f32>> = HashMap::new();
    let plugin_root = std::path::PathBuf::from("/tmp/fuzz-plugin");

    let mut decode_oks = 0u32;
    let mut decode_errs = 0u32;
    let mut render_panics = 0u32;

    for seed in 1..=1500u64 {
        let mut rng = Lcg::new(seed);
        let depth_remaining = rng.range(7) + 1;
        let value = random_node(&mut rng, depth_remaining);

        match decode(&value) {
            Ok(ast) => {
                decode_oks += 1;
                // Re-render inside catch_unwind so any panic surfaces
                // as a counted failure rather than aborting the test
                // run.
                let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    let ctx = BuildCtx {
                        plugin_id: "fuzz",
                        scope: DispatchScope::Screen { screen_id: "s" },
                        plugin_root: plugin_root.as_path(),
                        split_states: &splits,
                        active_drag: None,
                    };
                    let _ = widget_tree::build(&ast, &ctx);
                }));
                if result.is_err() {
                    render_panics += 1;
                }
            }
            Err(err) => {
                decode_errs += 1;
                // Every error must be one of the documented codes and
                // its path must be non-empty (the `PluginSourceSpan::
                // Widget { path }` we eventually attach has to point
                // somewhere).
                assert!(
                    matches!(
                        err.code,
                        "widget.unknown_kind"
                            | "widget.field_missing"
                            | "widget.field_type_mismatch"
                            | "widget.depth_exceeded"
                            | "widget.node_count_exceeded"
                            | "widget.string_too_long"
                            | "widget.image_too_large"
                            | "widget.asset_limit_exceeded"
                            | "widget.not_a_table"
                    ),
                    "unexpected error code: {} at {}",
                    err.code,
                    err.path
                );
                assert!(!err.path.is_empty(), "empty error path for {err}");
                assert!(
                    err.path.starts_with("root"),
                    "path not rooted: {}",
                    err.path
                );
            }
        }
    }

    assert_eq!(render_panics, 0, "renderer panicked on a valid AST");
    assert!(decode_oks + decode_errs == 1500);
    // Sanity: the corpus exercises both branches.
    assert!(decode_oks > 0, "expected at least some valid trees");
    assert!(decode_errs > 0, "expected at least some rejected trees");
}

#[test]
fn fuzz_pathological_inputs_decode_or_error() {
    let pathological: &[Value] = &[
        Value::Null,
        Value::Bool(true),
        Value::Number(42.into()),
        Value::String("not-a-table".into()),
        Value::Array(vec![Value::Null]),
        json!({}),
        json!({ "kind": Value::Null }),
        json!({ "kind": 7 }),
        json!({ "kind": "row", "children": "not-an-array" }),
        json!({ "kind": "row", "children": [{ "kind": "text", "size": "huge" }] }),
        json!({ "kind": "padding", "top": "much" }),
        json!({ "kind": "tablist", "tabs": "nope" }),
        json!({ "kind": "image", "size": [1, 2] }),
    ];
    for value in pathological {
        match decode(value) {
            Ok(_) => {}
            Err(err) => {
                assert!(err.path.starts_with("root"), "bad path: {}", err.path);
            }
        }
    }
}

#[test]
fn fuzz_extreme_depth_caught() {
    // Build a tree just over the configured depth limit (64) — enough
    // to trip the check on first recursion past it without the json!
    // macro itself blowing our stack at construction time.
    let mut node = json!({ "kind": "space" });
    for _ in 0..200 {
        node = json!({ "kind": "padding", "child": node });
    }
    let err = decode(&node).unwrap_err();
    assert_eq!(err.code, "widget.depth_exceeded");
}

#[test]
fn fuzz_extreme_node_count_caught() {
    let mut children = Vec::with_capacity(20_000);
    for _ in 0..20_000 {
        children.push(json!({ "kind": "space" }));
    }
    let tree = json!({ "kind": "row", "children": children });
    let err = decode(&tree).unwrap_err();
    assert_eq!(err.code, "widget.node_count_exceeded");
}

#[test]
fn fuzz_string_length_limit() {
    let huge = "x".repeat(200_000);
    let tree = json!({ "kind": "text", "value": huge });
    let err = decode_with_limits(&tree, WidgetLimits::DEFAULT).unwrap_err();
    assert_eq!(err.code, "widget.string_too_long");
    assert_eq!(err.path, "root.value");
}

#[test]
fn fuzz_asset_path_limit() {
    let huge = "x".repeat(10_000);
    let tree = json!({ "kind": "icon", "path": huge });
    let err = decode_with_limits(&tree, WidgetLimits::DEFAULT).unwrap_err();
    assert_eq!(err.code, "widget.asset_limit_exceeded");
    assert_eq!(err.path, "root.path");
}
