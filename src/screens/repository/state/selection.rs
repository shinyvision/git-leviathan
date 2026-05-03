//! Commit-list selection state: anchor, multi-select range, file view mode.

use super::super::FileView;

#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct SelectionState {
    anchor: usize,
    selected: std::collections::BTreeSet<usize>,
    file_view: FileView,
}

impl SelectionState {
    pub(in crate::screens::repository) fn new() -> Self {
        let mut selected = std::collections::BTreeSet::new();
        selected.insert(0);
        Self {
            anchor: 0,
            selected,
            file_view: FileView::Path,
        }
    }

    pub(in crate::screens::repository) fn select_commit(&mut self, idx: usize) {
        self.anchor = idx;
        self.selected.clear();
        self.selected.insert(idx);
    }

    pub(in crate::screens::repository) fn anchor(&self) -> usize {
        self.anchor
    }

    /// Retarget the selection to a single commit by index. Used when a fetch
    /// reload shifts the hash previously under the anchor to a new index (or
    /// the hash has vanished and the caller wants to re-anchor to HEAD).
    pub(in crate::screens::repository) fn replace_with_single(&mut self, idx: usize) {
        self.anchor = idx;
        self.selected.clear();
        self.selected.insert(idx);
    }

    /// Replace the selection with a set of indices, pinning the anchor.
    /// `indices` must be non-empty and contain `anchor`.
    pub(in crate::screens::repository) fn replace_with_set(
        &mut self,
        anchor: usize,
        indices: &[usize],
    ) {
        debug_assert!(!indices.is_empty());
        debug_assert!(indices.contains(&anchor));
        self.anchor = anchor;
        self.selected.clear();
        for idx in indices {
            self.selected.insert(*idx);
        }
    }

    pub(in crate::screens::repository) fn toggle_commit(&mut self, idx: usize) {
        if self.selected.contains(&idx) {
            self.selected.remove(&idx);
            if self.selected.is_empty() {
                self.selected.insert(idx);
            } else if self.anchor == idx {
                self.anchor = *self.selected.iter().next().unwrap();
            }
        } else {
            self.selected.insert(idx);
            self.anchor = idx;
        }
    }

    pub(in crate::screens::repository) fn extend_range_to(&mut self, idx: usize) {
        let (lo, hi) = if self.anchor <= idx {
            (self.anchor, idx)
        } else {
            (idx, self.anchor)
        };
        self.selected.clear();
        for i in lo..=hi {
            self.selected.insert(i);
        }
    }

    pub(in crate::screens::repository) fn selected_commit(&self) -> usize {
        self.anchor
    }

    pub(in crate::screens::repository) fn selected_indices(&self) -> Vec<usize> {
        self.selected.iter().copied().collect()
    }

    pub(in crate::screens::repository) fn is_multi(&self) -> bool {
        self.selected.len() > 1
    }

    pub(in crate::screens::repository) fn file_view(&self) -> FileView {
        self.file_view
    }

    pub(in crate::screens::repository) fn set_file_view(&mut self, view: FileView) {
        self.file_view = view;
    }
}
