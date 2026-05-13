use iced::keyboard;

use crate::plugin::ui::dialog::{
    DialogButtonRequest, DialogControlRequest, DialogDropdownRequest, DialogLabelRequest,
    DialogRequest, DialogTextInputRequest,
};

use super::dialog::model::{
    Dialog, DialogButton, DialogButtonId, DialogButtonStyle, DialogControl, DialogControlId,
    DialogDropdown, DialogDropdownOption, DialogKey, DialogLabel, DialogLabelStyle, DialogOwner,
    DialogTextInput,
};

impl From<DialogRequest> for Dialog {
    fn from(request: DialogRequest) -> Self {
        Self {
            id: super::dialog::model::DialogId(request.dialog_id),
            owner: DialogOwner::plugin(request.plugin_id),
            message: super::dialog::model::DialogMessage {
                text: request.text,
                title: request.title,
            },
            data: request
                .data
                .into_iter()
                .map(|item| super::dialog::model::DialogData {
                    id: item.id,
                    value: item.value,
                })
                .collect(),
            controls: request
                .controls
                .into_iter()
                .map(DialogControl::from)
                .collect(),
            buttons: request
                .buttons
                .into_iter()
                .map(DialogButton::from)
                .collect(),
            dismissible: request.dismissible,
            autofocus: request.autofocus.map(DialogControlId),
        }
    }
}

impl From<DialogControlRequest> for DialogControl {
    fn from(control: DialogControlRequest) -> Self {
        Self {
            id: DialogControlId(control.id),
            label: control.label.map(DialogLabel::from),
            text_input: control.text_input.map(DialogTextInput::from),
            dropdown: control.dropdown.map(DialogDropdown::from),
        }
    }
}

impl From<DialogLabelRequest> for DialogLabel {
    fn from(label: DialogLabelRequest) -> Self {
        Self {
            text: label.text,
            style: DialogLabelStyle(label.style),
        }
    }
}

impl From<DialogTextInputRequest> for DialogTextInput {
    fn from(input: DialogTextInputRequest) -> Self {
        Self {
            placeholder: input.placeholder,
            value: input.value,
            submit_button_id: input.submit_button_id.map(DialogButtonId),
            width: input.width,
        }
    }
}

impl From<DialogDropdownRequest> for DialogDropdown {
    fn from(dropdown: DialogDropdownRequest) -> Self {
        Self {
            placeholder: dropdown.placeholder,
            options: dropdown
                .options
                .into_iter()
                .map(|option| DialogDropdownOption {
                    id: option.id,
                    text: option.text,
                })
                .collect(),
            selected_option_id: dropdown.selected_option_id,
            open: dropdown.open,
            width: dropdown.width,
            leading_icon: dropdown.leading_icon,
        }
    }
}

impl From<DialogButtonRequest> for DialogButton {
    fn from(button: DialogButtonRequest) -> Self {
        Self {
            id: DialogButtonId(button.id),
            text: button.text,
            style: DialogButtonStyle(button.style),
            keys: button.keys.into_iter().map(DialogKey).collect(),
            closes_dialog: button.closes_dialog,
            enabled: button.enabled,
        }
    }
}

pub(super) fn dialog_key_name(key: &keyboard::Key) -> Option<String> {
    match key.as_ref() {
        keyboard::Key::Named(named) => dialog_named_key_name(named).map(str::to_string),
        keyboard::Key::Character(character) => {
            if character == " " {
                return Some("space".into());
            }
            let mut chars = character.chars();
            let ch = chars.next()?;
            if chars.next().is_some() || ch.is_control() {
                return None;
            }
            Some(ch.to_ascii_lowercase().to_string())
        }
        keyboard::Key::Unidentified => None,
    }
}

fn dialog_named_key_name(named: keyboard::key::Named) -> Option<&'static str> {
    Some(match named {
        keyboard::key::Named::Enter => "enter",
        keyboard::key::Named::Escape => "esc",
        keyboard::key::Named::Tab => "tab",
        keyboard::key::Named::Space => "space",
        keyboard::key::Named::Backspace => "backspace",
        keyboard::key::Named::Delete => "delete",
        keyboard::key::Named::ArrowUp => "up",
        keyboard::key::Named::ArrowDown => "down",
        keyboard::key::Named::ArrowLeft => "left",
        keyboard::key::Named::ArrowRight => "right",
        keyboard::key::Named::Home => "home",
        keyboard::key::Named::End => "end",
        keyboard::key::Named::PageUp => "pageup",
        keyboard::key::Named::PageDown => "pagedown",
        keyboard::key::Named::F1 => "f1",
        keyboard::key::Named::F2 => "f2",
        keyboard::key::Named::F3 => "f3",
        keyboard::key::Named::F4 => "f4",
        keyboard::key::Named::F5 => "f5",
        keyboard::key::Named::F6 => "f6",
        keyboard::key::Named::F7 => "f7",
        keyboard::key::Named::F8 => "f8",
        keyboard::key::Named::F9 => "f9",
        keyboard::key::Named::F10 => "f10",
        keyboard::key::Named::F11 => "f11",
        keyboard::key::Named::F12 => "f12",
        keyboard::key::Named::F13 => "f13",
        keyboard::key::Named::F14 => "f14",
        keyboard::key::Named::F15 => "f15",
        keyboard::key::Named::F16 => "f16",
        keyboard::key::Named::F17 => "f17",
        keyboard::key::Named::F18 => "f18",
        keyboard::key::Named::F19 => "f19",
        keyboard::key::Named::F20 => "f20",
        keyboard::key::Named::F21 => "f21",
        keyboard::key::Named::F22 => "f22",
        keyboard::key::Named::F23 => "f23",
        keyboard::key::Named::F24 => "f24",
        _ => return None,
    })
}
