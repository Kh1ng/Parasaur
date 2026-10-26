use super::super::super::test_util::profile;
use super::super::is_claim_stale;
use crate::ledger::LedgerEntry;

#[test]
fn ledger_claim_and_retry_reset_share_the_expiry_clock() {
    let tmp = tempfile::tempdir().unwrap();
    let start = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let clock = crate::clock::set_for_test(start);
    let profile = profile(tmp.path());
    let claim = LedgerEntry::new_claim("test", &profile, "#1");
    let reset = LedgerEntry::new_clear_attempts("test", &profile, "#1");
    assert_eq!(claim.timestamp, reset.timestamp);
    clock.advance(time::Duration::hours(7));
    assert!(is_claim_stale(&claim));
    assert!(is_claim_stale(&reset));
}
