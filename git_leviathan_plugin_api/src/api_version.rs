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
}
