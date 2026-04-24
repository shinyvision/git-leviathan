//! Toast stack messages — show a new toast, dismiss an existing one by id.

use crate::toast::ToastData;

#[derive(Debug, Clone)]
pub enum ToastMessage {
    Show(ToastData),
    Dismiss(u64),
}
