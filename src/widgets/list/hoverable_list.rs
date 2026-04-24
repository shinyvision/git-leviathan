//! Generic hoverable list widget.
//!
//! Consolidates the row-hover tracking pattern previously duplicated in
//! `context_menu.rs` and `branch_label::BranchPopoutList`. Rows share a
//! uniform height + spacing; the caller decides styling, per-item content,
//! and which messages fire on hover / press.
//!
//! The widget is generic over `Msg` so it can be used by host modules
//! that route different message families; state tracks `last_hovered_row`
//! and fires `on_hover_enter`/`on_hover_exit` when it changes.
use iced::{
    advanced::{
        layout, mouse, renderer,
        widget::{tree, Operation, Tree},
        Clipboard, Layout, Renderer as _, Shell, Widget,
    },
    border, Background, Border, Color, Element, Length, Rectangle, Shadow, Size, Theme,
};

/// One row. `content` is the inner element drawn inside the row; the
/// list widget handles row background + hover highlight.
pub struct HoverableListItem<Msg> {
    pub content: Element<'static, Msg>,
    pub on_press: Option<Msg>,
    pub on_right_press: Option<Msg>,
    pub on_hover_enter: Option<Msg>,
    pub on_hover_exit: Option<Msg>,
    /// Override the idle row background from `HoverableListStyle::row_bg_idle`.
    pub row_bg_idle: Option<Color>,
    /// Override the hovered row background from `HoverableListStyle::row_bg_hover`.
    pub row_bg_hover: Option<Color>,
    /// Override the per-row border radius (default: square — rows extend to
    /// the panel edges inside the shared border).
    pub row_border_radius: Option<border::Radius>,
}

impl<Msg> HoverableListItem<Msg> {
    pub fn new(content: Element<'static, Msg>) -> Self {
        Self {
            content,
            on_press: None,
            on_right_press: None,
            on_hover_enter: None,
            on_hover_exit: None,
            row_bg_idle: None,
            row_bg_hover: None,
            row_border_radius: None,
        }
    }

    pub fn on_press(mut self, msg: Msg) -> Self {
        self.on_press = Some(msg);
        self
    }

    pub fn on_right_press(mut self, msg: Msg) -> Self {
        self.on_right_press = Some(msg);
        self
    }

    pub fn on_hover_enter(mut self, msg: Msg) -> Self {
        self.on_hover_enter = Some(msg);
        self
    }

    pub fn on_hover_exit(mut self, msg: Msg) -> Self {
        self.on_hover_exit = Some(msg);
        self
    }
}

#[derive(Clone, Copy)]
pub struct HoverableListStyle {
    /// Fixed height of each row.
    pub row_height: f32,
    /// Vertical space between rows.
    pub row_spacing: f32,
    /// Outer padding around the entire list (inside the border).
    pub outer_padding: f32,
    /// Horizontal inset applied to each row's content.
    pub row_padding_x: f32,
    /// Panel border.
    pub border_color: Color,
    pub border_radius: f32,
    pub border_width: f32,
    /// Panel background (behind rows).
    pub background: Color,
    /// Optional drop shadow.
    pub shadow: Option<Shadow>,
    /// Idle row background.
    pub row_bg_idle: Color,
    /// Row background when hovered.
    pub row_bg_hover: Color,
}

pub struct HoverableList<Msg>
where
    Msg: 'static + Clone,
{
    items: Vec<HoverableListItem<Msg>>,
    style: HoverableListStyle,
    /// When true, `last_hovered_row` persists a visible "active" row even
    /// when the cursor leaves the list (used by the context menu so the
    /// panel doesn't flicker on close).
    sticky_active: bool,
}

impl<Msg> HoverableList<Msg>
where
    Msg: 'static + Clone,
{
    pub fn new(items: Vec<HoverableListItem<Msg>>, style: HoverableListStyle) -> Self {
        Self {
            items,
            style,
            sticky_active: false,
        }
    }

    pub fn sticky_active(mut self, sticky: bool) -> Self {
        self.sticky_active = sticky;
        self
    }

    fn hovered_row(&self, bounds: Rectangle, cursor: mouse::Cursor) -> Option<usize> {
        let pos = cursor.position_in(bounds)?;
        let y = pos.y - self.style.outer_padding;
        if y < 0.0 {
            return None;
        }
        let stride = self.style.row_height + self.style.row_spacing;
        let index = (y / stride) as usize;
        let row_local_y = y - index as f32 * stride;
        if row_local_y < self.style.row_height && index < self.items.len() {
            Some(index)
        } else {
            None
        }
    }
}

#[derive(Debug, Default)]
struct HoverableListState {
    /// Actual row the cursor is currently over (None if outside). Tracks the
    /// true hover state — used to fire enter/exit exactly once per transition.
    last_hovered_row: Option<usize>,
    /// Sticky "active" row kept for rendering when `sticky_active` is set and
    /// the cursor has left. Never affects event handling or mouse_interaction,
    /// so it cannot block clicks to underlying overlay layers.
    sticky_active_row: Option<usize>,
}

impl<Msg> Widget<Msg, Theme, iced::Renderer> for HoverableList<Msg>
where
    Msg: 'static + Clone,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        self.items
            .iter()
            .map(|item| Tree::new(&item.content))
            .collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let state: &mut HoverableListState = tree.state.downcast_mut();
        if let Some(idx) = state.last_hovered_row {
            if idx >= self.items.len() {
                state.last_hovered_row = None;
            }
        }
        if let Some(idx) = state.sticky_active_row {
            if idx >= self.items.len() {
                state.sticky_active_row = None;
            }
        }
        let children: Vec<&Element<'static, Msg>> =
            self.items.iter().map(|it| &it.content).collect();
        tree.diff_children(&children);
    }

    fn state(&self) -> tree::State {
        tree::State::new(HoverableListState::default())
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<HoverableListState>()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let available_content_width =
            (limits.max().width - self.style.outer_padding * 2.0 - self.style.row_padding_x * 2.0)
                .max(0.0);
        let child_limits = layout::Limits::new(
            Size::ZERO,
            Size::new(available_content_width, self.style.row_height),
        );

        let mut max_content_width: f32 = 0.0;
        let mut children = Vec::with_capacity(self.items.len());

        for (item, tree) in self.items.iter_mut().zip(&mut tree.children) {
            let mut child = item
                .content
                .as_widget_mut()
                .layout(tree, renderer, &child_limits);
            let row_index = children.len() as f32;
            let child_y = row_index * (self.style.row_height + self.style.row_spacing);

            let text_height = child.size().height;
            let vertical_padding = (self.style.row_height - text_height) / 2.0;

            max_content_width = max_content_width.max(child.size().width);
            child.move_to_mut(iced::Point::new(
                self.style.outer_padding + self.style.row_padding_x,
                self.style.outer_padding + child_y + vertical_padding,
            ));
            children.push(child);
        }

        let width =
            max_content_width + self.style.row_padding_x * 2.0 + self.style.outer_padding * 2.0;
        let row_count = self.items.len() as f32;
        let height = row_count * self.style.row_height
            + (self.items.len().saturating_sub(1) as f32 * self.style.row_spacing)
            + self.style.outer_padding * 2.0;

        layout::Node::with_children(Size::new(width, height), children)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((item, child_tree), child_layout) in self
                .items
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                item.content
                    .as_widget_mut()
                    .operate(child_tree, child_layout, renderer, operation);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Msg>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let hovered_row = self.hovered_row(bounds, cursor);

        {
            let state: &mut HoverableListState = tree.state.downcast_mut();
            if state.last_hovered_row != hovered_row {
                if let Some(prev) = state.last_hovered_row {
                    if let Some(item) = self.items.get(prev) {
                        if let Some(msg) = item.on_hover_exit.as_ref() {
                            shell.publish(msg.clone());
                        }
                    }
                }
                if let Some(next) = hovered_row {
                    if let Some(item) = self.items.get(next) {
                        if let Some(msg) = item.on_hover_enter.as_ref() {
                            shell.publish(msg.clone());
                        }
                    }
                }
                state.last_hovered_row = hovered_row;
                if hovered_row.is_some() {
                    state.sticky_active_row = hovered_row;
                }
            }
        }

        if let Some(index) = hovered_row {
            if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(btn)) = event {
                if let Some(item) = self.items.get(index) {
                    match btn {
                        iced::mouse::Button::Left => {
                            if let Some(msg) = item.on_press.as_ref() {
                                shell.publish(msg.clone());
                                shell.capture_event();
                            }
                        }
                        iced::mouse::Button::Right => {
                            if let Some(msg) = item.on_right_press.as_ref() {
                                shell.publish(msg.clone());
                                shell.capture_event();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state: &HoverableListState = tree.state.downcast_ref();
        let effective = self.hovered_row(layout.bounds(), cursor).or({
            if self.sticky_active {
                state.last_hovered_row
            } else {
                None
            }
        });
        if effective.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        renderer_style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clipped_viewport) = bounds.intersection(viewport) else {
            return;
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: self.style.border_color,
                    width: self.style.border_width,
                    radius: self.style.border_radius.into(),
                },
                shadow: self.style.shadow.unwrap_or_default(),
                ..renderer::Quad::default()
            },
            Background::Color(self.style.background),
        );

        let hovered_row = self.hovered_row(bounds, cursor);
        let state: &HoverableListState = tree.state.downcast_ref();
        let active_row = hovered_row.or(if self.sticky_active {
            state.sticky_active_row
        } else {
            None
        });

        let stride = self.style.row_height + self.style.row_spacing;
        for (index, ((item, child_tree), child_layout)) in self
            .items
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .enumerate()
        {
            let row_bounds = Rectangle {
                x: bounds.x + self.style.outer_padding,
                y: bounds.y + self.style.outer_padding + index as f32 * stride,
                width: bounds.width - self.style.outer_padding * 2.0,
                height: self.style.row_height,
            };

            let bg_color = if active_row == Some(index) {
                item.row_bg_hover.unwrap_or(self.style.row_bg_hover)
            } else {
                item.row_bg_idle.unwrap_or(self.style.row_bg_idle)
            };
            let row_radius = item.row_border_radius.unwrap_or_default();

            renderer.fill_quad(
                renderer::Quad {
                    bounds: row_bounds,
                    border: Border {
                        radius: row_radius,
                        ..Border::default()
                    },
                    ..renderer::Quad::default()
                },
                Background::Color(bg_color),
            );

            item.content.as_widget().draw(
                child_tree,
                renderer,
                theme,
                renderer_style,
                child_layout,
                cursor,
                &clipped_viewport,
            );
        }
    }
}

impl<Msg> From<HoverableList<Msg>> for Element<'static, Msg>
where
    Msg: 'static + Clone,
{
    fn from(list: HoverableList<Msg>) -> Self {
        Element::new(list)
    }
}
