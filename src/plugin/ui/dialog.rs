use std::collections::HashMap;

use mlua::RegistryKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogRequest {
    pub plugin_id: String,
    pub dialog_id: String,
    pub text: String,
    pub title: Option<String>,
    pub data: Vec<DialogDataRequest>,
    pub controls: Vec<DialogControlRequest>,
    pub buttons: Vec<DialogButtonRequest>,
    pub dismissible: bool,
    pub autofocus: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogDataRequest {
    pub id: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogControlRequest {
    pub id: String,
    pub label: Option<DialogLabelRequest>,
    pub text_input: Option<DialogTextInputRequest>,
    pub dropdown: Option<DialogDropdownRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogLabelRequest {
    pub text: String,
    pub style: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogTextInputRequest {
    pub placeholder: String,
    pub value: String,
    pub submit_button_id: Option<String>,
    pub width: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogDropdownRequest {
    pub placeholder: String,
    pub options: Vec<DialogDropdownOptionRequest>,
    pub selected_option_id: Option<String>,
    pub open: bool,
    pub width: Option<u16>,
    pub leading_icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogDropdownOptionRequest {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogButtonRequest {
    pub id: String,
    pub text: String,
    pub style: String,
    pub keys: Vec<String>,
    pub closes_dialog: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DialogCallbackKey {
    plugin_id: String,
    dialog_id: String,
    button_id: String,
}

#[derive(Default)]
pub struct DialogCallbacks {
    handlers: HashMap<DialogCallbackKey, RegistryKey>,
}

impl DialogCallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        plugin_id: impl Into<String>,
        dialog_id: impl Into<String>,
        button_id: impl Into<String>,
        key: RegistryKey,
    ) {
        self.handlers.insert(
            DialogCallbackKey {
                plugin_id: plugin_id.into(),
                dialog_id: dialog_id.into(),
                button_id: button_id.into(),
            },
            key,
        );
    }

    pub fn remove_dialog(&mut self, plugin_id: &str, dialog_id: &str) {
        self.handlers
            .retain(|key, _| key.plugin_id != plugin_id || key.dialog_id != dialog_id);
    }

    pub fn get(&self, plugin_id: &str, dialog_id: &str, button_id: &str) -> Option<&RegistryKey> {
        self.handlers.get(&DialogCallbackKey {
            plugin_id: plugin_id.to_string(),
            dialog_id: dialog_id.to_string(),
            button_id: button_id.to_string(),
        })
    }

    pub fn contains(&self, plugin_id: &str, dialog_id: &str, button_id: &str) -> bool {
        self.get(plugin_id, dialog_id, button_id).is_some()
    }
}
