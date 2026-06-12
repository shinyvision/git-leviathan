use crate::services::git_error::GitError;

pub fn can_confirm_branch_delete(current_branch: &str, branch_name: &str, is_remote: bool) -> bool {
    is_remote || current_branch != branch_name
}

pub fn validate_branch_name<'a>(
    branch_name: &str,
    existing_branches: impl IntoIterator<Item = &'a str>,
) -> Result<(), GitError> {
    let name = branch_name.trim();

    if name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    if name.ends_with('/') {
        return Err(GitError::InvalidBranchName(name.to_string()));
    }

    if name.contains("..")
        || name.contains('~')
        || name.contains('^')
        || name.contains(':')
        || name.contains('\0')
        || name.contains("//")
    {
        return Err(GitError::InvalidBranchName(name.to_string()));
    }

    if existing_branches.into_iter().any(|b| b == name) {
        return Err(GitError::BranchAlreadyExists(name.to_string()));
    }

    Ok(())
}

pub fn humanize_git_error(error: &GitError) -> String {
    match error {
        GitError::CantDeleteHead(_) => {
            "Cannot delete currently checked out branch. Switch branches first.".to_string()
        }
        _ => error.to_string(),
    }
}
