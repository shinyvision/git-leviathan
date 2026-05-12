//! Commit-list selection state: anchor and multi-select range.

#[derive(Debug, Clone)]
pub(in crate::screens::repository) struct SelectionState {
    anchor: usize,
    cursor: usize,
    selected: std::collections::BTreeSet<usize>,
}

impl SelectionState {
    pub(in crate::screens::repository) fn new() -> Self {
        let mut selected = std::collections::BTreeSet::new();
        selected.insert(0);
        Self {
            anchor: 0,
            cursor: 0,
            selected,
        }
    }

    pub(in crate::screens::repository) fn select_commit(&mut self, idx: usize) {
        self.anchor = idx;
        self.cursor = idx;
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
        self.cursor = idx;
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
        self.cursor = anchor;
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
                self.anchor = idx;
                self.cursor = idx;
            } else if self.anchor == idx {
                self.anchor = *self.selected.iter().next().unwrap();
                self.cursor = self.anchor;
            } else if self.cursor == idx {
                self.cursor = self.anchor;
            }
        } else {
            self.selected.insert(idx);
            self.anchor = idx;
            self.cursor = idx;
        }
    }

    pub(in crate::screens::repository) fn extend_range_to(&mut self, idx: usize) {
        self.cursor = idx;
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
        self.cursor
    }

    pub(in crate::screens::repository) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(in crate::screens::repository) fn selected_indices(&self) -> Vec<usize> {
        self.selected.iter().copied().collect()
    }

    pub(in crate::screens::repository) fn is_multi(&self) -> bool {
        self.selected.len() > 1
    }
}

#[cfg(test)]
mod tests {
    use super::SelectionState;

    #[test]
    fn range_extension_tracks_cursor_without_moving_anchor() {
        let mut selection = SelectionState::new();
        selection.select_commit(3);

        selection.extend_range_to(5);

        assert_eq!(selection.anchor(), 3);
        assert_eq!(selection.selected_commit(), 5);
        assert_eq!(selection.cursor(), 5);
        assert_eq!(selection.selected_indices(), vec![3, 4, 5]);

        selection.extend_range_to(4);

        assert_eq!(selection.anchor(), 3);
        assert_eq!(selection.selected_commit(), 4);
        assert_eq!(selection.cursor(), 4);
        assert_eq!(selection.selected_indices(), vec![3, 4]);
    }
}
