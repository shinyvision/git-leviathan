use serde::{Deserialize, Serialize};

/// One capability the plugin manifest may request. The grant system supports
/// parametric forms (path scopes, env globs, network
/// domains, process binaries, services, regions, git op subsets) so the
/// host can grant access at the right granularity. The string form is
/// canonical; serde uses it for both the `plugin.toml` capability list
/// and the persisted grant store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Capability {
    /// `fs:read[:scope]`. `scope` defaults to `plugin` for the bare
    /// `fs:read` form.
    FsRead { scope: FsScope },
    /// `fs:read:scope:<dir>` — read paths under an explicit user-chosen
    /// directory. Canonicalised at check time.
    FsReadDir { dir: String },
    /// `fs:write[:scope]`.
    FsWrite { scope: FsScope },
    /// `fs:write:scope:<dir>`.
    FsWriteDir { dir: String },
    /// `process:spawn` — any binary.
    ProcessSpawn,
    /// `process:spawn:<binary_basename>` — a specific binary by basename.
    ProcessSpawnBinary { binary: String },
    /// `net:fetch` — any host.
    NetFetch,
    /// `net:fetch:<domain>` — a specific domain.
    NetFetchDomain { domain: String },
    /// `clipboard` — read and write.
    Clipboard,
    /// `clipboard:read`.
    ClipboardRead,
    /// `clipboard:write`.
    ClipboardWrite,
    /// `notify`.
    Notify,
    /// `env` — read every environment variable.
    Env,
    /// `env:<glob>` — read variables whose name matches the glob (e.g.
    /// `env:GIT_*`).
    EnvVar { glob: String },
    /// `credentials` — read host-stored credentials (persistence and settings).
    Credentials,
    /// `repo:read` — observe the active repository projection.
    RepoRead,
    /// `git:read:<op>` (status / log / diff / show / blame).
    GitRead { op: GitReadOp },
    /// `git:write:<op>` (checkout / branch / tag / commit / stash /
    /// reset / fetch / push / merge / rebase).
    GitWrite { op: GitWriteOp },
    /// `ui:region:<region>`.
    UiRegion { region: String },
    /// `services:provide:<name>@<version>` (plugin services).
    ServicesProvide { name: String, version: u32 },
    /// `services:consume:<name>@<version>` (plugin services).
    ServicesConsume { name: String, version: u32 },
    /// `async:spawn` (async runtime) — allows `leviathan.async.spawn`.
    AsyncSpawn,
    /// `timer:create` (async runtime) — allows `leviathan.timer.after` and
    /// `leviathan.timer.every`.
    TimerCreate,
    /// `fs:watch[:scope]` (async runtime) — `leviathan.fs.watch` against a
    /// known fs scope (plugin/state/config/workdir/any).
    FsWatch { scope: FsScope },
    /// `fs:watch:scope:<dir>` (async runtime) — explicit user-chosen
    /// directory, canonicalised at check time.
    FsWatchDir { dir: String },
    /// `ui:overlay` (extension points) — register modal overlays.
    UiOverlay,
    /// `ui:context_menu` (extension points) — contribute context-menu items.
    UiContextMenu,
    /// `ui:graph_decoration` (extension points) — attach commit-row decorations.
    UiGraphDecoration,
    /// `ui:diff_decoration` (extension points) — attach diff line / hunk decorations.
    UiDiffDecoration,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FsScope {
    Plugin,
    State,
    Config,
    Workdir,
    Any,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitReadOp {
    Status,
    Log,
    Diff,
    Show,
    Blame,
}

impl GitReadOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Log => "log",
            Self::Diff => "diff",
            Self::Show => "show",
            Self::Blame => "blame",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "status" => Some(Self::Status),
            "log" => Some(Self::Log),
            "diff" => Some(Self::Diff),
            "show" => Some(Self::Show),
            "blame" => Some(Self::Blame),
            _ => None,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitWriteOp {
    Checkout,
    Branch,
    Tag,
    Commit,
    Stash,
    Reset,
    Fetch,
    Push,
    Merge,
    Rebase,
}

impl GitWriteOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Checkout => "checkout",
            Self::Branch => "branch",
            Self::Tag => "tag",
            Self::Commit => "commit",
            Self::Stash => "stash",
            Self::Reset => "reset",
            Self::Fetch => "fetch",
            Self::Push => "push",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "checkout" => Some(Self::Checkout),
            "branch" => Some(Self::Branch),
            "tag" => Some(Self::Tag),
            "commit" => Some(Self::Commit),
            "stash" => Some(Self::Stash),
            "reset" => Some(Self::Reset),
            "fetch" => Some(Self::Fetch),
            "push" => Some(Self::Push),
            "merge" => Some(Self::Merge),
            "rebase" => Some(Self::Rebase),
            _ => None,
        }
    }
}

impl TryFrom<String> for Capability {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.as_slice() {
            ["fs", "read"] => Ok(Capability::FsRead {
                scope: FsScope::Plugin,
            }),
            ["fs", "read", "scope", dir] => Ok(Capability::FsReadDir {
                dir: (*dir).to_string(),
            }),
            ["fs", "read", scope] => Ok(Capability::FsRead {
                scope: parse_scope(scope)?,
            }),
            ["fs", "write", "scope", dir] => Ok(Capability::FsWriteDir {
                dir: (*dir).to_string(),
            }),
            ["fs", "write", scope] => Ok(Capability::FsWrite {
                scope: parse_scope(scope)?,
            }),
            ["process", "spawn"] => Ok(Capability::ProcessSpawn),
            ["process", "spawn", binary] => Ok(Capability::ProcessSpawnBinary {
                binary: (*binary).to_string(),
            }),
            ["net", "fetch"] => Ok(Capability::NetFetch),
            ["net", "fetch", domain] => Ok(Capability::NetFetchDomain {
                domain: (*domain).to_string(),
            }),
            ["clipboard"] => Ok(Capability::Clipboard),
            ["clipboard", "read"] => Ok(Capability::ClipboardRead),
            ["clipboard", "write"] => Ok(Capability::ClipboardWrite),
            ["notify"] => Ok(Capability::Notify),
            ["env"] => Ok(Capability::Env),
            ["env", glob] => Ok(Capability::EnvVar {
                glob: (*glob).to_string(),
            }),
            ["credentials"] => Ok(Capability::Credentials),
            ["repo", "read"] => Ok(Capability::RepoRead),
            ["git", "read", op] => GitReadOp::parse(op)
                .map(|op| Capability::GitRead { op })
                .ok_or_else(|| format!("unknown git:read op: {op}")),
            ["git", "write", op] => GitWriteOp::parse(op)
                .map(|op| Capability::GitWrite { op })
                .ok_or_else(|| format!("unknown git:write op: {op}")),
            ["ui", "region", region] => Ok(Capability::UiRegion {
                region: (*region).to_string(),
            }),
            ["services", "provide", spec] => {
                let (name, ver) = parse_service_spec(spec)?;
                Ok(Capability::ServicesProvide { name, version: ver })
            }
            ["services", "consume", spec] => {
                let (name, ver) = parse_service_spec(spec)?;
                Ok(Capability::ServicesConsume { name, version: ver })
            }
            ["async", "spawn"] => Ok(Capability::AsyncSpawn),
            ["timer", "create"] => Ok(Capability::TimerCreate),
            ["fs", "watch"] => Ok(Capability::FsWatch {
                scope: FsScope::Plugin,
            }),
            ["fs", "watch", "scope", dir] => Ok(Capability::FsWatchDir {
                dir: (*dir).to_string(),
            }),
            ["fs", "watch", scope] => Ok(Capability::FsWatch {
                scope: parse_scope(scope)?,
            }),
            ["ui", "overlay"] => Ok(Capability::UiOverlay),
            ["ui", "context_menu"] => Ok(Capability::UiContextMenu),
            ["ui", "graph_decoration"] => Ok(Capability::UiGraphDecoration),
            ["ui", "diff_decoration"] => Ok(Capability::UiDiffDecoration),
            _ => Err(format!("unknown capability: {s}")),
        }
    }
}

fn parse_scope(s: &str) -> Result<FsScope, String> {
    match s {
        "plugin" => Ok(FsScope::Plugin),
        "state" => Ok(FsScope::State),
        "config" => Ok(FsScope::Config),
        "workdir" => Ok(FsScope::Workdir),
        "any" => Ok(FsScope::Any),
        _ => Err(format!("unknown fs scope: {s}")),
    }
}

fn parse_service_spec(s: &str) -> Result<(String, u32), String> {
    let (name, ver) = s
        .split_once('@')
        .ok_or_else(|| format!("service spec must be name@version: {s}"))?;
    let version: u32 = ver
        .parse()
        .map_err(|e| format!("invalid service version `{ver}`: {e}"))?;
    Ok((name.to_string(), version))
}

impl From<Capability> for String {
    fn from(c: Capability) -> String {
        match c {
            Capability::FsRead { scope } => format!("fs:read:{}", scope_str(scope)),
            Capability::FsReadDir { dir } => format!("fs:read:scope:{dir}"),
            Capability::FsWrite { scope } => format!("fs:write:{}", scope_str(scope)),
            Capability::FsWriteDir { dir } => format!("fs:write:scope:{dir}"),
            Capability::ProcessSpawn => "process:spawn".into(),
            Capability::ProcessSpawnBinary { binary } => format!("process:spawn:{binary}"),
            Capability::NetFetch => "net:fetch".into(),
            Capability::NetFetchDomain { domain } => format!("net:fetch:{domain}"),
            Capability::Clipboard => "clipboard".into(),
            Capability::ClipboardRead => "clipboard:read".into(),
            Capability::ClipboardWrite => "clipboard:write".into(),
            Capability::Notify => "notify".into(),
            Capability::Env => "env".into(),
            Capability::EnvVar { glob } => format!("env:{glob}"),
            Capability::Credentials => "credentials".into(),
            Capability::RepoRead => "repo:read".into(),
            Capability::GitRead { op } => format!("git:read:{}", op.as_str()),
            Capability::GitWrite { op } => format!("git:write:{}", op.as_str()),
            Capability::UiRegion { region } => format!("ui:region:{region}"),
            Capability::ServicesProvide { name, version } => {
                format!("services:provide:{name}@{version}")
            }
            Capability::ServicesConsume { name, version } => {
                format!("services:consume:{name}@{version}")
            }
            Capability::AsyncSpawn => "async:spawn".into(),
            Capability::TimerCreate => "timer:create".into(),
            Capability::FsWatch { scope } => format!("fs:watch:{}", scope_str(scope)),
            Capability::FsWatchDir { dir } => format!("fs:watch:scope:{dir}"),
            Capability::UiOverlay => "ui:overlay".into(),
            Capability::UiContextMenu => "ui:context_menu".into(),
            Capability::UiGraphDecoration => "ui:graph_decoration".into(),
            Capability::UiDiffDecoration => "ui:diff_decoration".into(),
        }
    }
}

fn scope_str(s: FsScope) -> &'static str {
    match s {
        FsScope::Plugin => "plugin",
        FsScope::State => "state",
        FsScope::Config => "config",
        FsScope::Workdir => "workdir",
        FsScope::Any => "any",
    }
}

/// Whether a manifest-declared capability string is recognised by the
/// host's known descriptor surface. Capability checks emit
/// `capability.unknown` diagnostics for everything else.
pub fn is_known_capability(s: &str) -> bool {
    Capability::try_from(s.to_string()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_all_known_forms() {
        let cases = [
            "fs:read:plugin",
            "fs:read:any",
            "fs:read:scope:/tmp/safe",
            "fs:write:state",
            "fs:write:scope:/var/data",
            "process:spawn",
            "process:spawn:git",
            "net:fetch",
            "net:fetch:github.com",
            "clipboard",
            "clipboard:read",
            "clipboard:write",
            "notify",
            "env",
            "env:GIT_*",
            "credentials",
            "repo:read",
            "git:read:status",
            "git:read:log",
            "git:read:diff",
            "git:read:show",
            "git:read:blame",
            "git:write:checkout",
            "git:write:branch",
            "git:write:tag",
            "git:write:commit",
            "git:write:stash",
            "git:write:reset",
            "git:write:fetch",
            "git:write:push",
            "git:write:merge",
            "git:write:rebase",
            "ui:region:repository.sidebar",
            "services:provide:diff_viewer@1",
            "services:consume:repository@2",
            "async:spawn",
            "timer:create",
            "fs:watch:plugin",
            "fs:watch:any",
            "fs:watch:scope:/tmp/safe",
            "ui:overlay",
            "ui:context_menu",
            "ui:graph_decoration",
            "ui:diff_decoration",
        ];
        for s in cases {
            let cap = Capability::try_from(s.to_string()).expect(s);
            let back = String::from(cap);
            assert_eq!(back, s, "round-trip failed for {s}");
        }
    }

    #[test]
    fn bare_fs_read_alias_decodes_to_plugin_scope() {
        assert_eq!(
            Capability::try_from("fs:read".to_string()).unwrap(),
            Capability::FsRead {
                scope: FsScope::Plugin
            }
        );
    }

    #[test]
    fn unknown_capability_rejected() {
        assert!(Capability::try_from("blast:radius".to_string()).is_err());
        assert!(Capability::try_from("git:read:nope".to_string()).is_err());
        assert!(Capability::try_from("fs:write:nope".to_string()).is_err());
    }

    #[test]
    fn is_known_returns_true_for_known_and_false_otherwise() {
        assert!(is_known_capability("env"));
        assert!(is_known_capability("git:write:checkout"));
        assert!(is_known_capability("fs:read:scope:/x"));
        assert!(!is_known_capability("destroy:everything"));
    }

    #[test]
    fn async_runtime_capabilities_round_trip() {
        assert!(is_known_capability("async:spawn"));
        assert!(is_known_capability("timer:create"));
        assert!(is_known_capability("fs:watch:plugin"));
        assert!(is_known_capability("fs:watch:scope:/tmp/x"));
        assert_eq!(
            Capability::try_from("fs:watch".to_string()).unwrap(),
            Capability::FsWatch {
                scope: FsScope::Plugin
            }
        );
    }
}
