#[derive(Debug, Clone)]
pub enum RegionKind {
    Chrome { sections: &'static [&'static str] },
    Content { panes: &'static [(&'static str, &'static [&'static str])] },
}

#[derive(Debug, Clone)]
pub struct RegionDescriptor {
    pub name: &'static str,
    pub kind: RegionKind,
}

impl RegionDescriptor {
    pub fn validate_address(&self, pane: Option<&str>, section: Option<&str>) -> Result<(), String> {
        match (&self.kind, pane, section) {
            (RegionKind::Chrome { sections }, None, Some(s)) if sections.contains(&s) => Ok(()),
            (RegionKind::Chrome { sections }, None, Some(s)) => Err(format!(
                "region '{}': unknown section '{}' (have: {})",
                self.name, s, sections.join(", "))),
            (RegionKind::Chrome { .. }, Some(_), _) => Err(format!(
                "region '{}' is chrome — `pane` not allowed", self.name)),
            (RegionKind::Content { panes }, Some(p), Some(s)) => {
                let entry = panes.iter().find(|(name, _)| *name == p);
                match entry {
                    Some((_, secs)) if secs.contains(&s) => Ok(()),
                    Some((_, secs)) => Err(format!(
                        "region '{}': pane '{}' has no section '{}' (have: {})",
                        self.name, p, s, secs.join(", "))),
                    None => Err(format!(
                        "region '{}': unknown pane '{}' (have: {})",
                        self.name, p,
                        panes.iter().map(|(n,_)| *n).collect::<Vec<_>>().join(", "))),
                }
            }
            _ => Err(format!("region '{}': missing or invalid address", self.name)),
        }
    }
}

pub struct DescriptorTable<T: 'static>(&'static [T]);

impl<T> DescriptorTable<T> {
    pub fn iter(&self) -> impl Iterator<Item = &T> { self.0.iter() }
}

impl DescriptorTable<RegionDescriptor> {
    pub fn get(&self, name: &str) -> Option<&RegionDescriptor> {
        self.0.iter().find(|d| d.name == name)
    }
    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|d| d.name).collect()
    }
}

pub static REGIONS: DescriptorTable<RegionDescriptor> = DescriptorTable(&[
    RegionDescriptor {
        name: "main_bar",
        kind: RegionKind::Chrome { sections: &["left", "center", "right"] },
    },
    RegionDescriptor {
        name: "tab_bar",
        kind: RegionKind::Chrome { sections: &["left", "center", "right"] },
    },
    RegionDescriptor {
        name: "repository",
        kind: RegionKind::Content {
            panes: &[
                ("sidebar", &["top", "bottom"]),
                ("main", &["top", "bottom"]),
            ],
        },
    },
]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_known_region() {
        let d = REGIONS.get("main_bar").expect("main_bar present");
        assert_eq!(d.name, "main_bar");
        assert!(matches!(d.kind, RegionKind::Chrome { .. }));
    }

    #[test]
    fn lookup_unknown_region() {
        assert!(REGIONS.get("nope").is_none());
    }

    #[test]
    fn validate_chrome_section() {
        let d = REGIONS.get("main_bar").unwrap();
        assert!(d.validate_address(None, Some("left")).is_ok());
        assert!(d.validate_address(None, Some("nope")).is_err());
        assert!(d.validate_address(Some("sidebar"), Some("top")).is_err());
    }

    #[test]
    fn validate_content_pane() {
        let d = REGIONS.get("repository").unwrap();
        assert!(d.validate_address(Some("sidebar"), Some("top")).is_ok());
        assert!(d.validate_address(Some("sidebar"), Some("nope")).is_err());
        assert!(d.validate_address(None, Some("top")).is_err());
    }
}
