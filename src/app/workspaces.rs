//! Workspaces: optional, named groups of tabs (modelled on Vivaldi's).
//!
//! Every tab belongs to exactly one workspace and the tab bar only shows the
//! tabs of the active one. The default workspace (`WorkspaceId::DEFAULT`)
//! always exists, is always called "Workspace", is always listed first and
//! cannot be renamed, reordered or deleted. User-created workspaces carry a
//! name, an icon and a position in a user-controlled sort order.
//!
//! This module is the pure list model: no tabs, no persistence. Tab
//! membership and the settings DB live in `TabManager`.

use crate::assets;
use crate::core::WorkspaceId;
use crate::services::PersistedWorkspace;

pub const DEFAULT_WORKSPACE_NAME: &str = "Workspace";
pub const MAX_NAME_CHARS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    /// Key into `assets::WORKSPACE_ICONS`.
    pub icon: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    Reserved,
    Duplicate,
}

impl NameError {
    pub fn message(self) -> String {
        match self {
            NameError::Empty => "Give the workspace a name.".to_string(),
            NameError::TooLong => format!("Keep the name under {MAX_NAME_CHARS} characters."),
            NameError::Reserved => {
                format!("“{DEFAULT_WORKSPACE_NAME}” is the name of the default workspace.")
            }
            NameError::Duplicate => "A workspace with this name already exists.".to_string(),
        }
    }
}

/// User-created workspaces in sort order. The default workspace is implicit.
#[derive(Debug, Clone, Default)]
pub struct Workspaces {
    custom: Vec<Workspace>,
}

impl Workspaces {
    pub fn from_persisted(rows: Vec<PersistedWorkspace>) -> Self {
        let mut custom: Vec<Workspace> = Vec::with_capacity(rows.len());
        for row in rows {
            let id = WorkspaceId(row.id);
            if id.is_default() || custom.iter().any(|w| w.id == id) {
                continue;
            }
            let icon = if assets::is_workspace_icon(&row.icon) {
                row.icon
            } else {
                assets::DEFAULT_WORKSPACE_ICON.to_string()
            };
            custom.push(Workspace {
                id,
                name: row.name,
                icon,
            });
        }
        Self { custom }
    }

    pub fn custom(&self) -> &[Workspace] {
        &self.custom
    }

    pub fn get(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.custom.iter().find(|w| w.id == id)
    }

    pub fn contains(&self, id: WorkspaceId) -> bool {
        id.is_default() || self.get(id).is_some()
    }

    pub fn name(&self, id: WorkspaceId) -> &str {
        self.get(id)
            .map(|w| w.name.as_str())
            .unwrap_or(DEFAULT_WORKSPACE_NAME)
    }

    pub fn icon(&self, id: WorkspaceId) -> &str {
        self.get(id)
            .map(|w| w.icon.as_str())
            .unwrap_or(assets::DEFAULT_WORKSPACE_ICON)
    }

    /// Every workspace in display order: the default first, then the
    /// user-created ones in their sort order.
    pub fn ordered_ids(&self) -> Vec<WorkspaceId> {
        std::iter::once(WorkspaceId::DEFAULT)
            .chain(self.custom.iter().map(|w| w.id))
            .collect()
    }

    /// Append a workspace at the end of the sort order.
    pub fn insert(&mut self, workspace: Workspace) {
        if workspace.id.is_default() || self.contains(workspace.id) {
            return;
        }
        self.custom.push(workspace);
    }

    pub fn update(&mut self, id: WorkspaceId, name: String, icon: String) -> bool {
        match self.custom.iter_mut().find(|w| w.id == id) {
            Some(w) => {
                w.name = name;
                w.icon = icon;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: WorkspaceId) -> Option<Workspace> {
        let idx = self.custom.iter().position(|w| w.id == id)?;
        Some(self.custom.remove(idx))
    }

    /// Apply a new sort order. Unknown ids are ignored; workspaces missing
    /// from `order` keep their relative order after the listed ones. Returns
    /// whether anything moved.
    pub fn reorder(&mut self, order: &[WorkspaceId]) -> bool {
        let before: Vec<WorkspaceId> = self.custom.iter().map(|w| w.id).collect();
        let mut remaining = std::mem::take(&mut self.custom);
        let mut reordered = Vec::with_capacity(remaining.len());
        for id in order {
            if let Some(idx) = remaining.iter().position(|w| w.id == *id) {
                reordered.push(remaining.remove(idx));
            }
        }
        reordered.extend(remaining);
        self.custom = reordered;
        self.custom.iter().map(|w| w.id).ne(before)
    }

    /// Id for a workspace created while the settings DB is unavailable.
    pub fn next_local_id(&self) -> WorkspaceId {
        WorkspaceId(self.custom.iter().map(|w| w.id.raw()).max().unwrap_or(0) + 1)
    }

    /// The workspace `step` places away from `from` in display order,
    /// wrapping around at either end.
    pub fn cycle(&self, from: WorkspaceId, step: isize) -> WorkspaceId {
        let ids = self.ordered_ids();
        let len = ids.len() as isize;
        let current = ids.iter().position(|id| *id == from).unwrap_or(0) as isize;
        ids[(current + step).rem_euclid(len) as usize]
    }

    /// Trim and validate a workspace name. `editing` is the workspace being
    /// renamed, whose current name does not count as a duplicate.
    pub fn validate_name(
        &self,
        raw: &str,
        editing: Option<WorkspaceId>,
    ) -> Result<String, NameError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(NameError::TooLong);
        }
        let folded = name.to_lowercase();
        if folded == DEFAULT_WORKSPACE_NAME.to_lowercase() {
            return Err(NameError::Reserved);
        }
        if self
            .custom
            .iter()
            .any(|w| Some(w.id) != editing && w.name.to_lowercase() == folded)
        {
            return Err(NameError::Duplicate);
        }
        Ok(name.to_string())
    }

    /// First picker icon no workspace uses yet, so each new workspace starts
    /// out visually distinct. Falls back to the first non-default icon.
    pub fn suggested_icon(&self) -> &'static str {
        let pickable = assets::WORKSPACE_ICONS
            .iter()
            .map(|(key, _)| *key)
            .filter(|key| *key != assets::DEFAULT_WORKSPACE_ICON);
        pickable
            .clone()
            .find(|key| !self.custom.iter().any(|w| w.icon == *key))
            .or_else(|| pickable.clone().next())
            .unwrap_or(assets::DEFAULT_WORKSPACE_ICON)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: u64, name: &str) -> Workspace {
        Workspace {
            id: WorkspaceId(id),
            name: name.to_string(),
            icon: "rocket".to_string(),
        }
    }

    fn list(names: &[(u64, &str)]) -> Workspaces {
        let mut w = Workspaces::default();
        for (id, name) in names {
            w.insert(ws(*id, name));
        }
        w
    }

    fn ids(w: &Workspaces) -> Vec<u64> {
        w.custom().iter().map(|w| w.id.raw()).collect()
    }

    #[test]
    fn default_workspace_is_implicit_and_first() {
        let w = list(&[(3, "Facilo"), (7, "Smits")]);
        assert!(w.contains(WorkspaceId::DEFAULT));
        assert_eq!(w.name(WorkspaceId::DEFAULT), DEFAULT_WORKSPACE_NAME);
        assert_eq!(w.icon(WorkspaceId::DEFAULT), assets::DEFAULT_WORKSPACE_ICON);
        assert_eq!(
            w.ordered_ids(),
            vec![WorkspaceId(0), WorkspaceId(3), WorkspaceId(7)]
        );
    }

    #[test]
    fn from_persisted_drops_bad_rows_and_unknown_icons() {
        let w = Workspaces::from_persisted(vec![
            PersistedWorkspace {
                id: 0,
                name: "Sneaky default".into(),
                icon: "rocket".into(),
            },
            PersistedWorkspace {
                id: 2,
                name: "A".into(),
                icon: "no-such-icon".into(),
            },
            PersistedWorkspace {
                id: 2,
                name: "Duplicate id".into(),
                icon: "code".into(),
            },
        ]);
        assert_eq!(ids(&w), vec![2]);
        assert_eq!(w.icon(WorkspaceId(2)), assets::DEFAULT_WORKSPACE_ICON);
    }

    #[test]
    fn insert_appends_and_ignores_default_or_duplicates() {
        let mut w = list(&[(1, "A")]);
        w.insert(ws(0, "Nope"));
        w.insert(ws(1, "Again"));
        w.insert(ws(2, "B"));
        assert_eq!(ids(&w), vec![1, 2]);
        assert_eq!(w.name(WorkspaceId(1)), "A");
    }

    #[test]
    fn update_and_remove() {
        let mut w = list(&[(1, "A"), (2, "B")]);
        assert!(w.update(WorkspaceId(2), "Bee".into(), "bug".into()));
        assert!(!w.update(WorkspaceId(9), "X".into(), "bug".into()));
        assert_eq!(w.name(WorkspaceId(2)), "Bee");
        assert_eq!(w.icon(WorkspaceId(2)), "bug");
        assert_eq!(w.remove(WorkspaceId(1)).map(|w| w.name), Some("A".into()));
        assert!(w.remove(WorkspaceId(1)).is_none());
        assert!(!w.contains(WorkspaceId(1)));
    }

    #[test]
    fn reorder_applies_order_and_keeps_missing_after() {
        let mut w = list(&[(1, "A"), (2, "B"), (3, "C")]);
        assert!(w.reorder(&[WorkspaceId(3), WorkspaceId(99), WorkspaceId(1)]));
        assert_eq!(ids(&w), vec![3, 1, 2]);
        assert!(!w.reorder(&[WorkspaceId(3), WorkspaceId(1), WorkspaceId(2)]));
    }

    #[test]
    fn cycle_wraps_through_default() {
        let w = list(&[(1, "A"), (2, "B")]);
        assert_eq!(w.cycle(WorkspaceId(0), 1), WorkspaceId(1));
        assert_eq!(w.cycle(WorkspaceId(2), 1), WorkspaceId(0));
        assert_eq!(w.cycle(WorkspaceId(0), -1), WorkspaceId(2));
        assert_eq!(
            Workspaces::default().cycle(WorkspaceId(0), 1),
            WorkspaceId(0)
        );
    }

    #[test]
    fn validate_name_rules() {
        let w = list(&[(1, "Facilo")]);
        assert_eq!(w.validate_name("  Smits  ", None), Ok("Smits".to_string()));
        assert_eq!(w.validate_name("   ", None), Err(NameError::Empty));
        assert_eq!(w.validate_name("workspace", None), Err(NameError::Reserved));
        assert_eq!(w.validate_name("FACILO", None), Err(NameError::Duplicate));
        // Renaming a workspace to (a re-cased form of) its own name is fine.
        assert_eq!(
            w.validate_name("facilo", Some(WorkspaceId(1))),
            Ok("facilo".to_string())
        );
        let long: String = "x".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(w.validate_name(&long, None), Err(NameError::TooLong));
        let max: String = "é".repeat(MAX_NAME_CHARS);
        assert!(w.validate_name(&max, None).is_ok());
    }

    #[test]
    fn suggested_icon_skips_used_icons() {
        let mut w = Workspaces::default();
        let first = w.suggested_icon();
        assert_ne!(first, assets::DEFAULT_WORKSPACE_ICON);
        w.insert(Workspace {
            id: WorkspaceId(1),
            name: "A".into(),
            icon: first.to_string(),
        });
        assert_ne!(w.suggested_icon(), first);
    }

    #[test]
    fn next_local_id_is_past_existing() {
        assert_eq!(Workspaces::default().next_local_id(), WorkspaceId(1));
        assert_eq!(list(&[(4, "A"), (2, "B")]).next_local_id(), WorkspaceId(5));
    }
}
