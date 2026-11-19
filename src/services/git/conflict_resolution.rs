use std::path::Path;

use super::helpers::{command_dir, process_output_detail};
use super::process::spawn_git_command;
use git2::IndexConflict;
use sha2::{Digest, Sha256};

use super::{helpers::wrap_git2_error, working_tree, GitService};
use crate::services::git_error::GitError;

#[derive(Debug, Clone)]
pub struct ConflictResolutionResult {
    pub file_path: String,
    pub ours_label: String,
    pub theirs_label: String,
    pub blocks: Vec<ConflictBlock>,
    pub has_trailing_newline: bool,
    pub source_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub enum ConflictBlock {
    Context(Vec<String>),
    Conflict(ConflictHunk),
}

#[derive(Debug, Clone)]
pub struct ConflictHunk {
    pub index: usize,
    pub ours_label: String,
    pub theirs_label: String,
    pub ours_lines: Vec<String>,
    pub theirs_lines: Vec<String>,
    pub original_lines: Vec<String>,
    pub ours_unterminated: bool,
    pub theirs_unterminated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifyDeleteConflict {
    pub file_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifyDeleteConflictChoice {
    KeepModified,
    DeleteFile,
    KeepBase,
}

pub(super) fn load_conflict_resolution(
    service: &GitService,
    file_path: &str,
) -> Result<ConflictResolutionResult, GitError> {
    let workdir = service
        .repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working directory".to_string()))?;
    let full_path = workdir.join(file_path);
    ensure_regular_file(&full_path)?;
    let content = std::fs::read_to_string(&full_path).map_err(|e| {
        GitError::Other(format!(
            "failed to read conflicted file '{}': {e}",
            file_path
        ))
    })?;

    if content.contains('\0') {
        return Err(GitError::Other(format!("'{file_path}' contains binary data. Resolve it in the worktree, then mark it resolved.")));
    }
    let attr = service
        .repo
        .get_attr(
            Path::new(file_path),
            "conflict-marker-size",
            Default::default(),
        )
        .map_err(|e| wrap_git2_error("read conflict marker size", e))?;
    let marker_size = match git2::AttrValue::from_string(attr) {
        git2::AttrValue::String(value) => {
            value.parse::<usize>().ok().filter(|&n| n > 0).unwrap_or(7)
        }
        _ => 7,
    };
    let mut result = parse_conflict_content(file_path, &content, marker_size)?;
    if let Some(ConflictBlock::Conflict(last)) = result.blocks.last_mut() {
        // Git inserts a newline before the marker even when the original
        // blob had none. At EOF only the index can distinguish those cases.
        if let Some(conflict) = index_conflict_for_path(service, file_path)? {
            let unterminated = |entry: Option<git2::IndexEntry>| {
                entry
                    .and_then(|e| service.repo.find_blob(e.id).ok())
                    .is_some_and(|blob| {
                        !blob.content().is_empty() && !blob.content().ends_with(b"\n")
                    })
            };
            last.ours_unterminated = unterminated(conflict.our);
            last.theirs_unterminated = unterminated(conflict.their);
        }
    }
    Ok(result)
}

pub(super) fn load_modify_delete_conflict(
    service: &GitService,
    file_path: &str,
) -> Result<Option<ModifyDeleteConflict>, GitError> {
    let Some(conflict) = index_conflict_for_path(service, file_path)? else {
        return Ok(None);
    };
    if !is_modify_delete_conflict(&conflict) {
        return Ok(None);
    }
    Ok(Some(ModifyDeleteConflict {
        file_path: file_path.to_string(),
    }))
}

pub(super) fn resolve_modify_delete_conflict(
    service: &mut GitService,
    file_path: &str,
    choice: ModifyDeleteConflictChoice,
) -> Result<(), GitError> {
    let conflict = index_conflict_for_path(service, file_path)?
        .ok_or_else(|| GitError::Other(format!("'{file_path}' is not conflicted")))?;
    if !is_modify_delete_conflict(&conflict) {
        return Err(GitError::Other(format!(
            "'{file_path}' is not a modify/delete conflict"
        )));
    }

    match choice {
        ModifyDeleteConflictChoice::DeleteFile => remove_workdir_file(service, file_path)?,
        _ => {
            let stage = match choice {
                ModifyDeleteConflictChoice::KeepBase => "--stage=1",
                _ if conflict.our.is_some() => "--stage=2",
                _ => "--stage=3",
            };
            // Let Git restore the entry type, executable bit and checkout filters.
            let output = spawn_git_command(
                &command_dir(&service.repo)?,
                &["checkout-index", "--force", stage, "--", file_path],
                "restore conflicted file",
            )?;
            if !output.status.success() {
                return Err(GitError::Other(process_output_detail(&output)));
            }
        }
    }

    working_tree::mark_conflict_resolved(service, file_path)
}

pub(super) fn save_conflict_resolution(
    service: &mut GitService,
    file_path: &str,
    content: &str,
    source_hash: [u8; 32],
) -> Result<(), GitError> {
    let workdir = service
        .repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working directory".to_string()))?;
    let full_path = workdir.join(file_path);
    if index_conflict_for_path(service, file_path)?.is_none() {
        return Err(GitError::Other(format!(
            "'{file_path}' is no longer conflicted"
        )));
    }
    ensure_regular_file(&full_path)?;
    let current = std::fs::read(&full_path)
        .map_err(|e| GitError::Other(format!("read '{file_path}': {e}")))?;
    if <[u8; 32]>::from(Sha256::digest(&current)) != source_hash {
        return Err(GitError::Other(format!("'{file_path}' changed since the resolver opened. Reopen it to review the current contents.")));
    }

    std::fs::write(&full_path, content).map_err(|e| {
        GitError::Other(format!(
            "failed to write resolved file '{}': {e}",
            file_path
        ))
    })?;
    working_tree::mark_conflict_resolved(service, file_path)
}

fn index_conflict_for_path(
    service: &GitService,
    file_path: &str,
) -> Result<Option<IndexConflict>, GitError> {
    let mut index = service
        .repo
        .index()
        .map_err(|e| wrap_git2_error("open repository index", e))?;
    index
        .read(true)
        .map_err(|e| wrap_git2_error("read repository index", e))?;
    let conflicts = index
        .conflicts()
        .map_err(|e| wrap_git2_error("read index conflicts", e))?;
    for conflict in conflicts {
        let conflict = conflict.map_err(|e| wrap_git2_error("read index conflict", e))?;
        if conflict_path_matches(&conflict, file_path) {
            return Ok(Some(conflict));
        }
    }
    Ok(None)
}

fn conflict_path_matches(conflict: &IndexConflict, file_path: &str) -> bool {
    [&conflict.our, &conflict.their, &conflict.ancestor]
        .into_iter()
        .flatten()
        .any(|entry| entry.path == file_path.as_bytes())
}

fn is_modify_delete_conflict(conflict: &IndexConflict) -> bool {
    conflict.ancestor.is_some()
        && ((conflict.our.is_some() && conflict.their.is_none())
            || (conflict.our.is_none() && conflict.their.is_some()))
}

fn ensure_regular_file(path: &Path) -> Result<(), GitError> {
    let metadata = path
        .symlink_metadata()
        .map_err(|e| GitError::Other(format!("inspect '{}': {e}", path.display())))?;
    if metadata.is_file() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "'{}' cannot be resolved as text. Resolve it in the worktree, then mark it resolved.",
            path.display()
        )))
    }
}

fn remove_workdir_file(service: &GitService, file_path: &str) -> Result<(), GitError> {
    let workdir = service
        .repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working directory".to_string()))?;
    let full_path = workdir.join(Path::new(file_path));
    match std::fs::remove_file(&full_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(GitError::Other(format!(
            "failed to delete '{}': {err}",
            file_path
        ))),
    }
}

fn parse_conflict_content(
    file_path: &str,
    content: &str,
    marker_size: usize,
) -> Result<ConflictResolutionResult, GitError> {
    // Keep CR bytes in the stored lines so choosing a hunk preserves mixed
    // line endings. Marker recognition and display can ignore the CR.
    let lines: Vec<_> = content.split_terminator('\n').map(str::to_owned).collect();
    let mut blocks = Vec::new();
    let mut hunk_index = 0;
    let mut ours_label = None;
    let mut theirs_label = None;
    let mut context_start = 0;
    let malformed = || {
        GitError::Other(format!("Incomplete conflict markers in '{file_path}'. Resolve the file in the worktree, then mark it resolved."))
    };
    let mut pos = 0;
    while pos < lines.len() {
        let Some(ours) = marker_label(&lines[pos], b'<', marker_size) else {
            pos += 1;
            continue;
        };
        if context_start < pos {
            blocks.push(ConflictBlock::Context(lines[context_start..pos].to_vec()));
        }
        let start = pos;
        pos += 1;
        while pos < lines.len()
            && marker_label(&lines[pos], b'|', marker_size).is_none()
            && !is_separator(&lines[pos], marker_size)
        {
            pos += 1;
        }
        let ours_lines = lines[start + 1..pos].to_vec();
        while pos < lines.len() && !is_separator(&lines[pos], marker_size) {
            pos += 1;
        }
        if pos == lines.len() {
            return Err(malformed());
        }
        let separator = pos;
        pos += 1;
        while pos < lines.len() && marker_label(&lines[pos], b'>', marker_size).is_none() {
            pos += 1;
        }
        let theirs = lines
            .get(pos)
            .and_then(|line| marker_label(line, b'>', marker_size))
            .ok_or_else(malformed)?;
        let ours = if ours.is_empty() { "A" } else { ours };
        let theirs = if theirs.is_empty() { "B" } else { theirs };
        ours_label.get_or_insert_with(|| ours.to_owned());
        theirs_label.get_or_insert_with(|| theirs.to_owned());
        blocks.push(ConflictBlock::Conflict(ConflictHunk {
            index: hunk_index,
            ours_label: ours.to_owned(),
            theirs_label: theirs.to_owned(),
            ours_lines,
            theirs_lines: lines[separator + 1..pos].to_vec(),
            original_lines: lines[start..=pos].to_vec(),
            ours_unterminated: false,
            theirs_unterminated: false,
        }));
        hunk_index += 1;
        pos += 1;
        context_start = pos;
    }
    if context_start < lines.len() {
        blocks.push(ConflictBlock::Context(lines[context_start..].to_vec()));
    }
    Ok(ConflictResolutionResult {
        file_path: file_path.to_owned(),
        ours_label: ours_label.unwrap_or_else(|| "A".into()),
        theirs_label: theirs_label.unwrap_or_else(|| "B".into()),
        blocks,
        has_trailing_newline: content.ends_with('\n'),
        source_hash: Sha256::digest(content).into(),
    })
}

fn marker_label(line: &str, byte: u8, size: usize) -> Option<&str> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let prefix = line.as_bytes().get(..size)?;
    if !prefix.iter().all(|&b| b == byte) {
        return None;
    }
    let tail = &line[size..];
    if tail.is_empty() || tail.starts_with(char::is_whitespace) {
        Some(tail.trim())
    } else {
        None
    }
}

fn is_separator(line: &str, size: usize) -> bool {
    marker_label(line, b'=', size) == Some("")
}

#[cfg(test)]
mod tests {
    use super::{parse_conflict_content, ConflictBlock};

    #[test]
    fn parses_custom_width_diff3_markers_and_retains_mixed_line_endings() {
        let input = "header\n<<<< HEAD\r\n======= literal\r\n|||| base\r\nold\r\n====\r\nnew\r\n>>>> topic\r\ntail";
        let parsed = parse_conflict_content("file.txt", input, 4).unwrap();
        assert!(!parsed.has_trailing_newline);
        let ConflictBlock::Conflict(hunk) = &parsed.blocks[1] else {
            panic!("expected conflict");
        };
        assert_eq!(hunk.ours_lines, ["======= literal\r"]);
        assert_eq!(hunk.theirs_lines, ["new\r"]);
        assert_eq!(hunk.ours_label, "HEAD");
        assert_eq!(hunk.theirs_label, "topic");
        assert!(parse_conflict_content("file.txt", "<<<<<<< HEAD\ncontent", 7).is_err());
    }

    #[test]
    fn parse_conflict_content_splits_context_and_hunks() {
        let parsed = parse_conflict_content(
            "tracked.txt",
            "top\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> refs/heads/feature\nbottom\n",
            7,
        )
        .unwrap();

        assert_eq!(parsed.file_path, "tracked.txt");
        assert_eq!(parsed.ours_label, "HEAD");
        assert_eq!(parsed.theirs_label, "refs/heads/feature");
        assert!(parsed.has_trailing_newline);
        assert_eq!(parsed.blocks.len(), 3);

        let ConflictBlock::Conflict(hunk) = &parsed.blocks[1] else {
            panic!("middle block should be a conflict");
        };
        assert_eq!(hunk.index, 0);
        assert_eq!(hunk.ours_lines, vec!["ours"]);
        assert_eq!(hunk.theirs_lines, vec!["theirs"]);
        assert_eq!(hunk.original_lines.len(), 5);
    }
}
