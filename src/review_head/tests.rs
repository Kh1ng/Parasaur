use super::*;
use crate::config::tests::test_profile_for_notifications;

fn review(profile: &Profile) -> LedgerEntry {
    let mut entry = LedgerEntry::new("test", profile, "codex", "review", "gah/1617", None, None);
    entry.branch = Some("gah/1617".into());
    entry.mr_url = Some("https://github.com/owner/repo/pull/42".into());
    entry.review_source_sha = Some("head-one".into());
    entry.review_verdict = Some("APPROVE".into());
    entry
}

#[test]
fn review_receipt_round_trip_requires_a_verdict_and_exact_head() {
    let profile = test_profile_for_notifications();
    let entry = review(&profile);
    let mut body = "GAH review verdict: `APPROVE`\n\nConfidence: `low`\n".to_string();
    stamp(&mut body, &entry).unwrap();
    assert!(has_receipt(&body, "head-one"));
    assert!(!has_receipt(&body, "head-two"));
    assert!(!has_receipt(
        &body.replace("APPROVE", "REVIEW_OUTPUT_INVALID"),
        "head-one"
    ));
    assert!(!has_receipt("GAH review verdict: `APPROVE`\n", "head-one"));
    assert!(!has_receipt(&format!("quoted comment\n{body}"), "head-one"));
}

#[test]
fn review_head_gate_works_without_ticket_identity_and_is_scoped_to_pr() {
    let profile = test_profile_for_notifications();
    let mut entry = review(&profile);
    entry.work_id = None;
    let check = |entry: LedgerEntry, name, branch, url, head| {
        locally_reviewed(&[entry], name, &profile, branch, Some(url), head)
    };
    let url = entry.mr_url.clone().unwrap();
    assert!(check(entry.clone(), "test", "gah/1617", &url, "head-one"));
    assert!(!check(entry.clone(), "other", "gah/1617", &url, "head-one"));
    assert!(!check(entry.clone(), "test", "other", &url, "head-one"));
    assert!(!check(
        entry.clone(),
        "test",
        "gah/1617",
        "other-pr",
        "head-one"
    ));
    assert!(!check(entry.clone(), "test", "gah/1617", &url, "head-two"));
    entry.review_verdict = Some("REVIEW_OUTPUT_INVALID".into());
    assert!(!check(entry.clone(), "test", "gah/1617", &url, "head-one"));
    entry.review_verdict = Some("APPROVE".into());
    entry.failure_class = Some("environment_error".into());
    assert!(!check(entry, "test", "gah/1617", &url, "head-one"));
}

/// An opinion that was never posted must not close the head: the operator
/// republishes it by reviewing again, and the skip records written while it
/// was unpublished must not close the head either.
#[test]
fn an_unpublished_opinion_leaves_the_head_reviewable() {
    let profile = test_profile_for_notifications();
    let url = "https://github.com/owner/repo/pull/42";
    let reviewed = |entries: &[LedgerEntry]| {
        locally_reviewed(entries, "test", &profile, "gah/1617", Some(url), "head-one")
    };
    let mut unpublished = review(&profile);
    unpublished.human_required_reason_code = Some("publication_failed".into());
    let mut skip = review(&profile);
    skip.review_verdict = None;
    skip.validation_result = Some("skipped_duplicate_review".into());

    assert!(!reviewed(&[unpublished.clone()]));
    assert!(!reviewed(&[unpublished.clone(), skip.clone()]));
    assert!(reviewed(&[unpublished, skip.clone(), review(&profile)]));
    assert!(reviewed(&[skip]), "a review found on the provider");
}

#[test]
fn review_receipts_read_later_pages_on_both_providers() {
    let _exec = crate::test_support::ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let mut profile = test_profile_for_notifications();
    profile.repo = "owner/repo".into();
    profile.provider_api_base = Some("https://gitlab.example/api/v4".into());
    profile.provider_project_id = Some("42".into());
    let mut body = "GAH review verdict: `NEEDS_FIX`\n".to_string();
    stamp(&mut body, &review(&profile)).unwrap();
    std::fs::write(
        tmp.path().join("first.json"),
        serde_json::to_string(&vec![serde_json::json!({"body":"unrelated"}); 100]).unwrap(),
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("second.json"),
        serde_json::to_string(&vec![serde_json::json!({"body":body})]).unwrap(),
    )
    .unwrap();
    for executable in ["gh", "glab"] {
        use std::os::unix::fs::PermissionsExt;
        let path = tmp.path().join(executable);
        std::fs::write(&path, "#!/bin/sh\ncase \"$*\" in *'&page=1'*) /bin/cat \"${0%/*}/first.json\" ;; *'&page=2'*) /bin/cat \"${0%/*}/second.json\" ;; *) exit 1 ;; esac\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    struct PathReset;
    impl Drop for PathReset {
        fn drop(&mut self) {
            crate::provider::clear_test_provider_path();
        }
    }
    crate::provider::set_test_provider_path(tmp.path().to_str().unwrap());
    let _reset = PathReset;
    for provider in ["github", "gitlab"] {
        profile.provider = provider.into();
        assert!(remotely_reviewed(&profile, "42", "head-one").unwrap());
        assert!(!remotely_reviewed(&profile, "42", "head-two").unwrap());
        assert!(remotely_reviewed(&profile, "../42", "head-one").is_err());
    }
}
