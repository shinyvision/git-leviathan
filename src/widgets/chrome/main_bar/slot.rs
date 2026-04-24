//! Slot type + render context for the main bar.
//!
//! A `MainBarSlot` is the atom of the main bar. Built-in items (Pull, Push,
//! Branch, …) and plugin-contributed items are expressed as the same thing:
//! an `id`, a `section`, a `priority`, and a builder closure that turns
//! immutable render-time state (`SlotCtx`) into an `iced::Element`.
//!
//! ELM discipline: builders are pure `Fn(&SlotCtx) -> Element`; no mutation,
//! no side effects. All per-frame state the slot needs is plumbed through
//! `SlotCtx`.

use std::time::Instant;

use iced::Element;

use crate::message::Message;

/// Where in the main bar a slot renders.
///
/// Sections are composed with per-section layout policy in [`super::view`]
/// (spacing, padding). The enum carries only identity, not style.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Section {
    /// Repo / branch / fetch indicators — left edge of the bar.
    Left,
    /// Action buttons (pull, push, branch, stash, …) — centered.
    Center,
    /// Search + plugin buttons — right edge.
    Right,
}

/// Read-only state handed to every slot builder for one render pass.
///
/// Borrows live for `'a` — the duration of the render call. Everything else
/// is `Copy` or owned so builders can clone cheaply when needed.
pub struct SlotCtx<'a> {
    pub repo_name: &'a str,
    pub branch_name: &'a str,
    pub fetch_started_at: Option<Instant>,
    pub push_started_at: Option<Instant>,
    pub pull_started_at: Option<Instant>,
    pub branch_action: Option<Message>,
    /// `true` when push or pull is in flight — used to disable other actions.
    pub busy: bool,
}

impl<'a> SlotCtx<'a> {
    pub fn new(
        repo_name: &'a str,
        branch_name: &'a str,
        fetch_started_at: Option<Instant>,
        push_started_at: Option<Instant>,
        pull_started_at: Option<Instant>,
        branch_action: Option<Message>,
    ) -> Self {
        let busy = push_started_at.is_some() || pull_started_at.is_some();
        Self {
            repo_name,
            branch_name,
            fetch_started_at,
            push_started_at,
            pull_started_at,
            branch_action,
            busy,
        }
    }
}

/// Builder closure signature for a slot. Two HRTB lifetimes:
///
/// - `'ctx` is the local borrow of the `SlotCtx` (short, scoped to the
///   render call).
/// - `'data` is the storage lifetime of the string refs inside the ctx
///   (long, scoped to the source data — repo/branch name strings).
///
/// The returned [`Element`] is tied to `'data`, not `'ctx` — so the caller
/// can keep the Element past the point where the `&SlotCtx` borrow ends,
/// as long as the underlying strings live on.
pub type SlotBuilder = Box<
    dyn for<'ctx, 'data> Fn(&'ctx SlotCtx<'data>) -> Element<'data, Message> + 'static,
>;

/// One contribution to the main bar.
///
/// The builder is the only behaviour; everything else (`id`, `section`,
/// `priority`) is addressing metadata used by the registry to place,
/// replace, or remove this slot.
pub struct MainBarSlot {
    pub id: String,
    pub section: Section,
    pub priority: i32,
    pub builder: SlotBuilder,
}

impl MainBarSlot {
    pub fn new<F>(id: impl Into<String>, section: Section, priority: i32, builder: F) -> Self
    where
        F: for<'ctx, 'data> Fn(&'ctx SlotCtx<'data>) -> Element<'data, Message> + 'static,
    {
        Self {
            id: id.into(),
            section,
            priority,
            builder: Box::new(builder),
        }
    }
}

impl std::fmt::Debug for MainBarSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainBarSlot")
            .field("id", &self.id)
            .field("section", &self.section)
            .field("priority", &self.priority)
            .finish_non_exhaustive()
    }
}
