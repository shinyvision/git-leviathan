//! Shared resize state for sidebar + detail panels.
//!
//! Resize is inherently cross-panel: the center panel's pointer-moved handler
//! drives width changes on the sidebar and detail panels it sits between.
//! Keeping resize flags and widths on each panel forced the center update to
//! take `&mut SidebarPanel` / `&mut DetailPanel` siblings. Hosting the state
//! here lets every panel reach the same fields through `RepositoryData`.

const MIN_SIDEBAR_WIDTH: f32 = 200.0;
const MAX_SIDEBAR_WIDTH: f32 = 600.0;
const MIN_DETAIL_WIDTH: f32 = 300.0;
const MAX_DETAIL_WIDTH: f32 = 900.0;

#[derive(Debug, Clone, Copy)]
struct DetailResizeDrag {
    pointer_x: f32,
    panel_width: f32,
}

pub(in crate::screens::repository) struct ResizeState {
    pub sidebar_width: f32,
    pub sidebar_resizing: bool,
    pub detail_width: f32,
    pub detail_resizing: bool,
    detail_drag: Option<DetailResizeDrag>,
}

impl ResizeState {
    pub(in crate::screens::repository) fn new() -> Self {
        Self {
            sidebar_width: crate::theme::SIDEBAR_WIDTH as f32,
            sidebar_resizing: false,
            detail_width: crate::theme::DETAIL_PANEL_WIDTH as f32,
            detail_resizing: false,
            detail_drag: None,
        }
    }

    pub(in crate::screens::repository) fn start_sidebar(&mut self) {
        self.sidebar_resizing = true;
    }

    pub(in crate::screens::repository) fn handle_sidebar(&mut self, position_x: f32) {
        self.sidebar_width = position_x
            .floor()
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    }

    pub(in crate::screens::repository) fn stop_sidebar(&mut self) {
        self.sidebar_resizing = false;
    }

    pub(in crate::screens::repository) fn start_detail(&mut self, pointer_x: f32) {
        self.detail_resizing = true;
        self.detail_drag = Some(DetailResizeDrag {
            pointer_x,
            panel_width: self.detail_width,
        });
    }

    pub(in crate::screens::repository) fn handle_detail(&mut self, pointer_x: f32) {
        let start = *self.detail_drag.get_or_insert(DetailResizeDrag {
            pointer_x,
            panel_width: self.detail_width,
        });
        self.detail_width = resized_detail_panel_width(start, pointer_x);
    }

    pub(in crate::screens::repository) fn stop_detail(&mut self) {
        self.detail_resizing = false;
        self.detail_drag = None;
    }

    pub(in crate::screens::repository) fn stop_all(&mut self) {
        self.sidebar_resizing = false;
        self.stop_detail();
    }
}

fn resized_detail_panel_width(start: DetailResizeDrag, pointer_x: f32) -> f32 {
    (start.panel_width + start.pointer_x - pointer_x).clamp(MIN_DETAIL_WIDTH, MAX_DETAIL_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::{resized_detail_panel_width, DetailResizeDrag};

    #[test]
    fn detail_panel_resize_uses_drag_delta() {
        let start = DetailResizeDrag {
            pointer_x: 1000.0,
            panel_width: 510.0,
        };

        assert_eq!(resized_detail_panel_width(start, 950.0), 560.0);
        assert_eq!(resized_detail_panel_width(start, 1050.0), 460.0);
    }

    #[test]
    fn detail_panel_resize_clamps_width() {
        let start = DetailResizeDrag {
            pointer_x: 1000.0,
            panel_width: 510.0,
        };

        assert_eq!(resized_detail_panel_width(start, 2000.0), 300.0);
        assert_eq!(resized_detail_panel_width(start, 100.0), 900.0);
    }
}
