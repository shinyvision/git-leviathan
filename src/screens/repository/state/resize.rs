//! Shared resize state for sidebar + detail panels.
//!
//! Resize is inherently cross-panel: the center panel's pointer-moved handler
//! drives width/height changes on the sidebar and detail panels it sits
//! between. Keeping resize flags and widths on each panel forced the center
//! update to take `&mut SidebarPanel` / `&mut DetailPanel` siblings. Hosting
//! the state here lets every panel reach the same fields through
//! `RepositoryData`.

const MIN_SIDEBAR_WIDTH: f32 = 200.0;
const MAX_SIDEBAR_WIDTH: f32 = 600.0;
const MIN_DETAIL_WIDTH: f32 = 300.0;
const MAX_DETAIL_WIDTH: f32 = 900.0;
const MIN_DETAIL_HEIGHT: f32 = 320.0;
const MAX_DETAIL_HEIGHT: f32 = 700.0;

#[derive(Debug, Clone, Copy)]
struct DetailResizeDrag {
    pointer_x: f32,
    panel_width: f32,
}

#[derive(Debug, Clone, Copy)]
struct DetailHeightDrag {
    pointer_y: f32,
    panel_height: f32,
}

pub(in crate::screens::repository) struct ResizeState {
    pub sidebar_width: f32,
    pub sidebar_resizing: bool,
    pub detail_width: f32,
    pub detail_resizing: bool,
    pub detail_height: f32,
    pub detail_height_resizing: bool,
    detail_drag: Option<DetailResizeDrag>,
    detail_height_drag: Option<DetailHeightDrag>,
}

impl ResizeState {
    pub(in crate::screens::repository) fn new() -> Self {
        Self {
            sidebar_width: crate::theme::SIDEBAR_WIDTH as f32,
            sidebar_resizing: false,
            detail_width: crate::theme::DETAIL_PANEL_WIDTH as f32,
            detail_resizing: false,
            detail_height: crate::theme::DETAIL_PANEL_HEIGHT as f32,
            detail_height_resizing: false,
            detail_drag: None,
            detail_height_drag: None,
        }
    }

    pub(in crate::screens::repository) fn start_sidebar(&mut self, effective_width: f32) {
        self.sidebar_width = effective_width
            .floor()
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
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

    pub(in crate::screens::repository) fn start_detail(
        &mut self,
        pointer_x: f32,
        effective_width: f32,
    ) {
        self.detail_width = effective_width.clamp(MIN_DETAIL_WIDTH, MAX_DETAIL_WIDTH);
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

    pub(in crate::screens::repository) fn start_detail_height(&mut self, pointer_y: f32) {
        self.detail_height_resizing = true;
        self.detail_height_drag = Some(DetailHeightDrag {
            pointer_y,
            panel_height: self.detail_height,
        });
    }

    pub(in crate::screens::repository) fn handle_detail_height(&mut self, pointer_y: f32) {
        let start = *self.detail_height_drag.get_or_insert(DetailHeightDrag {
            pointer_y,
            panel_height: self.detail_height,
        });
        self.detail_height = resized_detail_panel_height(start, pointer_y);
    }

    pub(in crate::screens::repository) fn stop_detail_height(&mut self) {
        self.detail_height_resizing = false;
        self.detail_height_drag = None;
    }

    pub(in crate::screens::repository) fn stop_all(&mut self) {
        self.sidebar_resizing = false;
        self.stop_detail();
        self.stop_detail_height();
    }

    pub(in crate::screens::repository) fn any_resizing(&self) -> bool {
        self.sidebar_resizing || self.detail_resizing || self.detail_height_resizing
    }

    pub(in crate::screens::repository) fn effective_layout(
        &self,
        window_width: f32,
        num_lanes: usize,
    ) -> EffectiveLayout {
        effective_layout(
            window_width,
            self.sidebar_width,
            self.detail_width,
            num_lanes,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::screens::repository) enum EffectiveLayout {
    SideBySide { sidebar: f32, detail: f32 },
    Stacked { sidebar: f32 },
}

fn effective_layout(
    window_width: f32,
    sidebar_pref: f32,
    detail_pref: f32,
    num_lanes: usize,
) -> EffectiveLayout {
    let sidebar_default = crate::theme::SIDEBAR_WIDTH as f32;
    let detail_default = crate::theme::DETAIL_PANEL_WIDTH as f32;
    let min_center = crate::theme::MIN_CENTER_WIDTH
        + crate::widgets::graph::graph_lanes_width(num_lanes);

    let sidebar_floor = sidebar_default.min(sidebar_pref);
    let detail_floor = detail_default.min(detail_pref);

    let side_by_side_needed = sidebar_pref + min_center + detail_pref;
    let overflow = side_by_side_needed - window_width;

    if overflow <= 0.0 {
        return EffectiveLayout::SideBySide {
            sidebar: sidebar_pref,
            detail: detail_pref,
        };
    }

    let sidebar_slack = (sidebar_pref - sidebar_floor).max(0.0);
    let detail_slack = (detail_pref - detail_floor).max(0.0);
    let total_slack = sidebar_slack + detail_slack;

    if overflow <= total_slack && total_slack > 0.0 {
        let sidebar_share = overflow * (sidebar_slack / total_slack);
        let detail_share = overflow - sidebar_share;
        return EffectiveLayout::SideBySide {
            sidebar: sidebar_pref - sidebar_share,
            detail: detail_pref - detail_share,
        };
    }

    let stacked_overflow = sidebar_pref + min_center - window_width;
    let stacked_sidebar = if stacked_overflow > 0.0 {
        (sidebar_pref - stacked_overflow).max(sidebar_floor)
    } else {
        sidebar_pref
    };
    EffectiveLayout::Stacked {
        sidebar: stacked_sidebar,
    }
}

fn resized_detail_panel_width(start: DetailResizeDrag, pointer_x: f32) -> f32 {
    (start.panel_width + start.pointer_x - pointer_x).clamp(MIN_DETAIL_WIDTH, MAX_DETAIL_WIDTH)
}

fn resized_detail_panel_height(start: DetailHeightDrag, pointer_y: f32) -> f32 {
    (start.panel_height + start.pointer_y - pointer_y).clamp(MIN_DETAIL_HEIGHT, MAX_DETAIL_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::{
        effective_layout, resized_detail_panel_height, resized_detail_panel_width,
        DetailHeightDrag, DetailResizeDrag, EffectiveLayout,
    };

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

    #[test]
    fn detail_panel_height_resize_uses_drag_delta() {
        let start = DetailHeightDrag {
            pointer_y: 600.0,
            panel_height: 320.0,
        };

        assert_eq!(resized_detail_panel_height(start, 550.0), 370.0);
        assert_eq!(resized_detail_panel_height(start, 650.0), 320.0);
    }

    #[test]
    fn detail_panel_height_resize_clamps() {
        let start = DetailHeightDrag {
            pointer_y: 600.0,
            panel_height: 320.0,
        };

        assert_eq!(resized_detail_panel_height(start, 0.0), 700.0);
        assert_eq!(resized_detail_panel_height(start, 2000.0), 320.0);
    }

    #[test]
    fn effective_layout_keeps_preferences_when_window_fits() {
        let layout = effective_layout(1600.0, 300.0, 600.0, 0);
        assert_eq!(
            layout,
            EffectiveLayout::SideBySide {
                sidebar: 300.0,
                detail: 600.0
            }
        );
    }

    #[test]
    fn effective_layout_at_default_sum_keeps_defaults() {
        let layout = effective_layout(1210.0, 240.0, 510.0, 0);
        assert_eq!(
            layout,
            EffectiveLayout::SideBySide {
                sidebar: 240.0,
                detail: 510.0
            }
        );
    }

    #[test]
    fn effective_layout_distributes_shrink_by_slack_share() {
        let layout = effective_layout(1300.0, 300.0, 600.0, 0);
        assert_eq!(
            layout,
            EffectiveLayout::SideBySide {
                sidebar: 276.0,
                detail: 564.0
            }
        );
    }

    #[test]
    fn effective_layout_panel_with_more_slack_shrinks_more() {
        let layout = effective_layout(1400.0, 250.0, 700.0, 0);
        match layout {
            EffectiveLayout::SideBySide { sidebar, detail } => {
                assert!((sidebar - 249.5).abs() < 0.01);
                assert!((detail - 690.5).abs() < 0.01);
            }
            _ => panic!("expected side-by-side"),
        }
    }

    #[test]
    fn effective_layout_stacks_when_slack_insufficient() {
        let layout = effective_layout(1100.0, 240.0, 510.0, 0);
        assert_eq!(layout, EffectiveLayout::Stacked { sidebar: 240.0 });
    }

    #[test]
    fn effective_layout_shrinks_sidebar_when_stacked_and_narrow() {
        let layout = effective_layout(600.0, 400.0, 510.0, 0);
        assert_eq!(layout, EffectiveLayout::Stacked { sidebar: 240.0 });
    }

    #[test]
    fn effective_layout_respects_below_default_preference() {
        let layout = effective_layout(800.0, 200.0, 510.0, 0);
        assert_eq!(layout, EffectiveLayout::Stacked { sidebar: 200.0 });
    }

    #[test]
    fn effective_layout_stacks_when_overflow_exceeds_slack() {
        let layout = effective_layout(1100.0, 280.0, 530.0, 0);
        assert_eq!(layout, EffectiveLayout::Stacked { sidebar: 280.0 });
    }

    #[test]
    fn effective_layout_more_lanes_force_stack_at_window_that_fits_few_lanes() {
        let few_lanes = effective_layout(1300.0, 240.0, 510.0, 2);
        assert_eq!(
            few_lanes,
            EffectiveLayout::SideBySide {
                sidebar: 240.0,
                detail: 510.0
            }
        );

        let many_lanes = effective_layout(1300.0, 240.0, 510.0, 20);
        assert_eq!(many_lanes, EffectiveLayout::Stacked { sidebar: 240.0 });
    }
}
