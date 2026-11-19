use super::{
    active_paid_route_approvals, active_review_hold_work_ids, append, backfill_review_verdict,
    effective_human_gate_from_entries, entries_for_work_id, index_entries_by_work_id, read_entries,
    repair_truncated_tail_at, review_already_exists, review_is_dedup_eligible,
    ReviewVerdictBackfill,
};
use crate::ledger::test_util as ledger_tests;
use std::fs;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[test]
fn matching_scan_preserves_order_aliases_and_rejects_discarded_corruption() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let mut entry = super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "codex",
        "fix",
        "fixture",
        None,
        None,
    );
    let mut text = String::new();
    for id in ["#42", "other", "TICKET-42"] {
        entry.work_id = Some(id.into());
        entry.error_summary = Some("x".repeat(1024 * 1024));
        text.push_str(&serde_json::to_string(&entry).unwrap());
        text.push('\n');
    }
    fs::write(cfg.defaults.ledger_path(), &text).unwrap();
    let selected = entries_for_work_id(&cfg, "#42").unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|entry| entry.work_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["#42", "TICKET-42"]
    );
    text.push_str("{broken discarded record\n");
    fs::write(cfg.defaults.ledger_path(), text).unwrap();
    assert!(super::read_matching_entries(&cfg, |_| false).is_err());
    assert!(entries_for_work_id(&cfg, "#42").is_err());
}

#[test]
fn streamed_snapshot_preserves_records_and_physical_bytes() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let path = cfg.defaults.ledger_path();
    let mut entry = super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "codex",
        "fix",
        "fixture",
        None,
        None,
    );
    entry.error_summary = Some("café 🤖\nrecord contents".into());
    let json = serde_json::to_string(&entry).unwrap();
    let text = format!("\r\n{json}\r\n \t\r\n{json}");
    fs::write(&path, &text).unwrap();
    let snapshot = super::read_snapshot(&cfg).unwrap();
    assert_eq!(snapshot.byte_len, text.len() as u64);
    assert_eq!(snapshot.entries.len(), 2);
    for actual in snapshot.entries {
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(&entry).unwrap()
        );
    }
    assert_eq!(super::read_entries_unlocked(&path).unwrap().len(), 2);
    assert_eq!(
        super::parse_jsonl_entries(&text, &path, 7).unwrap().len(),
        2
    );
}

#[test]
fn streamed_snapshot_rejects_corruption_and_io_failures_without_partial_results() {
    let (tmp, cfg) = ledger_tests::test_config();
    let path = cfg.defaults.ledger_path();
    assert!(super::read_snapshot(&cfg).unwrap().entries.is_empty());
    fs::write(&path, "\n \t\n{broken\n").unwrap();
    let error = super::read_snapshot(&cfg).err().unwrap().to_string();
    assert!(error.contains("parsing ledger entry 3"), "{error}");
    assert!(error.contains(&path.display().to_string()));
    let error = super::parse_jsonl_entries("\n{broken", &path, 7)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("parsing ledger entry 9"), "{error}");
    fs::write(&path, [0xff, b'\n']).unwrap();
    assert!(super::read_snapshot(&cfg)
        .err()
        .unwrap()
        .to_string()
        .contains("reading"));
    assert!(super::read_entries_unlocked(tmp.path()).is_err());
}

fn dedup_exists(
    cfg: &crate::config::GahConfig,
    profile_name: &str,
    repo_id: &str,
    work_id: &str,
    source_sha: &str,
    reviewer_class: &str,
) -> bool {
    let metadata_fingerprint = format!("metadata-for-{source_sha}");
    review_already_exists(
        cfg,
        profile_name,
        repo_id,
        work_id,
        source_sha,
        &metadata_fingerprint,
        reviewer_class,
    )
    .unwrap()
}

fn stamp_current_review(entry: &mut crate::ledger::LedgerEntry) {
    entry.review_contract_version = Some(crate::ledger::REVIEW_CONTRACT_VERSION);
    entry.review_generation = crate::ledger::review_generation(
        entry.review_source_sha.as_deref(),
        entry.review_metadata_fingerprint.as_deref(),
    );
}

#[test]
fn repair_truncated_tail_backs_up_only_an_unterminated_invalid_last_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");
    let valid_prefix = b"{\"record\":1}\n";
    let torn_tail = b"{\"record\":";
    let mut bytes = valid_prefix.to_vec();
    bytes.extend_from_slice(torn_tail);
    fs::write(&path, bytes).unwrap();

    let repaired = repair_truncated_tail_at(&path, false).unwrap();
    assert_eq!(repaired.dropped_bytes, torn_tail.len());
    let backup = repaired.backup_path.expect("torn tail must be backed up");
    assert_eq!(fs::read(&backup).unwrap(), torn_tail);
    assert_eq!(fs::read(&path).unwrap(), valid_prefix);
}

fn sample_entry() -> crate::ledger::LedgerEntry {
    crate::ledger::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "pm",
        "hello",
        Some("123".into()),
        None,
    )
}

#[test]
fn append_frames_a_complete_final_record_that_lost_its_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");
    // A kill between the record and its newline used to leave this tail;
    // the next append then joined two records on one unparseable line.
    fs::write(&path, b"{\"record\":1}\n{\"record\":2}").unwrap();
    super::append_locked(&path, &sample_entry()).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert_eq!(lines[1], "{\"record\":2}");
    for line in lines {
        serde_json::from_str::<serde_json::Value>(line).unwrap();
    }
    assert!(text.ends_with('\n'));
}

#[test]
fn append_still_refuses_a_torn_final_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(&path, b"{\"record\":1}\n{\"rec").unwrap();
    let err = super::append_locked(&path, &sample_entry()).unwrap_err();
    assert!(format!("{err:#}").contains("at byte 13"), "{err:#}");
}

#[test]
fn final_record_reads_across_chunk_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");
    let long = format!("{{\"pad\":\"{}\"}}", "x".repeat(200_000));
    fs::write(&path, format!("{{\"a\":1}}\n{long}")).unwrap();
    let tail = super::final_record(&path).unwrap();
    assert_eq!(tail.start, 8);
    assert_eq!(tail.bytes, long.as_bytes());
    fs::write(&path, format!("{long}\n")).unwrap();
    assert_eq!(super::final_record(&path).unwrap().bytes, b"\n");
    let missing = super::final_record(&dir.path().join("absent")).unwrap();
    assert!(missing.bytes.is_empty());
}

#[test]
fn ledger_append_writes_jsonl() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let entry = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "pm",
        "hello",
        Some("123".into()),
        None,
    );
    let path = append(&cfg, &entry).unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("\"profile\":\"test\""));
    assert!(text.ends_with('\n'));
}

#[test]
fn ledger_append_redacts_secret_like_strings_before_persisting() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let mut entry = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "pm",
        "hello",
        Some("123".into()),
        None,
    );
    entry.error_summary = Some("Authorization: Bearer abcdefghijklmnopqrstuvwxyz".into());
    let mut identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
        "opencode",
        Some("gpt-5"),
        Some("sk-proj-abcdefghijklmnopqrstuvwxyz"),
    );
    identity.account_label = Some("glpat-abcdefghijklmnopqrstuvwxyz".into());
    identity.auth_source_label = Some("ghp_abcdefghijklmnopqrstuvwxyz".into());
    identity.set_executable(Some("/private/credential-home/bin/opencode".into()));
    entry
        .attempt_routing
        .push(crate::ledger::AttemptRoutingRecord {
            skills: Vec::new(),
            attempt_number: 1,
            backend_instance: identity.backend_instance.clone(),
            effective_model: identity.effective_model.clone(),
            identity: Some(identity),
            routing_diagnostics: None,
        });
    let path = append(&cfg, &entry).unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    assert!(!text.contains("abcdefghijklmnopqrstuvwxyz"));
    assert!(!text.contains("credential-home"));
    assert!(!text.contains("executable"));
    assert!(text.contains("[REDACTED:TOKEN]"));
}

#[test]
fn effective_human_gate_is_latched_until_an_explicit_release_transition() {
    let profile = ledger_tests::profile();
    let mut gate =
        super::super::LedgerEntry::new("test", &profile, "auto", "fix", "#639", None, None);
    gate.work_id = Some("#639".into());
    gate.human_required = true;
    gate.human_required_reason_code = Some("stuck_loop_gate".into());
    gate.dispatch_reason = Some("stuck_loop_gate".into());

    let mut unrelated_completion = gate.clone();
    unrelated_completion.mode = "fix".into();
    unrelated_completion.human_required = false;
    unrelated_completion.human_required_reason_code = None;
    unrelated_completion.dispatch_reason = Some("post_review_repair".into());

    let active = effective_human_gate_from_entries(
        &[gate.clone(), unrelated_completion],
        "test",
        &profile.repo_id,
        "TICKET-639",
    )
    .expect("a non-review completion must not clear the gate");
    assert_eq!(active.reason_code.as_deref(), Some("stuck_loop_gate"));

    let mut review_release = gate;
    review_release.mode = "review".into();
    review_release.human_required = false;
    review_release.human_required_reason_code = None;
    assert!(
        effective_human_gate_from_entries(&[review_release], "test", &profile.repo_id, "#639",)
            .is_none()
    );
}

#[test]
fn paid_route_grant_releases_only_the_matching_profile_and_repository_gate() {
    let profile = ledger_tests::profile();
    let mut gate =
        super::super::LedgerEntry::new("test", &profile, "auto", "review", "#650", None, None);
    gate.work_id = Some("#650".into());
    gate.human_required = true;
    gate.human_required_reason_code = Some("policy_approval".into());
    gate.review_contract_version = Some(crate::ledger::REVIEW_CONTRACT_VERSION);
    let grant = super::super::LedgerEntry::new_paid_route_approval(
        "test",
        &profile,
        "#650",
        "opencode",
        Some("nous/glm-5.2"),
        true,
    );

    let active =
        effective_human_gate_from_entries(&[gate.clone(), grant], "test", &profile.repo_id, "#650")
            .expect("the raw policy gate stays latched until exact-route projection");
    assert_eq!(active.reason_code.as_deref(), Some("policy_approval"));
    assert!(
        effective_human_gate_from_entries(&[gate], "other-profile", &profile.repo_id, "#650",)
            .is_none()
    );
}

#[test]
fn review_noop_does_not_clear_a_paid_route_policy_gate() {
    let profile = ledger_tests::profile();
    let mut gate =
        super::super::LedgerEntry::new("test", &profile, "auto", "review", "#650", None, None);
    gate.work_id = Some("#650".into());
    gate.human_required = true;
    gate.human_required_reason_code = Some("policy_approval".into());
    gate.review_contract_version = Some(crate::ledger::REVIEW_CONTRACT_VERSION);

    let mut duplicate =
        super::super::LedgerEntry::new("test", &profile, "auto", "review", "#650", None, None);
    duplicate.work_id = Some("#650".into());
    duplicate.validation_result = Some("skipped_duplicate_review".into());

    let active =
        effective_human_gate_from_entries(&[gate, duplicate], "test", &profile.repo_id, "#650")
            .expect("an unchanged duplicate review must not release the approval hold");
    assert_eq!(active.reason_code.as_deref(), Some("policy_approval"));
}

#[test]
fn review_dedup_requires_exact_work_sha_metadata_and_reviewer_class() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let profile = ledger_tests::profile();
    let mut entry =
        super::super::LedgerEntry::new("test", &profile, "claude", "review", "x", None, None);
    entry.work_id = Some("#109".into());
    entry.review_source_sha = Some("abc123".into());
    entry.review_metadata_fingerprint = Some("metadata-for-abc123".into());
    stamp_current_review(&mut entry);
    entry.reviewer_class = Some("strong".into());
    entry.review_verdict = Some("APPROVE".into());
    append(&cfg, &entry).unwrap();

    assert!(dedup_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#109",
        "abc123",
        "strong"
    ));
    assert!(!review_already_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#109",
        "abc123",
        "metadata-changed-without-new-commit",
        "strong",
    )
    .unwrap());
    assert!(!dedup_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#109",
        "def456",
        "strong"
    ));
    assert!(!dedup_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#109",
        "abc123",
        "weak"
    ));
    assert!(!dedup_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#110",
        "abc123",
        "strong"
    ));
    assert!(!dedup_exists(
        &cfg,
        "other",
        &profile.repo_id,
        "#109",
        "abc123",
        "strong"
    ));
    assert!(!dedup_exists(
        &cfg,
        "test",
        "other-repo",
        "#109",
        "abc123",
        "strong"
    ));
}

#[test]
fn review_dedup_ignores_reviews_before_matching_clear_attempts() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let profile = ledger_tests::profile();
    let mut review =
        super::super::LedgerEntry::new("test", &profile, "claude", "review", "x", None, None);
    review.work_id = Some("#109".into());
    review.review_source_sha = Some("abc123".into());
    review.review_metadata_fingerprint = Some("metadata-for-abc123".into());
    stamp_current_review(&mut review);
    review.reviewer_class = Some("strong".into());
    review.review_verdict = Some("APPROVE".into());
    append(&cfg, &review).unwrap();

    let mut unrelated_profile_clear = super::super::LedgerEntry::new(
        "other-profile",
        &profile,
        "auto",
        "clear_attempts",
        "unrelated reset",
        None,
        None,
    );
    unrelated_profile_clear.work_id = Some("#109".into());
    append(&cfg, &unrelated_profile_clear).unwrap();
    assert!(dedup_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#109",
        "abc123",
        "strong"
    ));

    let mut other_repo = profile.clone();
    other_repo.repo_id = "other-repo".into();
    let mut unrelated_repo_clear = super::super::LedgerEntry::new(
        "test",
        &other_repo,
        "auto",
        "clear_attempts",
        "unrelated reset",
        None,
        None,
    );
    unrelated_repo_clear.work_id = Some("#109".into());
    append(&cfg, &unrelated_repo_clear).unwrap();
    assert!(dedup_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#109",
        "abc123",
        "strong"
    ));

    let mut clear = super::super::LedgerEntry::new(
        "test",
        &profile,
        "auto",
        "clear_attempts",
        "operator reset",
        None,
        None,
    );
    clear.work_id = Some("TICKET-109".into());
    append(&cfg, &clear).unwrap();

    assert!(!dedup_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#109",
        "abc123",
        "strong"
    ));

    append(&cfg, &review).unwrap();
    assert!(dedup_exists(
        &cfg,
        "test",
        &profile.repo_id,
        "#109",
        "abc123",
        "strong"
    ));
}

#[test]
fn review_dedup_retries_legacy_repairs_but_suppresses_structured_ones() {
    let profile = ledger_tests::profile();
    let mut entry =
        super::super::LedgerEntry::new("test", &profile, "claude", "review", "x", None, None);
    entry.review_verdict = Some("NEEDS_FIX".into());
    assert!(!review_is_dedup_eligible(&entry));

    entry.review_blocking_findings = vec!["src/lib.rs: broken retry".into()];
    assert!(review_is_dedup_eligible(&entry));

    entry.review_verdict = Some("APPROVE".into());
    entry.review_blocking_findings.clear();
    assert!(review_is_dedup_eligible(&entry));

    entry.failure_class = Some("harness_error".into());
    assert!(!review_is_dedup_eligible(&entry));
}

#[test]
fn entries_for_work_id_reads_legacy_ticket_alias_for_native_issue() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let mut matching = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "pm",
        "x",
        None,
        None,
    );
    matching.work_id = Some("TICKET-096".into());
    append(&cfg, &matching).unwrap();

    let mut other = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "pm",
        "y",
        None,
        None,
    );
    other.work_id = Some("TICKET-097".into());
    append(&cfg, &other).unwrap();

    let found = entries_for_work_id(&cfg, "#096").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].work_id.as_deref(), Some("TICKET-096"));
}

#[test]
fn index_entries_by_work_id_groups_only_tagged_entries() {
    let mut first = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "pm",
        "x",
        None,
        None,
    );
    first.work_id = Some("TICKET-096".into());
    let mut second = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "pm",
        "y",
        None,
        None,
    );
    second.work_id = Some("TICKET-096".into());
    let untagged = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "pm",
        "z",
        None,
        None,
    );

    let index_history = [first, second, untagged];
    let index = index_entries_by_work_id(&index_history);
    assert_eq!(index.len(), 2);
    assert_eq!(index["TICKET-096"].len(), 2);
    assert_eq!(index["#096"].len(), 2);
}

#[test]
fn backfill_review_verdict_attributes_to_implementation_entry_not_reviewer() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let mut impl_entry = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "vibe",
        "improve",
        "test1",
        None,
        None,
    );
    impl_entry.effective_backend = "vibe".to_string();
    impl_entry.branch = Some("gah/gah-123".to_string());
    append(&cfg, &impl_entry).unwrap();

    let mut review_entry = super::super::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "claude",
        "review",
        "test1",
        None,
        None,
    );
    review_entry.effective_backend = "claude".to_string();
    review_entry.branch = Some("gah/gah-123".to_string());
    append(&cfg, &review_entry).unwrap();

    let found = backfill_review_verdict(
        &cfg,
        "gah/gah-123",
        ReviewVerdictBackfill {
            verdict: "NEEDS_FIX",
            confidence: "high",
            reviewer_backend: "claude",
            reviewer_model: Some("claude-sonnet-4"),
            reviewer_tier: None,
            review_gate_reason: Some("test review evidence gate"),
            review_source_sha: Some("abc123"),
            review_metadata_fingerprint: Some("sha256:test"),
            review_contract_version: Some(super::super::REVIEW_CONTRACT_VERSION),
            review_generation: Some("review-v1:abc123:sha256:test"),
            blocking_findings: &["src/lib.rs: broken retry".to_string()],
            actionable_findings: &[],
            non_blocking_findings: &["consider a smaller helper".to_string()],
            risk_notes: &["retry state can be lost".to_string()],
            evidence: &[
                "file:src/lib.rs".to_string(),
                "ghp_abcdefghijklmnopqrstuvwxyz".to_string(),
            ],
            compatibility_evidence: &[],
        },
    )
    .unwrap();
    assert!(found);

    let entries = read_entries(&cfg).unwrap();
    let updated_impl = entries
        .iter()
        .find(|e| e.mode == "improve")
        .expect("implementation entry still present");
    assert_eq!(updated_impl.effective_backend, "vibe");
    assert_eq!(updated_impl.review_verdict.as_deref(), Some("NEEDS_FIX"));
    assert_eq!(updated_impl.reviewer_backend.as_deref(), Some("claude"));
    assert_eq!(
        updated_impl.review_gate_reason.as_deref(),
        Some("test review evidence gate")
    );
    assert_eq!(updated_impl.review_source_sha.as_deref(), Some("abc123"));
    assert_eq!(
        updated_impl.review_metadata_fingerprint.as_deref(),
        Some("sha256:test")
    );
    assert_eq!(
        updated_impl.review_blocking_findings,
        ["src/lib.rs: broken retry"]
    );
    assert_eq!(updated_impl.review_evidence[1], "[REDACTED:GITHUB_TOKEN]");

    let review_entry_after = entries
        .iter()
        .find(|e| e.mode == "review")
        .expect("review entry still present");
    assert_eq!(
        review_entry_after.review_verdict, None,
        "the reviewer's own entry must not be the one carrying the verdict"
    );
}

#[test]
fn backfill_review_verdict_returns_false_when_no_matching_branch() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let found = backfill_review_verdict(
        &cfg,
        "gah/no-such-branch",
        ReviewVerdictBackfill {
            verdict: "APPROVE",
            confidence: "high",
            reviewer_backend: "codex",
            reviewer_model: None,
            reviewer_tier: None,
            review_gate_reason: None,
            review_source_sha: None,
            review_metadata_fingerprint: None,
            review_contract_version: None,
            review_generation: None,
            blocking_findings: &[],
            actionable_findings: &[],
            non_blocking_findings: &[],
            risk_notes: &[],
            evidence: &[],
            compatibility_evidence: &[],
        },
    )
    .unwrap();
    assert!(!found);
}

#[test]
fn active_review_hold_work_ids_hold_with_no_release_is_active() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let prof = ledger_tests::profile();
    let hold = super::super::LedgerEntry::new_review_hold("test", &prof, "TICKET-600", None);
    append(&cfg, &hold).unwrap();

    let held = active_review_hold_work_ids(&cfg, "test");
    assert!(held.contains("TICKET-600"));
}

#[test]
fn review_hold_staleness_uses_the_injected_clock() {
    let clock = crate::clock::set_for_test(OffsetDateTime::UNIX_EPOCH);
    let (_tmp, cfg) = ledger_tests::test_config();
    let prof = ledger_tests::profile();
    let hold = super::super::LedgerEntry::new_review_hold("test", &prof, "TICKET-600", None);
    append(&cfg, &hold).unwrap();
    assert!(active_review_hold_work_ids(&cfg, "test").contains("TICKET-600"));
    clock.advance(time::Duration::hours(3));
    assert!(!active_review_hold_work_ids(&cfg, "test").contains("TICKET-600"));
}

#[test]
fn active_review_hold_work_ids_hold_then_release_is_not_active() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let prof = ledger_tests::profile();
    let hold = super::super::LedgerEntry::new_review_hold("test", &prof, "TICKET-600", None);
    append(&cfg, &hold).unwrap();
    let release = super::super::LedgerEntry::new_review_hold_release("test", &prof, "TICKET-600");
    append(&cfg, &release).unwrap();

    let held = active_review_hold_work_ids(&cfg, "test");
    assert!(!held.contains("TICKET-600"));
}

#[test]
fn active_review_hold_work_ids_stale_hold_is_not_active_even_without_release() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let prof = ledger_tests::profile();
    let mut hold = super::super::LedgerEntry::new_review_hold("test", &prof, "TICKET-600", None);
    hold.timestamp = (OffsetDateTime::now_utc() - time::Duration::hours(3))
        .format(&Rfc3339)
        .unwrap();
    append(&cfg, &hold).unwrap();

    let held = active_review_hold_work_ids(&cfg, "test");
    assert!(!held.contains("TICKET-600"));
}

#[test]
fn active_review_hold_work_ids_rehold_after_release_is_active_again() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let prof = ledger_tests::profile();
    append(
        &cfg,
        &super::super::LedgerEntry::new_review_hold("test", &prof, "TICKET-600", None),
    )
    .unwrap();
    append(
        &cfg,
        &super::super::LedgerEntry::new_review_hold_release("test", &prof, "TICKET-600"),
    )
    .unwrap();
    append(
        &cfg,
        &super::super::LedgerEntry::new_review_hold("test", &prof, "TICKET-600", None),
    )
    .unwrap();

    let held = active_review_hold_work_ids(&cfg, "test");
    assert!(held.contains("TICKET-600"));
}

#[test]
fn paid_route_approval_is_exact_and_revocable() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let prof = ledger_tests::profile();
    append(
        &cfg,
        &super::super::LedgerEntry::new_paid_route_approval(
            "test",
            &prof,
            "ISSUE-42",
            "opencode",
            Some("openai/gpt-paid"),
            true,
        ),
    )
    .unwrap();

    let active = active_paid_route_approvals(&cfg, "test", "ISSUE-42").unwrap();
    assert!(active.contains(&("opencode".to_string(), Some("openai/gpt-paid".to_string()))));
    assert!(!active.contains(&("opencode".to_string(), Some("different-model".to_string()))));

    append(
        &cfg,
        &super::super::LedgerEntry::new_paid_route_approval(
            "test",
            &prof,
            "ISSUE-42",
            "opencode",
            Some("openai/gpt-paid"),
            false,
        ),
    )
    .unwrap();
    assert!(active_paid_route_approvals(&cfg, "test", "ISSUE-42")
        .unwrap()
        .is_empty());
}

#[test]
fn stale_contract_version_review_derived_gate_is_superseded() {
    let (_tmp, _cfg) = ledger_tests::test_config();
    let prof = ledger_tests::profile();

    // Historical gate entry written under older review contract (version None)
    let mut entry =
        super::super::LedgerEntry::new("test", &prof, "codex", "review", "target", None, None);
    entry.work_id = Some("#711".into());
    entry.human_required = true;
    entry.human_required_reason_code = Some("stuck_loop_gate".into());
    entry.dispatch_reason = Some("stuck_loop_gate".into());
    entry.review_contract_version = None; // Historical/pre-bump

    let entries = vec![entry];
    let gate = effective_human_gate_from_entries(&entries, "test", &prof.repo_id, "#711");
    assert_eq!(
        gate, None,
        "Pre-bump review-derived gate must be superseded"
    );
}

#[test]
fn true_operator_human_hold_remains_active_across_contract_version_bump() {
    let (_tmp, cfg) = ledger_tests::test_config();
    let prof = ledger_tests::profile();

    // Explicit operator review_hold (not review-derived)
    let mut hold_entry = super::super::LedgerEntry::new_review_hold(
        "test",
        &prof,
        "#711",
        Some("operator pause".into()),
    );
    hold_entry.review_contract_version = None;

    append(&cfg, &hold_entry).unwrap();
    let held = active_review_hold_work_ids(&cfg, "test");
    assert!(
        held.contains("#711"),
        "Explicit operator hold must remain active"
    );
}
