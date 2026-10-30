//! Compact value formatting for Magpie request tables.

/// Compact token count for a table cell.
pub fn tokens_compact(n: i64) -> String {
    if n <= 0 {
        return BLANK.into();
    }
    let f = n as f64;
    if f >= 1_000_000.0 {
        format!("{:.2}M", f / 1_000_000.0)
    } else if f >= 1000.0 {
        format!("{:.1}k", f / 1000.0)
    } else {
        n.to_string()
    }
}

/// What a missing value draws: nothing at all. Named so every call site
/// agrees and the emptiness reads as deliberate rather than an oversight.
pub const BLANK: &str = "";

/// Billed cost, e.g. `$0.1388`. Amounts under a dollar keep four places: two
/// would turn `$0.1388` and `$0.1289` into the same `$0.14` / `$0.13`.
pub fn cost(value: f64) -> String {
    if value >= 1.0 {
        format!("${value:.2}")
    } else {
        format!("${value:.4}")
    }
}

/// `[456.5s|789.2s]`: time to first token, then total latency. A missing side
/// is left out, so a request that failed before the first token reads `[4.5s]`.
pub fn latency(total_ms: Option<i64>, first_ms: Option<i64>) -> Option<String> {
    let total = total_ms.filter(|ms| *ms > 0).map(seconds);
    let first = first_ms.filter(|ms| *ms > 0).map(seconds);
    match (first, total) {
        (Some(first), Some(total)) => Some(format!("[{first}|{total}]")),
        (Some(only), None) | (None, Some(only)) => Some(format!("[{only}]")),
        (None, None) => None,
    }
}

/// One reading of the latency bracket: `4.5s`, `3m·10s`, `1h·02m`. The tenth is
/// what a first token is measured in, so it is kept while the reading is under
/// a minute; past that it is noise next to a number that long.
fn seconds(ms: i64) -> String {
    let secs = ms as f64 / 1000.0;
    if secs < 60.0 {
        return format!("{secs:.1}s");
    }
    let whole = secs.round() as i64;
    if whole < 3600 {
        format!("{}m·{:02}s", whole / 60, whole % 60)
    } else {
        format!("{}h·{:02}m", whole / 3600, (whole % 3600) / 60)
    }
}

/// Relative age such as `30s` / `2m` / `1h` / `3d`, falling back to an
/// absolute clock reading beyond a week.
///
/// Recent requests are what the panel is for, so the first minutes are shown in
/// seconds and anything older than a week shows the wall clock.
pub fn relative_time(iso: Option<&str>, now: i64) -> String {
    let Some(then) = iso.and_then(crate::time::parse_unix) else {
        return BLANK.into();
    };
    let delta = now - then;
    if delta < 0 {
        // Clock skew: the gateway's clock is briefly ahead of ours, which shows
        // up on brand-new (often still-processing) requests. Treat the instant
        // as just now instead of jumping to a wall-clock reading.
        return "0s".into();
    }
    let clock = local_clock(iso);
    match delta {
        0..=59 => format!("{delta}s"),
        60..=3599 => format!("{}m", delta / 60),
        3600..=86_399 => format!("{}h", delta / 3600),
        _ if delta < 7 * 86_400 => format!("{}d", delta / 86_400),
        _ => clock,
    }
}

/// `HH:MM:SS` in local time, derived from the wire's RFC3339 instant.
pub fn local_clock(iso: Option<&str>) -> String {
    // Parse the instant first: interpreting offset date/time components as UTC
    // would apply the timezone shift twice.
    let local = iso
        .and_then(crate::time::parse_unix)
        .and_then(crate::time::local_parts);
    match local {
        Some(p) => format!("{:02}:{:02}:{:02}", p.hour, p.minute, p.second),
        None => BLANK.into(),
    }
}

#[cfg(test)]
#[path = "tests/format.rs"]
mod tests;
