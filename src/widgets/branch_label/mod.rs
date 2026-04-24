//! Branch + tag label rendering: inline pills, overflow popouts, and the
//! right-click context menu.

mod cell;
mod context_menu;
mod layout;
mod popout;

pub use cell::{branch_label_cell, branch_stack_background};
pub use context_menu::branch_context_menu;
pub use layout::any_name_truncated;
pub use popout::branch_popout_panel;

use crate::view_model::{BranchDisplayRow, BranchLabel, BranchLabelKind};

pub const BRANCH_POPOUT_RADIUS: f32 = 6.0;
pub const BRANCH_LABEL_INSET_X: u16 = 6;

// ─── Container IDs ────────────────────────────────────────────────────────────

pub fn branch_popout_trigger_id() -> iced::widget::Id {
    iced::widget::Id::new("branch-popout-trigger")
}

pub fn branch_popout_panel_id() -> iced::widget::Id {
    iced::widget::Id::new("branch-popout-panel")
}

pub fn branch_popout_content_id() -> iced::widget::Id {
    iced::widget::Id::new("branch-popout-content")
}

// ─── Label grouping ───────────────────────────────────────────────────────────

fn can_merge_into_row(row: &BranchDisplayRow, label: &BranchLabel) -> bool {
    row.name == label.name && row.is_tag == matches!(label.kind, BranchLabelKind::Tag)
}

pub fn branch_display_rows(labels: &[BranchLabel]) -> Vec<BranchDisplayRow> {
    let mut rows: Vec<BranchDisplayRow> = Vec::new();
    // Upstream ref (e.g. "origin/other-branch") captured for each row at build
    // time, so a second pass can merge a renamed remote into its local pill.
    let mut row_upstreams: Vec<Option<String>> = Vec::new();

    for label in labels {
        let label_is_local = matches!(
            label.kind,
            BranchLabelKind::Local | BranchLabelKind::CurrentLocal
        );
        let label_is_remote = matches!(label.kind, BranchLabelKind::Remote);
        let label_is_current = matches!(label.kind, BranchLabelKind::CurrentLocal);
        let label_is_tag = matches!(label.kind, BranchLabelKind::Tag);
        if let Some(idx) = rows.iter().position(|row| can_merge_into_row(row, label)) {
            let existing = &mut rows[idx];
            existing.has_local |= label_is_local;
            existing.has_remote |= label_is_remote;
            existing.is_current |= label_is_current;
            if label_is_remote && existing.remote_name.is_none() {
                existing.remote_name = label.remote_name.clone();
                existing.remote_branch_name = Some(label.name.clone());
            }
            if label_is_local && row_upstreams[idx].is_none() {
                row_upstreams[idx] = label.upstream_ref.clone();
            }
            continue;
        }

        rows.push(BranchDisplayRow {
            name: label.name.clone(),
            lane_color: label.lane_color,
            has_local: label_is_local,
            has_remote: label_is_remote,
            is_current: label_is_current,
            is_tag: label_is_tag,
            remote_name: if label_is_remote {
                label.remote_name.clone()
            } else {
                None
            },
            remote_branch_name: if label_is_remote {
                Some(label.name.clone())
            } else {
                None
            },
            worktree_path: None,
        });
        row_upstreams.push(if label_is_local {
            label.upstream_ref.clone()
        } else {
            None
        });
    }

    // Second pass: merge a pure-remote row into its tracking local row when
    // the local branch's upstream points to a differently-named remote
    // branch at the same commit. Keeps renamed-upstream pills from
    // double-listing.
    let mut remove: Vec<usize> = Vec::new();
    for local_idx in 0..rows.len() {
        if rows[local_idx].is_tag || !rows[local_idx].has_local {
            continue;
        }
        let Some(upstream_ref) = row_upstreams[local_idx].clone() else {
            continue;
        };
        let Some((upstream_remote, upstream_short)) = upstream_ref.split_once('/') else {
            continue;
        };
        let Some(remote_idx) = rows.iter().position(|row| {
            !row.is_tag
                && !row.has_local
                && row.has_remote
                && row.name == upstream_short
                && row.remote_name.as_deref() == Some(upstream_remote)
        }) else {
            continue;
        };
        if remote_idx == local_idx {
            continue;
        }
        rows[local_idx].has_remote = true;
        rows[local_idx].remote_name = Some(upstream_remote.to_string());
        rows[local_idx].remote_branch_name = Some(upstream_short.to_string());
        remove.push(remote_idx);
    }
    remove.sort_unstable();
    remove.dedup();
    for idx in remove.into_iter().rev() {
        rows.remove(idx);
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::branch_display_rows;
    use crate::view_model::BranchLabelKind;

    fn label(name: &str, kind: BranchLabelKind) -> crate::view_model::BranchLabel {
        crate::view_model::BranchLabel {
            name: name.to_string(),
            kind,
            lane_color: 0,
            remote_name: None,
            upstream_ref: None,
        }
    }

    #[test]
    fn branch_display_rows_merges_non_adjacent_local_and_remote_labels() {
        let rows = branch_display_rows(&[
            label("feature/foo", BranchLabelKind::Remote),
            label("release", BranchLabelKind::Local),
            label("feature/foo", BranchLabelKind::CurrentLocal),
        ]);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "feature/foo");
        assert!(rows[0].has_local);
        assert!(rows[0].has_remote);
        assert!(rows[0].is_current);
        assert_eq!(rows[1].name, "release");
    }

    #[test]
    fn branch_display_rows_merges_multiple_remote_refs_once() {
        let rows = branch_display_rows(&[
            label("feature/foo", BranchLabelKind::Remote),
            label("feature/foo", BranchLabelKind::Remote),
        ]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "feature/foo");
        assert!(!rows[0].has_local);
        assert!(rows[0].has_remote);
    }

    #[test]
    fn branch_display_rows_keeps_tag_separate_from_branch() {
        let rows = branch_display_rows(&[
            label("release", BranchLabelKind::Tag),
            label("release", BranchLabelKind::Remote),
            label("release", BranchLabelKind::Local),
        ]);

        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_tag);
        assert_eq!(rows[0].name, "release");
        assert!(!rows[1].is_tag);
        assert!(rows[1].has_local);
        assert!(rows[1].has_remote);
    }
}
