//! Wall-clock helpers for the TUI: current unix seconds and a coarse
//! "time ago" formatter. Kept out of the store (which only needs RFC3339
//! strings) so the humanize logic is unit-testable without a clock.
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parses the fixed `YYYY-MM-DDTHH:MM:SSZ` shape `store::now()` writes into
/// unix seconds. None on any malformation.
fn rfc3339_to_secs(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d) = (n(0..4)?, n(5..7)? as u32, n(8..10)? as u32);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let days = days_from_civil(y, mo, d);
    let secs = days * 86_400 + h * 3_600 + mi * 60 + se;
    u64::try_from(secs).ok()
}

/// Inverse of the store's civil_from_days (Howard Hinnant).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Coarse "time ago": "just now", "5m", "3h", "2d", "3w". Empty string when
/// the timestamp can't be parsed (render as nothing).
pub fn humanize_since(updated: &str, now_secs: u64) -> String {
    let Some(then) = rfc3339_to_secs(updated) else {
        return String::new();
    };
    let d = now_secs.saturating_sub(then);
    match d {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{}m", d / 60),
        3_600..=86_399 => format!("{}h", d / 3_600),
        86_400..=604_799 => format!("{}d", d / 86_400),
        _ => format!("{}w", d / 604_800),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_buckets() {
        let base = rfc3339_to_secs("2026-07-09T12:00:00Z").unwrap();
        assert_eq!(humanize_since("2026-07-09T12:00:00Z", base), "just now");
        assert_eq!(humanize_since("2026-07-09T11:55:00Z", base), "5m");
        assert_eq!(humanize_since("2026-07-09T09:00:00Z", base), "3h");
        assert_eq!(humanize_since("2026-07-07T12:00:00Z", base), "2d");
        assert_eq!(humanize_since("2026-06-18T12:00:00Z", base), "3w");
        assert_eq!(humanize_since("garbage", base), "");
    }

    #[test]
    fn roundtrips_against_store_epoch() {
        // 2026-07-09T00:00:00Z is a known day boundary; sanity-check parse.
        assert_eq!(rfc3339_to_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_to_secs("1970-01-01T00:01:00Z"), Some(60));
    }
}
