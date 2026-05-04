//! `Screen` trait — the lifecycle contract every top-level screen implements.
//!
//! Concrete screens (`BlankScreen`, `NoGitScreen`, `RepositoryScreen`) each
//! own their state and own `Action` (associated `Message`) type. The `App`
//! dispatches by matching on the `enum Screen` wrapper (see
//! [`super::active::ActiveScreenRef`]) so dispatch stays monomorphic — no
//! `Box<dyn Screen>` runtime erasure.

use std::fmt::Debug;
use std::time::Instant;

use iced::{Element, Task};

use crate::message::Message;
use crate::widgets::chrome::MainBarRegistry;

/// Extra context passed to `Screen::toolbar` so screens can integrate app-level
/// contributions (e.g. plugin-contributed main-bar buttons) without growing the
/// method signature for each new surface.
pub struct ToolbarCtx<'a> {
    pub now: Option<Instant>,
    pub main_bar_registry: &'a MainBarRegistry,
}

pub trait Screen {
    /// Per-screen action enum. Every screen owns its own message space; the
    /// outer routing wrappers (screen routing) name them.
    type Message: Debug + Send + Clone + 'static;

    fn update(&mut self, msg: Self::Message) -> Task<Message>;

    fn view(&self) -> Element<'_, Message>;

    /// Per-frame animation tick. Receives a clamped `dt_ms` from the App's
    /// animation clock. Returns an optional follow-up task.
    fn tick_animation(&mut self, _dt_ms: f32) -> Option<Task<Message>> {
        None
    }

    /// Whether the screen wants the App to keep firing animation ticks.
    fn is_animating(&self) -> bool {
        false
    }

    /// Floating overlay layers (dialogs, popouts) drawn above the screen body.
    fn overlay_layers(&self) -> Vec<Element<'_, Message>> {
        Vec::new()
    }

    fn title(&self) -> &str {
        ""
    }

    fn breadcrumbs(&self) -> &[String] {
        &[]
    }

    fn has_focus(&self) -> bool {
        true
    }

    /// Optional toolbar drawn between the tab bar and the screen body.
    /// `ctx` carries the animation timestamp plus plugin-contributed bar items.
    fn toolbar<'a>(&'a self, _ctx: &ToolbarCtx<'a>) -> Option<Element<'a, Message>> {
        None
    }
}
