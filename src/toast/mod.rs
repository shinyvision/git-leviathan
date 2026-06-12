mod card;
mod manager;
mod text_paths;

pub use card::ToastData;
pub use manager::{auto_dismiss_task, ToastManager};

const TOAST_WIDTH: f32 = 420.0;

#[derive(Debug, Clone, Copy)]
struct ToastVisual {
    offset_x: f32,
    scale: f32,
    alpha: f32,
}
