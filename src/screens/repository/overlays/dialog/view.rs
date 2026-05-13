use iced::{
    widget::{container, text, text_input},
    Color, Element, Length, Theme,
};

use crate::{
    assets,
    message::Message,
    style, theme,
    widgets::dropdown::{dropdown_item, dropdown_menu, dropdown_trigger, icon_label, Dropdown},
};

use super::super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::super::widgets::{
    overlay_button_disabled, overlay_button_msg, overlay_neutral_button_disabled,
    overlay_neutral_button_msg, overlay_row, overlay_text_input_style, sliding_main_bar_overlay,
    BROWSE_BUTTON, CREATE_BUTTON, DANGER_BUTTON, RESOLVE_BUTTON,
};
use super::model::{
    Dialog, DialogButton, DialogButtonStyle, DialogControl, DialogControlId, DialogDropdown,
    DialogLabel, DialogLabelStyle, DialogTextInput,
};

const DIALOG_DROPDOWN_WIDTH: f32 = 160.0;
const DIALOG_TEXT_INPUT_WIDTH: f32 = 160.0;

pub(crate) fn view<'a>(dialog: &'a Dialog, slide_offset: f32) -> Element<'a, Message> {
    let mut items = Vec::with_capacity(1 + dialog.controls.len() + dialog.buttons.len());
    items.push(message_text(dialog).into());

    for control in &dialog.controls {
        items.extend(control_elements(dialog, control));
    }

    for button in &dialog.buttons {
        items.push(button_element(dialog, button));
    }

    sliding_main_bar_overlay(overlay_row(items), slide_offset)
}

fn message_text(dialog: &Dialog) -> iced::widget::Text<'static> {
    let message = match dialog.message.title.as_deref() {
        Some(title) if !title.is_empty() && !dialog.message.text.is_empty() => {
            format!("{title}: {}", dialog.message.text)
        }
        Some(title) if !title.is_empty() => title.to_string(),
        _ => dialog.message.text.clone(),
    };

    text(message)
        .size(theme::FONT_SM)
        .style(style::primary_text)
}

fn control_elements<'a>(
    dialog: &'a Dialog,
    control: &'a DialogControl,
) -> Vec<Element<'a, Message>> {
    let mut elements = Vec::new();

    if let Some(label) = &control.label {
        elements.push(label_element(label));
    }

    if let Some(input) = &control.text_input {
        elements.push(text_input_element(dialog, control, input));
    }

    if let Some(dropdown) = &control.dropdown {
        elements.push(dropdown_element(dialog, control, dropdown));
    }

    elements
}

fn label_element(label: &DialogLabel) -> Element<'static, Message> {
    let label_style = label_style(&label.style);
    text(label.text.clone())
        .size(theme::FONT_SM)
        .style(move |_: &Theme| text::Style {
            color: Some(label_style),
        })
        .into()
}

fn text_input_element<'a>(
    dialog: &'a Dialog,
    control: &'a DialogControl,
    input: &'a DialogTextInput,
) -> Element<'a, Message> {
    let dialog_id = dialog.id.clone();
    let control_id = control.id.clone();
    let on_input = move |value| {
        dialog_message(OverlayPanelAction::DialogInputChanged {
            dialog_id: dialog_id.clone(),
            control_id: control_id.clone(),
            value,
        })
    };

    let mut input_widget = text_input(&input.placeholder, &input.value)
        .on_input(on_input)
        .size(theme::FONT_SM)
        .padding(theme::INPUT_PADDING)
        .width(Length::Fixed(
            input
                .width
                .map(f32::from)
                .unwrap_or(DIALOG_TEXT_INPUT_WIDTH),
        ))
        .style(overlay_text_input_style);

    if let Some(button_id) = input.submit_button_id.as_ref() {
        input_widget =
            input_widget.on_submit(dialog_message(OverlayPanelAction::DialogButtonPressed {
                dialog_id: dialog.id.clone(),
                button_id: button_id.clone(),
            }));
    }

    if dialog.autofocus.as_ref() == Some(&control.id) {
        input_widget = input_widget.id(input_id(dialog, &control.id));
    }

    input_widget.into()
}

pub(crate) fn input_id(dialog: &Dialog, control_id: &DialogControlId) -> iced::widget::Id {
    iced::widget::Id::from(format!("dialog:{}:{}", dialog.id.0, control_id.0))
}

fn dropdown_element<'a>(
    dialog: &'a Dialog,
    control: &'a DialogControl,
    dropdown: &'a DialogDropdown,
) -> Element<'a, Message> {
    let label = dropdown_label(dropdown);
    let toggle_msg = dialog_message(OverlayPanelAction::DialogDropdownToggled {
        dialog_id: dialog.id.clone(),
        control_id: control.id.clone(),
    });
    let trigger = dropdown_trigger(Some(label), "", toggle_msg.clone());
    let menu = if dropdown.open {
        Some(dropdown_menu(
            dropdown
                .options
                .iter()
                .map(|option| {
                    let option_id = option.id.clone();
                    dropdown_item(
                        option_label(option.text.clone(), dropdown),
                        dialog_message(OverlayPanelAction::DialogDropdownChanged {
                            dialog_id: dialog.id.clone(),
                            control_id: control.id.clone(),
                            option_id,
                        }),
                    )
                })
                .collect(),
        ))
    } else {
        None
    };

    let width = dropdown
        .width
        .map(f32::from)
        .unwrap_or(DIALOG_DROPDOWN_WIDTH);

    container(Dropdown::new(trigger, menu, toggle_msg).menu_width(width))
        .width(Length::Fixed(width))
        .into()
}

fn dropdown_label<'a>(dropdown: &'a DialogDropdown) -> Element<'a, Message> {
    if let Some(selected) = selected_option_text(dropdown) {
        option_label(selected, dropdown)
    } else {
        text(dropdown.placeholder.clone())
            .size(theme::FONT_SM)
            .style(style::dim_text)
            .into()
    }
}

fn option_label<'a>(label: String, dropdown: &'a DialogDropdown) -> Element<'a, Message> {
    let label: Element<Message> = text(label.to_string())
        .size(theme::FONT_SM)
        .style(style::primary_text)
        .into();

    match dropdown.leading_icon.as_deref() {
        Some("cloud") => icon_label(assets::CLOUD, theme::TEXT_SECONDARY, label),
        _ => label,
    }
}

fn selected_option_text(dropdown: &DialogDropdown) -> Option<String> {
    let selected_id = dropdown.selected_option_id.as_ref()?;
    dropdown
        .options
        .iter()
        .find(|option| &option.id == selected_id)
        .map(|option| option.text.clone())
        .or_else(|| Some(selected_id.clone()))
}

fn button_element(dialog: &Dialog, button: &DialogButton) -> Element<'static, Message> {
    let label = button.text.clone();
    let on_press = dialog_message(OverlayPanelAction::DialogButtonPressed {
        dialog_id: dialog.id.clone(),
        button_id: button.id.clone(),
    });
    match button_visual(&button.style) {
        DialogButtonVisual::Neutral if button.enabled => {
            overlay_neutral_button_msg(label, on_press)
        }
        DialogButtonVisual::Neutral => overlay_neutral_button_disabled(label),
        DialogButtonVisual::Green if button.enabled => {
            overlay_button_msg(label, CREATE_BUTTON, on_press)
        }
        DialogButtonVisual::Green => overlay_button_disabled(label, CREATE_BUTTON),
        DialogButtonVisual::Red if button.enabled => {
            overlay_button_msg(label, DANGER_BUTTON, on_press)
        }
        DialogButtonVisual::Red => overlay_button_disabled(label, DANGER_BUTTON),
        DialogButtonVisual::Blue if button.enabled => {
            overlay_button_msg(label, BROWSE_BUTTON, on_press)
        }
        DialogButtonVisual::Blue => overlay_button_disabled(label, BROWSE_BUTTON),
        DialogButtonVisual::Yellow if button.enabled => {
            overlay_button_msg(label, RESOLVE_BUTTON, on_press)
        }
        DialogButtonVisual::Yellow => overlay_button_disabled(label, RESOLVE_BUTTON),
    }
}

fn dialog_message(action: OverlayPanelAction) -> Message {
    Message::repo(RepositoryMessage::OverlayPanel(action))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogButtonVisual {
    Green,
    Red,
    Blue,
    Yellow,
    Neutral,
}

fn button_visual(style: &DialogButtonStyle) -> DialogButtonVisual {
    match style.0.trim().to_ascii_lowercase().as_str() {
        "green" | "create" | "primary" | "confirm" => DialogButtonVisual::Green,
        "red" | "danger" | "destructive" | "delete" | "remove" => DialogButtonVisual::Red,
        "blue" | "browse" => DialogButtonVisual::Blue,
        "yellow" | "resolve" => DialogButtonVisual::Yellow,
        "white" | "neutral" | "secondary" | "cancel" => DialogButtonVisual::Neutral,
        _ => DialogButtonVisual::Neutral,
    }
}

fn label_style(style: &DialogLabelStyle) -> Color {
    match style.0.trim().to_ascii_lowercase().as_str() {
        "secondary" => theme::TEXT_SECONDARY,
        "dim" | "placeholder" => theme::TEXT_DIM,
        _ => theme::TEXT_PRIMARY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(raw: &str) -> DialogButtonStyle {
        DialogButtonStyle(raw.to_string())
    }

    #[test]
    fn button_visual_maps_supported_style_names() {
        assert_eq!(button_visual(&style("green")), DialogButtonVisual::Green);
        assert_eq!(button_visual(&style("red")), DialogButtonVisual::Red);
        assert_eq!(button_visual(&style("blue")), DialogButtonVisual::Blue);
        assert_eq!(button_visual(&style("yellow")), DialogButtonVisual::Yellow);
        assert_eq!(button_visual(&style("white")), DialogButtonVisual::Neutral);
    }

    #[test]
    fn button_visual_defaults_unknown_styles_to_neutral() {
        assert_eq!(button_visual(&style("custom")), DialogButtonVisual::Neutral);
    }
}
