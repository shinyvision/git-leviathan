//! Slot type + render context for the main bar.
//!
//! `MainBarSlot` impls `crate::plugin::slots::IsSlot` so it can live in
//! the generic `SlotRegistry<MainBarSlot>`. The builder closure type is
//! unchanged from before the refactor — HRTB over the `SlotCtx` borrow.

use std::time::Instant;

use iced::Element;

use crate::message::Message;
use crate::plugin::slots::{Container, IsSlot, SlotAddress};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Section {
    Left,
    Center,
    Right,
}

impl Section {
    pub fn as_str(self) -> &'static str {
        match self {
            Section::Left => "left",
            Section::Center => "center",
            Section::Right => "right",
        }
    }

    pub fn container(self) -> Container {
        Container::Section(self.as_str().to_string())
    }
}

pub struct SlotCtx<'a> {
    pub repo_name: &'a str,
    pub branch_name: &'a str,
    pub fetch_started_at: Option<Instant>,
    pub push_started_at: Option<Instant>,
    pub pull_started_at: Option<Instant>,
    pub branch_action: Option<Message>,
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

pub type SlotBuilder =
    Box<dyn for<'ctx, 'data> Fn(&'ctx SlotCtx<'data>) -> Element<'data, Message> + 'static>;

pub struct MainBarSlot {
    pub id: String,
    pub address: SlotAddress,
    pub container: Container,
    pub priority: i32,
    pub builder: SlotBuilder,
}

impl MainBarSlot {
    pub fn new<F>(id: impl Into<String>, section: Section, priority: i32, builder: F) -> Self
    where
        F: for<'ctx, 'data> Fn(&'ctx SlotCtx<'data>) -> Element<'data, Message> + 'static,
    {
        let id = id.into();
        let container = section.container();
        Self {
            address: SlotAddress::builtin("main_bar", container.clone(), id.clone()),
            id,
            container,
            priority,
            builder: Box::new(builder),
        }
    }
}

impl IsSlot for MainBarSlot {
    fn address(&self) -> &SlotAddress {
        &self.address
    }
    fn display_id(&self) -> &str {
        &self.id
    }
    fn priority(&self) -> i32 {
        self.priority
    }
    fn set_priority(&mut self, priority: i32) {
        self.priority = priority;
    }
}

impl std::fmt::Debug for MainBarSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainBarSlot")
            .field("id", &self.id)
            .field("container", &self.container)
            .field("priority", &self.priority)
            .finish_non_exhaustive()
    }
}
