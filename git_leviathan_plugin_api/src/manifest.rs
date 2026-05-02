use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use semver::{Version, VersionReq};
use crate::api_version::ApiVersion;
use crate::capability::Capability;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub api_version: ApiVersion,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub provides_services: Vec<ServiceDecl>,
    #[serde(default)]
    pub consumes_services: Vec<ServiceDecl>,
    #[serde(default)]
    pub dependencies: HashMap<String, VersionReq>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDecl { pub name: String, pub version: u32 }

impl<'de> Deserialize<'de> for ServiceDecl {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let (n, v) = s.split_once('@').ok_or_else(|| serde::de::Error::custom(
            "service decl must be 'name@version'"))?;
        Ok(ServiceDecl {
            name: n.to_string(),
            version: v.parse().map_err(serde::de::Error::custom)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Capability, FsScope};

    #[test]
    fn parse_minimal_manifest() {
        let toml = r#"
            id = "demo"
            name = "Demo"
            version = "0.1.0"
            api_version = "1.0"
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        assert_eq!(m.id, "demo");
        assert!(m.capabilities.is_empty());
    }

    #[test]
    fn parse_full_manifest() {
        let toml = r#"
            id = "git-tools"
            name = "Git Tools"
            version = "1.2.0"
            api_version = "1.0"
            capabilities = ["fs:read", "fs:write:state", "process:spawn", "net:fetch"]
            provides_services = ["diff_viewer@1"]
            consumes_services = ["repository@1"]

            [dependencies]
            repository_info = ">=0.5.0"
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        assert!(m.capabilities.contains(&Capability::FsWrite { scope: FsScope::State }));
        assert_eq!(m.provides_services.len(), 1);
    }
}
