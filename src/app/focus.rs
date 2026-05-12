use crate::plugin::ui::focus::{FocusReason, FocusSnapshot, FocusSurface};

use super::App;

impl App {
    pub(crate) fn compute_focus_snapshot(&mut self) -> FocusSnapshot {
        let surface = if let Some(plugin_screen) = self.tabs.active_plugin_screen() {
            FocusSurface::PluginScreen {
                plugin_id: plugin_screen.plugin_id().to_string(),
                screen_id: plugin_screen.screen_id().to_string(),
            }
        } else if let Some(screen) = self.tabs.active_screen() {
            FocusSurface::Repository(screen.focus_surface())
        } else {
            FocusSurface::Global
        };
        let reason = self
            .pending_focus_reason
            .take()
            .unwrap_or(FocusReason::RepositoryState);
        FocusSnapshot { surface, reason }
    }
}
