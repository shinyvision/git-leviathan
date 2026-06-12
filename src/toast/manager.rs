//! Toast stack state machine: enter/hold/exit animation, overflow handling,
//! and stack-position interpolation.
use iced::{widget::Stack, Element, Length, Task};

use crate::message::Message;
use crate::work::timer_work;

use super::card::{measure_toast_height, toast_view, ToastData};
use super::{ToastVisual, TOAST_WIDTH};

const TOAST_STACK_GAP: f32 = 10.0;
const MAX_VISIBLE_TOASTS: usize = 5;

const ENTER_MS: f32 = 180.0;
const HOLD_MS: f32 = 8000.0;
const EXIT_MS: f32 = 200.0;
const POSITION_EPSILON: f32 = 0.5;

const ENTER_START_X: f32 = -240.0;
const EXIT_END_X: f32 = -(TOAST_WIDTH + 32.0);
const MIN_SCALE: f32 = 0.88;
const STACK_SLIDE_SPEED_PX_PER_MS: f32 = 2.0;

#[derive(Debug, Clone)]
struct ActiveToast {
    id: u64,
    data: ToastData,
    height: f32,
    age_ms: f32,
    current_bottom_offset: f32,
    target_bottom_offset: f32,
}

impl ActiveToast {
    fn new(id: u64, data: ToastData, height: f32) -> Self {
        Self {
            id,
            data,
            height,
            age_ms: 0.0,
            current_bottom_offset: 0.0,
            target_bottom_offset: 0.0,
        }
    }

    fn advance_age(&mut self, delta_ms: f32) {
        self.age_ms = (self.age_ms + delta_ms).min(ENTER_MS);
    }

    fn update_position(&mut self, delta_ms: f32) {
        self.current_bottom_offset = move_toward(
            self.current_bottom_offset,
            self.target_bottom_offset,
            STACK_SLIDE_SPEED_PX_PER_MS * delta_ms,
        );
    }

    fn visual(&self) -> ToastVisual {
        if self.age_ms < ENTER_MS {
            let progress = ease_out_cubic(self.age_ms / ENTER_MS);
            ToastVisual {
                offset_x: lerp(ENTER_START_X, 0.0, progress),
                scale: lerp(MIN_SCALE, 1.0, progress),
                alpha: progress,
            }
        } else {
            ToastVisual {
                offset_x: 0.0,
                scale: 1.0,
                alpha: 1.0,
            }
        }
    }

    fn into_exiting(self, placement: ExitPlacement) -> ExitingToast {
        let visual = self.visual();

        ExitingToast {
            data: self.data,
            height: self.height,
            elapsed_ms: 0.0,
            current_bottom_offset: self.current_bottom_offset,
            target_bottom_offset: self.current_bottom_offset,
            placement,
            start_offset_x: visual.offset_x,
            start_scale: visual.scale,
            start_alpha: visual.alpha,
        }
    }

    fn is_animating(&self) -> bool {
        self.age_ms < ENTER_MS
            || (self.current_bottom_offset - self.target_bottom_offset).abs() > POSITION_EPSILON
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitPlacement {
    Fixed,
    AboveStack,
}

#[derive(Debug, Clone)]
struct ExitingToast {
    data: ToastData,
    height: f32,
    elapsed_ms: f32,
    current_bottom_offset: f32,
    target_bottom_offset: f32,
    placement: ExitPlacement,
    start_offset_x: f32,
    start_scale: f32,
    start_alpha: f32,
}

impl ExitingToast {
    fn update_position(&mut self, delta_ms: f32) {
        self.current_bottom_offset = move_toward(
            self.current_bottom_offset,
            self.target_bottom_offset,
            STACK_SLIDE_SPEED_PX_PER_MS * delta_ms,
        );
    }

    fn tick(&mut self, delta_ms: f32) -> bool {
        self.elapsed_ms += delta_ms;
        self.elapsed_ms >= EXIT_MS
    }

    fn is_animating(&self) -> bool {
        self.elapsed_ms < EXIT_MS
            || (self.current_bottom_offset - self.target_bottom_offset).abs() > POSITION_EPSILON
    }

    fn visual(&self) -> ToastVisual {
        let progress = ease_in_cubic(self.elapsed_ms / EXIT_MS);

        ToastVisual {
            offset_x: lerp(self.start_offset_x, EXIT_END_X, progress),
            scale: lerp(self.start_scale, MIN_SCALE, progress),
            alpha: lerp(self.start_alpha, 0.0, progress),
        }
    }
}

#[derive(Debug, Default)]
pub struct ToastManager {
    active: Vec<ActiveToast>,
    exiting: Vec<ExitingToast>,
    next_id: u64,
}

impl ToastManager {
    pub fn push(&mut self, toast: ToastData) -> u64 {
        let mut overflowed = false;

        while self.active.len() >= MAX_VISIBLE_TOASTS {
            self.force_pop_oldest(ExitPlacement::AboveStack);
            overflowed = true;
        }

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let height = measure_toast_height(&toast);
        self.active.push(ActiveToast::new(id, toast, height));
        self.update_targets();

        if overflowed {
            self.snap_overflow_reflow_positions();
        }

        id
    }

    pub fn dismiss(&mut self, id: u64) {
        let Some(index) = self.active.iter().position(|toast| toast.id == id) else {
            return;
        };

        self.exiting
            .push(self.active.remove(index).into_exiting(ExitPlacement::Fixed));
        self.update_targets();
    }

    pub fn tick(&mut self, delta_ms: f32) {
        if delta_ms <= 0.0 {
            return;
        }

        for toast in &mut self.active {
            toast.advance_age(delta_ms);
            if toast.is_animating() {
                toast.update_position(delta_ms);
            }
        }

        for toast in &mut self.exiting {
            if toast.is_animating() {
                toast.update_position(delta_ms);
            }
        }

        self.exiting.retain_mut(|toast| !toast.tick(delta_ms));
    }

    pub fn has_active_toasts(&self) -> bool {
        !self.active.is_empty() || !self.exiting.is_empty()
    }

    pub fn is_animating(&self) -> bool {
        self.active.iter().any(ActiveToast::is_animating)
            || self.exiting.iter().any(ExitingToast::is_animating)
    }

    pub fn overlay(&self) -> Option<Element<'static, Message>> {
        if !self.has_active_toasts() {
            return None;
        }

        let mut children = self
            .active
            .iter()
            .map(|toast| {
                toast_view(
                    &toast.data,
                    toast.height,
                    toast.visual(),
                    toast.current_bottom_offset,
                    Some(toast.id),
                )
            })
            .collect::<Vec<_>>();

        children.extend(self.exiting.iter().map(|toast| {
            toast_view(
                &toast.data,
                toast.height,
                toast.visual(),
                toast.current_bottom_offset,
                None,
            )
        }));

        Some(
            Stack::with_children(children)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }

    fn force_pop_oldest(&mut self, placement: ExitPlacement) {
        if self.active.is_empty() {
            return;
        }

        self.exiting
            .push(self.active.remove(0).into_exiting(placement));
        self.update_targets();
    }

    fn update_targets(&mut self) {
        let mut next_offset = 0.0;

        for toast in self.active.iter_mut().rev() {
            toast.target_bottom_offset = next_offset;
            next_offset += toast.height + TOAST_STACK_GAP;
        }

        for toast in self
            .exiting
            .iter_mut()
            .rev()
            .filter(|toast| toast.placement == ExitPlacement::AboveStack)
        {
            toast.target_bottom_offset = next_offset;
            next_offset += toast.height + TOAST_STACK_GAP;
        }
    }

    fn snap_overflow_reflow_positions(&mut self) {
        // When the stack is already full, snap the reflowed toasts into place so
        // only the entering toast and the overflowed exit animate.
        for toast in &mut self.active {
            toast.current_bottom_offset = toast.target_bottom_offset;
        }

        for toast in &mut self.exiting {
            if toast.placement == ExitPlacement::AboveStack {
                toast.current_bottom_offset = toast.target_bottom_offset;
            }
        }
    }
}

pub fn auto_dismiss_task(id: u64) -> Task<Message> {
    let delay = std::time::Duration::from_millis((ENTER_MS + HOLD_MS).round() as u64);
    Task::perform(
        async move {
            timer_work(delay).await;
            id
        },
        Message::dismiss_toast,
    )
}

fn lerp(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress.clamp(0.0, 1.0)
}

fn move_toward(current: f32, target: f32, max_step: f32) -> f32 {
    let delta = target - current;

    if delta.abs() <= max_step {
        target
    } else if delta.is_sign_positive() {
        current + max_step
    } else {
        current - max_step
    }
}

fn ease_out_cubic(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(3)
}

fn ease_in_cubic(progress: f32) -> f32 {
    progress.clamp(0.0, 1.0).powi(3)
}

#[cfg(test)]
mod tests {
    use super::super::ToastData;
    use super::{ToastManager, ENTER_MS, EXIT_END_X, EXIT_MS, MAX_VISIBLE_TOASTS};

    #[test]
    fn toast_manager_removes_toast_after_dismiss_animation_finishes() {
        let mut manager = ToastManager::default();
        let toast_id = manager.push(ToastData::error("Test", "Body"));

        manager.tick(ENTER_MS);
        assert!(manager.has_active_toasts());
        assert_eq!(manager.active.len(), 1);
        assert!(!manager.is_animating());

        manager.dismiss(toast_id);
        assert!(manager.has_active_toasts());
        assert!(manager.active.is_empty());
        assert_eq!(manager.exiting.len(), 1);

        manager.tick(EXIT_MS);
        assert!(!manager.has_active_toasts());
    }

    #[test]
    fn toast_manager_force_pops_top_to_keep_stack_at_five() {
        let mut manager = ToastManager::default();

        for index in 0..=MAX_VISIBLE_TOASTS {
            manager.push(ToastData::error(format!("Toast {index}"), "Body"));
        }

        assert_eq!(manager.active.len(), MAX_VISIBLE_TOASTS);
        assert_eq!(manager.exiting.len(), 1);
        assert_eq!(manager.active.last().unwrap().data.title, "Toast 5");
        assert_eq!(manager.active.first().unwrap().data.title, "Toast 1");
    }

    #[test]
    fn overflow_popped_toast_retargets_above_visible_stack() {
        let mut manager = ToastManager::default();

        for index in 0..MAX_VISIBLE_TOASTS {
            manager.push(ToastData::error(format!("Toast {index}"), "Body"));
        }

        for toast in &mut manager.active {
            toast.current_bottom_offset = toast.target_bottom_offset;
        }

        let previous_top_offset = manager.active.first().unwrap().current_bottom_offset;

        manager.push(ToastData::error("Toast 5", "Body"));

        let exiting = manager.exiting.last().unwrap();
        assert!(exiting.current_bottom_offset > previous_top_offset);
        assert!(exiting.target_bottom_offset > previous_top_offset);

        for toast in &manager.active {
            assert_eq!(toast.current_bottom_offset, toast.target_bottom_offset);
        }
    }

    #[test]
    fn toast_manager_only_animates_while_toasts_are_moving() {
        let mut manager = ToastManager::default();

        manager.push(ToastData::error("Test", "Body"));
        assert!(manager.is_animating());

        manager.tick(ENTER_MS);
        assert!(!manager.is_animating());
    }

    #[test]
    fn overflow_insert_limits_animation_to_enter_and_exit_transitions() {
        let mut manager = ToastManager::default();

        for index in 0..MAX_VISIBLE_TOASTS {
            manager.push(ToastData::error(format!("Toast {index}"), "Body"));
        }

        for toast in &mut manager.active {
            toast.age_ms = ENTER_MS;
            toast.current_bottom_offset = toast.target_bottom_offset;
        }
        assert!(!manager.is_animating());

        manager.push(ToastData::error("Toast 5", "Body"));

        assert_eq!(manager.exiting.len(), 1);
        assert_eq!(
            manager
                .active
                .iter()
                .filter(|toast| toast.is_animating())
                .count(),
            1
        );
        assert!(manager
            .active
            .iter()
            .all(|toast| toast.current_bottom_offset == toast.target_bottom_offset));
    }

    #[test]
    fn exiting_toast_moves_left_off_screen() {
        const { assert!(EXIT_END_X < 0.0) };
    }
}
