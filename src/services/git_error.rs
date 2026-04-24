#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    CantDeleteHead(String),
    CantRenameHead(String),
    BranchNotFound(String),
    RemoteBranchNotFound(String),
    BranchAlreadyExists(String),
    InvalidBranchName(String),
    EmptyBranchName,
    StashFailed(String),
    CheckoutFailed {
        branch: String,
        reason: String,
    },
    ResetFailed {
        branch: String,
        reason: String,
    },
    FastForwardFailed {
        branch: String,
        reason: String,
    },
    RemoteDeleteFailed {
        remote: String,
        branch: String,
        reason: String,
    },
    ReferenceBroken(String),
    CommitNotFound(String),
    /// A referenced commit/tree/blob object is present but corrupt or missing.
    /// Raised when git2 reports `ErrorClass::Odb` or `ErrorCode::Invalid`.
    CorruptObject {
        op: String,
        reason: String,
    },
    /// Network-level failure (no connectivity, DNS, TLS). Raised on
    /// `ErrorClass::Net`.
    NetworkFailure {
        op: String,
        reason: String,
    },
    /// Credentials rejected or missing. Raised on `ErrorClass::Http`/`Ssh`
    /// auth codes.
    AuthFailed {
        op: String,
        reason: String,
    },
    /// Tree walk/read failed (cannot access tree entry). Raised on
    /// `ErrorClass::Tree`.
    TreeAccessFailed {
        op: String,
        reason: String,
    },
    /// Unstructured error. Kept behind a named variant so the unknown-cause
    /// call sites can be counted and whittled down over time. Do NOT introduce
    /// new `Other(String)` call sites when a structured variant fits.
    Other(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CantDeleteHead(name) => {
                write!(
                    f,
                    "cannot delete '{}' because it is currently checked out",
                    name
                )
            }
            Self::CantRenameHead(name) => {
                write!(
                    f,
                    "cannot rename '{}' because it is currently checked out",
                    name
                )
            }
            Self::BranchNotFound(name) => write!(f, "branch '{}' not found", name),
            Self::RemoteBranchNotFound(name) => {
                write!(f, "no remote branch '{}' found", name)
            }
            Self::BranchAlreadyExists(name) => {
                write!(f, "a branch named '{}' already exists", name)
            }
            Self::InvalidBranchName(name) => {
                write!(f, "branch name '{}' is invalid", name)
            }
            Self::EmptyBranchName => write!(f, "branch name cannot be empty"),
            Self::StashFailed(reason) => write!(f, "stash failed: {}", reason),
            Self::CheckoutFailed { branch, reason } => {
                write!(f, "checkout of '{}' failed: {}", branch, reason)
            }
            Self::ResetFailed { branch, reason } => {
                write!(f, "reset of '{}' failed: {}", branch, reason)
            }
            Self::FastForwardFailed { branch, reason } => {
                write!(f, "fast-forward of '{}' failed: {}", branch, reason)
            }
            Self::RemoteDeleteFailed {
                remote,
                branch,
                reason,
            } => {
                write!(
                    f,
                    "failed to delete remote branch '{}/{}': {}",
                    remote, branch, reason
                )
            }
            Self::ReferenceBroken(name) => write!(f, "reference '{}' is broken", name),
            Self::CommitNotFound(hash) => write!(f, "commit '{}' not found", hash),
            Self::CorruptObject { op, reason } => {
                write!(f, "{op} failed: object store corrupt ({reason})")
            }
            Self::NetworkFailure { op, reason } => {
                write!(f, "{op} failed: network unreachable ({reason})")
            }
            Self::AuthFailed { op, reason } => {
                write!(f, "{op} failed: authentication rejected ({reason})")
            }
            Self::TreeAccessFailed { op, reason } => {
                write!(f, "{op} failed: cannot read tree ({reason})")
            }
            Self::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl GitError {
    pub fn is_cant_delete_head(&self) -> bool {
        matches!(self, Self::CantDeleteHead(_))
    }
}
