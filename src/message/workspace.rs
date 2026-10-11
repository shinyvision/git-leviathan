//! Workspace messages: the tab-bar workspace switcher and its panel, the tab
//! context menu, and workspace keyboard commands. Handled by `App` in
//! `app::workspace_ops`.

use iced::Point;

use crate::core::{TabId, WorkspaceId};

#[derive(Debug, Clone)]
pub enum WorkspaceMessage {
    /// Switcher button: open the panel, or close it when open.
    TogglePanel,
    /// Click outside the open panel.
    ClosePanel,
    /// Esc inside the panel: a form or confirmation steps back to the list,
    /// the list closes the panel.
    PanelEscape,
    Select(WorkspaceId),
    /// Drag-and-drop in the panel finished with a new order for the
    /// user-created workspaces.
    Reorder(Vec<WorkspaceId>),
    StartCreate,
    /// "Move to New Workspace…" from a tab's context menu: open the create
    /// form; the tab moves into the workspace once it is created.
    StartCreateWithTab(TabId),
    StartEdit(WorkspaceId),
    StartDelete(WorkspaceId),
    BackToList,
    NameChanged(String),
    IconPicked(&'static str),
    SubmitForm,
    ConfirmDelete,
    MoveTab {
        tab_id: TabId,
        workspace: WorkspaceId,
    },
    /// Right click on a tab, at `position` in window coordinates.
    OpenTabMenu {
        tab_id: TabId,
        position: Point,
    },
    CloseTabMenu,
    CloseTab(TabId),
    Next,
    Previous,
}
