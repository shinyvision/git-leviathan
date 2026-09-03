//! Fetches the raw bytes (or on-disk location) of both sides of a media
//! change so the media decoders can take over from the text diff pipeline.
//! Mirrors the old/new resolution rules of `working_tree_diff`:
//!
//! | mode            | old                     | new             |
//! |-----------------|-------------------------|-----------------|
//! | dirty, unstaged | index blob              | working tree    |
//! | dirty, staged   | HEAD blob               | index blob      |
//! | commit          | first parent's tree     | commit tree     |
//! | merged          | oldest commit's parent  | newest commit   |

use std::path::Path;
use std::sync::Arc;

use git2::Repository;

use crate::services::git_error::GitError;
use crate::services::media::{
    MediaDiffSources, MediaKind, MediaSource, MAX_AUDIO_FILE_BYTES, MAX_IMAGE_FILE_BYTES,
    MAX_VIDEO_FILE_BYTES,
};

use super::helpers::{find_commit_or, wrap_git2_error};
use super::GitService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaDiffRequest {
    Dirty {
        path: String,
        is_staged: bool,
        kind: MediaKind,
    },
    Commit {
        commit_hash: String,
        path: String,
        kind: MediaKind,
    },
    Merged {
        hashes: Vec<String>,
        path: String,
        kind: MediaKind,
    },
}

impl MediaDiffRequest {
    pub fn path(&self) -> &str {
        match self {
            MediaDiffRequest::Dirty { path, .. }
            | MediaDiffRequest::Commit { path, .. }
            | MediaDiffRequest::Merged { path, .. } => path,
        }
    }

    pub fn kind(&self) -> MediaKind {
        match self {
            MediaDiffRequest::Dirty { kind, .. }
            | MediaDiffRequest::Commit { kind, .. }
            | MediaDiffRequest::Merged { kind, .. } => *kind,
        }
    }
}

fn size_limit(kind: MediaKind) -> u64 {
    match kind {
        MediaKind::Image => MAX_IMAGE_FILE_BYTES,
        MediaKind::Audio => MAX_AUDIO_FILE_BYTES,
        MediaKind::Video => MAX_VIDEO_FILE_BYTES,
    }
}

pub(super) fn load_dirty_media_sources(
    service: &GitService,
    file_path: &str,
    is_staged: bool,
    kind: MediaKind,
) -> Result<MediaDiffSources, GitError> {
    let repo = &service.repo;
    let limit = size_limit(kind);
    let (old, new) = if is_staged {
        (
            source_from_head(repo, file_path, limit),
            source_from_index(repo, file_path, limit),
        )
    } else {
        (
            source_from_index(repo, file_path, limit),
            source_from_workdir(repo, file_path, limit),
        )
    };
    Ok(MediaDiffSources {
        file_path: file_path.to_string(),
        old,
        new,
    })
}

pub(super) fn load_commit_media_sources(
    repo: &Repository,
    commit_hash: &str,
    file_path: &str,
    kind: MediaKind,
) -> Result<MediaDiffSources, GitError> {
    let limit = size_limit(kind);
    let oid = git2::Oid::from_str(commit_hash)
        .map_err(|e| GitError::Other(format!("invalid commit hash: {e}")))?;
    let commit = find_commit_or(repo, oid)?;
    let tree = commit.tree().ok();
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let mut new = source_from_tree(repo, tree.as_ref(), file_path, limit);
    let old = source_from_tree(repo, parent_tree.as_ref(), file_path, limit);

    // Stash commits keep untracked files in a third parent.
    if new.is_missing() && old.is_missing() && super::diff::is_current_stash_commit(repo, oid) {
        if let Some(untracked) = commit.parent(2).ok().and_then(|p| p.tree().ok()) {
            new = source_from_tree(repo, Some(&untracked), file_path, limit);
        }
    }

    Ok(MediaDiffSources {
        file_path: file_path.to_string(),
        old,
        new,
    })
}

pub(super) fn load_merged_media_sources(
    repo_path: &str,
    hashes: &[String],
    file_path: &str,
    kind: MediaKind,
) -> Result<MediaDiffSources, GitError> {
    let repo = Repository::open(repo_path).map_err(|e| wrap_git2_error("open repo", e))?;
    if hashes.len() < 2 {
        let hash = hashes
            .first()
            .ok_or_else(|| GitError::Other("no commits selected".to_string()))?;
        return load_commit_media_sources(&repo, hash, file_path, kind);
    }
    let limit = size_limit(kind);
    let newest = find_commit_or(&repo, parse_oid(&hashes[0])?)?;
    let oldest = find_commit_or(&repo, parse_oid(hashes.last().expect("non-empty"))?)?;
    let newest_tree = newest.tree().ok();
    let oldest_parent_tree = oldest.parent(0).ok().and_then(|p| p.tree().ok());
    Ok(MediaDiffSources {
        file_path: file_path.to_string(),
        old: source_from_tree(&repo, oldest_parent_tree.as_ref(), file_path, limit),
        new: source_from_tree(&repo, newest_tree.as_ref(), file_path, limit),
    })
}

fn parse_oid(hash: &str) -> Result<git2::Oid, GitError> {
    git2::Oid::from_str(hash).map_err(|e| GitError::Other(format!("invalid commit hash: {e}")))
}

fn source_from_head(repo: &Repository, file_path: &str, limit: u64) -> MediaSource {
    let head_tree = repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .and_then(|commit| commit.tree().ok());
    source_from_tree(repo, head_tree.as_ref(), file_path, limit)
}

fn source_from_index(repo: &Repository, file_path: &str, limit: u64) -> MediaSource {
    let Some(entry) = repo
        .index()
        .ok()
        .and_then(|index| index.get_path(Path::new(file_path), 0))
    else {
        return MediaSource::Missing;
    };
    source_from_oid(repo, entry.id, limit)
}

fn source_from_tree(
    repo: &Repository,
    tree: Option<&git2::Tree<'_>>,
    file_path: &str,
    limit: u64,
) -> MediaSource {
    let Some(entry) = tree.and_then(|tree| tree.get_path(Path::new(file_path)).ok()) else {
        return MediaSource::Missing;
    };
    if entry.kind() != Some(git2::ObjectType::Blob) {
        return MediaSource::Missing;
    }
    source_from_oid(repo, entry.id(), limit)
}

fn source_from_oid(repo: &Repository, oid: git2::Oid, limit: u64) -> MediaSource {
    // Check the size from the object header before pulling the blob into
    // memory — a multi-GB asset must never be loaded just to be rejected.
    if let Ok(odb) = repo.odb() {
        if let Ok((size, _kind)) = odb.read_header(oid) {
            if size as u64 > limit {
                return MediaSource::TooLarge {
                    bytes: size as u64,
                    max: limit,
                };
            }
        }
    }
    let Ok(blob) = repo.find_blob(oid) else {
        return MediaSource::Missing;
    };
    let bytes: Arc<[u8]> = Arc::from(blob.content());
    MediaSource::Blob {
        bytes,
        oid: oid.to_string(),
    }
}

fn source_from_workdir(repo: &Repository, file_path: &str, limit: u64) -> MediaSource {
    let Some(path) = repo.workdir().map(|wd| wd.join(file_path)) else {
        return MediaSource::Missing;
    };
    let Ok(meta) = std::fs::metadata(&path) else {
        return MediaSource::Missing;
    };
    if !meta.is_file() {
        return MediaSource::Missing;
    }
    if meta.len() > limit {
        return MediaSource::TooLarge {
            bytes: meta.len(),
            max: limit,
        };
    }
    MediaSource::WorkdirFile {
        path,
        size: meta.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::{commit_all, init_test_repo, write_file};

    fn png_bytes() -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3]
    }

    fn write_bytes(root: &std::path::Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn untracked_media_has_missing_old_and_workdir_new() {
        let (temp, _repo) = init_test_repo("media_untracked");
        write_bytes(&temp.path, "img/new.png", &png_bytes());
        let service = GitService::open(temp.path_str()).unwrap();
        let sources =
            load_dirty_media_sources(&service, "img/new.png", false, MediaKind::Image).unwrap();
        assert!(sources.old.is_missing());
        match sources.new {
            MediaSource::WorkdirFile { size, .. } => assert_eq!(size, 11),
            other => panic!("expected workdir file, got {other:?}"),
        }
    }

    #[test]
    fn committed_media_resolves_parent_and_commit_blobs() {
        let (temp, repo) = init_test_repo("media_commit");
        write_bytes(&temp.path, "a.png", &png_bytes());
        commit_all(&repo, "add");
        let mut changed = png_bytes();
        changed.push(9);
        write_bytes(&temp.path, "a.png", &changed);
        let commit = commit_all(&repo, "change");

        let sources =
            load_commit_media_sources(&repo, &commit.to_string(), "a.png", MediaKind::Image)
                .unwrap();
        match (&sources.old, &sources.new) {
            (MediaSource::Blob { bytes: o, .. }, MediaSource::Blob { bytes: n, .. }) => {
                assert_eq!(o.len(), 11);
                assert_eq!(n.len(), 12);
            }
            other => panic!("unexpected sources {other:?}"),
        }
    }

    #[test]
    fn deleted_media_has_missing_new_side() {
        let (temp, repo) = init_test_repo("media_deleted");
        write_bytes(&temp.path, "a.png", &png_bytes());
        commit_all(&repo, "add");
        std::fs::remove_file(temp.path.join("a.png")).unwrap();
        let service = GitService::open(temp.path_str()).unwrap();
        let sources =
            load_dirty_media_sources(&service, "a.png", false, MediaKind::Image).unwrap();
        assert!(matches!(sources.old, MediaSource::Blob { .. }));
        assert!(sources.new.is_missing());
    }

    #[test]
    fn staged_media_uses_head_and_index() {
        let (temp, repo) = init_test_repo("media_staged");
        write_bytes(&temp.path, "a.png", &png_bytes());
        commit_all(&repo, "add");
        let mut changed = png_bytes();
        changed.extend_from_slice(&[7, 7]);
        write_bytes(&temp.path, "a.png", &changed);
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.png")).unwrap();
        index.write().unwrap();
        let service = GitService::open(temp.path_str()).unwrap();
        let sources =
            load_dirty_media_sources(&service, "a.png", true, MediaKind::Image).unwrap();
        match (&sources.old, &sources.new) {
            (MediaSource::Blob { bytes: o, .. }, MediaSource::Blob { bytes: n, .. }) => {
                assert_eq!(o.len(), 11);
                assert_eq!(n.len(), 13);
            }
            other => panic!("unexpected sources {other:?}"),
        }
    }

    #[test]
    fn merged_range_spans_oldest_parent_to_newest() {
        let (temp, repo) = init_test_repo("media_merged");
        write_file(&temp.path, "seed.txt", "x");
        commit_all(&repo, "seed");
        write_bytes(&temp.path, "a.png", &png_bytes());
        let c1 = commit_all(&repo, "add");
        let mut changed = png_bytes();
        changed.push(1);
        write_bytes(&temp.path, "a.png", &changed);
        let c2 = commit_all(&repo, "change");
        let sources = load_merged_media_sources(
            temp.path_str(),
            &[c2.to_string(), c1.to_string()],
            "a.png",
            MediaKind::Image,
        )
        .unwrap();
        assert!(sources.old.is_missing());
        match sources.new {
            MediaSource::Blob { bytes, .. } => assert_eq!(bytes.len(), 12),
            other => panic!("unexpected {other:?}"),
        }
    }
}
