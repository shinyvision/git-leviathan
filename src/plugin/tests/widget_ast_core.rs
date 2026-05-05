use serde_json::json;

use crate::plugin::ui::widget_ast::{
    self, codes, decode, decode_with_limits, AstLength, WidgetLimits, WidgetNode,
};

#[test]
fn text_input_required_fields_and_style_decode() {
    let ast = decode(&json!({
        "kind": "text_input",
        "id": "palette-query",
        "placeholder": "Run command",
        "value": "che",
        "on_input": "palette.changed",
        "on_submit": "palette.submit",
        "width": "fill",
        "height": 32,
        "autofocus": true,
        "style": {
            "background": "#101119",
            "text_color": "#e1e5f4",
            "placeholder_color": "#585d6e",
            "border": { "width": 1, "radius": 4, "color": "#242535" }
        }
    }))
    .unwrap();
    assert_eq!(ast.node_id.value, "palette-query");
    let WidgetNode::TextInput(input) = &ast.node else {
        panic!("expected text_input");
    };
    assert_eq!(input.width, AstLength::Fill);
    assert_eq!(input.height, AstLength::Fixed(32.0));
    assert!(input.autofocus);
    assert_eq!(
        input
            .style
            .placeholder_color
            .as_ref()
            .map(|c| c.raw.as_str()),
        Some("#585d6e")
    );
}

#[test]
fn bad_widgets_return_actionable_paths() {
    let err = decode(&json!({
        "kind": "row",
        "children": [
            { "kind": "text", "value": "a" },
            { "kind": "button", "text": 7 }
        ]
    }))
    .unwrap_err();
    assert_eq!(err.code, codes::FIELD_TYPE_MISMATCH);
    assert_eq!(err.path, "root.children[1].text");

    let err = decode(&json!({ "kind": "rwo" })).unwrap_err();
    assert_eq!(err.code, codes::UNKNOWN_KIND);
    assert_eq!(err.path, "root.kind");
}

#[test]
fn terminal_requires_positive_session_id() {
    let ast = decode(&json!({
        "kind": "terminal",
        "session": 7,
        "height": "fill",
    }))
    .unwrap();
    let WidgetNode::Terminal(terminal) = ast.node else {
        panic!("expected terminal");
    };
    assert_eq!(terminal.session, 7);

    let err = decode(&json!({ "kind": "terminal", "session": 0 })).unwrap_err();
    assert_eq!(err.code, codes::FIELD_TYPE_MISMATCH);
    assert_eq!(err.path, "root.session");
}

#[test]
fn limits_and_empty_lua_tables_are_handled() {
    let limits = WidgetLimits {
        max_node_count: 4,
        ..WidgetLimits::DEFAULT
    };
    let err = decode_with_limits(
        &json!({
            "kind": "row",
            "children": [
                { "kind": "text", "value": "a" },
                { "kind": "text", "value": "b" },
                { "kind": "text", "value": "c" },
                { "kind": "text", "value": "d" }
            ]
        }),
        limits,
    )
    .unwrap_err();
    assert_eq!(err.code, codes::NODE_COUNT_EXCEEDED);

    let ast = decode(&json!({ "kind": "row", "children": {} })).unwrap();
    let WidgetNode::Row(row) = ast.node else {
        panic!("expected row");
    };
    assert!(row.children.is_empty());
}

#[test]
fn raw_color_paths_find_native_chrome_risk() {
    let ast = decode(&json!({
        "kind": "button",
        "style": { "text_color": "#ffffff" },
        "child": { "kind": "text", "value": "Go", "color": { "token": "text.primary" } }
    }))
    .unwrap();
    assert_eq!(
        widget_ast::raw_color_paths(&ast),
        vec!["root.style.text_color"]
    );
}
