use crate::toast::ToastData;

#[derive(Debug, Clone)]
pub enum ToastMessage {
    Show(ToastData),
    Dismiss(u64),
}
