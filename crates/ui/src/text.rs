use gpui::SharedString;

fn without_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let tail = &rest[open + 1..];
        let tag = tail.find('>').map(|close| &tail[..close]).filter(|tag| {
            let name = tag.trim_start_matches('/');
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        });
        if let Some(tag) = tag {
            out.push_str(&rest[..open]);
            out.push(' ');
            rest = &tail[tag.len() + 1..];
        } else {
            out.push_str(&rest[..=open]);
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

pub fn one_line(title: Option<&str>) -> SharedString {
    let words = title.map(without_markup).unwrap_or_default();
    let line = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        "Untitled".into()
    } else {
        line.into()
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

/// A moment as a local clock reads it: the time alone today, else with its date, and with its year
/// when that isn't this year.
pub fn clock(at_ms: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(at) = Local.timestamp_millis_opt(at_ms).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    if at.date_naive() == today {
        at.format("%H:%M").to_string()
    } else if at.year() == today.year() {
        at.format("%b %-d, %H:%M").to_string()
    } else {
        at.format("%b %-d %Y, %H:%M").to_string()
    }
}

pub fn ago(now_ms: i64, at_ms: i64) -> String {
    let minutes = (now_ms - at_ms).max(0) / 60_000;
    match minutes {
        0 => "now".to_owned(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        1440..20160 => format!("{}d", minutes / 1440),
        _ => format!("{}w", minutes / 10080),
    }
}

/// `ago` for a sentence, such as "just now" or "5m ago".
pub fn ago_phrase(now_ms: i64, at_ms: i64) -> String {
    if now_ms - at_ms < 60_000 {
        "just now".to_owned()
    } else {
        format!("{} ago", ago(now_ms, at_ms))
    }
}

/// A length of time as a person says it: "850 ms", "1m 12s", "2h 5m".
pub fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    match seconds {
        0 => format!("{ms} ms"),
        1..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// A size in bytes, to one decimal past a kilobyte: "512 B", "4.1 KB", "17.8 MB".
pub fn bytes(count: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if count < 1024 {
        return format!("{count} B");
    }
    let mut tenths = count * 10 / 1024;
    let mut unit = 0;
    while tenths >= 10_240 && unit + 1 < UNITS.len() {
        tenths /= 1024;
        unit += 1;
    }
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::one_line;

    #[test]
    fn titles_lose_harness_markup_but_keep_comparisons() {
        assert_eq!(
            one_line(Some(
                "<local-command-caveat>The command\nran</local-command-caveat>"
            ))
            .as_ref(),
            "The command ran"
        );
        assert_eq!(
            one_line(Some("if a < b and c > d")).as_ref(),
            "if a < b and c > d"
        );
        assert_eq!(one_line(Some("<x></x>")).as_ref(), "Untitled");
    }
}
