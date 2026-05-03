pub(super) fn short_hash(hash: &str) -> String {
    hash.chars().take(7).collect()
}

pub(super) fn relative_time(commit_secs: i64) -> Option<String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let diff = now - commit_secs;
    if diff < 0 {
        return None;
    }

    Some(relative_time_from_diff(diff))
}

pub(super) fn relative_time_from_diff(diff: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const WEEK: i64 = 7 * DAY;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;

    if diff < MINUTE {
        "just now".to_string()
    } else if diff < HOUR {
        format_time_unit(diff / MINUTE, "minute")
    } else if diff < 2 * HOUR {
        "1 hour ago".to_string()
    } else if diff < DAY {
        format_time_unit(diff / HOUR, "hour")
    } else if diff < 2 * DAY {
        "yesterday".to_string()
    } else if diff < WEEK {
        format_time_unit(diff / DAY, "day")
    } else if diff < MONTH {
        format_time_unit(diff / WEEK, "week")
    } else if diff < YEAR {
        format_time_unit(diff / MONTH, "month")
    } else {
        format_time_unit(diff / YEAR, "year")
    }
}

fn format_time_unit(value: i64, unit: &str) -> String {
    let suffix = if value == 1 { "" } else { "s" };
    format!("{} {}{} ago", value, unit, suffix)
}

pub(super) fn format_date(timestamp: i64, offset_minutes: i32) -> String {
    use chrono::{FixedOffset, TimeZone, Utc};

    let offset_secs = offset_minutes * 60;
    let Some(timezone) = FixedOffset::east_opt(offset_secs) else {
        return timestamp.to_string();
    };

    match Utc.timestamp_opt(timestamp, 0).single() {
        Some(datetime) => datetime
            .with_timezone(&timezone)
            .format("%m/%d/%Y @ %-I:%M %p")
            .to_string(),
        None => timestamp.to_string(),
    }
}
