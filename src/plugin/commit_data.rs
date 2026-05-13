use serde::Serialize;

use crate::core::{Commit, CommitKind};
use crate::services::CommitSnapshot;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitActionAvailability {
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitActions {
    pub reword: CommitActionAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitData {
    pub kind: String,
    pub hash: String,
    pub short_hash: String,
    pub summary: String,
    pub message: String,
    pub author: String,
    pub date: String,
    pub timestamp: Option<i64>,
    pub parents: Vec<String>,
    pub index: Option<usize>,
    pub is_merge: bool,
    pub is_merge_in_progress: bool,
    pub actions: CommitActions,
}

impl CommitActionAvailability {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            reason: None,
        }
    }

    pub fn disabled(reason: impl Into<String>) -> Self {
        Self {
            enabled: false,
            reason: Some(reason.into()),
        }
    }
}

impl CommitActions {
    pub fn with_reword(reword: CommitActionAvailability) -> Self {
        Self { reword }
    }
}

impl CommitData {
    pub fn from_commit(
        commit: &Commit,
        index: Option<usize>,
        timestamp: Option<i64>,
        actions: CommitActions,
    ) -> Self {
        Self {
            kind: commit_kind_name(commit.kind).to_string(),
            hash: commit.hash.clone(),
            short_hash: commit.short_hash.clone(),
            summary: summary(&commit.message),
            message: commit.message.clone(),
            author: commit.author.clone(),
            date: commit.date.clone(),
            timestamp,
            parents: commit.parent_hashes.clone(),
            index,
            is_merge: commit.parent_hashes.len() > 1,
            is_merge_in_progress: commit.is_merge_in_progress,
            actions,
        }
    }

    pub fn from_snapshot(
        commit: &CommitSnapshot,
        index: Option<usize>,
        reword: CommitActionAvailability,
    ) -> Self {
        Self {
            kind: "commit".to_string(),
            hash: commit.hash.clone(),
            short_hash: short_hash(&commit.hash),
            summary: summary(&commit.message),
            message: commit.message.clone(),
            author: commit.author_name.clone(),
            date: String::new(),
            timestamp: Some(commit.authored_at),
            parents: commit.parent_hashes.clone(),
            index,
            is_merge: commit.parent_hashes.len() > 1,
            is_merge_in_progress: false,
            actions: CommitActions::with_reword(reword),
        }
    }

    pub fn from_hash(hash: &str) -> Self {
        Self {
            kind: "commit".to_string(),
            hash: hash.to_string(),
            short_hash: short_hash(hash),
            summary: String::new(),
            message: String::new(),
            author: String::new(),
            date: String::new(),
            timestamp: None,
            parents: Vec::new(),
            index: None,
            is_merge: false,
            is_merge_in_progress: false,
            actions: CommitActions::with_reword(CommitActionAvailability::disabled(
                "commit_data_unavailable",
            )),
        }
    }
}

pub fn commit_kind_name(kind: CommitKind) -> &'static str {
    match kind {
        CommitKind::Commit => "commit",
        CommitKind::Dirty => "dirty",
        CommitKind::Stash => "stash",
    }
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(7).collect()
}

fn summary(message: &str) -> String {
    message.lines().next().unwrap_or("").to_string()
}
