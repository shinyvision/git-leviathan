//! Network-op timer state (push/pull "in flight" indicators).
//!
//! command registry scope bundles this under `state/` so every piece of screen-local
//! time-driven state has one home. Overlay slide animations live with the
//! overlay manager since they are owned by the dialog lifecycle, not the
//! repository data.

use std::time::Instant;

#[derive(Debug, Default)]
pub(in crate::screens::repository) struct AnimationState {
    push_started_at: Option<Instant>,
    pull_started_at: Option<Instant>,
}

impl AnimationState {
    pub(in crate::screens::repository) fn new() -> Self {
        Self::default()
    }

    pub(in crate::screens::repository) fn push_started_at(&self) -> Option<Instant> {
        self.push_started_at
    }

    pub(in crate::screens::repository) fn pull_started_at(&self) -> Option<Instant> {
        self.pull_started_at
    }

    pub(in crate::screens::repository) fn mark_push_started(&mut self) {
        self.push_started_at = Some(Instant::now());
    }

    pub(in crate::screens::repository) fn mark_pull_started(&mut self) {
        self.pull_started_at = Some(Instant::now());
    }

    pub(in crate::screens::repository) fn clear_push(&mut self) {
        self.push_started_at = None;
    }

    pub(in crate::screens::repository) fn clear_pull(&mut self) {
        self.pull_started_at = None;
    }

    pub(in crate::screens::repository) fn network_op_in_flight(&self) -> bool {
        self.push_started_at.is_some() || self.pull_started_at.is_some()
    }
}
