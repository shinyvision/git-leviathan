//! Small shared helpers for the plugin subsystem.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch for `t`, or `0` on clock error.
pub fn system_time_unix_ms(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Milliseconds since the Unix epoch for the current time, or `0` on
/// clock error.
pub fn now_unix_ms() -> u128 {
    system_time_unix_ms(SystemTime::now())
}
