use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Capability {
    FsRead { scope: FsScope },
    FsWrite { scope: FsScope },
    ProcessSpawn,
    NetFetch,
    Clipboard,
    Notify,
    Env,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FsScope { Plugin, State, Config, Workdir, Any }

impl TryFrom<String> for Capability {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        match s.split(':').collect::<Vec<_>>().as_slice() {
            ["fs", "read"] => Ok(Capability::FsRead { scope: FsScope::Plugin }),
            ["fs", "read", scope] => Ok(Capability::FsRead { scope: parse_scope(scope)? }),
            ["fs", "write", scope] => Ok(Capability::FsWrite { scope: parse_scope(scope)? }),
            ["process", "spawn"] => Ok(Capability::ProcessSpawn),
            ["net", "fetch"] => Ok(Capability::NetFetch),
            ["clipboard"] => Ok(Capability::Clipboard),
            ["notify"] => Ok(Capability::Notify),
            ["env"] => Ok(Capability::Env),
            _ => Err(format!("unknown capability: {s}")),
        }
    }
}

fn parse_scope(s: &str) -> Result<FsScope, String> {
    match s {
        "plugin" => Ok(FsScope::Plugin),
        "state"  => Ok(FsScope::State),
        "config" => Ok(FsScope::Config),
        "workdir" => Ok(FsScope::Workdir),
        "any" => Ok(FsScope::Any),
        _ => Err(format!("unknown fs scope: {s}")),
    }
}

impl From<Capability> for String {
    fn from(c: Capability) -> String {
        match c {
            Capability::FsRead { scope } => format!("fs:read:{}", scope_str(scope)),
            Capability::FsWrite { scope } => format!("fs:write:{}", scope_str(scope)),
            Capability::ProcessSpawn => "process:spawn".into(),
            Capability::NetFetch => "net:fetch".into(),
            Capability::Clipboard => "clipboard".into(),
            Capability::Notify => "notify".into(),
            Capability::Env => "env".into(),
        }
    }
}

fn scope_str(s: FsScope) -> &'static str {
    match s {
        FsScope::Plugin => "plugin", FsScope::State => "state",
        FsScope::Config => "config", FsScope::Workdir => "workdir", FsScope::Any => "any",
    }
}
