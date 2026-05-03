//! Reusable dropdown widget. iced's built-in `pick_list` only supports
//! `ToString` items, blocking per-item custom rendering (icons, colored
//! segments, etc). `Dropdown` solves that by letting the caller supply any
//! `Element` for both the trigger label and each menu row.
//!
//! Open/close state is owned by the caller (kept explicit so open/close can be
//! driven by any `Message` and persisted in the caller's model). The widget
//! itself handles the floating-overlay concerns:
//!   • Menu is rendered as an iced overlay (floats above the rest of the UI).
//!   • Pressing Escape while open emits `on_close`.
//!   • Clicking outside the menu emits `on_close`.
//!
//! Caller responsibilities:
//!   • Track `is_open: bool` in model state.
//!   • Pass an `on_toggle` message for the trigger button.
//!   • Pass an `on_close` message used for Esc / outside-click.
//!   • Supply per-item `Element`s and their click messages (helpers below).

use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::{layout, overlay, renderer, Clipboard, Layout, Shell};
use iced::{
    keyboard, mouse,
    widget::{button, column, container, row, scrollable, text as text_widget},
    Border, Color, Element, Event, Length, Padding, Point, Rectangle, Size, Theme, Vector,
};

use crate::{assets, message::Message, style, theme, widgets::shared::horizontal_space};

pub const DROPDOWN_TRIGGER_HEIGHT: f32 = 28.0;
pub const DROPDOWN_ITEM_HEIGHT: f32 = 26.0;
pub const DROPDOWN_MAX_HEIGHT: f32 = 240.0;
const MENU_GAP: f32 = 2.0;

/// Builder for a custom-rendered dropdown. Wraps a trigger element and, when
/// open, shows a floating menu below the trigger that closes on outside click
/// or Esc.
pub struct Dropdown<'a> {
    trigger: Element<'a, Message>,
    menu: Option<Element<'a, Message>>,
    on_close: Message,
    menu_width: Option<f32>,
    menu_max_height: f32,
}

impl<'a> Dropdown<'a> {
    pub fn new(
        trigger: Element<'a, Message>,
        menu: Option<Element<'a, Message>>,
        on_close: Message,
    ) -> Self {
        Self {
            trigger,
            menu,
            on_close,
            menu_width: None,
            menu_max_height: DROPDOWN_MAX_HEIGHT,
        }
    }

    /// Forces a specific menu width (in pixels). Without this the menu inherits
    /// the trigger's computed width.
    pub fn menu_width(mut self, width: f32) -> Self {
        self.menu_width = Some(width);
        self
    }
}

impl<'a> From<Dropdown<'a>> for Element<'a, Message> {
    fn from(widget: Dropdown<'a>) -> Self {
        Element::new(widget)
    }
}

struct DropdownState {
    menu_tree: Tree,
}

impl Default for DropdownState {
    fn default() -> Self {
        Self {
            menu_tree: Tree::empty(),
        }
    }
}

impl<'a> Widget<Message, Theme, iced::Renderer> for Dropdown<'a> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DropdownState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DropdownState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.trigger)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.trigger]);
        // Keep the menu's state tree in sync with whatever menu element we
        // were given this frame. When the menu is absent (closed) we still
        // keep the old tree around — cheap, and avoids reinitializing item
        // state on each toggle.
        let state = tree.state.downcast_mut::<DropdownState>();
        if let Some(menu) = &self.menu {
            state.menu_tree.diff(menu.as_widget());
        }
    }

    fn size(&self) -> Size<Length> {
        self.trigger.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.trigger.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.trigger
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.trigger
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.trigger.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.trigger.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.trigger.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let menu = self.menu.as_mut()?;
        let state = tree.state.downcast_mut::<DropdownState>();
        let trigger_bounds = layout.bounds() + translation;
        let width = self.menu_width.unwrap_or(trigger_bounds.width);

        Some(overlay::Element::new(Box::new(DropdownOverlay {
            menu,
            menu_tree: &mut state.menu_tree,
            on_close: self.on_close.clone(),
            trigger_bounds,
            menu_width: width,
            menu_max_height: self.menu_max_height,
        })))
    }
}

struct DropdownOverlay<'a, 'b> {
    menu: &'b mut Element<'a, Message>,
    menu_tree: &'b mut Tree,
    on_close: Message,
    trigger_bounds: Rectangle,
    menu_width: f32,
    menu_max_height: f32,
}

impl<'a, 'b> overlay::Overlay<Message, Theme, iced::Renderer> for DropdownOverlay<'a, 'b> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(
            Size::ZERO,
            Size::new(self.menu_width, self.menu_max_height.min(bounds.height)),
        )
        .width(Length::Fixed(self.menu_width));

        let node = self
            .menu
            .as_widget_mut()
            .layout(self.menu_tree, renderer, &limits);
        let size = node.size();

        // Prefer below-trigger; flip above if it would overflow the viewport.
        let below_y = self.trigger_bounds.y + self.trigger_bounds.height + MENU_GAP;
        let position = if below_y + size.height <= bounds.height {
            Point::new(self.trigger_bounds.x, below_y)
        } else {
            let above_y = (self.trigger_bounds.y - size.height - MENU_GAP).max(0.0);
            Point::new(self.trigger_bounds.x, above_y)
        };

        node.move_to(position)
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let menu_bounds = layout.bounds();

        // Esc always closes, regardless of cursor position. Capture so other
        // widgets don't also react to it.
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) = event
        {
            shell.publish(self.on_close.clone());
            shell.capture_event();
            return;
        }

        // Left-press outside the menu closes. Clicks inside the menu fall
        // through to the menu's own widgets (items, scrollbar).
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if !cursor.is_over(menu_bounds) {
                shell.publish(self.on_close.clone());
                shell.capture_event();
                return;
            }
        }

        self.menu.as_widget_mut().update(
            self.menu_tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &menu_bounds,
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            self.menu.as_widget().mouse_interaction(
                self.menu_tree,
                layout,
                cursor,
                &layout.bounds(),
                renderer,
            )
        } else {
            mouse::Interaction::Idle
        }
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(
            self.menu_tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }
}

// ─── Composition helpers ─────────────────────────────────────────────────────
//
// These build the three standard pieces of a dropdown:
//   • `dropdown_trigger`  — the closed/open button (chevron on the right).
//   • `dropdown_menu`     — scrollable container around the item rows.
//   • `dropdown_item`     — a single clickable row (hover highlight).
// Combine them with `Dropdown::new(trigger, Some(menu), on_close)`.

/// Closed/open trigger button. When no selection exists, renders `placeholder`
/// in dim text; otherwise the caller-supplied `label` element is shown.
pub fn dropdown_trigger<'a>(
    label: Option<Element<'a, Message>>,
    placeholder: &'static str,
    on_toggle: Message,
) -> Element<'a, Message> {
    let label_elem: Element<Message> = label.unwrap_or_else(|| {
        text_widget(placeholder)
            .size(theme::FONT_SM)
            .style(style::dim_text)
            .into()
    });

    let chevron = assets::icon(assets::CHEVRON_DOWN, 12.0, theme::TEXT_DIM);

    let content = row![label_elem, horizontal_space().width(Length::Fill), chevron]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([0, 8]));

    let inner = container(content)
        .width(Length::Fill)
        .height(Length::Fixed(DROPDOWN_TRIGGER_HEIGHT))
        .align_y(iced::alignment::Vertical::Center);

    button(inner)
        .on_press(on_toggle)
        .padding(0)
        .width(Length::Fill)
        .style(|_: &Theme, status: button::Status| button::Style {
            background: Some(theme::BG_BASE.into()),
            text_color: theme::TEXT_PRIMARY,
            border: Border {
                color: match status {
                    button::Status::Hovered | button::Status::Pressed => theme::ACCENT_BLUE,
                    _ => theme::BORDER,
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            shadow: Default::default(),
            snap: false,
        })
        .into()
}

/// Scrollable menu container holding one row per item.
pub fn dropdown_menu<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let list = column(items).spacing(0).width(Length::Fill);
    let scroll = scrollable(list).height(Length::Shrink).width(Length::Fill);

    container(scroll)
        .max_height(DROPDOWN_MAX_HEIGHT)
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_PANEL.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// One clickable row inside a dropdown menu. Caller supplies row content
/// (usually `row![icon, text]`) and the message dispatched on click.
pub fn dropdown_item<'a>(content: Element<'a, Message>, on_press: Message) -> Element<'a, Message> {
    let inner = container(
        row![content]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from([0, 10])),
    )
    .width(Length::Fill)
    .height(Length::Fixed(DROPDOWN_ITEM_HEIGHT))
    .align_y(iced::alignment::Vertical::Center);

    button(inner)
        .on_press(on_press)
        .padding(0)
        .width(Length::Fill)
        .style(|_: &Theme, status: button::Status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(theme::BG_HOVER.into()),
                _ => None,
            },
            text_color: theme::TEXT_PRIMARY,
            border: Border::default(),
            shadow: Default::default(),
            snap: false,
        })
        .into()
}

/// Label row helper: leading icon (tinted `icon_color`) + content element.
pub fn icon_label<'a>(
    icon_bytes: &'static [u8],
    icon_color: Color,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    row![assets::icon(icon_bytes, 12.0, icon_color), content]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
}
