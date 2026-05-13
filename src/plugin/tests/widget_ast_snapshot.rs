//! Snapshot tests for the typed widget AST.
//!
//! Each test pins the textual snapshot of a canonical AST so a
//! regression in the decoder surfaces as a small, easy-to-eyeball diff.
//! Hand-rolled (no `insta` dep) — `widget_ast::snapshot` produces a
//! stable, line-oriented form that we compare against an inline literal.

use serde_json::json;

use crate::plugin::ui::widget_ast::{decode, snapshot};

#[test]
fn snapshot_text_with_explicit_id() {
    let ast =
        decode(&json!({ "kind": "text", "id": "greeting", "value": "hello", "size": 18 })).unwrap();
    let snap = snapshot(&ast);
    assert_eq!(
        snap,
        "text id=*greeting value=\"hello\" size=18 color=none\n"
    );
}

#[test]
fn snapshot_complex_screen_tree() {
    let ast = decode(&json!({
        "kind": "column",
        "id": "screen-root",
        "spacing": 6,
        "children": [
            {
                "kind": "row",
                "spacing": 4,
                "children": [
                    { "kind": "icon", "path": "icons/home.svg", "size": 16 },
                    { "kind": "text", "value": "Home" }
                ]
            },
            {
                "kind": "scrollable",
                "child": {
                    "kind": "container",
                    "bg": "#11181f",
                    "max_width": 400,
                    "child": { "kind": "padding", "top": 8, "left": 8, "child": {
                        "kind": "text", "value": "body", "size": 14
                    }}
                }
            }
        ]
    }))
    .unwrap();
    let snap = snapshot(&ast);
    let expected = "\
column id=*screen-root spacing=6 width=auto height=auto align_x=None
  row id=root.children[0] spacing=4 width=auto height=auto align_y=None
    icon id=root.children[0].children[0] path=\"icons/home.svg\" size=16 color=none
    text id=root.children[0].children[1] value=\"Home\" size=14 color=none
  scrollable id=root.children[1] width=auto height=auto
    container id=root.children[1].child bg=\"#11181f\" width=auto height=auto center=(false,false)
      padding id=root.children[1].child.child top=8 right=0 bottom=0 left=8 width=auto height=auto
        text id=root.children[1].child.child.child value=\"body\" size=14 color=none
";
    assert_eq!(snap, expected);
}

#[test]
fn snapshot_scrollable_carries_scroll_request() {
    let ast = decode(&json!({
        "kind": "scrollable",
        "id": "branches",
        "scroll_y": 84,
        "height": 240,
        "child": { "kind": "text", "value": "body" }
    }))
    .unwrap();
    let snap = snapshot(&ast);
    let expected = "\
scrollable id=*branches width=auto height=fixed(240) scroll_id=\"branches\" scroll_y=84
  text id=root.child value=\"body\" size=14 color=none
";
    assert_eq!(snap, expected);
}

#[test]
fn snapshot_button_with_style_and_action() {
    let ast = decode(&json!({
        "kind": "button",
        "id": "go",
        "on_click": "go",
        "value": { "what": 1 },
        "width": "fill",
        "child": { "kind": "text", "value": "Go" }
    }))
    .unwrap();
    let snap = snapshot(&ast);
    let expected = "\
button id=*go text=None on_click=Some(\"go\") width=fill height=auto
  text id=root.child value=\"Go\" size=14 color=none
";
    assert_eq!(snap, expected);
}

#[test]
fn snapshot_resizable_split_preserves_direction_and_id() {
    let ast = decode(&json!({
        "kind": "resizable_split",
        "id": "main",
        "direction": "vertical",
        "children": [
            { "kind": "container" },
            { "kind": "container" }
        ]
    }))
    .unwrap();
    let snap = snapshot(&ast);
    let expected = "\
resizable_split id=*main split_id=Some(\"main\") direction=Vertical
  container id=root.children[0] bg=none width=auto height=auto center=(false,false)
  container id=root.children[1] bg=none width=auto height=auto center=(false,false)
";
    assert_eq!(snap, expected);
}

#[test]
fn snapshot_tablist_carries_active_and_handlers() {
    let ast = decode(&json!({
        "kind": "tablist",
        "tabs": [
            { "id": "a", "name": "A" },
            { "id": "b", "name": "B" }
        ],
        "active": "a",
        "orderable": true,
        "on_select": "select",
        "on_close": "close",
        "on_reorder": "reorder"
    }))
    .unwrap();
    let snap = snapshot(&ast);
    assert!(snap.contains("tablist id=root active=\"a\" orderable=true"));
    assert!(snap.contains("on_select=Some(\"select\")"));
    assert!(snap.contains("tab[0] id=\"a\" name=\"A\""));
    assert!(snap.contains("tab[1] id=\"b\" name=\"B\""));
}
