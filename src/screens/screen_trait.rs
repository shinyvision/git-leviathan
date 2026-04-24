//! `Screen` trait — the lifecycle contract every top-level screen implements.
//!
//! Concrete screens (`BlankScreen`, `NoGitScreen`, `RepositoryScreen`) each
//! own their state and own `Action` (associated `Message`) type. The `App`
//! dispatches by matching on the `enum Screen` wrapper (see
//! [`super::active::ActiveScreenRef`]) so dispatch stays monomorphic — no
//! `Box<dyn Screen>` runtime erasure.

use std::fmt::Debug;
use std::time::Instant;

use iced::{Element, Subscription, Task};

use crate::message::Message;

#[allow(dead_code)]
pub trait Screen {
    /// Per-screen action enum. Every screen owns its own message space; the
    /// outer routing wrappers name them.
    type Message: Debug + Send + Clone + 'static;

    fn update(&mut self, msg: Self::Message) -> Task<Message>;

    fn view(&self) -> Element<'_, Message>;

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    fn on_activate(&mut self) -> Task<Message> {
        Task::none()
    }

    fn on_deactivate(&mut self) {}

    /// Per-frame animation tick. Receives a clamped `dt_ms` from the App's
    /// animation clock. Returns an optional follow-up task.
    fn tick_animation(&mut self, _dt_ms: f32) -> Option<Task<Message>> {
        None
    }

    fn is_animating(&self) -> bool {
        false
    }

    fn overlay_layers(&self) -> Vec<Element<'_, Message>> {
        Vec::new()
    }

    /// Optional toolbar drawn between the tab bar and the screen body.
    /// `now` carries the current animation timestamp for spinner rendering.
    fn toolbar(&self, _now: Option<Instant>) -> Option<Element<'_, Message>> {
        None
    }
}
