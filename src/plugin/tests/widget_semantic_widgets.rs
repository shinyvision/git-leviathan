use std::collections::HashMap;

use serde_json::{json, Value};

use crate::plugin::bridge::widget_tree::{self, BuildCtx, DispatchScope};
use crate::plugin::ui::widget_ast::{decode, WidgetNode};

fn samples() -> Vec<(&'static str, Value)> {
    vec![
        (
            "command_button",
            json!({"kind":"command_button","label":"Fetch","command":"repository.fetch"}),
        ),
        (
            "toolbar_button",
            json!({"kind":"toolbar_button","label":"Search","on_click":"search"}),
        ),
        ("status_item", json!({"kind":"status_item","label":"Ready"})),
        (
            "badge",
            json!({"kind":"badge","label":"OK","color":{"token":"accent.green"}}),
        ),
        ("tag", json!({"kind":"tag","label":"v1"})),
        (
            "list",
            json!({"kind":"list","items":[{"label":"one"},{"label":"two"}]}),
        ),
        (
            "tree",
            json!({"kind":"tree","items":[{"label":"src","children":[{"label":"main.rs"}]}]}),
        ),
        (
            "table",
            json!({"kind":"table","columns":[{"id":"name","title":"Name"}],"rows":[{"cells":["README.md"]}]}),
        ),
        (
            "section",
            json!({"kind":"section","title":"Files","children":[{"kind":"text","value":"a"}]}),
        ),
        (
            "form",
            json!({"kind":"form","label":"Settings","children":[{"kind":"checkbox","label":"On","checked":true}]}),
        ),
        (
            "checkbox",
            json!({"kind":"checkbox","label":"Include tags","checked":true,"on_change":"tags"}),
        ),
        (
            "toggle",
            json!({"kind":"toggle","label":"Enabled","checked":false}),
        ),
        (
            "select",
            json!({"kind":"select","label":"Branch","options":[{"label":"main","value":"main"}],"selected":"main"}),
        ),
        (
            "radio_group",
            json!({"kind":"radio_group","label":"Mode","options":[{"label":"A","value":"a"}]}),
        ),
        ("divider", json!({"kind":"divider"})),
        (
            "tooltip",
            json!({"kind":"tooltip","text":"Info","child":{"kind":"text","value":"?"}}),
        ),
        (
            "popover",
            json!({"kind":"popover","title":"More","child":{"kind":"text","value":"open"}}),
        ),
        (
            "menu",
            json!({"kind":"menu","items":[{"label":"Copy","value":"copy"}]}),
        ),
        (
            "empty_state",
            json!({"kind":"empty_state","title":"Nothing here"}),
        ),
        (
            "code",
            json!({"kind":"code","language":"rust","text":"fn main() {}"}),
        ),
        ("diff", json!({"kind":"diff","text":"+ added"})),
        ("commit_ref", json!({"kind":"commit_ref","label":"abc123"})),
        ("branch_ref", json!({"kind":"branch_ref","label":"main"})),
        ("remote_ref", json!({"kind":"remote_ref","label":"origin"})),
        (
            "progress",
            json!({"kind":"progress","label":"Loading","progress":0.4}),
        ),
        (
            "stack",
            json!({"kind":"stack","children":[{"kind":"text","value":"a"}]}),
        ),
        (
            "grid",
            json!({"kind":"grid","columns":2,"children":[{"kind":"text","value":"a"},{"kind":"text","value":"b"}]}),
        ),
        (
            "dock",
            json!({"kind":"dock","direction":"horizontal","children":[{"kind":"text","value":"left"},{"kind":"text","value":"right"}]}),
        ),
        (
            "split",
            json!({"kind":"split","direction":"vertical","children":[{"kind":"text","value":"top"},{"kind":"text","value":"bottom"}]}),
        ),
        (
            "tabs",
            json!({"kind":"tabs","active":"a","tabs":[{"id":"a","title":"A","child":{"kind":"text","value":"A"}}]}),
        ),
        (
            "virtual_list",
            json!({"kind":"virtual_list","children":[{"kind":"text","value":"row"}]}),
        ),
    ]
}

#[test]
fn semantic_and_layout_widgets_decode_and_render() {
    let splits = HashMap::new();
    let root = std::path::PathBuf::from("/tmp/plugin-semantic-widget-test");
    let ctx = BuildCtx {
        plugin_id: "widgets",
        scope: DispatchScope::Screen { screen_id: "tool" },
        plugin_root: root.as_path(),
        split_states: &splits,
        active_drag: None,
    };
    for (kind, value) in samples() {
        let ast = decode(&value).unwrap_or_else(|err| panic!("{kind} failed: {err}"));
        assert_eq!(ast.node.kind(), kind);
        let _ = widget_tree::build(&ast, &ctx);
    }
}

#[test]
fn theme_tokens_and_asset_handles_decode() {
    let ast = decode(&json!({
        "kind": "row",
        "spacing": { "token": "space.2" },
        "children": [{
            "kind": "icon",
            "asset": {
                "__leviathan_asset_handle": "asset:svg:icons/foo.svg",
                "kind": "svg",
                "path": "icons/foo.svg"
            },
            "color": { "token": "text.primary" }
        }]
    }))
    .unwrap();
    let WidgetNode::Row(row) = ast.node else {
        panic!("expected row");
    };
    assert_eq!(row.spacing, 8.0);
    let WidgetNode::Icon(icon) = &row.children[0].node else {
        panic!("expected icon");
    };
    assert_eq!(icon.path, "icons/foo.svg");
    assert_eq!(
        icon.color.as_ref().and_then(|c| c.token.as_deref()),
        Some("text.primary")
    );
}

#[test]
fn semantic_color_and_spacing_decode() {
    let ast = decode(&json!({
        "kind": "badge",
        "label": "OK",
        "color": { "token": "accent.green" },
        "spacing": { "token": "space.3" }
    }))
    .unwrap();
    let WidgetNode::Semantic(node) = ast.node else {
        panic!("expected semantic");
    };
    assert_eq!(
        node.color.as_ref().and_then(|c| c.token.as_deref()),
        Some("accent.green")
    );
    assert_eq!(node.spacing, Some(12.0));
}
