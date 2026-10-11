//! Workspace switcher: the stack button at the left of the tab bar and the
//! panel it opens, plus the tab context menu that moves tabs between
//! workspaces.
//!
//! The panel is one dropdown with three modes:
//!   • List — the default workspace pinned on top, then the user-created
//!     ones, drag-and-drop sortable by their handles (or anywhere on the
//!     row), with edit / delete revealed on hover, and "New Workspace".
//!   • Form — create or edit: name, icon picker, Cancel / Create.
//!   • ConfirmDelete — confirmation naming how many tabs move where.
//!
//! All state lives in `App`; this module only renders the
//! `WorkspaceSwitcherModel` snapshot it is handed.

use iced::{
    alignment::{Horizontal, Vertical},
    widget::{
        button, column, container, mouse_area, row, scrollable, text, text_input, tooltip, Space,
    },
    Alignment, Border, Color, Element, Length, Padding, Point, Shadow, Theme, Vector,
};

use crate::{
    assets,
    core::{TabId, WorkspaceId},
    message::{AppMessage, Message, WorkspaceMessage},
    screens::repository::overlay_widgets::{
        overlay_button_disabled, overlay_button_msg, overlay_neutral_button_msg,
        overlay_text_input_style, CREATE_BUTTON, DANGER_BUTTON,
    },
    services::text_measurement::{truncate_to_width, FontFamily},
    style, theme,
    widgets::{
        context_menu::{ContextMenu, ContextMenuItem},
        dropdown::Dropdown,
        primitives::hoverable::hoverable_swap,
        reorder_list::{LiftedStyle, ReorderList},
        shared::{horizontal_space, scrollbar_style},
    },
};

pub const PANEL_WIDTH: f32 = 300.0;
pub const NAME_INPUT_ID: &str = "workspace.name_input";

const PANEL_MAX_HEIGHT: f32 = 600.0;
const LIST_MAX_HEIGHT: f32 = 336.0;
const ROW_HEIGHT: f32 = 42.0;
const ROW_ICON: f32 = 16.0;
const ICON_CELL: f32 = 30.0;
const ICON_GRID_COLUMNS: usize = 8;
const ICON_GRID_GAP: f32 = 4.0;
const TRIGGER_NAME_WIDTH: f32 = 150.0;
// Name widths that fit the label column of a 300px panel row: the default
// row (check only), a user row at rest (check + handle) and a hovered user
// row (edit + delete + check + handle).
const DEFAULT_ROW_NAME_WIDTH: f32 = 200.0;
const ROW_NAME_WIDTH: f32 = 180.0;
const HOVERED_ROW_NAME_WIDTH: f32 = 120.0;
const TAB_MENU_NAME_WIDTH: f32 = 130.0;
const TAB_MENU_WIDTH: f32 = 230.0;
const TAB_MENU_INSET: f32 = 4.0;
const TOOLTIP_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

/// Everything the switcher needs to render, rebuilt by `App` after each
/// update. Owned data so the tab bar can borrow it for a frame.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceSwitcherModel {
    pub active: WorkspaceId,
    /// The default workspace first, then the user-created ones in order.
    pub entries: Vec<WorkspaceEntry>,
    /// `Some` while the panel is open.
    pub panel: Option<PanelMode>,
    /// Row highlighted with the arrow keys.
    pub highlight: Option<WorkspaceId>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEntry {
    pub id: WorkspaceId,
    pub name: String,
    pub icon: &'static [u8],
    pub tab_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PanelMode {
    List,
    Form(WorkspaceForm),
    ConfirmDelete(WorkspaceId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceForm {
    /// `None` creates a workspace; `Some` edits that one.
    pub editing: Option<WorkspaceId>,
    pub name: String,
    pub icon: &'static str,
    /// Validation message shown under the name field.
    pub error: Option<String>,
    pub valid: bool,
    /// Tab (id, name) that moves into the workspace once it is created.
    pub moving_tab: Option<(TabId, String)>,
}

/// Right-click menu state for a tab.
#[derive(Debug, Clone, PartialEq)]
pub struct TabMenu {
    pub tab_id: TabId,
    pub position: Point,
}

impl WorkspaceSwitcherModel {
    fn entry(&self, id: WorkspaceId) -> Option<&WorkspaceEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn active_entry(&self) -> Option<&WorkspaceEntry> {
        self.entry(self.active)
    }
}

fn workspace(msg: WorkspaceMessage) -> Message {
    Message::App(AppMessage::Workspace(msg))
}

fn tab_count_label(count: usize) -> String {
    match count {
        0 => "No tabs".to_string(),
        1 => "1 tab".to_string(),
        n => format!("{n} tabs"),
    }
}

fn truncated(name: &str, width: f32, size: f32) -> String {
    truncate_to_width(name, width, FontFamily::Default, size)
}

// ─── Trigger ────────────────────────────────────────────────────────────────

/// The switcher button with its panel. In the default workspace it is just
/// the stack icon and a chevron; in a user-created workspace it shows that
/// workspace's icon and name so it is always clear where you are.
pub fn switcher_view(model: &WorkspaceSwitcherModel) -> Element<'_, Message> {
    let open = model.panel.is_some();
    let active = model.active_entry();
    let icon_color = if open || !model.active.is_default() {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
    };

    let mut content = row![assets::icon(
        active.map(|e| e.icon).unwrap_or(assets::STACK),
        15.0,
        icon_color
    )]
    .spacing(6)
    .align_y(Alignment::Center);
    if let Some(entry) = active.filter(|e| !e.id.is_default()) {
        content = content.push(
            text(truncated(&entry.name, TRIGGER_NAME_WIDTH, theme::FONT_SM))
                .size(theme::FONT_SM)
                .wrapping(text::Wrapping::None)
                .style(style::primary_text),
        );
    }
    content = content.push(assets::icon(assets::CHEVRON_DOWN, 10.0, theme::TEXT_DIM));

    let trigger: Element<'_, Message> = button(
        container(content)
            .height(Length::Fill)
            .align_y(Vertical::Center),
    )
    .padding(Padding::from([0, 10]))
    .height(Length::Fixed(theme::TAB_HEIGHT as f32))
    .style(move |_: &Theme, status: button::Status| button::Style {
        background: (open || matches!(status, button::Status::Hovered | button::Status::Pressed))
            .then(|| theme::BG_HOVER.into()),
        text_color: theme::TEXT_PRIMARY,
        border: Border::default(),
        shadow: Default::default(),
        snap: false,
    })
    .on_press(workspace(WorkspaceMessage::TogglePanel))
    .into();
    // The button is icon-only in the default workspace; name it on hover.
    // No tooltip while the panel is open — it would sit on top of it.
    let trigger = if open {
        trigger
    } else {
        tooltip(
            trigger,
            container(text("Workspaces").size(theme::FONT_XS)).padding(Padding::from([3, 7])),
            tooltip::Position::Bottom,
        )
        .gap(4)
        .delay(TOOLTIP_DELAY)
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_HEADER.into()),
            text_color: Some(theme::TEXT_PRIMARY),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
    };

    let panel = model.panel.as_ref().map(|mode| panel_view(model, mode));
    Dropdown::new(trigger, panel, workspace(WorkspaceMessage::ClosePanel))
        .on_escape(workspace(WorkspaceMessage::PanelEscape))
        .menu_width(PANEL_WIDTH)
        .menu_max_height(PANEL_MAX_HEIGHT)
        .into()
}

// ─── Panel ──────────────────────────────────────────────────────────────────

fn panel_view<'a>(model: &'a WorkspaceSwitcherModel, mode: &'a PanelMode) -> Element<'a, Message> {
    let body = match mode {
        PanelMode::List => list_view(model),
        PanelMode::Form(form) => form_view(model, form),
        PanelMode::ConfirmDelete(id) => confirm_delete_view(model, *id),
    };
    container(body)
        .width(Length::Fill)
        .padding(Padding::from([6, 0]))
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_PANEL.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 22.0,
            },
            ..Default::default()
        })
        .into()
}

fn list_view(model: &WorkspaceSwitcherModel) -> Element<'_, Message> {
    let mut items: Vec<Element<'_, Message>> = Vec::new();

    if let Some(default) = model.entries.first() {
        items.push(padded(default_row(
            default,
            model.active == default.id,
            model.highlight == Some(default.id),
        )));
    }
    items.push(section_divider());

    let custom = &model.entries[model.entries.len().min(1)..];
    if custom.is_empty() {
        items.push(
            container(
                column![
                    text("Group your repositories into workspaces.")
                        .size(theme::FONT_SM)
                        .style(style::secondary_text),
                    text("Each workspace keeps its own set of tabs.")
                        .size(theme::FONT_XS)
                        .style(style::dim_text),
                ]
                .spacing(3),
            )
            .padding(Padding::from([8, 16]))
            .into(),
        );
    } else {
        let rows: Vec<(WorkspaceId, Element<'_, Message>)> = custom
            .iter()
            .map(|entry| {
                (
                    entry.id,
                    custom_row(
                        entry,
                        model.active == entry.id,
                        model.highlight == Some(entry.id),
                    ),
                )
            })
            .collect();
        let list = ReorderList::new(rows)
            .lifted_style(LiftedStyle {
                background: theme::BG_HOVER,
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 5.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 12.0,
                },
            })
            .on_select(|id| workspace(WorkspaceMessage::Select(id)))
            .on_reorder(|order| workspace(WorkspaceMessage::Reorder(order)));
        items.push(
            container(
                scrollable(padded(list.into()))
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::new().width(5).scroller_width(5),
                    ))
                    .style(scrollbar_style),
            )
            .max_height(LIST_MAX_HEIGHT)
            .into(),
        );
    }

    items.push(section_divider());
    items.push(padded(new_workspace_button()));
    column(items).width(Length::Fill).into()
}

fn padded(content: Element<'_, Message>) -> Element<'_, Message> {
    container(content)
        .width(Length::Fill)
        .padding(Padding::from([0, 6]))
        .into()
}

fn section_divider<'a>() -> Element<'a, Message> {
    container(
        container(Space::new().width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_: &Theme| container::Style {
                background: Some(theme::BORDER.into()),
                ..Default::default()
            }),
    )
    .padding(Padding::from([5, 10]))
    .width(Length::Fill)
    .into()
}

/// Shared row layout: icon, name over tab count, then trailing widgets.
fn row_body<'a>(
    entry: &WorkspaceEntry,
    active: bool,
    name_width: f32,
    trailing: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let icon_color = if active {
        theme::ACCENT_BLUE
    } else {
        theme::TEXT_SECONDARY
    };
    let labels = column![
        text(truncated(&entry.name, name_width, theme::FONT_MD))
            .size(theme::FONT_MD)
            .wrapping(text::Wrapping::None)
            .style(style::primary_text),
        text(tab_count_label(entry.tab_count))
            .size(theme::FONT_XS)
            .style(style::dim_text),
    ]
    .spacing(2)
    .width(Length::Fill);

    let mut children: Vec<Element<'a, Message>> = vec![
        container(assets::icon(entry.icon, ROW_ICON, icon_color))
            .width(Length::Fixed(22.0))
            .align_x(Horizontal::Center)
            .into(),
        labels.into(),
    ];
    children.extend(trailing);
    container(row(children).spacing(8).align_y(Alignment::Center))
        .height(Length::Fixed(ROW_HEIGHT))
        .padding(Padding::from([0, 10]))
        .align_y(Vertical::Center)
        .into()
}

fn check_slot<'a>(active: bool) -> Element<'a, Message> {
    let content: Element<'a, Message> = if active {
        assets::icon(assets::CHECK, 14.0, theme::ACCENT_BLUE)
    } else {
        Space::new().width(Length::Fixed(14.0)).into()
    };
    container(content).width(Length::Fixed(14.0)).into()
}

fn row_style(hovered: bool) -> impl Fn(&Theme) -> container::Style {
    move |_: &Theme| container::Style {
        background: hovered.then(|| theme::BG_HOVER.into()),
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn default_row(entry: &WorkspaceEntry, active: bool, highlighted: bool) -> Element<'_, Message> {
    let body = |hovered: bool| -> Element<'_, Message> {
        container(row_body(
            entry,
            active,
            DEFAULT_ROW_NAME_WIDTH,
            vec![check_slot(active)],
        ))
        .style(row_style(hovered || highlighted))
        .into()
    };
    mouse_area(hoverable_swap(
        body(false),
        body(true),
        |_| container::Style::default(),
        |_| container::Style::default(),
    ))
    .interaction(iced::mouse::Interaction::Pointer)
    .on_press(workspace(WorkspaceMessage::Select(entry.id)))
    .into()
}

fn custom_row(entry: &WorkspaceEntry, active: bool, highlighted: bool) -> Element<'_, Message> {
    let id = entry.id;
    let handle = |hovered: bool| -> Element<'_, Message> {
        mouse_area(
            container(assets::icon(
                assets::DRAG_HANDLE,
                16.0,
                if hovered {
                    theme::TEXT_SECONDARY
                } else {
                    theme::TEXT_DIM
                },
            ))
            .padding(Padding::from([6, 0])),
        )
        .interaction(iced::mouse::Interaction::Grab)
        .into()
    };
    let actions = || -> Vec<Element<'_, Message>> {
        vec![
            row_action(
                assets::PENCIL_OUTLINE,
                theme::TEXT_PRIMARY,
                workspace(WorkspaceMessage::StartEdit(id)),
            ),
            row_action(
                assets::TRASH,
                theme::ACCENT_DANGER,
                workspace(WorkspaceMessage::StartDelete(id)),
            ),
        ]
    };
    let idle = container(row_body(
        entry,
        active,
        ROW_NAME_WIDTH,
        vec![check_slot(active), handle(false)],
    ))
    .style(row_style(highlighted));
    let mut hover_trailing = actions();
    hover_trailing.push(check_slot(active));
    hover_trailing.push(handle(true));
    let hovered = container(row_body(
        entry,
        active,
        HOVERED_ROW_NAME_WIDTH,
        hover_trailing,
    ))
    .style(row_style(true));
    // Selection and dragging are handled by the surrounding ReorderList.
    hoverable_swap(
        idle,
        hovered,
        |_| container::Style::default(),
        |_| container::Style::default(),
    )
}

/// Small icon button revealed on row hover (edit / delete).
fn row_action<'a>(
    icon: &'static [u8],
    hover_color: Color,
    on_press: Message,
) -> Element<'a, Message> {
    let idle = button(assets::icon(icon, 13.0, theme::TEXT_SECONDARY))
        .padding(Padding::from(5))
        .style(|_: &Theme, _| button::Style {
            background: None,
            ..icon_button_base()
        })
        .on_press(on_press.clone());
    let hot = button(assets::icon(icon, 13.0, hover_color))
        .padding(Padding::from(5))
        .style(|_: &Theme, _| button::Style {
            background: Some(theme::BG_PANEL.into()),
            ..icon_button_base()
        })
        .on_press(on_press);
    hoverable_swap(
        idle,
        hot,
        |_| container::Style::default(),
        |_| container::Style::default(),
    )
}

fn icon_button_base() -> button::Style {
    button::Style {
        background: None,
        text_color: theme::TEXT_PRIMARY,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        shadow: Default::default(),
        snap: false,
    }
}

fn new_workspace_button<'a>() -> Element<'a, Message> {
    let content = row![
        container(assets::icon(assets::PLUS, 12.0, theme::ACCENT_BLUE))
            .width(Length::Fixed(22.0))
            .align_x(Horizontal::Center),
        text("New Workspace")
            .size(theme::FONT_MD)
            .style(style::primary_text),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    button(
        container(content)
            .height(Length::Fixed(34.0))
            .padding(Padding::from([0, 10]))
            .align_y(Vertical::Center),
    )
    .width(Length::Fill)
    .padding(0)
    .style(|_: &Theme, status: button::Status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| theme::BG_HOVER.into()),
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..icon_button_base()
    })
    .on_press(workspace(WorkspaceMessage::StartCreate))
    .into()
}

// ─── Form ───────────────────────────────────────────────────────────────────

fn form_view<'a>(
    model: &'a WorkspaceSwitcherModel,
    form: &'a WorkspaceForm,
) -> Element<'a, Message> {
    let title = if form.editing.is_some() {
        "Edit Workspace"
    } else {
        "New Workspace"
    };
    let back = button(assets::icon(
        assets::CHEVRON_LEFT,
        14.0,
        theme::TEXT_SECONDARY,
    ))
    .padding(Padding::from(4))
    .style(|_: &Theme, status: button::Status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| theme::BG_HOVER.into()),
        ..icon_button_base()
    })
    .on_press(workspace(WorkspaceMessage::BackToList));
    let header = row![
        back,
        assets::icon(assets::workspace_icon(form.icon), 15.0, theme::ACCENT_BLUE),
        text(title).size(theme::FONT_LG).style(style::primary_text),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let name_input = text_input("e.g. Client work", &form.name)
        .id(iced::widget::Id::new(NAME_INPUT_ID))
        .on_input(|value| workspace(WorkspaceMessage::NameChanged(value)))
        .on_submit(workspace(WorkspaceMessage::SubmitForm))
        .size(theme::FONT_SM)
        .padding(theme::INPUT_PADDING)
        .width(Length::Fill)
        .style(overlay_text_input_style);

    let mut fields = column![field_label("Name"), name_input].spacing(6);
    if let Some(error) = &form.error {
        fields =
            fields.push(
                text(error.clone())
                    .size(theme::FONT_XS)
                    .style(|_: &Theme| text::Style {
                        color: Some(theme::ACCENT_DANGER),
                    }),
            );
    }

    let mut body = column![
        header,
        fields,
        column![field_label("Icon"), icon_grid(form.icon)].spacing(6),
    ]
    .spacing(14);

    if let Some((_, tab_name)) = &form.moving_tab {
        body = body.push(
            text(format!("“{tab_name}” will move into this workspace."))
                .size(theme::FONT_XS)
                .style(style::secondary_text),
        );
    }

    let submit_label = if form.editing.is_some() {
        "Save"
    } else {
        "Create"
    };
    let submit = if form.valid {
        overlay_button_msg(
            submit_label,
            CREATE_BUTTON,
            workspace(WorkspaceMessage::SubmitForm),
        )
    } else {
        overlay_button_disabled(submit_label, CREATE_BUTTON)
    };
    body = body.push(
        row![
            horizontal_space(),
            overlay_neutral_button_msg("Cancel", workspace(WorkspaceMessage::BackToList)),
            submit,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );

    let _ = model;
    container(body)
        .padding(Padding::from([8, 14]))
        .width(Length::Fill)
        .into()
}

fn field_label<'a>(label: &'static str) -> Element<'a, Message> {
    text(label)
        .size(theme::FONT_XS)
        .style(style::secondary_text)
        .into()
}

fn icon_grid<'a>(selected: &'static str) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = assets::WORKSPACE_ICONS
        .chunks(ICON_GRID_COLUMNS)
        .map(|chunk| {
            row(chunk
                .iter()
                .map(|(key, data)| icon_cell(key, data, *key == selected)))
            .spacing(ICON_GRID_GAP)
            .into()
        })
        .collect();
    container(column(rows).spacing(ICON_GRID_GAP))
        .width(Length::Fill)
        .align_x(Horizontal::Center)
        .into()
}

fn icon_cell<'a>(key: &'static str, data: &'static [u8], selected: bool) -> Element<'a, Message> {
    let color = if selected {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
    };
    button(
        container(assets::icon(data, 16.0, color))
            .width(Length::Fixed(ICON_CELL))
            .height(Length::Fixed(ICON_CELL))
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center),
    )
    .padding(0)
    .style(move |_: &Theme, status: button::Status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if selected {
                Some(theme::BG_SELECTED.into())
            } else if hovered {
                Some(theme::BG_HOVER.into())
            } else {
                None
            },
            border: Border {
                color: if selected {
                    theme::ACCENT_BLUE
                } else {
                    Color::TRANSPARENT
                },
                width: 1.0,
                radius: 5.0.into(),
            },
            ..icon_button_base()
        }
    })
    .on_press(workspace(WorkspaceMessage::IconPicked(key)))
    .into()
}

// ─── Delete confirmation ────────────────────────────────────────────────────

fn confirm_delete_view(model: &WorkspaceSwitcherModel, id: WorkspaceId) -> Element<'_, Message> {
    let (name, tabs) = model
        .entry(id)
        .map(|e| (e.name.as_str(), e.tab_count))
        .unwrap_or(("this workspace", 0));
    let default_name = model
        .entries
        .first()
        .map(|e| e.name.as_str())
        .unwrap_or("Workspace");
    let detail = match tabs {
        0 => "It has no tabs.".to_string(),
        1 => format!("Its tab will move to “{default_name}”."),
        n => format!("Its {n} tabs will move to “{default_name}”."),
    };
    container(
        column![
            row![
                assets::icon(assets::TRASH, 16.0, theme::ACCENT_DANGER),
                text(format!(
                    "Delete “{}”?",
                    truncated(name, 170.0, theme::FONT_LG)
                ))
                .size(theme::FONT_LG)
                .style(style::primary_text),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(detail)
                .size(theme::FONT_SM)
                .style(style::secondary_text),
            row![
                horizontal_space(),
                overlay_neutral_button_msg("Cancel", workspace(WorkspaceMessage::BackToList)),
                overlay_button_msg(
                    "Delete",
                    DANGER_BUTTON,
                    workspace(WorkspaceMessage::ConfirmDelete)
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(12),
    )
    .padding(Padding::from([10, 14]))
    .width(Length::Fill)
    .into()
}

// ─── Tab context menu ───────────────────────────────────────────────────────

/// Layers for a tab's right-click menu: a click-away backdrop and the menu,
/// kept inside the window's right edge.
pub fn tab_menu_layers<'a>(
    model: &WorkspaceSwitcherModel,
    menu: &TabMenu,
    tab_workspace: WorkspaceId,
) -> Vec<Element<'a, Message>> {
    let close = workspace(WorkspaceMessage::CloseTabMenu);
    let backdrop = mouse_area(
        container(horizontal_space())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(close.clone())
    .on_right_press(close)
    // Claiming an interaction keeps the layers below from seeing the cursor
    // (no hover, no wheel scrolling) while the menu is open.
    .interaction(iced::mouse::Interaction::Idle)
    .into();

    let tab_id = menu.tab_id;
    let targets: Vec<(WorkspaceId, &'static [u8], String)> = model
        .entries
        .iter()
        .filter(|entry| entry.id != tab_workspace)
        .map(|entry| {
            (
                entry.id,
                entry.icon,
                truncated(&entry.name, TAB_MENU_NAME_WIDTH, theme::FONT_SM),
            )
        })
        .collect();
    let position = menu.position;
    let positioned = iced::widget::responsive(move |size| {
        let mut items: Vec<ContextMenuItem> = targets
            .iter()
            .map(|(id, icon, name)| {
                menu_item(
                    icon,
                    format!("Move to {name}"),
                    workspace(WorkspaceMessage::MoveTab {
                        tab_id,
                        workspace: *id,
                    }),
                )
            })
            .collect();
        items.push(menu_item(
            assets::PLUS,
            "Move to New Workspace…".to_string(),
            workspace(WorkspaceMessage::StartCreateWithTab(tab_id)),
        ));
        items.push(menu_item(
            assets::CLOSE,
            "Close Tab".to_string(),
            workspace(WorkspaceMessage::CloseTab(tab_id)),
        ));
        let left = (position.x + TAB_MENU_INSET)
            .min(size.width - TAB_MENU_WIDTH - TAB_MENU_INSET)
            .max(0.0);
        let menu: Element<'_, Message> = ContextMenu::new(items).into();
        container(container(menu).width(Length::Fixed(TAB_MENU_WIDTH)))
            .padding(Padding {
                top: (position.y + TAB_MENU_INSET).max(0.0),
                left,
                ..Default::default()
            })
            .into()
    })
    .into();

    vec![backdrop, positioned]
}

fn menu_item(icon: &'static [u8], label: String, on_press: Message) -> ContextMenuItem {
    ContextMenuItem {
        on_press: Some(on_press),
        on_hover_enter: None,
        on_hover_exit: None,
        content: row![
            assets::icon(icon, 12.0, theme::TEXT_SECONDARY),
            text(label)
                .size(theme::FONT_SM)
                .style(|_: &Theme| text::Style {
                    color: Some(Color::WHITE),
                }),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
    }
}
