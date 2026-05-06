use std::path::Path;

use git2::{IndexConflict, IndexEntry};

use super::{helpers::wrap_git2_error, working_tree, GitService};
use crate::services::git_error::GitError;

#[derive(Debug, Clone)]
pub struct ConflictResolutionResult {
    pub file_path: String,
    pub ours_label: String,
    pub theirs_label: String,
    pub blocks: Vec<ConflictBlock>,
    pub has_trailing_newline: bool,
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
    let content = std::fs::read_to_string(&full_path).map_err(|e| {
        GitError::Other(format!(
            "failed to read conflicted file '{}': {e}",
            file_path
        ))
    })?;

    Ok(parse_conflict_content(file_path, &content))
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
        ModifyDeleteConflictChoice::KeepModified => {
            let entry = modified_entry(&conflict).ok_or_else(|| {
                GitError::Other(format!(
                    "modify/delete conflict for '{file_path}' has no modified version"
                ))
            })?;
            write_entry_blob_to_workdir(service, file_path, entry)?;
        }
        ModifyDeleteConflictChoice::DeleteFile => {
            remove_workdir_file(service, file_path)?;
        }
        ModifyDeleteConflictChoice::KeepBase => {
            let entry = conflict.ancestor.as_ref().ok_or_else(|| {
                GitError::Other(format!(
                    "modify/delete conflict for '{file_path}' has no base version"
                ))
            })?;
            write_entry_blob_to_workdir(service, file_path, entry)?;
        }
    }

    working_tree::mark_conflict_resolved(service, file_path)
}

pub(super) fn save_conflict_resolution(
    service: &mut GitService,
    file_path: &str,
    content: &str,
) -> Result<(), GitError> {
    let workdir = service
        .repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working directory".to_string()))?;
    let full_path = workdir.join(file_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            GitError::Other(format!(
                "failed to create parent directory for '{}': {e}",
                file_path
            ))
        })?;
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
    conflict
        .our
        .as_ref()
        .or(conflict.their.as_ref())
        .or(conflict.ancestor.as_ref())
        .is_some_and(|entry| String::from_utf8_lossy(&entry.path) == file_path)
}

fn is_modify_delete_conflict(conflict: &IndexConflict) -> bool {
    conflict.ancestor.is_some()
        && ((conflict.our.is_some() && conflict.their.is_none())
            || (conflict.our.is_none() && conflict.their.is_some()))
}

fn modified_entry(conflict: &IndexConflict) -> Option<&IndexEntry> {
    match (&conflict.our, &conflict.their) {
        (Some(entry), None) | (None, Some(entry)) => Some(entry),
        _ => None,
    }
}

fn write_entry_blob_to_workdir(
    service: &GitService,
    file_path: &str,
    entry: &IndexEntry,
) -> Result<(), GitError> {
    let blob = service
        .repo
        .find_blob(entry.id)
        .map_err(|e| wrap_git2_error(&format!("read '{file_path}' blob"), e))?;
    let content = blob.content().to_vec();
    let workdir = service
        .repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working directory".to_string()))?;
    let full_path = workdir.join(Path::new(file_path));
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            GitError::Other(format!(
                "failed to create parent directory for '{}': {e}",
                file_path
            ))
        })?;
    }
    std::fs::write(&full_path, content)
        .map_err(|e| GitError::Other(format!("failed to write '{}': {e}", file_path)))?;
    apply_file_mode(&full_path, entry.mode)?;
    Ok(())
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

#[cfg(unix)]
fn apply_file_mode(path: &Path, mode: u32) -> Result<(), GitError> {
    use std::os::unix::fs::PermissionsExt;

    let executable = mode & 0o111 != 0;
    let permissions = std::fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 });
    std::fs::set_permissions(path, permissions)
        .map_err(|e| GitError::Other(format!("failed to set file permissions: {e}")))
}

#[cfg(not(unix))]
fn apply_file_mode(_path: &Path, _mode: u32) -> Result<(), GitError> {
    Ok(())
}

fn parse_conflict_content(file_path: &str, content: &str) -> ConflictResolutionResult {
    let has_trailing_newline = content.ends_with('\n');
    let lines = content
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect::<Vec<_>>();

    let mut blocks = Vec::new();
    let mut context = Vec::new();
    let mut index = 0;
    let mut hunk_index = 0;
    let mut first_ours_label = None;
    let mut first_theirs_label = None;

    while index < lines.len() {
        if !lines[index].starts_with("<<<<<<<") {
            context.push(lines[index].clone());
            index += 1;
            continue;
        }

        if !context.is_empty() {
            blocks.push(ConflictBlock::Context(std::mem::take(&mut context)));
        }

        let start_marker = lines[index].clone();
        let ours_label = marker_label(&start_marker, "<<<<<<<")
            .unwrap_or("A")
            .to_string();
        let mut original_lines = vec![start_marker];
        index += 1;

        let mut ours_lines = Vec::new();
        while index < lines.len()
            && !lines[index].starts_with("|||||||")
            && !lines[index].starts_with("=======")
            && !lines[index].starts_with(">>>>>>>")
        {
            original_lines.push(lines[index].clone());
            ours_lines.push(lines[index].clone());
            index += 1;
        }

        if index < lines.len() && lines[index].starts_with("|||||||") {
            original_lines.push(lines[index].clone());
            index += 1;
            while index < lines.len()
                && !lines[index].starts_with("=======")
                && !lines[index].starts_with(">>>>>>>")
            {
                original_lines.push(lines[index].clone());
                index += 1;
            }
        }

        if index < lines.len() && lines[index].starts_with("=======") {
            original_lines.push(lines[index].clone());
            index += 1;
        }

        let mut theirs_lines = Vec::new();
        while index < lines.len() && !lines[index].starts_with(">>>>>>>") {
            original_lines.push(lines[index].clone());
            theirs_lines.push(lines[index].clone());
            index += 1;
        }

        let theirs_label = if index < lines.len() && lines[index].starts_with(">>>>>>>") {
            let marker = lines[index].clone();
            original_lines.push(marker.clone());
            index += 1;
            marker_label(&marker, ">>>>>>>").unwrap_or("B").to_string()
        } else {
            "B".to_string()
        };

        first_ours_label.get_or_insert_with(|| ours_label.clone());
        first_theirs_label.get_or_insert_with(|| theirs_label.clone());
        blocks.push(ConflictBlock::Conflict(ConflictHunk {
            index: hunk_index,
            ours_label,
            theirs_label,
            ours_lines,
            theirs_lines,
            original_lines,
        }));
        hunk_index += 1;
    }

    if !context.is_empty() {
        blocks.push(ConflictBlock::Context(context));
    }

    ConflictResolutionResult {
        file_path: file_path.to_string(),
        ours_label: first_ours_label.unwrap_or_else(|| "A".to_string()),
        theirs_label: first_theirs_label.unwrap_or_else(|| "B".to_string()),
        blocks,
        has_trailing_newline,
    }
}

fn marker_label<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let label = line.strip_prefix(prefix)?.trim();
    (!label.is_empty()).then_some(label)
}

#[cfg(test)]
mod tests {
    use super::{parse_conflict_content, ConflictBlock};

    #[test]
    fn parse_conflict_content_splits_context_and_hunks() {
        let parsed = parse_conflict_content(
            "tracked.txt",
            "top\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> refs/heads/feature\nbottom\n",
        );

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
