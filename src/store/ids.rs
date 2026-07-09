// Port of internal/store/ids.go.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Short, sortable, per-process-unique id like "t_l8x2ab3": base36 unix-nanos
/// plus a base36 counter mod 1296 (36^2), same recipe as the Go newID.
pub(crate) fn new_id(prefix: &str) -> String {
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{prefix}{}{}", base36(nanos), base36(n % 1296))
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap_or_default() // ascii-only, cannot fail
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Port of TestNewIDUnique (project_test.go).
    #[test]
    fn test_new_id_unique() {
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let id = new_id("t_");
            assert!(id.starts_with("t_"), "bad id: {id:?}");
            assert!(seen.insert(id.clone()), "dup id: {id:?}");
        }
    }
}
