//! Wall-clock source for controller and expiry decisions.
//!
//! `GAH_TEST_NOW` is read once when the process starts. Production processes
//! without that variable continue to use the system UTC clock.

use std::sync::OnceLock;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

static TEST_NOW: OnceLock<Option<OffsetDateTime>> = OnceLock::new();

/// Initialize the test clock before command dispatch starts.
pub fn init() {
    let _ = test_now();
}

/// Return the configured test instant, or the current UTC time in production.
pub fn now_utc() -> OffsetDateTime {
    test_now().unwrap_or_else(OffsetDateTime::now_utc)
}

/// Return the same instant as a Chrono UTC value for legacy claim metadata.
pub fn now_chrono() -> chrono::DateTime<chrono::Utc> {
    let now = now_utc();
    chrono::DateTime::from_timestamp(now.unix_timestamp(), now.nanosecond())
        .expect("UTC clock instant is representable by Chrono")
}

fn test_now() -> Option<OffsetDateTime> {
    *TEST_NOW.get_or_init(|| {
        std::env::var("GAH_TEST_NOW")
            .ok()
            .and_then(|value| OffsetDateTime::parse(&value, &Rfc3339).ok())
            .map(|value| value.to_offset(UtcOffset::UTC))
    })
}

// ponytail: direct wall-clock reads elsewhere remain outside this targeted seam; expand only when a test needs another time-dependent path.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_parses_rfc3339_and_normalizes_to_utc() {
        let parsed = OffsetDateTime::parse("2026-10-26T12:00:00-05:00", &Rfc3339).unwrap();
        assert_eq!(parsed.to_offset(UtcOffset::UTC).hour(), 17);
    }
}
