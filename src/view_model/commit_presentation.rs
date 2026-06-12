use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BranchLabel {
    pub name: String,
    pub kind: BranchLabelKind,
    pub lane_color: usize,
    /// For remote-kind labels, the remote this branch belongs to (e.g. "origin").
    pub remote_name: Option<String>,
    /// For local-kind labels, the configured upstream ref if any
    /// (e.g. "origin/other-branch"). Used to merge a renamed remote into the
    /// same pill as its tracking local branch.
    pub upstream_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchLabelKind {
    CurrentLocal,
    Local,
    Remote,
    Tag,
}

/// Pre-computed display row that merges local/remote/tag labels by name.
/// Created once during projection and cached in CommitPresentation to avoid
/// recomputing on every frame.
#[derive(Debug, Clone)]
pub struct BranchDisplayRow {
    pub name: String,
    pub lane_color: usize,
    pub has_local: bool,
    pub has_remote: bool,
    pub is_current: bool,
    pub is_tag: bool,
    /// Remote this branch exists on, if any (e.g. "origin"). Populated from
    /// the remote-kind label; None when no remote counterpart.
    pub remote_name: Option<String>,
    /// Short branch name on the remote side (e.g. "other-branch"). Differs
    /// from `name` when a local branch tracks a remote of a different name.
    /// Used for remote-side rename/delete git operations.
    pub remote_branch_name: Option<String>,
    pub worktree_path: Option<PathBuf>,
}

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

#[derive(Debug, Clone)]
pub struct CommitPresentation {
    pub branch_labels: Vec<BranchLabel>,
    /// Pre-computed display rows (merged local/remote/tag labels).
    /// Avoids recomputing branch_display_rows() on every frame.
    pub branch_display_rows: Vec<BranchDisplayRow>,
    /// Relative time string shown in the commit list (e.g. "1 hour ago")
    pub relative_time: Option<String>,
    /// First line of the commit message, precomputed during projection.
    pub summary: String,
    /// Summary followed by the remaining message lines joined into one string.
    /// Equals `summary` exactly when there is no body, so the view can detect
    /// "has body" via `combined_message != summary`.
    pub combined_message: String,
}

#[cfg(test)]
mod tests {
    use super::{branch_display_rows, BranchLabel, BranchLabelKind};

    fn label(name: &str, kind: BranchLabelKind) -> BranchLabel {
        BranchLabel {
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
