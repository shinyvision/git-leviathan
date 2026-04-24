//! Graph-revision bump decision based on `ProjectionSignature` equality.
//!
//! Each new projection carries a signature hash over the inputs that affect
//! graph rendering. Consumers (one per repository screen) keep a
//! [`SignatureTracker`] that compares the incoming signature against the
//! last-seen one: when they differ the tracked `graph_revision` bumps, so
//! the canvas tile cache is invalidated. When they match the cache survives,
//! which matters for file-watcher cascades that project the same graph
//! repeatedly.
//!
//! Lives under `services/presenter/` so the signature lifecycle is owned by
//! the module that produces the signatures, not scattered across screen state.

use crate::core::RepoVersion;
use crate::view_model::ProjectionSignature;

#[derive(Debug, Default)]
pub struct SignatureTracker {
    last: Option<ProjectionSignature>,
    revision: RepoVersion,
}

impl SignatureTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new projection signature. Returns the (possibly bumped) revision.
    pub fn observe(&mut self, signature: ProjectionSignature) -> RepoVersion {
        if self.last != Some(signature) {
            self.last = Some(signature);
            self.revision = self.revision.bump();
        }
        self.revision
    }

    pub fn revision(&self) -> RepoVersion {
        self.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_unchanged_for_identical_signatures() {
        let mut tracker = SignatureTracker::new();
        let sig = ProjectionSignature(42);

        let first = tracker.observe(sig);
        let second = tracker.observe(sig);

        assert_eq!(first, second);
    }

    #[test]
    fn revision_bumps_on_new_signature() {
        let mut tracker = SignatureTracker::new();

        let r1 = tracker.observe(ProjectionSignature(1));
        let r2 = tracker.observe(ProjectionSignature(2));
        let r3 = tracker.observe(ProjectionSignature(2));

        assert_ne!(r1, r2);
        assert_eq!(r2, r3);
    }
}
