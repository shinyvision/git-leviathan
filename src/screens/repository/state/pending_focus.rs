//! Pending-focus request: "after the next commit reload, navigate/select
//! this target." Owned by `RepositoryData` because it's a data-layer
//! concern — the pending target survives across panel lifecycles and gets
//! resolved by any code path that completes a repo reload.

#[derive(Debug, Clone)]
pub(in crate::screens::repository) enum PendingFocus {
    Branch { name: String, is_remote: bool },
    Stash { hash: String },
    Commit { hash: String },
}
