use std::{cell::RefCell, path::Path};

use crate::services::git_error::GitError;

use super::helpers::{find_commit_or, wrap_git2_error};
use super::GitService;

pub const MAX_HIGHLIGHT_FILE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_HIGHLIGHT_LINES: usize = 50_000;
pub const MAX_DIFF_LINES_RENDERED_FULLY: usize = 5_000;
pub const MAX_INTRA_LINE_DIFF_LINE_CHARS: usize = 2_000;
pub const MAX_INTRA_LINE_DIFF_PAIRS: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Context,
    Addition,
    AdditionHighlight,
    Deletion,
    DeletionHighlight,
}

#[derive(Debug, Clone)]
pub struct DiffSegment {
    pub kind: SegmentKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct WorkingTreeDiffLine {
    pub line_type: DiffLineType,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub segments: Vec<DiffSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineType {
    Context,
    Addition,
    Deletion,
    HunkHeader,
    FileHeader,
    AddEofnl,
    DeleteEofnl,
    ContextEofnl,
}

#[derive(Debug, Clone)]
pub struct WorkingTreeDiffResult {
    pub file_path: String,
    pub lines: Vec<WorkingTreeDiffLine>,
    pub old_file_content: Option<String>,
    pub new_file_content: Option<String>,
    pub dirty_signature: Option<DirtyDiffSignature>,
    pub fallbacks: DiffFallbacks,
}

#[derive(Debug, Clone, Default)]
pub struct DiffFallbacks {
    pub highlight_skips: Vec<DiffContentSkip>,
    pub intra_line_skip: Option<IntraLineSkip>,
    pub truncation: Option<DiffTruncation>,
}

impl DiffFallbacks {
    /// Media kind sniffed from binary content on either side, if any.
    pub fn media_kind(&self) -> Option<crate::services::media::MediaKind> {
        self.highlight_skips.iter().find_map(|skip| match skip.reason {
            DiffContentSkipReason::Media(kind) => Some(kind),
            _ => None,
        })
    }

    pub fn has_binary_or_non_utf8(&self) -> bool {
        self.highlight_skips.iter().any(|skip| {
            matches!(
                skip.reason,
                DiffContentSkipReason::Binary
                    | DiffContentSkipReason::Media(_)
                    | DiffContentSkipReason::NonUtf8
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    Old,
    New,
}

#[derive(Debug, Clone)]
pub struct DiffContentSkip {
    pub side: DiffSide,
    pub reason: DiffContentSkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffContentSkipReason {
    Binary,
    /// Binary content whose header identifies an image/audio/video format.
    Media(crate::services::media::MediaKind),
    NonUtf8,
    TooManyBytes { bytes: usize, max: usize },
    TooManyLines { lines: usize, max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntraLineSkip {
    pub skipped_pairs: usize,
    pub max_line_chars: usize,
    pub max_pairs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffTruncation {
    pub shown_lines: usize,
    pub omitted_lines: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirtyDiffSignature {
    pub left: Option<String>,
    pub right: Option<String>,
}

pub(super) fn load_working_tree_diff(
    service: &GitService,
    file_path: &str,
    is_staged: bool,
) -> Result<WorkingTreeDiffResult, GitError> {
    let repo = &service.repo;
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file_path).disable_pathspec_match(true);

    let diff = if is_staged {
        let head_tree = repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok())
            .and_then(|commit| commit.tree().ok());
        repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))
            .map_err(|e| wrap_git2_error("create staged diff", e))?
    } else {
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true);
        repo.diff_index_to_workdir(None, Some(&mut opts))
            .map_err(|e| wrap_git2_error("create working tree diff", e))?
    };

    let mut result = process_git_diff(
        diff,
        file_path,
        || {
            if is_staged {
                content_from_head(repo, file_path)
            } else {
                content_from_index(repo, file_path)
            }
        },
        || {
            if is_staged {
                content_from_index(repo, file_path)
            } else {
                content_from_workdir(repo, file_path)
            }
        },
    )?;
    result.dirty_signature = Some(compute_dirty_signature(repo, file_path, is_staged));
    Ok(result)
}

pub(super) fn compute_dirty_signature(
    repo: &git2::Repository,
    file_path: &str,
    is_staged: bool,
) -> DirtyDiffSignature {
    if is_staged {
        let head = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .and_then(|c| c.tree().ok())
            .and_then(|t| t.get_path(Path::new(file_path)).ok())
            .map(|e| e.id().to_string());
        let index = repo
            .index()
            .ok()
            .and_then(|i| i.get_path(Path::new(file_path), 0))
            .map(|e| e.id.to_string());
        DirtyDiffSignature {
            left: head,
            right: index,
        }
    } else {
        let index = repo
            .index()
            .ok()
            .and_then(|i| i.get_path(Path::new(file_path), 0))
            .map(|e| e.id.to_string());
        let workdir = repo
            .workdir()
            .map(|wd| wd.join(file_path))
            .and_then(|p| std::fs::metadata(&p).ok())
            .map(|m| {
                let size = m.len();
                let mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                format!("{size}:{mtime}")
            });
        DirtyDiffSignature {
            left: index,
            right: workdir,
        }
    }
}

pub(super) fn load_commit_file_diff(
    service: &GitService,
    commit_hash: &str,
    file_path: &str,
) -> Result<WorkingTreeDiffResult, GitError> {
    load_commit_file_diff_from_repo(&service.repo, commit_hash, file_path)
}

pub(super) fn load_commit_file_diff_standalone(
    repo_path: &str,
    commit_hash: &str,
    file_path: &str,
) -> Result<WorkingTreeDiffResult, GitError> {
    let repo = git2::Repository::open(repo_path).map_err(|e| wrap_git2_error("open repo", e))?;
    load_commit_file_diff_from_repo(&repo, commit_hash, file_path)
}

fn load_commit_file_diff_from_repo(
    repo: &git2::Repository,
    commit_hash: &str,
    file_path: &str,
) -> Result<WorkingTreeDiffResult, GitError> {
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file_path).disable_pathspec_match(true);

    let commit_oid = git2::Oid::from_str(commit_hash)
        .map_err(|e| GitError::Other(format!("invalid commit hash: {e}")))?;
    let commit = find_commit_or(repo, commit_oid)?;

    let tree = commit.tree().ok();
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), tree.as_ref(), Some(&mut opts))
        .map_err(|e| wrap_git2_error("create commit diff", e))?;

    if diff.deltas().len() == 0 {
        if let Some(untracked_tree) = stash_untracked_tree_for_path(repo, &commit, file_path) {
            let mut untracked_opts = git2::DiffOptions::new();
            untracked_opts
                .pathspec(file_path)
                .disable_pathspec_match(true);
            let untracked_diff = repo
                .diff_tree_to_tree(None, Some(&untracked_tree), Some(&mut untracked_opts))
                .map_err(|e| wrap_git2_error("create stash untracked file diff", e))?;
            return process_git_diff(untracked_diff, file_path, FileContent::missing, || {
                content_from_tree(repo, Some(&untracked_tree), file_path)
            });
        }
    }

    process_git_diff(
        diff,
        file_path,
        || content_from_tree(repo, parent_tree.as_ref(), file_path),
        || content_from_tree(repo, tree.as_ref(), file_path),
    )
}

fn stash_untracked_tree_for_path<'repo>(
    repo: &'repo git2::Repository,
    commit: &git2::Commit<'repo>,
    file_path: &str,
) -> Option<git2::Tree<'repo>> {
    if !super::diff::is_current_stash_commit(repo, commit.id()) {
        return None;
    }
    let tree = commit.parent(2).ok().and_then(|p| p.tree().ok())?;
    tree.get_path(Path::new(file_path)).ok()?;
    Some(tree)
}

pub(super) fn process_git_diff<G, H>(
    diff: git2::Diff<'_>,
    file_path: &str,
    old_content_getter: G,
    new_content_getter: H,
) -> Result<WorkingTreeDiffResult, GitError>
where
    G: FnOnce() -> FileContent,
    H: FnOnce() -> FileContent,
{
    let in_target_file = RefCell::new(false);
    let lines: RefCell<Vec<WorkingTreeDiffLine>> = RefCell::new(Vec::new());
    let omitted_lines = RefCell::new(0usize);
    let non_utf8_diff_line = RefCell::new(false);

    diff.foreach(
        &mut |delta: git2::DiffDelta<'_>, _progress| {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            *in_target_file.borrow_mut() = path == file_path;
            true
        },
        None,
        Some(
            &mut |_delta: git2::DiffDelta<'_>, hunk: git2::DiffHunk<'_>| {
                if *in_target_file.borrow() {
                    if lines.borrow().len() >= MAX_DIFF_LINES_RENDERED_FULLY {
                        *omitted_lines.borrow_mut() += 1;
                        return true;
                    }
                    let header_str = format!(
                        "@@ -{},{} +{},{} @@",
                        hunk.old_start(),
                        hunk.old_lines(),
                        hunk.new_start(),
                        hunk.new_lines()
                    );
                    lines.borrow_mut().push(WorkingTreeDiffLine {
                        line_type: DiffLineType::HunkHeader,
                        content: header_str,
                        old_lineno: None,
                        new_lineno: None,
                        segments: vec![],
                    });
                }
                true
            },
        ),
        Some(&mut |_delta: git2::DiffDelta<'_>,
                   _hunk: Option<git2::DiffHunk<'_>>,
                   line: git2::DiffLine<'_>| {
            if *in_target_file.borrow() {
                if lines.borrow().len() >= MAX_DIFF_LINES_RENDERED_FULLY {
                    *omitted_lines.borrow_mut() += 1;
                    return true;
                }
                let origin = line.origin();
                let line_type = match origin {
                    '+' => DiffLineType::Addition,
                    '-' => DiffLineType::Deletion,
                    '=' => DiffLineType::ContextEofnl,
                    '>' => DiffLineType::AddEofnl,
                    '<' => DiffLineType::DeleteEofnl,
                    _ => DiffLineType::Context,
                };
                let raw_content = match std::str::from_utf8(line.content()) {
                    Ok(content) => content,
                    Err(_) => {
                        *non_utf8_diff_line.borrow_mut() = true;
                        "<non-UTF-8 diff line>\n"
                    }
                };
                let content = raw_content.trim_end_matches('\n').replace('\t', "    ");
                let segments = vec![DiffSegment {
                    kind: match line_type {
                        DiffLineType::Addition => SegmentKind::Addition,
                        DiffLineType::Deletion => SegmentKind::Deletion,
                        _ => SegmentKind::Context,
                    },
                    text: content.clone(),
                }];
                lines.borrow_mut().push(WorkingTreeDiffLine {
                    line_type,
                    content,
                    old_lineno: line.old_lineno(),
                    new_lineno: line.new_lineno(),
                    segments,
                });
            }
            true
        }),
    )
    .map_err(|e| wrap_git2_error("iterate diff", e))?;

    let result_lines = lines.into_inner();
    let result_lines = normalize_eof_newline_changes(result_lines);
    let (result_lines, intra_line_skip) = compute_intra_line_diffs(result_lines);

    let old_file_content = old_content_getter();
    let new_file_content = new_content_getter();
    let mut fallbacks = DiffFallbacks {
        intra_line_skip,
        truncation: None,
        highlight_skips: Vec::new(),
    };
    if let Some(skip) = old_file_content.skip {
        fallbacks.highlight_skips.push(DiffContentSkip {
            side: DiffSide::Old,
            reason: skip,
        });
    }
    if let Some(skip) = new_file_content.skip {
        fallbacks.highlight_skips.push(DiffContentSkip {
            side: DiffSide::New,
            reason: skip,
        });
    }
    if *non_utf8_diff_line.borrow() {
        fallbacks.highlight_skips.push(DiffContentSkip {
            side: DiffSide::New,
            reason: DiffContentSkipReason::NonUtf8,
        });
    }
    let omitted_lines = omitted_lines.into_inner();
    if omitted_lines > 0 {
        fallbacks.truncation = Some(DiffTruncation {
            shown_lines: result_lines.len(),
            omitted_lines,
        });
    }

    Ok(WorkingTreeDiffResult {
        file_path: file_path.to_string(),
        lines: result_lines,
        old_file_content: old_file_content.text,
        new_file_content: new_file_content.text,
        dirty_signature: None,
        fallbacks,
    })
}

#[derive(Debug, Clone)]
pub(super) struct FileContent {
    text: Option<String>,
    skip: Option<DiffContentSkipReason>,
}

impl FileContent {
    pub(super) fn missing() -> Self {
        Self {
            text: None,
            skip: None,
        }
    }

    fn skipped(reason: DiffContentSkipReason) -> Self {
        Self {
            text: None,
            skip: Some(reason),
        }
    }

    fn text(text: String) -> Self {
        Self {
            text: Some(text),
            skip: None,
        }
    }
}

fn content_from_workdir(repo: &git2::Repository, file_path: &str) -> FileContent {
    let Some(path) = repo.workdir().map(|wd| wd.join(file_path)) else {
        return FileContent::missing();
    };
    let Ok(meta) = std::fs::metadata(&path) else {
        return FileContent::missing();
    };
    if meta.len() as usize > MAX_HIGHLIGHT_FILE_BYTES {
        return FileContent::skipped(DiffContentSkipReason::TooManyBytes {
            bytes: meta.len() as usize,
            max: MAX_HIGHLIGHT_FILE_BYTES,
        });
    }
    let Ok(bytes) = std::fs::read(path) else {
        return FileContent::missing();
    };
    content_from_bytes(&bytes)
}

fn content_from_head(repo: &git2::Repository, file_path: &str) -> FileContent {
    let head_tree = repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .and_then(|commit| commit.tree().ok());

    content_from_tree(repo, head_tree.as_ref(), file_path)
}

fn content_from_index(repo: &git2::Repository, file_path: &str) -> FileContent {
    let Some(entry) = repo
        .index()
        .ok()
        .and_then(|index| index.get_path(Path::new(file_path), 0))
    else {
        return FileContent::missing();
    };
    let Ok(blob) = repo.find_blob(entry.id) else {
        return FileContent::missing();
    };
    content_from_blob(&blob)
}

fn content_from_tree(
    repo: &git2::Repository,
    tree: Option<&git2::Tree<'_>>,
    file_path: &str,
) -> FileContent {
    let Some(entry) = tree.and_then(|tree| tree.get_path(Path::new(file_path)).ok()) else {
        return FileContent::missing();
    };
    let Ok(blob) = repo.find_blob(entry.id()) else {
        return FileContent::missing();
    };
    content_from_blob(&blob)
}

pub(super) fn content_from_blob(blob: &git2::Blob<'_>) -> FileContent {
    if blob.is_binary() {
        return FileContent::skipped(binary_skip_reason(blob.content()));
    }
    content_from_bytes(blob.content())
}

/// Binary content is either opaque or, when the header says so, media the
/// viewer can render.
fn binary_skip_reason(bytes: &[u8]) -> DiffContentSkipReason {
    match crate::services::media::sniff_media_kind(&bytes[..bytes.len().min(64 * 1024)]) {
        Some(kind) => DiffContentSkipReason::Media(kind),
        None => DiffContentSkipReason::Binary,
    }
}

fn content_from_bytes(bytes: &[u8]) -> FileContent {
    if bytes.len() > MAX_HIGHLIGHT_FILE_BYTES {
        return FileContent::skipped(DiffContentSkipReason::TooManyBytes {
            bytes: bytes.len(),
            max: MAX_HIGHLIGHT_FILE_BYTES,
        });
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        // Working-tree files never go through `Blob::is_binary`; sniff here so
        // an untracked image without a media extension still gets a viewer.
        if let Some(kind) =
            crate::services::media::sniff_media_kind(&bytes[..bytes.len().min(64 * 1024)])
        {
            return FileContent::skipped(DiffContentSkipReason::Media(kind));
        }
        return FileContent::skipped(DiffContentSkipReason::NonUtf8);
    };
    let line_count = text.lines().take(MAX_HIGHLIGHT_LINES + 1).count();
    if line_count > MAX_HIGHLIGHT_LINES {
        return FileContent::skipped(DiffContentSkipReason::TooManyLines {
            lines: line_count,
            max: MAX_HIGHLIGHT_LINES,
        });
    }
    FileContent::text(text.to_string())
}

fn normalize_eof_newline_changes(lines: Vec<WorkingTreeDiffLine>) -> Vec<WorkingTreeDiffLine> {
    let mut normalized = Vec::with_capacity(lines.len());
    let mut index = 0;

    while index < lines.len() {
        if let Some((skip, collapsed)) = collapse_eof_newline_change(&lines, index) {
            normalized.extend(collapsed);
            index += skip;
        } else {
            normalized.push(lines[index].clone());
            index += 1;
        }
    }

    normalized
}

fn collapse_eof_newline_change(
    lines: &[WorkingTreeDiffLine],
    index: usize,
) -> Option<(usize, [WorkingTreeDiffLine; 2])> {
    let deletion = lines.get(index)?;
    if deletion.line_type != DiffLineType::Deletion {
        return None;
    }

    let first = lines.get(index + 1)?;

    if let Some(eof_line_type) = eof_marker_type(first.line_type) {
        let addition = lines.get(index + 2)?;
        if addition.line_type == DiffLineType::Addition && deletion.content == addition.content {
            return Some((
                3,
                eof_newline_replacement_lines(deletion, addition, eof_line_type),
            ));
        }
    }

    if first.line_type == DiffLineType::Addition && deletion.content == first.content {
        if let Some(eof_line_type) = lines
            .get(index + 2)
            .and_then(|line| eof_marker_type(line.line_type))
        {
            return Some((
                3,
                eof_newline_replacement_lines(deletion, first, eof_line_type),
            ));
        }

        if is_hunk_boundary(lines, index + 2) {
            return Some((
                2,
                eof_newline_replacement_lines(deletion, first, DiffLineType::AddEofnl),
            ));
        }
    }

    None
}

fn eof_marker_type(line_type: DiffLineType) -> Option<DiffLineType> {
    match line_type {
        DiffLineType::AddEofnl | DiffLineType::DeleteEofnl => Some(line_type),
        _ => None,
    }
}

fn eof_newline_replacement_lines(
    deletion: &WorkingTreeDiffLine,
    addition: &WorkingTreeDiffLine,
    eof_line_type: DiffLineType,
) -> [WorkingTreeDiffLine; 2] {
    [
        WorkingTreeDiffLine {
            line_type: DiffLineType::Context,
            content: deletion.content.clone(),
            old_lineno: deletion.old_lineno,
            new_lineno: addition.new_lineno,
            segments: vec![DiffSegment {
                kind: SegmentKind::Context,
                text: deletion.content.clone(),
            }],
        },
        WorkingTreeDiffLine {
            line_type: eof_line_type,
            content: String::new(),
            old_lineno: None,
            new_lineno: None,
            segments: vec![],
        },
    ]
}

fn is_hunk_boundary(lines: &[WorkingTreeDiffLine], index: usize) -> bool {
    lines.get(index).is_none_or(|line| {
        matches!(
            line.line_type,
            DiffLineType::HunkHeader | DiffLineType::FileHeader
        )
    })
}

/// Compute intra-line diffs for hunks with matching deletion/addition pairs.
/// Uses longest common substring to find changed portions.
fn compute_intra_line_diffs(
    lines: Vec<WorkingTreeDiffLine>,
) -> (Vec<WorkingTreeDiffLine>, Option<IntraLineSkip>) {
    let span = crate::perf::Span::new("cpu.intra_line_diff").field("input_lines", lines.len());
    let mut result = Vec::with_capacity(lines.len());
    let mut i = 0;
    let mut pairs = 0usize;
    let mut skipped_pairs = 0usize;

    while i < lines.len() {
        let line = &lines[i];

        // Look for deletion followed by addition with same base content
        if line.line_type == DiffLineType::Deletion
            && i + 1 < lines.len()
            && lines[i + 1].line_type == DiffLineType::Addition
        {
            let deletion = &lines[i];
            let addition = &lines[i + 1];
            let deletion_chars = deletion.content.chars().count();
            let addition_chars = addition.content.chars().count();
            if pairs >= MAX_INTRA_LINE_DIFF_PAIRS
                || deletion_chars > MAX_INTRA_LINE_DIFF_LINE_CHARS
                || addition_chars > MAX_INTRA_LINE_DIFF_LINE_CHARS
            {
                skipped_pairs += 1;
                result.push(deletion.clone());
                result.push(addition.clone());
            } else {
                pairs += 1;
                let (del_segments, add_segments) =
                    compute_line_diff(&deletion.content, &addition.content);

                let mut del_line = deletion.clone();
                del_line.segments = del_segments;

                let mut add_line = addition.clone();
                add_line.segments = add_segments;

                result.push(del_line);
                result.push(add_line);
            }
            i += 2;
        } else {
            // For lines without segments (headers, context, EOF markers), ensure empty segments vec
            if line.line_type != DiffLineType::Addition
                && line.line_type != DiffLineType::Deletion
                && line.segments.is_empty()
            {
                let mut line = line.clone();
                line.segments = vec![DiffSegment {
                    kind: match line.line_type {
                        DiffLineType::Addition => SegmentKind::Addition,
                        DiffLineType::Deletion => SegmentKind::Deletion,
                        _ => SegmentKind::Context,
                    },
                    text: line.content.clone(),
                }];
                result.push(line);
            } else {
                result.push(line.clone());
            }
            i += 1;
        }
    }

    span.field("skipped_pairs", skipped_pairs)
        .finish_with("pairs", pairs);
    let skip = (skipped_pairs > 0).then_some(IntraLineSkip {
        skipped_pairs,
        max_line_chars: MAX_INTRA_LINE_DIFF_LINE_CHARS,
        max_pairs: MAX_INTRA_LINE_DIFF_PAIRS,
    });
    (result, skip)
}

fn longest_common_substring(a: &str, b: &str) -> (usize, usize, usize) {
    if a.is_empty() || b.is_empty() {
        return (0, 0, 0);
    }

    let a_chars: Vec<(usize, char)> = a.char_indices().collect();
    let b_chars: Vec<(usize, char)> = b.char_indices().collect();
    let m = b_chars.len();

    let mut dp = vec![vec![0u16; m + 1]; 2];
    let mut best_len = 0usize;
    let mut best_a_char = 0usize;
    let mut best_b_char = 0usize;

    for (i, (_, a_char)) in a_chars.iter().enumerate() {
        let cur = (i + 1) % 2;
        let prev = i % 2;
        for (j, (_, b_char)) in b_chars.iter().enumerate() {
            if a_char == b_char {
                dp[cur][j + 1] = dp[prev][j] + 1;
                if dp[cur][j + 1] as usize > best_len {
                    best_len = dp[cur][j + 1] as usize;
                    best_a_char = (i + 1).saturating_sub(best_len);
                    best_b_char = (j + 1).saturating_sub(best_len);
                }
            } else {
                dp[cur][j + 1] = 0;
            }
        }
    }

    let best_a = a_chars.get(best_a_char).map_or(a.len(), |(idx, _)| *idx);
    let best_b = b_chars.get(best_b_char).map_or(b.len(), |(idx, _)| *idx);
    let end_a_char = best_a_char + best_len;
    let end_a = a_chars.get(end_a_char).map_or(a.len(), |(idx, _)| *idx);

    (best_a, best_b, end_a.saturating_sub(best_a))
}

fn compute_line_diff(deletion: &str, addition: &str) -> (Vec<DiffSegment>, Vec<DiffSegment>) {
    let (lcs_a, lcs_b, lcs_len) = longest_common_substring(deletion, addition);

    if lcs_len == 0 {
        return (
            vec![DiffSegment {
                kind: SegmentKind::DeletionHighlight,
                text: deletion.to_string(),
            }],
            vec![DiffSegment {
                kind: SegmentKind::AdditionHighlight,
                text: addition.to_string(),
            }],
        );
    }

    let mut del_segments = Vec::new();
    let mut add_segments = Vec::new();

    if lcs_a > 0 {
        del_segments.push(DiffSegment {
            kind: SegmentKind::DeletionHighlight,
            text: deletion[..lcs_a].to_string(),
        });
    }
    del_segments.push(DiffSegment {
        kind: SegmentKind::Deletion,
        text: deletion[lcs_a..lcs_a + lcs_len].to_string(),
    });
    if lcs_a + lcs_len < deletion.len() {
        del_segments.push(DiffSegment {
            kind: SegmentKind::DeletionHighlight,
            text: deletion[lcs_a + lcs_len..].to_string(),
        });
    }

    if lcs_b > 0 {
        add_segments.push(DiffSegment {
            kind: SegmentKind::AdditionHighlight,
            text: addition[..lcs_b].to_string(),
        });
    }
    add_segments.push(DiffSegment {
        kind: SegmentKind::Addition,
        text: addition[lcs_b..lcs_b + lcs_len].to_string(),
    });
    if lcs_b + lcs_len < addition.len() {
        add_segments.push(DiffSegment {
            kind: SegmentKind::AdditionHighlight,
            text: addition[lcs_b + lcs_len..].to_string(),
        });
    }

    (del_segments, add_segments)
}

#[cfg(test)]
mod tests {
    use super::{
        compute_dirty_signature, compute_intra_line_diffs, compute_line_diff,
        load_commit_file_diff, load_working_tree_diff, longest_common_substring, DiffLineType,
        SegmentKind, WorkingTreeDiffLine, MAX_INTRA_LINE_DIFF_LINE_CHARS,
        MAX_INTRA_LINE_DIFF_PAIRS,
    };
    use crate::services::{
        test_support::{commit_all, configure_test_signature, init_test_repo, write_file},
        GitService,
    };

    fn plain_line(line_type: DiffLineType, content: &str) -> WorkingTreeDiffLine {
        WorkingTreeDiffLine {
            line_type,
            content: content.to_string(),
            old_lineno: None,
            new_lineno: None,
            segments: Vec::new(),
        }
    }

    #[test]
    fn longest_common_substring_returns_zero_on_empty_inputs() {
        assert_eq!(longest_common_substring("", ""), (0, 0, 0));
        assert_eq!(longest_common_substring("abc", ""), (0, 0, 0));
        assert_eq!(longest_common_substring("", "abc"), (0, 0, 0));
    }

    #[test]
    fn longest_common_substring_finds_middle_segment() {
        // "foo BAR baz" and "qux BAR quux" share " BAR " (5 bytes).
        let (a, b, len) = longest_common_substring("foo BAR baz", "qux BAR quux");
        assert_eq!(len, 5);
        assert_eq!(&"foo BAR baz"[a..a + len], " BAR ");
        assert_eq!(&"qux BAR quux"[b..b + len], " BAR ");
    }

    #[test]
    fn compute_line_diff_highlights_whole_lines_when_no_common_substring() {
        let (del, add) = compute_line_diff("abc", "xyz");
        assert_eq!(del.len(), 1);
        assert_eq!(del[0].kind, SegmentKind::DeletionHighlight);
        assert_eq!(del[0].text, "abc");
        assert_eq!(add.len(), 1);
        assert_eq!(add[0].kind, SegmentKind::AdditionHighlight);
        assert_eq!(add[0].text, "xyz");
    }

    #[test]
    fn compute_line_diff_brackets_shared_middle_with_highlights_on_sides() {
        let (del, add) = compute_line_diff("let x = 1;", "let y = 1;");
        // Deletion: highlight("let ") + shared... but "let " is part of shared + ... complicated.
        // Assert: the shared Deletion/Addition segment covers the longest common chunk
        // and at least one side has DeletionHighlight/AdditionHighlight framing the change.
        let shared_del: Vec<_> = del
            .iter()
            .filter(|s| s.kind == SegmentKind::Deletion)
            .collect();
        let shared_add: Vec<_> = add
            .iter()
            .filter(|s| s.kind == SegmentKind::Addition)
            .collect();
        assert_eq!(shared_del.len(), 1, "one shared segment in deletion");
        assert_eq!(shared_add.len(), 1, "one shared segment in addition");
        assert_eq!(shared_del[0].text, shared_add[0].text);
        // A highlight segment exists for the differing identifier character.
        assert!(del
            .iter()
            .any(|s| s.kind == SegmentKind::DeletionHighlight && s.text.contains('x')));
        assert!(add
            .iter()
            .any(|s| s.kind == SegmentKind::AdditionHighlight && s.text.contains('y')));
    }

    #[test]
    fn compute_line_diff_handles_unicode_boundaries() {
        let (del, add) = compute_line_diff("alpha é beta", "alpha ø beta");
        assert!(del.iter().any(|s| s.text.contains('é')));
        assert!(add.iter().any(|s| s.text.contains('ø')));
    }

    #[test]
    fn compute_intra_line_diffs_pairs_consecutive_deletion_addition() {
        let lines = vec![
            plain_line(DiffLineType::Deletion, "foo"),
            plain_line(DiffLineType::Addition, "bar"),
        ];
        let (out, skip) = compute_intra_line_diffs(lines);
        assert!(skip.is_none());
        assert_eq!(out.len(), 2);
        // Paired lines get segments computed.
        assert!(!out[0].segments.is_empty());
        assert!(!out[1].segments.is_empty());
    }

    #[test]
    fn compute_intra_line_diffs_leaves_unpaired_deletion_alone_segments() {
        let lines = vec![
            plain_line(DiffLineType::Deletion, "orphan"),
            plain_line(DiffLineType::Context, "ctx"),
        ];
        let (out, skip) = compute_intra_line_diffs(lines);
        assert!(skip.is_none());
        assert_eq!(out.len(), 2);
        // Unpaired deletion keeps empty segments — no downstream highlight.
        assert!(out[0].segments.is_empty());
        // Context line gets a single Context segment synthesised.
        assert_eq!(out[1].segments.len(), 1);
        assert_eq!(out[1].segments[0].kind, SegmentKind::Context);
    }

    #[test]
    fn compute_intra_line_diffs_does_not_cross_pair_addition_then_deletion() {
        // Order matters: (Addition, Deletion) must NOT be paired — only (Deletion, Addition).
        let lines = vec![
            plain_line(DiffLineType::Addition, "aaa"),
            plain_line(DiffLineType::Deletion, "bbb"),
        ];
        let (out, skip) = compute_intra_line_diffs(lines);
        assert!(skip.is_none());
        // Addition line: skipped in the synth path (it's an addition with empty segments), stays empty.
        // Deletion line: also skipped, stays empty.
        assert_eq!(out.len(), 2);
        assert!(
            out[0].segments.is_empty() || out[0].segments[0].kind != SegmentKind::AdditionHighlight,
            "addition was not cross-paired"
        );
    }

    #[test]
    fn compute_intra_line_diffs_skips_overlong_pairs() {
        let long = "x".repeat(MAX_INTRA_LINE_DIFF_LINE_CHARS + 1);
        let lines = vec![
            plain_line(DiffLineType::Deletion, &long),
            plain_line(DiffLineType::Addition, &long),
        ];
        let (out, skip) = compute_intra_line_diffs(lines);
        assert_eq!(out.len(), 2);
        assert_eq!(skip.unwrap().skipped_pairs, 1);
        assert!(out[0].segments.is_empty());
        assert!(out[1].segments.is_empty());
    }

    #[test]
    fn compute_intra_line_diffs_caps_total_pairs() {
        let mut lines = Vec::new();
        for idx in 0..=MAX_INTRA_LINE_DIFF_PAIRS {
            lines.push(plain_line(DiffLineType::Deletion, &format!("old {idx}")));
            lines.push(plain_line(DiffLineType::Addition, &format!("new {idx}")));
        }
        let (_out, skip) = compute_intra_line_diffs(lines);
        assert_eq!(skip.unwrap().skipped_pairs, 1);
    }

    #[test]
    fn added_final_newline_is_context_line_plus_newline_marker() {
        let (temp_repo, repo) = init_test_repo("diff_add_eof_newline");
        write_file(&temp_repo.path, "tracked.txt", "alpha\nomega");
        commit_all(&repo, "remove final newline");
        write_file(&temp_repo.path, "tracked.txt", "alpha\nomega\n");

        let service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let result = load_working_tree_diff(&service, "tracked.txt", false)
            .expect("working tree diff should load");

        let final_content_lines: Vec<_> = result
            .lines
            .iter()
            .filter(|line| line.content == "omega")
            .collect();
        assert_eq!(final_content_lines.len(), 1);
        assert_eq!(final_content_lines[0].line_type, DiffLineType::Context);
        assert_eq!(final_content_lines[0].old_lineno, Some(2));
        assert_eq!(final_content_lines[0].new_lineno, Some(2));
        assert!(result
            .lines
            .iter()
            .any(|line| line.line_type == DiffLineType::AddEofnl && line.content.is_empty()));
        assert!(!result
            .lines
            .iter()
            .any(|line| line.line_type == DiffLineType::Deletion && line.content == "omega"));
        assert!(!result
            .lines
            .iter()
            .any(|line| line.line_type == DiffLineType::Addition && line.content == "omega"));
    }

    #[test]
    fn unstaged_view_of_fully_staged_file_returns_empty_lines() {
        let (temp_repo, repo) = init_test_repo("diff_unstaged_view_all_staged");
        write_file(&temp_repo.path, "tracked.txt", "line1\nline2\nline3\n");
        commit_all(&repo, "seed");
        write_file(&temp_repo.path, "tracked.txt", "line1\nCHANGED\nline3\n");

        let mut index = repo.index().expect("open index");
        index
            .add_path(std::path::Path::new("tracked.txt"))
            .expect("stage tracked.txt");
        index.write().expect("write index");

        let service = GitService::open(temp_repo.path_str()).expect("open service");
        let result = load_working_tree_diff(&service, "tracked.txt", false).expect("diff loads");

        assert!(
            result.lines.is_empty(),
            "expected no diff lines for a fully-staged file viewed from the unstaged side, got {:?}",
            result
                .lines
                .iter()
                .map(|l| (l.line_type, l.content.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn staged_view_of_committed_file_returns_empty_lines() {
        let (temp_repo, repo) = init_test_repo("diff_staged_view_committed");
        write_file(&temp_repo.path, "tracked.txt", "one\ntwo\nthree\n");
        commit_all(&repo, "seed");
        write_file(&temp_repo.path, "tracked.txt", "one\nTWO\nthree\n");
        commit_all(&repo, "tweak line 2");

        let service = GitService::open(temp_repo.path_str()).expect("open service");
        let result = load_working_tree_diff(&service, "tracked.txt", true).expect("diff loads");

        assert!(
            result.lines.is_empty(),
            "expected empty staged diff once change is committed, got {:?}",
            result
                .lines
                .iter()
                .map(|l| (l.line_type, l.content.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn dirty_signature_stable_when_content_unchanged() {
        let (temp_repo, repo) = init_test_repo("diff_sig_stable");
        write_file(&temp_repo.path, "tracked.txt", "hi\n");
        commit_all(&repo, "seed");
        write_file(&temp_repo.path, "tracked.txt", "hi\nmore\n");

        let service = GitService::open(temp_repo.path_str()).expect("open service");
        let sig_a = compute_dirty_signature(&service.repo, "tracked.txt", false);
        let sig_b = compute_dirty_signature(&service.repo, "tracked.txt", false);
        assert_eq!(sig_a, sig_b);
    }

    #[test]
    fn dirty_signature_changes_when_workdir_file_edited() {
        let (temp_repo, repo) = init_test_repo("diff_sig_workdir_edit");
        write_file(&temp_repo.path, "tracked.txt", "one\n");
        commit_all(&repo, "seed");
        write_file(&temp_repo.path, "tracked.txt", "one\ntwo\n");

        let service = GitService::open(temp_repo.path_str()).expect("open service");
        let before = compute_dirty_signature(&service.repo, "tracked.txt", false);

        std::thread::sleep(std::time::Duration::from_millis(10));
        write_file(&temp_repo.path, "tracked.txt", "one\ntwo\nthree\n");

        let after = compute_dirty_signature(&service.repo, "tracked.txt", false);
        assert_ne!(before, after);
    }

    #[test]
    fn dirty_signature_changes_when_index_updated() {
        let (temp_repo, repo) = init_test_repo("diff_sig_index_change");
        write_file(&temp_repo.path, "tracked.txt", "one\n");
        commit_all(&repo, "seed");
        write_file(&temp_repo.path, "tracked.txt", "one\ntwo\n");

        let before = {
            let service = GitService::open(temp_repo.path_str()).expect("open service");
            compute_dirty_signature(&service.repo, "tracked.txt", true)
        };

        let mut index = repo.index().expect("open index");
        index
            .add_path(std::path::Path::new("tracked.txt"))
            .expect("stage");
        index.write().expect("write index");

        let after = {
            let service = GitService::open(temp_repo.path_str()).expect("open service");
            compute_dirty_signature(&service.repo, "tracked.txt", true)
        };
        assert_ne!(before, after);
    }

    #[test]
    fn unstaged_view_of_untracked_file_still_shows_additions() {
        let (temp_repo, _repo) = init_test_repo("diff_untracked_visible");
        write_file(&temp_repo.path, "new.txt", "hello\nworld\n");

        let service = GitService::open(temp_repo.path_str()).expect("open service");
        let result = load_working_tree_diff(&service, "new.txt", false).expect("diff loads");

        assert!(
            result
                .lines
                .iter()
                .any(|l| l.line_type == DiffLineType::Addition && l.content == "hello"),
            "expected untracked file contents to appear as additions, got {:?}",
            result
                .lines
                .iter()
                .map(|l| (l.line_type, l.content.clone()))
                .collect::<Vec<_>>()
        );
        assert!(result
            .lines
            .iter()
            .any(|l| l.line_type == DiffLineType::Addition && l.content == "world"));
    }

    #[test]
    fn commit_file_diff_includes_old_and_new_file_contents() {
        let (temp_repo, repo) = init_test_repo("diff_commit_file_contents");
        write_file(
            &temp_repo.path,
            "index.php",
            "<?php\nfunction oldName(): string {\n    return 'old';\n}\n",
        );
        commit_all(&repo, "add php file");
        write_file(
            &temp_repo.path,
            "index.php",
            "<?php\nfunction newName(): string {\n    return 'new';\n}\n",
        );
        let commit = commit_all(&repo, "change php file");

        let service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let result = load_commit_file_diff(&service, &commit.to_string(), "index.php")
            .expect("commit diff should load");

        assert!(result
            .old_file_content
            .as_deref()
            .is_some_and(|content| content.contains("oldName")));
        assert!(result
            .new_file_content
            .as_deref()
            .is_some_and(|content| content.contains("newName")));
    }

    #[test]
    fn stash_commit_file_diff_loads_untracked_parent_file() {
        let (temp_repo, repo) = init_test_repo("stash_untracked_file_diff");
        configure_test_signature(&repo);
        write_file(&temp_repo.path, "untracked.txt", "new\ncontent\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service.create_stash(None).expect("failed to create stash");
        let stash = service
            .load_repo(super::super::COMMIT_LOAD_LIMIT)
            .stashes
            .into_iter()
            .next()
            .expect("stash should exist");

        let result = load_commit_file_diff(&service, &stash.hash, "untracked.txt")
            .expect("stash untracked file diff should load");

        assert!(result.old_file_content.is_none());
        assert_eq!(result.new_file_content.as_deref(), Some("new\ncontent\n"));
        assert!(result
            .lines
            .iter()
            .any(|line| line.line_type == DiffLineType::Addition && line.content == "new"));
    }
}
