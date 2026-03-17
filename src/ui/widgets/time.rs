/// Format a timestamp as relative time (e.g., "just now", "5m ago", "3h ago")
pub fn format_time_ago(timestamp_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_secs = (now - timestamp_ms) / 1000;

    if diff_secs < 60 {
        "just now".to_string()
    } else if diff_secs < 3600 {
        format!("{}m ago", diff_secs / 60)
    } else if diff_secs < 86400 {
        format!("{}h ago", diff_secs / 3600)
    } else {
        format!("{}d ago", diff_secs / 86400)
    }
}

/// Format a timestamp as a date string (e.g., "Mar 17, 2026")
pub fn format_date(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.format("%b %d, %Y").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Format a timestamp as time (e.g., "14:30")
pub fn format_time(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_else(|| "??:??".to_string())
}

/// Format a timestamp as a full datetime (e.g., "Mar 17, 2026 14:30")
pub fn format_datetime(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.format("%b %d, %Y %H:%M").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Format duration from a start timestamp to now
pub fn format_duration(started_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_secs = (now - started_ms) / 1000;

    let days = diff_secs / 86400;
    let hours = (diff_secs % 86400) / 3600;
    let mins = (diff_secs % 3600) / 60;

    if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

/// Duration in hours from a start timestamp (for colorization)
pub fn duration_hours(started_ms: i64) -> f64 {
    let now = chrono::Utc::now().timestamp_millis();
    (now - started_ms) as f64 / (1000.0 * 3600.0)
}

/// Truncate a string to fit within a display width, accounting for
/// wide characters (CJK) that take 2 columns.
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    use unicode_width::UnicodeWidthStr;

    if max_width < 4 {
        return String::new();
    }

    // If the whole string fits, return as-is
    if s.width() <= max_width {
        return s.to_string();
    }

    // Need to truncate: reserve 3 chars for "..."
    let target = max_width.saturating_sub(3);
    let mut width = 0;
    let mut result = String::new();

    for ch in s.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > target {
            result.push_str("...");
            return result;
        }
        width += ch_width;
        result.push(ch);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_to_width_ascii() {
        assert_eq!(truncate_to_width("hello world", 20), "hello world");
        assert_eq!(truncate_to_width("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_to_width_unicode() {
        // CJK characters are 2 columns wide
        let s = "café résumé";
        let truncated = truncate_to_width(s, 8);
        assert!(truncated.ends_with("..."));
        assert!(!truncated.contains('\u{FFFD}')); // no replacement chars
    }

    #[test]
    fn test_truncate_to_width_short() {
        assert_eq!(truncate_to_width("hi", 3), String::new());
    }

    #[test]
    fn test_truncate_to_width_exact() {
        assert_eq!(truncate_to_width("hello", 5), "hello");
    }

    #[test]
    fn test_format_time_ago() {
        let now = chrono::Utc::now().timestamp_millis();
        assert_eq!(format_time_ago(now), "just now");
        assert_eq!(format_time_ago(now - 120_000), "2m ago");
        assert_eq!(format_time_ago(now - 7_200_000), "2h ago");
        assert_eq!(format_time_ago(now - 172_800_000), "2d ago");
    }
}
