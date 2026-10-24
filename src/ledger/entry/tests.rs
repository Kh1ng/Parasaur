use super::*;
use crate::config::{Profile, RoutingPolicy};
use std::collections::HashMap;

fn profile() -> Profile {
    Profile {
        delivery_mode: crate::config::DeliveryMode::default(),
        manager_wake_autonomy: crate::config::WakeAutonomy::default(),
        prune_older_than_days: None,
        chat_session_idle_days: None,
        display_name: "Repo".into(),
        repo_id: "repo".into(),
        provider: "github".into(),
        repo: "owner/repo".into(),
        local_path: "/tmp/repo".into(),
        artifact_root: "/tmp/artifacts".into(),
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
        opencode_idle_timeout_seconds_by_model: HashMap::new(),
        max_concurrent_per_model: HashMap::new(),
        openhands_idle_timeout_seconds: None,
        vibe_idle_timeout_seconds: None,
        codex_idle_timeout_seconds: None,
        claude_idle_timeout_seconds: None,
        hermes_idle_timeout_seconds: None,
        max_parallel_workers: None,
        max_open_managed_mrs: None,
        worker_scaling: Default::default(),
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
        notify_command: None,
        routing: RoutingPolicy::default(),
        external_credential_scopes: std::collections::HashMap::new(),
        pacing: Default::default(),
        publishing: Default::default(),
    }
}

#[test]
fn target_summary_is_trimmed_to_first_line() {
    let entry = LedgerEntry::new(
        "test",
        &profile(),
        "claude",
        "pm",
        "first line\nsecond line",
        Some("123".into()),
        None,
    );
    assert_eq!(entry.target_summary.as_deref(), Some("first line"));
}

#[test]
fn routing_diagnostics_round_trip_through_json() {
    let mut entry = LedgerEntry::new("test", &profile(), "claude", "pm", "x", None, None);
    entry.routing_diagnostics = Some(RoutingDiagnostics {
        policy_reordered_candidates: true,
        selected_backend: Some("codex".into()),
        selected_model: Some("gpt-5.4".into()),
        selected_quota_pool: Some("codex-main".into()),
        selected_pace_band: Some("aggressive_burn".into()),
        selected_cost_class: Some("included_quota".into()),
        selected_over: vec!["openhands/gpt-5.4 (paid $0.2500)".into()],
        candidates: vec![RoutingCandidateDiagnostic {
            backend_instance: None,
            backend: "codex".into(),
            model: Some("gpt-5.4".into()),
            quota_pool: Some("codex-main".into()),
            default_order: Some(1),
            consideration_order: Some(0),
            pace_band: Some("aggressive_burn".into()),
            cost_class: Some("included_quota".into()),
            skip_reason: None,
            unavailable_until: None,
            ..Default::default()
        }],
        human_summary: Some("selected codex/gpt-5.4".into()),
        ..Default::default()
    });
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: LedgerEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed
            .routing_diagnostics
            .as_ref()
            .unwrap()
            .selected_backend
            .as_deref(),
        Some("codex")
    );
}

#[test]
fn persistence_derives_entry_route_summary_from_attempt_identities() {
    let mut entry = LedgerEntry::new("test", &profile(), "auto", "fix", "x", None, None);
    entry.attempt_routing = vec![
        AttemptRoutingRecord {
            skills: Vec::new(),
            attempt_number: 1,
            backend_instance: "codex:primary".into(),
            effective_model: Some("gpt-5".into()),
            identity: Some(crate::execution_identity::ExecutionIdentity::legacy_route(
                "auto",
                Some("requested-model"),
                "codex",
                Some("gpt-5"),
                Some("primary"),
            )),
            routing_diagnostics: None,
        },
        AttemptRoutingRecord {
            skills: Vec::new(),
            attempt_number: 2,
            backend_instance: "claude:secondary".into(),
            effective_model: Some("sonnet".into()),
            identity: Some(crate::execution_identity::ExecutionIdentity::legacy_route(
                "auto",
                Some("requested-model"),
                "claude",
                Some("sonnet"),
                Some("secondary"),
            )),
            routing_diagnostics: None,
        },
    ];

    let normalized = entry.normalized_for_persistence();

    assert_eq!(normalized.requested_backend, "auto");
    assert_eq!(
        normalized.requested_model.as_deref(),
        Some("requested-model")
    );
    assert_eq!(normalized.effective_backend, "claude");
    assert_eq!(normalized.backend, "claude");
    assert_eq!(normalized.effective_model.as_deref(), Some("sonnet"));
}

#[test]
fn historical_attempt_routing_keeps_identity_unknown() {
    let raw = r#"{"attempt_number":1,"backend_instance":"codex","effective_model":"gpt-5","routing_diagnostics":null}"#;
    let record: AttemptRoutingRecord = serde_json::from_str(raw).unwrap();
    assert_eq!(record.identity, None);
}

#[test]
fn new_entry_has_no_failure_attribution_by_default() {
    let entry = LedgerEntry::new("test", &profile(), "claude", "pm", "x", None, None);
    assert_eq!(entry.failure_class, None);
    assert_eq!(entry.failure_stage, None);
    assert_eq!(entry.work_id, None);
    assert_eq!(entry.work_title, None);
}

#[test]
fn set_failure_populates_both_fields_as_lowercase_strings() {
    let mut entry = LedgerEntry::new("test", &profile(), "claude", "pm", "x", None, None);
    entry.set_failure(FailureClass::BackendError, FailureStage::AgentRun);
    assert_eq!(entry.failure_class.as_deref(), Some("backend_error"));
    assert_eq!(entry.failure_stage.as_deref(), Some("agent_run"));
}

#[test]
fn failure_attribution_round_trips_through_json() {
    let mut entry = LedgerEntry::new("test", &profile(), "claude", "pm", "x", None, None);
    entry.set_failure(FailureClass::AgentNoProgress, FailureStage::PostValidation);
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"failure_class\":\"agent_no_progress\""));
    assert!(json.contains("\"failure_stage\":\"post_validation\""));
    let parsed: LedgerEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.failure_class.as_deref(), Some("agent_no_progress"));
    assert_eq!(parsed.failure_stage.as_deref(), Some("post_validation"));
}

/// TICKET-063 requirement: existing historical JSONL entries — written
/// before failure_class/failure_stage existed — must still deserialize.
/// This is the exact fixture line used in
/// tests/gah_cli.rs::ledger_summary_reports_recent_counts, which has no
/// failure_class/failure_stage keys at all.
#[test]
fn pre_existing_ledger_line_without_failure_fields_still_deserializes() {
    let old_line = "{\"timestamp\":\"2099-01-01T00:00:00Z\",\"session_id\":\"1\",\"profile\":\"real\",\"display_name\":\"Real\",\"repo_id\":\"real\",\"repo\":\"owner/real\",\"local_path\":\"/tmp/repo\",\"provider\":\"github\",\"backend\":\"claude\",\"requested_backend\":\"claude\",\"effective_backend\":\"claude\",\"requested_model\":null,\"effective_model\":null,\"routing_reason\":\"explicit\",\"fallback_used\":false,\"confidence_impact\":null,\"human_required\":false,\"mode\":\"pm\",\"target_summary\":\"x\",\"branch\":null,\"session_dir\":null,\"duration_seconds\":1.0,\"backend_exit_code\":0,\"validation_result\":\"not_run\",\"commit_attempted\":false,\"commit_created\":false,\"push_attempted\":false,\"push_succeeded\":false,\"mr_attempted\":false,\"mr_created\":false,\"mr_url\":null,\"files_changed\":null,\"insertions\":null,\"deletions\":null,\"error_summary\":null,\"usage\":{\"input_tokens\":null,\"output_tokens\":null,\"total_tokens\":null,\"estimated_cost_usd\":null,\"usage_source\":null}}";
    let parsed: LedgerEntry = serde_json::from_str(old_line).unwrap();
    assert_eq!(parsed.failure_class, None);
    assert_eq!(parsed.failure_stage, None);
    assert_eq!(parsed.routing_diagnostics, None);
    assert_eq!(parsed.work_id, None);
    assert_eq!(parsed.work_title, None);
    assert!(parsed.review_blocking_findings.is_empty());
    assert!(parsed.review_non_blocking_findings.is_empty());
    assert!(parsed.review_risk_notes.is_empty());
    assert!(parsed.review_evidence.is_empty());
    assert!(parsed.review_compatibility_evidence.is_empty());
    assert_eq!(parsed.profile, "real");
}

#[test]
fn legacy_ledger_line_deserializes_attempts_as_unknown() {
    let raw = r#"{
            "timestamp": "2026-07-01T00:00:00Z",
            "profile": "test",
            "display_name": "Repo",
            "repo_id": "repo",
            "repo": "owner/repo",
            "local_path": "/tmp/repo",
            "provider": "github",
            "backend": "codex",
            "requested_backend": "codex",
            "effective_backend": "codex",
            "mode": "fix",
            "commit_attempted": false,
            "commit_created": false,
            "push_attempted": false,
            "push_succeeded": false,
            "mr_attempted": false,
            "mr_created": false,
            "fallback_used": false,
            "human_required": false,
            "attempts": [],
            "usage": {}
        }"#;
    let entry: LedgerEntry = serde_json::from_str(raw).unwrap();
    assert_eq!(
        entry.schema_version, 1,
        "legacy entries default to schema v1"
    );
    assert_eq!(
        entry.attempts_started, None,
        "pre-tracking attempts_started must be unknown, not 0"
    );
    assert_eq!(entry.attempts_completed, None);
}

/// Issue #240: a v1 fixture that explicitly carried `0` attempt counters
/// (i.e. the field was present) must round-trip as a *known* zero —
/// distinct from the unknown (`None`) case above. This guards against the
/// "convert unknown to zero" mistake.
#[test]
fn v1_ledger_line_with_zero_attempts_stays_known_zero() {
    let raw = r#"{
            "schema_version": 1,
            "timestamp": "2026-07-01T00:00:00Z",
            "profile": "test",
            "display_name": "Repo",
            "repo_id": "repo",
            "repo": "owner/repo",
            "local_path": "/tmp/repo",
            "provider": "github",
            "backend": "codex",
            "requested_backend": "codex",
            "effective_backend": "codex",
            "mode": "fix",
            "commit_attempted": false,
            "commit_created": false,
            "push_attempted": false,
            "push_succeeded": false,
            "mr_attempted": false,
            "mr_created": false,
            "fallback_used": false,
            "human_required": false,
            "attempts_started": 0,
            "attempts_completed": 0,
            "attempts": [],
            "usage": {}
        }"#;
    let entry: LedgerEntry = serde_json::from_str(raw).unwrap();
    assert_eq!(entry.schema_version, 1);
    assert_eq!(
        entry.attempts_started,
        Some(0),
        "explicit 0 is a known zero"
    );
    assert_eq!(entry.attempts_completed, Some(0));
}

/// Issue #240 acceptance #1: every newly constructed entry carries the
/// current `LEDGER_SCHEMA_VERSION`.
#[test]
fn new_entries_carry_current_schema_version() {
    let prof = profile();
    let entry = LedgerEntry::new("test", &prof, "codex", "improve", "t", None, None);
    assert_eq!(entry.schema_version, super::LEDGER_SCHEMA_VERSION);
    assert_eq!(entry.attempts_started, Some(0));
    assert_eq!(entry.attempts_completed, Some(0));

    let claim = LedgerEntry::new_claim("test", &prof, "TICKET-1");
    assert_eq!(claim.schema_version, super::LEDGER_SCHEMA_VERSION);
    let hold = LedgerEntry::new_review_hold("test", &prof, "TICKET-1", None);
    assert_eq!(hold.schema_version, super::LEDGER_SCHEMA_VERSION);
    let clear = LedgerEntry::new_clear_attempts("test", &prof, "TICKET-1");
    assert_eq!(clear.schema_version, super::LEDGER_SCHEMA_VERSION);
}

// Issue #95: new_clear_attempts creates a valid tombstone entry
#[test]
fn new_clear_attempts_creates_valid_tombstone() {
    let prof = profile();
    let entry = LedgerEntry::new_clear_attempts("test-profile", &prof, "TICKET-99");
    assert_eq!(entry.mode, "clear_attempts");
    assert_eq!(entry.work_id.as_deref(), Some("TICKET-99"));
    assert_eq!(entry.profile, "test-profile");
    assert_eq!(entry.repo_id, prof.repo_id);
    assert!(!entry.human_required);
    assert_eq!(entry.failure_class, None);
    assert_eq!(entry.attempts_started, Some(0));
    // Must serialize/deserialize cleanly (JSONL round-trip)
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: LedgerEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.mode, "clear_attempts");
    assert_eq!(parsed.work_id.as_deref(), Some("TICKET-99"));
}

#[test]
fn new_claim_creates_valid_claim_entry() {
    let prof = profile();
    let entry = LedgerEntry::new_claim("test-profile", &prof, "TICKET-500");
    assert_eq!(entry.mode, "claim");
    assert_eq!(entry.work_id.as_deref(), Some("TICKET-500"));
    assert_eq!(entry.profile, "test-profile");
    assert!(!entry.human_required);
    assert_eq!(entry.branch, None);
    assert_eq!(entry.mr_url, None);
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: LedgerEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.mode, "claim");
    assert_eq!(parsed.work_id.as_deref(), Some("TICKET-500"));
}

#[test]
fn new_review_hold_creates_valid_hold_entry() {
    let prof = profile();
    let entry = LedgerEntry::new_review_hold(
        "test-profile",
        &prof,
        "TICKET-600",
        Some("reviewing PR #204".into()),
    );
    assert_eq!(entry.mode, "review_hold");
    assert_eq!(entry.work_id.as_deref(), Some("TICKET-600"));
    assert_eq!(entry.target_summary.as_deref(), Some("reviewing PR #204"));
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: LedgerEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.mode, "review_hold");
}
