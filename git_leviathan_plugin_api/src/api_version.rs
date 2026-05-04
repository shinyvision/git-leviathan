use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
}

impl ApiVersion {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
    pub fn is_compatible_with(self, host: ApiVersion) -> bool {
        self.major == host.major && self.minor <= host.minor
    }
}

impl<'de> Deserialize<'de> for ApiVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let (maj, min) = s.split_once('.').ok_or_else(|| {
            serde::de::Error::custom(format!("api_version must be 'MAJOR.MINOR', got {s:?}"))
        })?;
        Ok(Self::new(
            maj.parse().map_err(serde::de::Error::custom)?,
            min.parse().map_err(serde::de::Error::custom)?,
        ))
    }
}

pub const HOST_API_VERSION: ApiVersion = ApiVersion::new(1, 0);
pub const SUPPORTED_PLUGIN_API_VERSIONS: &[ApiVersion] = &[HOST_API_VERSION];
pub const WIDGET_SCHEMA_VERSION: u32 = 1;
pub const EXTENSION_POINT_SCHEMA_VERSION: u32 = 1;

pub fn supported_plugin_api_versions_label() -> String {
    SUPPORTED_PLUGIN_API_VERSIONS
        .iter()
        .map(|version| format!("{}.{}", version.major, version.minor))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn plugin_api_compatibility_error(requested: ApiVersion) -> Option<String> {
    if requested.is_compatible_with(HOST_API_VERSION) {
        return None;
    }
    Some(format!(
        "plugin api_version {}.{} is not supported by host {}.{}; supported plugin api versions: {}; widget schema {}; extension-point schema {}",
        requested.major,
        requested.minor,
        HOST_API_VERSION.major,
        HOST_API_VERSION.minor,
        supported_plugin_api_versions_label(),
        WIDGET_SCHEMA_VERSION,
        EXTENSION_POINT_SCHEMA_VERSION
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_compat_check() {
        let v1_host = ApiVersion::new(1, 0);
        assert!(ApiVersion::new(1, 0).is_compatible_with(v1_host));
        assert!(!ApiVersion::new(2, 0).is_compatible_with(v1_host));
        assert!(!ApiVersion::new(1, 1).is_compatible_with(v1_host));
        assert!(!ApiVersion::new(0, 9).is_compatible_with(v1_host));
    }

    #[test]
    fn compatibility_error_lists_stable_contract() {
        let err = plugin_api_compatibility_error(ApiVersion::new(1, 1)).unwrap();
        assert!(err.contains("supported plugin api versions: 1.0"));
        assert!(err.contains("widget schema 1"));
        assert!(err.contains("extension-point schema 1"));
        assert!(plugin_api_compatibility_error(ApiVersion::new(1, 0)).is_none());
    }
}
