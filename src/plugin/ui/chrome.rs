use std::cell::RefCell;
use std::rc::Rc;

use mlua::RegistryKey;

use crate::plugin::ui::invalidation::{DynamicWidgetTelemetry, UiDependency};
use crate::plugin::ui::widget_ast::WidgetAst;
use crate::widgets::chrome::repo_region::ChromePane;

pub type ChromeAstCache = Rc<RefCell<Option<WidgetAst>>>;

pub struct ChromeRegistration {
    pub contribution_id: String,
    pub pane: ChromePane,
    pub priority: i32,
    pub key: RegistryKey,
    pub cache: ChromeAstCache,
    pub dependencies: Vec<UiDependency>,
    pub telemetry: Rc<RefCell<DynamicWidgetTelemetry>>,
    pub source_location: Option<String>,
}

impl ChromeRegistration {
    pub fn handle(point_id: &str, contribution_id: &str) -> String {
        format!("{point_id}:{contribution_id}")
    }
}
