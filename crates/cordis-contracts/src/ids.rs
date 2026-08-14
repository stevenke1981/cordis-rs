use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generates sortable-enough opaque identifiers without a platform RNG dependency.
pub fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut hash = Sha256::new();
    hash.update(prefix.as_bytes());
    hash.update(nanos.to_le_bytes());
    hash.update(count.to_le_bytes());
    hash.update(pid.to_le_bytes());
    let digest = hash.finalize();
    format!("{prefix}-{}", hex::encode(&digest[..8]))
}

/// Returns a UTC timestamp in RFC 3339 form using only the standard library.
pub fn now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    unix_seconds_to_rfc3339(seconds)
}

fn unix_seconds_to_rfc3339(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Howard Hinnant's civil calendar conversion, translated to Rust.
fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_formatted() {
        assert_eq!(unix_seconds_to_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_seconds_to_rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn identifiers_are_prefixed_and_unique() {
        let first = new_id("task");
        let second = new_id("task");
        assert!(first.starts_with("task-"));
        assert_ne!(first, second);
    }
}
