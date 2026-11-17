use super::{
    classify, extract_work_id_from_title, github_ci_failed, github_ci_passed, gitlab_ci_failed,
    gitlab_ci_passed, gitlab_ci_pending, recommended_action, review_metadata_fingerprint,
    sync_mr_to_json, GithubCheck, GithubPr, GitlabMr, RecommendedAction, SyncMr,
};

/// An MR with no pipeline waits for one only while it is fresh.
#[test]
fn a_pipeline_is_awaited_only_within_the_grace_period() {
    let now = time::OffsetDateTime::parse(
        "2026-11-12T18:40:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let _clock = crate::clock::set_for_test(now);
    let grace = super::NO_PIPELINE_GRACE;
    assert!(super::touched_within(
        Some("2026-11-12T18:35:36.152Z"),
        grace
    ));
    assert!(!super::touched_within(
        Some("2026-11-12T18:25:36.152Z"),
        grace
    ));
    assert!(
        super::touched_within(None, grace),
        "unknown age keeps waiting"
    );
    assert!(super::touched_within(Some("not a time"), grace));
}

#[test]
fn gitlab_ci_status_only_passed_on_explicit_success() {
    assert!(gitlab_ci_passed(Some("success")));
    assert!(!gitlab_ci_passed(Some("running")));
    assert!(!gitlab_ci_passed(Some("pending")));
    assert!(!gitlab_ci_passed(None));
}

#[test]
fn gitlab_ci_status_failed_on_failed_or_canceled() {
    assert!(gitlab_ci_failed(Some("failed")));
    assert!(gitlab_ci_failed(Some("canceled")));
    assert!(!gitlab_ci_failed(Some("success")));
    assert!(!gitlab_ci_failed(Some("running")));
    // Absent pipeline is "unknown", not "failed" -- matches the
    // pre-existing (if previously hardcoded) semantics.
    assert!(!gitlab_ci_failed(None));
}

#[test]
fn gitlab_ci_pending_on_missing_or_non_terminal_status() {
    // Issue #156: a missing or non-terminal pipeline must NOT collapse
    // into the same false/false bucket as a genuinely failed one -- it
    // needs its own distinct, observable classification.
    assert!(gitlab_ci_pending(None));
    assert!(gitlab_ci_pending(Some("running")));
    assert!(gitlab_ci_pending(Some("pending")));
    assert!(gitlab_ci_pending(Some("created")));
    assert!(gitlab_ci_pending(Some("skipped")));
    assert!(gitlab_ci_pending(Some("manual")));
    // Conclusive states are never "pending".
    assert!(!gitlab_ci_pending(Some("success")));
    assert!(!gitlab_ci_pending(Some("failed")));
    assert!(!gitlab_ci_pending(Some("canceled")));
    // The three states are mutually exclusive.
    let s = Some("running");
    assert!(gitlab_ci_pending(s) && !gitlab_ci_passed(s) && !gitlab_ci_failed(s));
}

fn check(conclusion: Option<&str>) -> GithubCheck {
    GithubCheck {
        conclusion: conclusion.map(str::to_string),
    }
}

#[test]
fn github_ci_passed_requires_at_least_one_check_all_terminal_and_green() {
    assert!(github_ci_passed(Some(&[check(Some("SUCCESS"))])));
    assert!(github_ci_passed(Some(&[
        check(Some("SUCCESS")),
        check(Some("SKIPPED"))
    ])));
    // No checks at all must not read as "passed" -- a repo with no CI
    // configured shouldn't silently qualify for auto-merge.
    assert!(!github_ci_passed(Some(&[])));
    // Still running (conclusion not yet set) is not passed.
    assert!(!github_ci_passed(Some(&[
        check(Some("SUCCESS")),
        check(None)
    ])));
    assert!(!github_ci_passed(Some(&[check(Some("FAILURE"))])));
}

#[test]
fn github_ci_failed_on_any_failure_conclusion() {
    assert!(github_ci_failed(Some(&[
        check(Some("SUCCESS")),
        check(Some("FAILURE"))
    ])));
    assert!(!github_ci_failed(Some(&[check(Some("SUCCESS"))])));
    assert!(!github_ci_failed(Some(&[check(None)])));
}

#[test]
fn github_status_check_rollup_handles_null_explicitly() {
    // Test that explicit null deserializes correctly
    let json_with_null = r#"{"title":"Test","headRefName":"gah/test","statusCheckRollup":null}"#;
    let pr: GithubPr = serde_json::from_str(json_with_null).unwrap();
    assert_eq!(pr.status_check_rollup, None);

    // Test that missing field deserializes correctly
    let json_without_field = r#"{"title":"Test","headRefName":"gah/test"}"#;
    let pr: GithubPr = serde_json::from_str(json_without_field).unwrap();
    assert_eq!(pr.status_check_rollup, None);

    // Test that populated array deserializes correctly
    let json_with_checks = r#"{"title":"Test","headRefName":"gah/test","statusCheckRollup":[{"conclusion":"SUCCESS"}]}"#;
    let pr: GithubPr = serde_json::from_str(json_with_checks).unwrap();
    assert!(pr.status_check_rollup.is_some());
    assert_eq!(pr.status_check_rollup.as_ref().unwrap().len(), 1);

    // Test that empty array deserializes correctly
    let json_with_empty = r#"{"title":"Test","headRefName":"gah/test","statusCheckRollup":[]}"#;
    let pr: GithubPr = serde_json::from_str(json_with_empty).unwrap();
    assert!(pr.status_check_rollup.is_some());
    assert_eq!(pr.status_check_rollup.as_ref().unwrap().len(), 0);

    let with_source = r#"{"title":"Test","headRefName":"gah/test","headRefOid":"abc123"}"#;
    let pr: GithubPr = serde_json::from_str(with_source).unwrap();
    assert_eq!(pr.head_ref_oid.as_deref(), Some("abc123"));
}

#[test]
fn github_merged_pr_uses_merge_commit_for_terminal_delivery() {
    let pr: GithubPr = serde_json::from_str(
        r#"{"title":"Test","headRefName":"gah/test","headRefOid":"head-sha","mergedAt":"2026-08-29T00:00:00Z","mergeCommit":{"oid":"merge-sha"}}"#,
    )
    .unwrap();

    assert_eq!(
        (pr.head_ref_oid.as_deref(), pr.merge_commit_sha().as_deref()),
        (Some("head-sha"), Some("merge-sha"))
    );
}

#[test]
fn gitlab_list_fixture_retains_source_sha_for_review_identity() {
    let mr: GitlabMr = serde_json::from_str(
        r#"{"title":"Test","source_branch":"gah/test","draft":true,"sha":"def456"}"#,
    )
    .unwrap();
    assert_eq!(mr.sha.as_deref(), Some("def456"));
    assert!(mr.draft);
}

#[test]
fn github_ci_functions_handle_none() {
    // Test that None (null or missing) is treated as empty for CI functions
    assert!(!github_ci_failed(None));
    assert!(!github_ci_passed(None));
}

fn base_mr() -> SyncMr {
    SyncMr {
        title: "x".into(),
        body: None,
        branch: "gah/test".into(),
        labels: vec![],
        url: None,
        id: None,
        state: Some("OPEN".into()),
        draft: false,
        source_sha: None,
        merge_commit_sha: None,
        merge_status: None,
        merged: false,
        updated_at: None,
        merged_at: None,
        ci_failed: false,
        ci_passed: false,
        ci_pending: false,
        work_id: None,
    }
}

fn set_review_identity(
    entry: &mut crate::ledger::LedgerEntry,
    mr: &SyncMr,
    fingerprint: Option<String>,
) {
    entry.review_source_sha = mr.source_sha.clone();
    entry.review_metadata_fingerprint = fingerprint;
    entry.review_contract_version = Some(crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION);
    entry.review_generation = crate::ledger::review_generation(
        entry.review_source_sha.as_deref(),
        entry.review_metadata_fingerprint.as_deref(),
    );
}

#[test]
fn work_id_extracted_from_authoritative_title() {
    assert_eq!(
        extract_work_id_from_title("[GAH] Fix: TICKET-093 Derive PR titles"),
        Some("TICKET-093".to_string())
    );
}

#[test]
fn work_id_extracted_from_native_issue_title() {
    assert_eq!(
        extract_work_id_from_title("[GAH] Fix: #319 Use native issue numbers"),
        Some("#319".to_string())
    );
}

#[test]
fn work_id_absent_when_title_has_no_ticket_token() {
    assert_eq!(extract_work_id_from_title("[GAH] Fix: gah-real-1234"), None);
}

#[test]
fn ready_label_maps_to_ready_for_human() {
    let mut mr = base_mr();
    mr.labels = vec!["gah-ready-for-human".into()];
    assert_eq!(classify(&mr), "READY_FOR_HUMAN");
}

#[test]
fn provisional_review_escalation_label_maps_to_needs_review() {
    let mut mr = base_mr();
    mr.labels = vec!["gah-review-escalating".into()];
    assert_eq!(classify(&mr), "NEEDS_REVIEW");
}

#[test]
fn sync_row_exposes_gate_reason_even_when_it_comes_from_review_entry() {
    let mut mr = base_mr();
    mr.source_sha = Some("source-sha".into());
    mr.work_id = Some("TICKET-295".into());
    let mut review = ledger_entry("review", "gah/test", Some("review"), Some(1));
    review.work_id = Some("TICKET-295".into());
    review.effective_backend = "claude".into();
    review.review_verdict = Some("HUMAN_REVIEW".into());
    review.review_gate_reason = Some("APPROVE omitted grounded evidence".into());
    set_review_identity(&mut review, &mr, Some(mr.review_metadata_fingerprint()));
    let mut ledger = crate::ledger::LedgerEntriesByWorkId::new();
    ledger.insert("TICKET-295".into(), vec![&review]);

    let row = super::sync_mr_to_json(&mr, Some("test".into()), &ledger);

    assert_eq!(row.review_verdict.as_deref(), Some("HUMAN_REVIEW"));
    assert_eq!(
        row.review_gate_reason.as_deref(),
        Some("APPROVE omitted grounded evidence")
    );
}

fn reviewed_needs_fix_row(
    fingerprint: Option<String>,
) -> (SyncMr, Vec<crate::ledger::LedgerEntry>) {
    let mut mr = base_mr();
    mr.work_id = Some("#701".into());
    mr.source_sha = Some("source-sha".into());
    mr.body = Some("corrected metadata".into());
    mr.labels = vec!["gah-needs-fix".into()];

    let mut review = ledger_entry("review", "gah/test", Some("review"), Some(1));
    review.work_id = Some("#701".into());
    review.review_verdict = Some("NEEDS_FIX".into());
    review.validation_result = Some("NEEDS_FIX".into());
    set_review_identity(&mut review, &mr, fingerprint);
    review.review_blocking_findings = vec!["MR description omitted the finding".into()];

    (mr, vec![review])
}

#[test]
fn metadata_only_correction_invalidates_needs_fix_and_schedules_review() {
    let old =
        review_metadata_fingerprint(Some("source-sha"), Some("x"), Some("old metadata"), false);
    let (mr, ledger) = reviewed_needs_fix_row(Some(old));

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "NEEDS_REVIEW");
    assert_eq!(row.recommended_action, RecommendedAction::RunReview);
    assert_eq!(row.review_verdict, None);
}

#[test]
fn unchanged_review_metadata_preserves_needs_fix() {
    let (mr, mut ledger) = reviewed_needs_fix_row(None);
    set_review_identity(&mut ledger[0], &mr, Some(mr.review_metadata_fingerprint()));

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "NEEDS_FIX");
    assert_eq!(row.recommended_action, RecommendedAction::ReuseBranch);
    assert_eq!(row.review_verdict.as_deref(), Some("NEEDS_FIX"));
}

#[test]
fn backfilled_implementation_review_identity_is_valid_review_evidence() {
    let (mr, mut ledger) = reviewed_needs_fix_row(None);
    let entry = &mut ledger[0];
    entry.mode = "fix".into();
    set_review_identity(entry, &mr, Some(mr.review_metadata_fingerprint()));

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "NEEDS_FIX");
    assert_eq!(row.review_verdict.as_deref(), Some("NEEDS_FIX"));
}

#[test]
fn stale_needs_fix_label_with_nonterminal_human_review_routes_to_review() {
    let (mr, mut ledger) = reviewed_needs_fix_row(None);
    let review = &mut ledger[0];
    review.review_verdict = Some("HUMAN_REVIEW".into());
    review.validation_result = Some("HUMAN_REVIEW".into());
    review.human_required = false;
    set_review_identity(review, &mr, Some(mr.review_metadata_fingerprint()));

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "NEEDS_REVIEW");
    assert_eq!(row.recommended_action, RecommendedAction::RunReview);
    assert_eq!(row.review_verdict.as_deref(), Some("HUMAN_REVIEW"));
}

#[test]
fn invalid_review_output_cannot_select_fixmr_even_with_stale_needs_fix_label() {
    let (mr, mut ledger) = reviewed_needs_fix_row(None);
    let review = &mut ledger[0];
    review.review_verdict = Some("REVIEW_OUTPUT_INVALID".into());
    review.validation_result = Some("review_output_invalid".into());
    review.review_gate_reason = Some("finding explicitly withdrew itself".into());
    review.review_blocking_findings.clear();
    set_review_identity(review, &mr, Some(mr.review_metadata_fingerprint()));

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "NEEDS_REVIEW");
    assert_eq!(row.recommended_action, RecommendedAction::RunReview);
    assert_eq!(row.review_verdict.as_deref(), Some("REVIEW_OUTPUT_INVALID"));
    assert_eq!(
        row.review_gate_reason.as_deref(),
        Some("finding explicitly withdrew itself")
    );
}

#[test]
fn terminal_human_review_hold_is_not_redispatched_or_auto_merged() {
    let (mr, mut ledger) = reviewed_needs_fix_row(None);
    let review = &mut ledger[0];
    review.review_verdict = Some("HUMAN_REVIEW".into());
    review.validation_result = Some("HUMAN_REVIEW".into());
    review.human_required = true;
    set_review_identity(review, &mr, Some(mr.review_metadata_fingerprint()));

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "READY_FOR_HUMAN");
    assert_eq!(
        row.recommended_action,
        RecommendedAction::HumanMergeDecision
    );
    assert_eq!(row.review_verdict.as_deref(), Some("HUMAN_REVIEW"));
}

#[test]
fn metadata_rereview_supersedes_old_backfilled_verdict_without_losing_attribution() {
    let (mut mr, mut ledger) = reviewed_needs_fix_row(None);
    mr.labels = vec!["gah-ready-for-human".into()];
    mr.url = Some("https://example.test/mr/7".into());

    let implementation = &mut ledger[0];
    implementation.mode = "fix".into();
    implementation.mr_url = mr.url.clone();
    implementation.effective_backend = "hy3".into();
    implementation.effective_model = Some("worker-model".into());

    let mut rereview = ledger_entry("review", "gah/test", Some("review"), Some(2));
    rereview.work_id = Some("#701".into());
    rereview.mr_url = mr.url.clone();
    rereview.review_verdict = Some("APPROVE".into());
    rereview.validation_result = Some("APPROVE".into());
    set_review_identity(&mut rereview, &mr, Some(mr.review_metadata_fingerprint()));
    ledger.push(rereview);

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "READY_FOR_HUMAN");
    assert_eq!(row.review_verdict.as_deref(), Some("APPROVE"));
    assert_eq!(row.effective_backend.as_deref(), Some("hy3"));
    assert_eq!(row.effective_model.as_deref(), Some("worker-model"));
}

#[test]
fn historical_review_without_metadata_identity_fails_safe_to_rereview() {
    let (mr, ledger) = reviewed_needs_fix_row(None);

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "NEEDS_REVIEW");
    assert_eq!(row.review_verdict, None);
}

#[test]
fn review_metadata_fingerprint_covers_source_title_body_and_draft_state() {
    let baseline = review_metadata_fingerprint(Some("sha"), Some("title"), Some("body"), false);
    assert_eq!(
        baseline,
        review_metadata_fingerprint(Some("sha"), Some("title"), Some("body"), false)
    );
    assert_ne!(
        baseline,
        review_metadata_fingerprint(Some("other"), Some("title"), Some("body"), false)
    );
    assert_ne!(
        baseline,
        review_metadata_fingerprint(Some("sha"), Some("other"), Some("body"), false)
    );
    assert_ne!(
        baseline,
        review_metadata_fingerprint(Some("sha"), Some("title"), Some("other"), false)
    );
    assert_ne!(
        baseline,
        review_metadata_fingerprint(Some("sha"), Some("title"), Some("body"), true)
    );
    assert_eq!(
        review_metadata_fingerprint(Some("sha"), Some("Draft: title"), Some("body"), true),
        review_metadata_fingerprint(Some("sha"), Some("title"), Some("body"), true)
    );
}

#[test]
fn gah_draft_to_ready_transition_preserves_the_applicable_approval() {
    let (mut mr, mut ledger) = reviewed_needs_fix_row(None);
    mr.title = "approved title".into();
    mr.draft = false;
    mr.labels = vec!["gah-ready-for-human".into()];
    let review = &mut ledger[0];
    review.review_verdict = Some("APPROVE".into());
    review.validation_result = Some("APPROVE".into());
    set_review_identity(
        review,
        &mr,
        Some(review_metadata_fingerprint(
            mr.source_sha.as_deref(),
            Some("Draft: approved title"),
            mr.body.as_deref(),
            true,
        )),
    );

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "READY_FOR_HUMAN");
    assert_eq!(row.review_verdict.as_deref(), Some("APPROVE"));
}

#[test]
fn redrafting_an_approved_mr_invalidates_the_old_review() {
    let (mut mr, mut ledger) = reviewed_needs_fix_row(None);
    mr.draft = true;
    let review = &mut ledger[0];
    review.review_verdict = Some("APPROVE".into());
    review.validation_result = Some("APPROVE".into());
    set_review_identity(
        review,
        &mr,
        Some(review_metadata_fingerprint(
            mr.source_sha.as_deref(),
            Some(&mr.title),
            mr.body.as_deref(),
            false,
        )),
    );

    let row = sync_mr_to_json(
        &mr,
        Some("test".into()),
        &crate::ledger::index_entries_by_work_id(&ledger),
    );

    assert_eq!(row.classification, "NEEDS_REVIEW");
    assert_eq!(row.review_verdict, None);
}

#[test]
fn closed_unmerged_takes_precedence_over_labels_and_other_state() {
    let mut mr = base_mr();
    mr.state = Some("closed".into());
    mr.draft = true;
    mr.merge_status = Some("DIRTY".into());
    mr.labels = vec!["gah-ready-for-human".into(), "gah-human-review".into()];
    mr.ci_failed = true;

    assert_eq!(classify(&mr), "CLOSED_UNMERGED");
    assert_eq!(recommended_action(classify(&mr)), "none");
}

#[test]
fn merged_still_wins_over_closed_state() {
    let mut mr = base_mr();
    mr.state = Some("closed".into());
    mr.merged = true;

    assert_eq!(classify(&mr), "MERGED");
}

// ===== Bug 1: count_fix_attempts_per_branch counting semantics =====
// These tests prove the retry cap counts ONLY actual post-review repair
// dispatches, not internal OpenHands retries or initial dispatches.

const TEST_REVIEW_GENERATION: &str = "review-v1:source-sha:metadata-fingerprint";

fn ledger_entry(
    mode: &str,
    branch: &str,
    dispatch_reason: Option<&str>,
    attempts_started: Option<u32>,
) -> crate::ledger::LedgerEntry {
    let tmp = tempfile::tempdir().unwrap();
    let prof = crate::config::Profile {
        delivery_mode: crate::config::DeliveryMode::default(),
        manager_wake_autonomy: crate::config::WakeAutonomy::default(),
        prune_older_than_days: None,
        chat_session_idle_days: None,
        display_name: "test".into(),
        repo_id: "test".into(),
        repo: "test".into(),
        provider: String::new(),
        local_path: tmp.path().display().to_string(),
        artifact_root: tmp.path().display().to_string(),
        default_target_branch: "main".into(),
        provider_api_base: None,
        provider_project_id: None,
        oh_profile: None,
        skill_resolve_failure_policy: Default::default(),
        openhands_args: vec![],
        codex_args: vec![],
        codex_path: None,
        claude_args: vec![],
        claude_path: None,
        agy_path: None,
        vibe_args: vec![],
        vibe_path: None,
        opencode_args: vec![],
        opencode_path: None,
        hermes_args: vec![],
        hermes_path: None,
        agy_second_home: None,
        agy_print_timeout_seconds: std::collections::HashMap::new(),
        agy_idle_timeout_seconds: None,
        opencode_idle_timeout_seconds: None,
        opencode_idle_timeout_seconds_by_model: std::collections::HashMap::new(),
        max_concurrent_per_model: std::collections::HashMap::new(),
        openhands_idle_timeout_seconds: None,
        vibe_idle_timeout_seconds: None,
        codex_idle_timeout_seconds: None,
        claude_idle_timeout_seconds: None,
        hermes_idle_timeout_seconds: None,
        max_parallel_workers: None,
        max_open_managed_mrs: None,
        worker_scaling: Default::default(),
        notify_command: None,
        policy_path: None,
        env_file: None,
        env_file_prod: None,
        validation_commands: vec![],
        auto_fix_commands: vec![],
        test_file_patterns: vec![],
        known_baseline_failure_markers: vec![],
        model_improve: None,
        model_pm: None,
        model_review: None,
        review_timeout_seconds: None,
        review_hard_timeout_seconds: None,
        validation_timeout_seconds: None,
        routing: crate::config::RoutingPolicy::default(),
        external_credential_scopes: std::collections::HashMap::new(),
        pacing: crate::quota::PacingConfig::default(),
        publishing: Default::default(),
    };
    let mut entry = crate::ledger::LedgerEntry::new("test", &prof, "codex", mode, "x", None, None);
    entry.branch = Some(branch.into());
    entry.attempts_started = attempts_started;
    entry.dispatch_reason = dispatch_reason.map(|s| s.into());
    entry.review_contract_version = Some(crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION);
    entry.review_generation = Some(TEST_REVIEW_GENERATION.into());
    entry
}

fn count_current_fixes(
    entries: &[crate::ledger::LedgerEntry],
) -> std::collections::HashMap<String, usize> {
    let mut mr = super::sync_mr_to_json(
        &base_mr(),
        Some("test".into()),
        &crate::ledger::LedgerEntriesByWorkId::new(),
    );
    mr.branch = "branch-A".into();
    mr.review_generation = Some(TEST_REVIEW_GENERATION.into());
    super::count_current_fix_attempts_for_mrs(entries, "test", "test", &[mr])
}

fn clear_entry(work_id: &str) -> crate::ledger::LedgerEntry {
    let mut entry = ledger_entry("clear_attempts", "", None, None);
    entry.branch = None;
    entry.work_id = Some(work_id.into());
    entry
}

/// Two internal OpenHands attempts inside one initial dispatch consume
/// zero post-review repair retries.
#[test]
fn internal_retries_in_initial_dispatch_count_zero() {
    let counts = count_current_fixes(&[ledger_entry("fix", "branch-A", Some("initial"), Some(2))]);
    assert_eq!(
        counts.get("branch-A").copied().unwrap_or(0),
        0,
        "internal retries in an initial dispatch must not consume repair budget"
    );
}

/// One actual post-review repair dispatch increments the count by exactly 1,
/// regardless of how many internal attempts it used.
#[test]
fn one_repair_dispatch_counts_one() {
    let counts = count_current_fixes(&[
        ledger_entry("fix", "branch-A", Some("initial"), Some(2)),
        ledger_entry("fix", "branch-A", Some("post_review_repair"), Some(3)),
    ]);
    assert_eq!(
        counts.get("branch-A").copied().unwrap_or(0),
        1,
        "one post_review_repair entry = count 1, not attempts_started"
    );
}

/// Internal retries within a FixMr (post_review_repair) dispatch do not
/// inflate the repair-cycle count — it increments exactly once per entry.
#[test]
fn internal_retries_in_repair_dispatch_do_not_inflate() {
    let counts = count_current_fixes(&[ledger_entry(
        "fix",
        "branch-A",
        Some("post_review_repair"),
        Some(5),
    )]);
    assert_eq!(
        counts.get("branch-A").copied().unwrap_or(0),
        1,
        "attempts_started=5 within one repair dispatch must count as 1 repair cycle"
    );
}

/// Retry cap triggers only after the configured number of actual
/// post-review repair cycles (AUTO_RETRY_CAP=2).
#[test]
fn two_repair_dispatches_hit_cap() {
    let counts = count_current_fixes(&[
        ledger_entry("fix", "branch-A", Some("initial"), Some(2)),
        ledger_entry("fix", "branch-A", Some("post_review_repair"), Some(1)),
        ledger_entry("fix", "branch-A", Some("post_review_repair"), Some(1)),
    ]);
    assert_eq!(
        counts.get("branch-A").copied().unwrap_or(0),
        2,
        "two post_review_repair entries = count 2 = AUTO_RETRY_CAP"
    );
}

#[test]
fn clear_attempts_resets_repair_cycles_for_matching_work_item() {
    let mut before_one = ledger_entry("fix", "branch-A", Some("post_review_repair"), Some(1));
    before_one.work_id = Some("branch-A".into());
    let mut before_two = before_one.clone();
    before_two.timestamp = "2026-01-02T00:00:00Z".into();
    let mut attributed_review = ledger_entry("review", "branch-A", Some("review"), Some(1));
    attributed_review.work_id = Some("#437".into());
    attributed_review.timestamp = "2026-01-02T12:00:00Z".into();
    let mut clear = clear_entry("TICKET-437");
    clear.timestamp = "2026-01-03T00:00:00Z".into();
    let mut after = before_one.clone();
    after.work_id = Some("#437".into());
    after.timestamp = "2026-01-04T00:00:00Z".into();

    let counts = count_current_fixes(&[before_one, before_two, attributed_review, clear, after]);
    assert_eq!(counts.get("branch-A"), Some(&1));
}

#[test]
fn clear_attempts_is_scoped_and_resets_merge_retry_budget() {
    let mut before = ledger_entry("merge", "branch-A", None, Some(2));
    before.work_id = Some("#437".into());
    let mut foreign_clear = clear_entry("#437");
    foreign_clear.profile = "other-profile".into();
    let matching_clear = clear_entry("#437");
    let mut after = before.clone();
    after.attempts_started = Some(1);

    let entries = [before, foreign_clear, matching_clear, after];
    let counts = super::count_merge_attempts_per_branch_for_scope(&entries, "test", "test");
    assert_eq!(counts.get("branch-A"), Some(&1));
    assert!(
        super::count_merge_attempts_per_branch_for_scope(&entries, "other-profile", "test")
            .is_empty()
    );
}

/// Entries with mode != "fix" (review, merge) never count.
#[test]
fn review_and_merge_entries_never_count() {
    let counts = count_current_fixes(&[
        ledger_entry("review", "branch-A", Some("review"), Some(1)),
        ledger_entry("merge", "branch-A", None, Some(1)),
    ]);
    assert!(
        counts.is_empty(),
        "review/merge entries must not count toward fix retry cap"
    );
}

/// Pre-existing ledger entries without dispatch_reason (legacy) must not
/// count — they cannot be proven to be post-review repairs.
#[test]
fn legacy_entries_without_dispatch_reason_do_not_count() {
    let counts = count_current_fixes(&[ledger_entry("fix", "branch-A", None, Some(2))]);
    assert_eq!(
        counts.get("branch-A").copied().unwrap_or(0),
        0,
        "legacy entries without dispatch_reason must not count (unprovable)"
    );
}

#[test]
fn pre_bump_post_review_repair_entries_do_not_count_toward_fix_retry_cap() {
    let mut old_entry = ledger_entry("fix", "branch-A", Some("post_review_repair"), Some(1));
    old_entry.review_contract_version = None; // Pre-bump
    let counts = count_current_fixes(&[old_entry]);
    assert_eq!(
        counts.get("branch-A").copied().unwrap_or(0),
        0,
        "pre-bump post_review_repair entries must not count under new contract"
    );
}

#[test]
fn stale_review_generation_does_not_consume_current_fix_budget() {
    let mut stale = ledger_entry("fix", "branch-A", Some("post_review_repair"), Some(1));
    stale.review_generation = Some("review-v1:old-source:old-metadata".into());
    let current = ledger_entry("fix", "branch-A", Some("post_review_repair"), Some(4));

    let counts = count_current_fixes(&[stale, current]);
    assert_eq!(counts.get("branch-A"), Some(&1));
}
