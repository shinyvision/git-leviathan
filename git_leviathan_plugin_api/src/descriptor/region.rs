/// One pane inside a content region. Each pane carries its own list
/// of fixed sections plus, optionally, dynamic-section prefixes
/// (e.g. `"line:"`, `"hunk:"`, `"commit_hash:"`) that allow any
/// non-empty tail.
#[derive(Debug, Clone, Copy)]
pub struct PaneDescriptor {
    pub name: &'static str,
    pub sections: &'static [&'static str],
    /// Prefixes that allow `<prefix><tail>` section ids where `tail`
    /// is any non-empty string. Used for per-row / per-line / per-hunk
    /// extension points.
    pub dynamic_section_prefixes: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub enum RegionKind {
    Chrome {
        sections: &'static [&'static str],
        /// Prefixes that allow dynamic section ids (Phase 17).
        /// Empty for chrome regions that don't need them.
        dynamic_section_prefixes: &'static [&'static str],
    },
    Content {
        panes: &'static [PaneDescriptor],
    },
}

#[derive(Debug, Clone)]
pub struct RegionDescriptor {
    pub name: &'static str,
    pub kind: RegionKind,
}

impl RegionDescriptor {
    /// Validate `(pane?, section?)` against the descriptor. Sections
    /// match either a fixed entry or a dynamic prefix declared on the
    /// pane (or chrome region). Errors mention only the fixed
    /// sections to keep messages short — dynamic prefixes are listed
    /// when the section starts with one but the tail is empty.
    pub fn validate_address(
        &self,
        pane: Option<&str>,
        section: Option<&str>,
    ) -> Result<(), String> {
        match (&self.kind, pane, section) {
            (
                RegionKind::Chrome {
                    sections,
                    dynamic_section_prefixes,
                },
                None,
                Some(s),
            ) => {
                if sections.contains(&s) {
                    return Ok(());
                }
                if let Some(prefix) = matched_dynamic_prefix(dynamic_section_prefixes, s) {
                    if s.len() > prefix.len() {
                        return Ok(());
                    }
                    return Err(format!(
                        "region '{}': section '{}' has empty tail after prefix '{}'",
                        self.name, s, prefix
                    ));
                }
                Err(format!(
                    "region '{}': unknown section '{}' (have: {})",
                    self.name,
                    s,
                    fixed_and_prefixes(sections, dynamic_section_prefixes)
                ))
            }
            (RegionKind::Chrome { .. }, Some(_), _) => Err(format!(
                "region '{}' is chrome — `pane` not allowed",
                self.name
            )),
            (RegionKind::Content { panes }, Some(p), Some(s)) => {
                let entry = panes.iter().find(|pane| pane.name == p);
                match entry {
                    Some(pane_desc) => {
                        if pane_desc.sections.contains(&s) {
                            return Ok(());
                        }
                        if let Some(prefix) =
                            matched_dynamic_prefix(pane_desc.dynamic_section_prefixes, s)
                        {
                            if s.len() > prefix.len() {
                                return Ok(());
                            }
                            return Err(format!(
                                "region '{}': pane '{}' section '{}' has empty tail after prefix '{}'",
                                self.name, p, s, prefix
                            ));
                        }
                        Err(format!(
                            "region '{}': pane '{}' has no section '{}' (have: {})",
                            self.name,
                            p,
                            s,
                            fixed_and_prefixes(
                                pane_desc.sections,
                                pane_desc.dynamic_section_prefixes
                            )
                        ))
                    }
                    None => Err(format!(
                        "region '{}': unknown pane '{}' (have: {})",
                        self.name,
                        p,
                        panes
                            .iter()
                            .map(|pane| pane.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                }
            }
            (RegionKind::Content { panes }, None, _) => Err(format!(
                "region '{}': unknown pane (none given; have: {})",
                self.name,
                panes
                    .iter()
                    .map(|pane| pane.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            _ => Err(format!(
                "region '{}': missing or invalid address",
                self.name
            )),
        }
    }
}

fn matched_dynamic_prefix<'a>(
    prefixes: &'a [&'static str],
    section: &str,
) -> Option<&'a &'static str> {
    prefixes.iter().find(|prefix| section.starts_with(*prefix))
}

fn fixed_and_prefixes(fixed: &[&'static str], prefixes: &[&'static str]) -> String {
    let mut parts: Vec<String> = fixed.iter().map(|s| (*s).to_string()).collect();
    for prefix in prefixes {
        parts.push(format!("{prefix}<id>"));
    }
    parts.join(", ")
}

pub struct DescriptorTable<T: 'static>(&'static [T]);

impl<T> DescriptorTable<T> {
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }
}

impl DescriptorTable<RegionDescriptor> {
    pub fn get(&self, name: &str) -> Option<&RegionDescriptor> {
        self.0.iter().find(|d| d.name == name)
    }
    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|d| d.name).collect()
    }
}

const NO_PREFIXES: &[&str] = &[];

pub static REGIONS: DescriptorTable<RegionDescriptor> = DescriptorTable(&[
    RegionDescriptor {
        name: "main_bar",
        kind: RegionKind::Chrome {
            sections: &["left", "center", "right"],
            dynamic_section_prefixes: NO_PREFIXES,
        },
    },
    RegionDescriptor {
        name: "tab_bar",
        kind: RegionKind::Chrome {
            sections: &["left", "center", "right"],
            dynamic_section_prefixes: NO_PREFIXES,
        },
    },
    // Phase 17: dedicated chrome region for the bottom status bar.
    RegionDescriptor {
        name: "status_bar",
        kind: RegionKind::Chrome {
            sections: &["left", "center", "right"],
            dynamic_section_prefixes: NO_PREFIXES,
        },
    },
    RegionDescriptor {
        name: "repository",
        kind: RegionKind::Content {
            panes: &[
                PaneDescriptor {
                    name: "sidebar",
                    sections: &["top", "bottom"],
                    // Phase 17: plugins can attach to plugin-defined
                    // sidebar sections via `section:<id>`.
                    dynamic_section_prefixes: &["section:"],
                },
                PaneDescriptor {
                    name: "main",
                    sections: &["top", "bottom"],
                    dynamic_section_prefixes: NO_PREFIXES,
                },
            ],
        },
    },
    // Phase 17: graph extension points (commit row decorations and
    // graph context menu live here).
    RegionDescriptor {
        name: "repository.graph",
        kind: RegionKind::Content {
            panes: &[PaneDescriptor {
                name: "graph",
                sections: &["top", "decorations", "context_menu"],
                // `row:<commit_hash>` for per-row decorations.
                dynamic_section_prefixes: &["row:"],
            }],
        },
    },
    // Phase 17: details panel — commit header + files panes.
    RegionDescriptor {
        name: "repository.details",
        kind: RegionKind::Content {
            panes: &[PaneDescriptor {
                name: "details",
                sections: &["commit_header", "files"],
                dynamic_section_prefixes: NO_PREFIXES,
            }],
        },
    },
    // Phase 17: diff panel — toolbar, hunk badges, line gutters,
    // context menu.
    RegionDescriptor {
        name: "repository.diff",
        kind: RegionKind::Content {
            panes: &[PaneDescriptor {
                name: "diff",
                sections: &["toolbar", "context_menu"],
                // `line:<file>:<line>` and `hunk:<id>`.
                dynamic_section_prefixes: &["line:", "hunk:"],
            }],
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

    #[test]
    fn validate_status_bar_sections() {
        let d = REGIONS.get("status_bar").unwrap();
        assert!(d.validate_address(None, Some("left")).is_ok());
        assert!(d.validate_address(None, Some("center")).is_ok());
        assert!(d.validate_address(None, Some("right")).is_ok());
        assert!(d.validate_address(None, Some("middle")).is_err());
    }

    #[test]
    fn validate_dynamic_sidebar_section() {
        let d = REGIONS.get("repository").unwrap();
        assert!(d
            .validate_address(Some("sidebar"), Some("section:foo"))
            .is_ok());
        // Empty tail rejected with a precise message.
        let err = d
            .validate_address(Some("sidebar"), Some("section:"))
            .unwrap_err();
        assert!(err.contains("empty tail"), "got: {err}");
    }

    #[test]
    fn validate_dynamic_diff_addresses() {
        let d = REGIONS.get("repository.diff").unwrap();
        assert!(d
            .validate_address(Some("diff"), Some("line:src/foo.rs:42"))
            .is_ok());
        assert!(d.validate_address(Some("diff"), Some("hunk:7")).is_ok());
        assert!(d.validate_address(Some("diff"), Some("toolbar")).is_ok());
        assert!(d
            .validate_address(Some("diff"), Some("context_menu"))
            .is_ok());
        // Unknown prefix.
        assert!(d
            .validate_address(Some("diff"), Some("zzz:1"))
            .is_err());
    }

    #[test]
    fn validate_dynamic_graph_row() {
        let d = REGIONS.get("repository.graph").unwrap();
        assert!(d
            .validate_address(Some("graph"), Some("row:abc123"))
            .is_ok());
        assert!(d
            .validate_address(Some("graph"), Some("decorations"))
            .is_ok());
        assert!(d
            .validate_address(Some("graph"), Some("context_menu"))
            .is_ok());
        // Static-only fixed section that doesn't exist.
        assert!(d.validate_address(Some("graph"), Some("nope")).is_err());
    }

    #[test]
    fn validate_details_sections() {
        let d = REGIONS.get("repository.details").unwrap();
        assert!(d
            .validate_address(Some("details"), Some("commit_header"))
            .is_ok());
        assert!(d.validate_address(Some("details"), Some("files")).is_ok());
        assert!(d
            .validate_address(Some("details"), Some("nope"))
            .is_err());
    }
}
