#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DialogId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dialog {
    pub id: DialogId,
    pub owner: DialogOwner,
    pub message: DialogMessage,
    pub data: Vec<DialogData>,
    pub controls: Vec<DialogControl>,
    pub buttons: Vec<DialogButton>,
    pub dismissible: bool,
    pub autofocus: Option<DialogControlId>,
}

impl Dialog {
    pub(crate) fn enabled_button_for_key(&self, key_name: &str) -> Option<&DialogButton> {
        let key_name = normalize_dialog_key(key_name)?;
        self.buttons.iter().find(|button| {
            button.enabled
                && button
                    .keys
                    .iter()
                    .any(|candidate| candidate.matches_name(&key_name))
        })
    }

    pub(crate) fn data_value(&self, id: &str) -> Option<&str> {
        self.data
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.value.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DialogOwner {
    Native(NativeDialogOwner),
    Plugin { plugin_id: String },
}

impl DialogOwner {
    pub(crate) fn native(id: impl Into<String>) -> Self {
        Self::Native(NativeDialogOwner { id: id.into() })
    }

    pub(crate) fn plugin(plugin_id: impl Into<String>) -> Self {
        Self::Plugin {
            plugin_id: plugin_id.into(),
        }
    }

    pub(crate) fn is_native(&self, id: &str) -> bool {
        matches!(self, Self::Native(owner) if owner.id == id)
    }

    pub(crate) fn plugin_id(&self) -> Option<&str> {
        match self {
            Self::Plugin { plugin_id } => Some(plugin_id.as_str()),
            Self::Native(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeDialogOwner {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogMessage {
    pub text: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogData {
    pub id: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogControl {
    pub id: DialogControlId,
    pub label: Option<DialogLabel>,
    pub text_input: Option<DialogTextInput>,
    pub dropdown: Option<DialogDropdown>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DialogControlId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogLabel {
    pub text: String,
    pub style: DialogLabelStyle,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DialogLabelStyle(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogTextInput {
    pub placeholder: String,
    pub value: String,
    pub submit_button_id: Option<DialogButtonId>,
    pub width: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogDropdown {
    pub placeholder: String,
    pub options: Vec<DialogDropdownOption>,
    pub selected_option_id: Option<String>,
    pub open: bool,
    pub width: Option<u16>,
    pub leading_icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogDropdownOption {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogButton {
    pub id: DialogButtonId,
    pub text: String,
    pub style: DialogButtonStyle,
    pub keys: Vec<DialogKey>,
    pub closes_dialog: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DialogButtonId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DialogButtonStyle(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DialogKey(pub String);

impl DialogKey {
    fn matches_name(&self, key_name: &str) -> bool {
        normalize_dialog_key(&self.0).as_deref() == Some(key_name)
    }
}

fn normalize_dialog_key(raw: &str) -> Option<String> {
    let compact: String = raw
        .trim()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect();

    if compact.chars().count() == 1 && compact.chars().all(|c| !c.is_control()) {
        return Some(compact);
    }

    Some(
        match compact.as_str() {
            "enter" | "return" => "enter",
            "esc" | "escape" => "esc",
            "tab" => "tab",
            "space" => "space",
            "backspace" => "backspace",
            "delete" | "del" => "delete",
            "arrowup" | "up" => "up",
            "arrowdown" | "down" => "down",
            "arrowleft" | "left" => "left",
            "arrowright" | "right" => "right",
            "home" => "home",
            "end" => "end",
            "pageup" => "pageup",
            "pagedown" => "pagedown",
            "f1" => "f1",
            "f2" => "f2",
            "f3" => "f3",
            "f4" => "f4",
            "f5" => "f5",
            "f6" => "f6",
            "f7" => "f7",
            "f8" => "f8",
            "f9" => "f9",
            "f10" => "f10",
            "f11" => "f11",
            "f12" => "f12",
            "f13" => "f13",
            "f14" => "f14",
            "f15" => "f15",
            "f16" => "f16",
            "f17" => "f17",
            "f18" => "f18",
            "f19" => "f19",
            "f20" => "f20",
            "f21" => "f21",
            "f22" => "f22",
            "f23" => "f23",
            "f24" => "f24",
            _ => return None,
        }
        .to_string(),
    )
}
