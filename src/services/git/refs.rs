use git2::BranchType;

use crate::services::{RepoRef, RepoRefKind};

use super::GitService;

pub(super) fn load_refs(service: &GitService) -> Vec<RepoRef> {
    let mut refs = Vec::new();

    if let Ok(branches) = service.repo.branches(Some(BranchType::Local)) {
        for branch_result in branches.flatten() {
            let (branch, _) = branch_result;
            let name = match branch.name() {
                Ok(Some(name)) => name.to_string(),
                _ => continue,
            };
            let target_hash = match branch.get().peel_to_commit() {
                Ok(commit) => commit.id().to_string(),
                Err(_) => continue,
            };

            refs.push(RepoRef {
                name: name.clone(),
                kind: RepoRefKind::LocalBranch,
                target_hash,
                remote_name: None,
                is_current: branch.is_head(),
                upstream_ref: branch
                    .upstream()
                    .ok()
                    .and_then(|up| up.name().ok().flatten().map(|s| s.to_string())),
            });
        }
    }

    if let Ok(branches) = service.repo.branches(Some(BranchType::Remote)) {
        for branch_result in branches.flatten() {
            let (branch, _) = branch_result;
            let full_name = match branch.name() {
                Ok(Some(name)) if !name.ends_with("/HEAD") => name.to_string(),
                _ => continue,
            };
            let target_hash = match branch.get().peel_to_commit() {
                Ok(commit) => commit.id().to_string(),
                Err(_) => continue,
            };

            let (remote_name, name) = match full_name.find('/') {
                Some(pos) => (
                    Some(full_name[..pos].to_string()),
                    full_name[pos + 1..].to_string(),
                ),
                None => (None, full_name.clone()),
            };

            refs.push(RepoRef {
                name,
                kind: RepoRefKind::RemoteBranch,
                target_hash,
                remote_name,
                is_current: false,
                upstream_ref: None,
            });
        }
    }

    let mut raw_tag_refs = Vec::new();
    let _ = service.repo.tag_foreach(|_oid, name_bytes| {
        if let Ok(name) = std::str::from_utf8(name_bytes) {
            raw_tag_refs.push(name.to_string());
        }
        true
    });

    for full_name in raw_tag_refs {
        let target_hash = match service
            .repo
            .find_reference(&full_name)
            .ok()
            .and_then(|reference| reference.peel_to_commit().ok())
        {
            Some(commit) => commit.id().to_string(),
            None => continue,
        };

        refs.push(RepoRef {
            name: full_name.trim_start_matches("refs/tags/").to_string(),
            kind: RepoRefKind::Tag,
            target_hash,
            remote_name: None,
            is_current: false,
            upstream_ref: None,
        });
    }

    refs
}
