use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationKind {
    GitWrite,
    StageFile,
    StageAll,
    UnstageFile,
    UnstageAll,
    Commit,
    Discard,
    ResolveConflict,
    AbortMerge,
}

impl OperationKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::GitWrite => "Working...",
            Self::StageFile => "Staging...",
            Self::StageAll => "Staging...",
            Self::UnstageFile => "Unstaging...",
            Self::UnstageAll => "Unstaging...",
            Self::Commit => "Committing...",
            Self::Discard => "Discarding...",
            Self::ResolveConflict => "Resolving...",
            Self::AbortMerge => "Aborting...",
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct OperationCoordinator {
    next_id: u64,
    active_write: Option<(OperationId, OperationKind)>,
    pending_watcher_reload: Option<PathBuf>,
}

impl OperationCoordinator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn begin_write(&mut self) -> Option<OperationId> {
        self.begin_write_kind(OperationKind::GitWrite)
    }

    pub(crate) fn begin_write_kind(&mut self, kind: OperationKind) -> Option<OperationId> {
        if self.active_write.is_some() {
            return None;
        }
        let id = self.next_id();
        self.active_write = Some((id, kind));
        Some(id)
    }

    pub(crate) fn finish_write(&mut self, id: OperationId) -> bool {
        Self::finish(&mut self.active_write, id)
    }

    pub(crate) fn is_writing(&self) -> bool {
        self.active_write.is_some()
    }

    pub(crate) fn active_kind(&self) -> Option<OperationKind> {
        self.active_write.map(|(_, kind)| kind)
    }

    pub(crate) fn active_label(&self) -> Option<&'static str> {
        self.active_kind().map(OperationKind::label)
    }

    pub(crate) fn mark_watcher_reload_pending(&mut self, path: impl Into<PathBuf>) {
        self.pending_watcher_reload = Some(path.into());
    }

    pub(crate) fn take_pending_watcher_reload(&mut self) -> bool {
        self.pending_watcher_reload.take().is_some()
    }

    #[cfg(test)]
    pub(crate) fn pending_watcher_path(&self) -> Option<&std::path::Path> {
        self.pending_watcher_reload.as_deref()
    }

    fn next_id(&mut self) -> OperationId {
        self.next_id += 1;
        OperationId(self.next_id)
    }

    fn finish(slot: &mut Option<(OperationId, OperationKind)>, id: OperationId) -> bool {
        if slot.is_some_and(|(active_id, _)| active_id == id) {
            *slot = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationCoordinator, OperationKind};

    #[test]
    fn write_start_is_single_slot() {
        let mut ops = OperationCoordinator::new();
        let first = ops.begin_write();

        assert!(first.is_some());
        assert!(ops.begin_write().is_none());
        assert!(ops.is_writing());
    }

    #[test]
    fn finishing_stale_write_does_not_clear_active_write() {
        let mut ops = OperationCoordinator::new();
        let first = ops.begin_write().unwrap();

        assert!(ops.finish_write(first));
        let second = ops.begin_write().unwrap();

        assert!(!ops.finish_write(first));
        assert!(ops.is_writing());
        assert!(ops.finish_write(second));
        assert!(!ops.is_writing());
    }

    #[test]
    fn write_kind_is_recorded_and_cleared() {
        let mut ops = OperationCoordinator::new();
        let id = ops.begin_write_kind(OperationKind::Commit).unwrap();

        assert_eq!(ops.active_kind(), Some(OperationKind::Commit));
        assert_eq!(ops.active_label(), Some("Committing..."));

        assert!(ops.finish_write(id));
        assert_eq!(ops.active_kind(), None);
        assert_eq!(ops.active_label(), None);
    }

    #[test]
    fn kinded_write_uses_same_backpressure_slot() {
        let mut ops = OperationCoordinator::new();
        let first = ops.begin_write_kind(OperationKind::StageFile).unwrap();

        assert!(ops.begin_write_kind(OperationKind::UnstageAll).is_none());
        assert_eq!(ops.active_kind(), Some(OperationKind::StageFile));

        assert!(ops.finish_write(first));
        assert!(ops.begin_write_kind(OperationKind::UnstageAll).is_some());
    }

    #[test]
    fn pending_watcher_reload_coalesces() {
        let mut ops = OperationCoordinator::new();

        ops.mark_watcher_reload_pending("/repo");
        ops.mark_watcher_reload_pending("/repo");

        assert!(ops.take_pending_watcher_reload());
        assert!(!ops.take_pending_watcher_reload());
    }

    #[test]
    fn pending_watcher_reload_keeps_latest_path() {
        let mut ops = OperationCoordinator::new();

        ops.mark_watcher_reload_pending("/repo/one");
        ops.mark_watcher_reload_pending("/repo/two");

        assert_eq!(
            ops.pending_watcher_path(),
            Some(std::path::Path::new("/repo/two"))
        );
    }
}
