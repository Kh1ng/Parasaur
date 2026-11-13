use super::*;
mod review_budget;
use crate::availability::{AvailabilityRecord, AvailabilityState, Reason, Source, Status};
use std::fs;
use tempfile::TempDir;

fn make_test_cfg(tmp: &TempDir) -> GahConfig {
    let path = tmp.path().join("cfg.toml");
    fs::write(
        &path,
        r#"
[profiles.test]
display_name = "Test"
repo_id = "test/test"
provider = "github"
repo = "test/test"
local_path = "/tmp"
artifact_root = "/tmp"
default_target_branch = "main"
"#,
    )
    .unwrap();
    let mut cfg = crate::config::load(Some(path.to_str().unwrap())).unwrap();
    cfg.defaults.artifact_root = tmp.path().to_string_lossy().into_owned();
    cfg
}

fn test_mr(work_id: Option<&str>, branch: &str) -> sync::SyncMrJson {
    sync::SyncMrJson {
        profile: Some("test".into()),
        branch: branch.into(),
        work_id: work_id.map(str::to_string),
        id: Some("1".into()),
        url: Some("https://tracker.example/mr/1".into()),
        state: Some("open".into()),
        draft: true,
        merge_status: None,
        merged: false,
        ci_passed: true,
        title: Some("test MR".into()),
        merged_at: None,
        effective_backend: None,
        effective_model: None,
        review_verdict: None,
        review_gate_reason: None,
        merge_review_block_reason: None,
        last_reviewed_head: None,
        source_sha: None,
        merge_commit_sha: None,
        review_contract_version: ledger::REVIEW_CONTRACT_VERSION,
        review_generation: None,
        review_generation_status: None,
        ci_pending: false,
        classification: "NEEDS_REVIEW".into(),
        recommended_action: sync::RecommendedAction::RunReview,
    }
}

#[test]
fn repo_policy_hold_survives_route_recovery_and_review_generation_changes() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = make_test_cfg(&tmp);
    let profile = cfg.profiles.get_mut("test").unwrap();
    profile.claude_path = Some("/bin/sh".into());
    profile.routing = toml::from_str(
        "review_candidates = [{ backend = 'claude', model = 'sonnet', included_in_quota = true }]",
    )
    .unwrap();
    let _availability =
        crate::test_support::AvailabilityEnvGuard::set(tmp.path().join("availability.json"));
    let profile = &cfg.profiles["test"];
    for mode in ["review", "merge", "mark_ready_for_review", "fix"] {
        let mut entry = LedgerEntry::new("test", profile, "none", mode, "#1544", None, None);
        entry.work_id = Some("#1544".into());
        let error = anyhow::anyhow!("POLICY BLOCKED");
        crate::policy::stamp_refusal(&mut entry, &error);
        let gate = ledger::effective_human_gate_from_entries(
            std::slice::from_ref(&entry),
            "test",
            &profile.repo_id,
            "#1544",
        )
        .unwrap();
        assert!(execution_gate_still_required(
            &cfg,
            "test",
            profile,
            std::slice::from_ref(&entry),
            "#1544",
            &gate
        ));
        if mode == "review" {
            let mut route_gate = gate.clone();
            route_gate.dispatch_reason = Some("review".into());
            assert!(
                !execution_gate_still_required(&cfg, "test", profile, &[], "#1544", &route_gate,),
                "the recovered route would release an ordinary approval hold"
            );
        }
        let mut mr = test_mr(Some("#1544"), "gah/1544");
        mr.review_generation = Some("a-new-generation".into());
        let mut blockers = vec![];
        project_effective_mr_gates(&cfg, "test", profile, &[entry], &[mr], &mut blockers);
        assert_eq!(blockers.len(), 1, "{mode}");
        assert_eq!(blockers[0].reason_code.as_deref(), Some("policy_approval"));
    }
}

#[test]
fn repeated_setup_status_blocker_only_holds_ticket_selection() {
    use crate::test_support::{AvailabilityEnvGuard, ClaimStateEnvGuard, ExecGuard, PathGuard};
    use std::os::unix::fs::PermissionsExt;

    let _exec_guard = ExecGuard::new();
    let tmp = TempDir::new().unwrap();
    let mut cfg = make_test_cfg(&tmp);
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    fs::write(
        repo.join("docs/tickets/TICKET-1462-test.md"),
        "# TICKET-1462: Test\n\nGoal: test\n",
    )
    .unwrap();
    cfg.profiles.get_mut("test").unwrap().local_path = repo.display().to_string();
    let profile = &cfg.profiles["test"];
    let mut entry = LedgerEntry::new("test", profile, "auto", "fix", "TICKET-1462", None, None);
    entry.work_id = Some("TICKET-1462".into());
    entry.branch = Some("gah/fix-1462".into());
    entry.set_failure(
        ledger::FailureClass::HarnessError,
        ledger::FailureStage::Dispatch,
    );
    entry.error_summary = Some("setup failed".into());
    for _ in 0..3 {
        ledger::append(&cfg, &entry).unwrap();
    }
    let bin = tmp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    let _path = PathGuard::set(&bin);
    let _availability = AvailabilityEnvGuard::set(tmp.path().join("availability.json"));
    let _claims = ClaimStateEnvGuard::set(tmp.path().join("claims.json"));
    for active_mr in [false, true] {
        let prs = if active_mr {
            r#"[{"number":1,"title":"TICKET-1462: Test","html_url":"https://example/pr/1","state":"open","draft":true,"head":{"ref":"gah/fix-1462"}}]"#
        } else {
            "[]"
        };
        fs::write(
            &gh,
            format!(
                "#!/bin/sh\ncase \"$*\" in\n  *'/pulls?'*) printf '%s\\n' '{prs}' ;;\n  *) printf '%s\\n' '[]' ;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let snapshot =
            super::super::build_snapshot(&cfg, "test", time::OffsetDateTime::now_utc()).unwrap();
        assert_eq!(snapshot.observations.sync.status, "ok");
        let ticket = snapshot
            .available_tickets
            .iter()
            .find(|ticket| ticket.work_id.as_deref() == Some("TICKET-1462"))
            .expect("ticket must be scanned");
        assert!(ticket.human_required);
        assert_eq!(ticket.has_active_mr, active_mr);
        assert_eq!(
            snapshot.blocked_work_items.iter().any(|blocker| {
                blocker.reason_code.as_deref() == Some("repeated_setup_failure")
            }),
            !active_mr,
        );
    }
}

#[test]
fn repeated_setup_blocker_gets_exact_derived_reason() {
    let tmp = TempDir::new().unwrap();
    let cfg = make_test_cfg(&tmp);
    let profile = &cfg.profiles["test"];
    let mut entry = LedgerEntry::new("test", profile, "auto", "fix", "#1462", None, None);
    entry.work_id = Some("#1462".into());
    entry.failure_class = Some("harness_error".into());
    entry.failure_stage = Some("dispatch".into());
    entry.error_summary = Some("insufficient free space: 9 GiB available".into());
    let mut blockers = vec![Blocker {
        kind: "human_required".into(),
        reason: Some("repeated_setup_failure".into()),
        message: Some("Ledger indicates human intervention required".into()),
        backend: None,
        model: None,
        quota_pool: None,
        until: None,
        source_reference: Some("#1462".into()),
        reason_code: Some("repeated_setup_failure".into()),
        remediation_plan: None,
    }];
    project_effective_mr_gates(
        &cfg,
        "test",
        profile,
        &[entry.clone(), entry.clone(), entry],
        &[],
        &mut blockers,
    );
    assert_eq!(blockers.len(), 1);
    assert_eq!(
        blockers[0].reason_code.as_deref(),
        Some("repeated_setup_failure")
    );
    assert_eq!(blockers[0].message.as_deref(), Some("the same setup failure happened 3 times in a row: insufficient free space: 9 GiB available"));
}

#[test]
fn open_mr_gate_is_projected_without_a_dispatchable_source_ticket_for_both_providers() {
    for provider in ["github", "gitlab"] {
        let tmp = TempDir::new().unwrap();
        let mut cfg = make_test_cfg(&tmp);
        cfg.profiles.get_mut("test").unwrap().provider = provider.into();
        let profile = &cfg.profiles["test"];
        let mut gate = LedgerEntry::new("test", profile, "auto", "fix", "#639", None, None);
        gate.work_id = Some("#639".into());
        gate.branch = Some("gah/fix-639".into());
        gate.human_required = true;
        gate.human_required_reason_code = Some("stuck_loop_gate".into());
        gate.dispatch_reason = Some("stuck_loop_gate".into());

        let mut blockers = Vec::new();
        project_effective_mr_gates(
            &cfg,
            "test",
            profile,
            &[gate],
            &[test_mr(None, "gah/fix-639")],
            &mut blockers,
        );

        assert_eq!(blockers.len(), 1, "provider={provider}");
        assert_eq!(blockers[0].source_reference.as_deref(), Some("#639"));
        assert_eq!(blockers[0].reason_code.as_deref(), Some("stuck_loop_gate"));
    }
}

#[test]
fn mr_gate_projection_is_idempotent_when_ticket_projection_already_added_it() {
    let tmp = TempDir::new().unwrap();
    let cfg = make_test_cfg(&tmp);
    let profile = &cfg.profiles["test"];
    let mut gate = LedgerEntry::new("test", profile, "auto", "fix", "#639", None, None);
    gate.work_id = Some("#639".into());
    gate.human_required = true;
    gate.human_required_reason_code = Some("stuck_loop_gate".into());
    gate.error_summary = Some("exact durable gate detail".into());
    let mut blockers = vec![Blocker {
        kind: "human_required".into(),
        reason: Some("stuck_loop_gate".into()),
        message: None,
        backend: None,
        model: None,
        quota_pool: None,
        until: None,
        source_reference: Some("#639".into()),
        reason_code: Some("stuck_loop_gate".into()),
        remediation_plan: None,
    }];

    project_effective_mr_gates(
        &cfg,
        "test",
        profile,
        &[gate],
        &[test_mr(Some("#639"), "gah/fix-639")],
        &mut blockers,
    );

    assert_eq!(blockers.len(), 1);
    assert_eq!(
        blockers[0].message.as_deref(),
        Some("exact durable gate detail")
    );
}

#[test]
fn paid_policy_gate_releases_when_a_subscription_route_recovers() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("cfg.toml");
    fs::write(
        &path,
        format!(
            r#"
[defaults]
artifact_root = "{}"

[profiles.test]
display_name = "Test"
repo_id = "test/test"
provider = "github"
repo = "test/test"
local_path = "/tmp"
artifact_root = "{}"
default_target_branch = "main"
claude_path = "/bin/sh"
opencode_path = "/bin/sh"

[[profiles.test.routing.review_candidates]]
backend = "claude"
model = "sonnet"
priority = 0
included_in_quota = true

[[profiles.test.routing.review_candidates]]
backend = "opencode"
model = "nous/glm-5.2"
priority = 10
requires_approval = true
"#,
            tmp.path().display(),
            tmp.path().display()
        ),
    )
    .unwrap();
    let cfg = crate::config::load(Some(path.to_str().unwrap())).unwrap();
    let profile = &cfg.profiles["test"];
    let gate = ledger::EffectiveHumanGate {
        reason_code: Some("policy_approval".into()),
        dispatch_reason: Some("review".into()),
        message: None,
        mode: "review".into(),
        timestamp: "2026-07-16T00:00:00Z".into(),
        routing_diagnostics: None,
        review_contract_version: Some(crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION),
        review_generation: None,
    };
    let availability_path = tmp.path().join("availability.json");
    let _availability_guard = crate::test_support::AvailabilityEnvGuard::set(&availability_path);

    assert!(!execution_gate_still_required(
        &cfg,
        "test",
        profile,
        &[],
        "#650",
        &gate,
    ));

    let mut used_claude = LedgerEntry::new("test", profile, "claude", "review", "#650", None, None);
    used_claude.work_id = Some("#650".into());
    used_claude.effective_backend = "claude".into();
    used_claude.effective_model = Some("sonnet".into());
    used_claude.review_verdict = Some("HUMAN_REVIEW".into());
    used_claude.review_contract_version = Some(ledger::REVIEW_CONTRACT_VERSION);
    let unavailable = AvailabilityState {
        version: 1,
        records: vec![AvailabilityRecord {
            backend: "claude".into(),
            backend_instance: None,
            model: Some("sonnet".into()),
            quota_pool: None,
            status: Status::Unavailable,
            reason: Reason::QuotaExhausted,
            observed_at: "2026-07-16T00:00:00Z".into(),
            unavailable_until: Some("2099-01-01T00:00:00Z".into()),
            source: Source::BackendError,
            last_error_summary: None,
        }],
    };
    fs::write(
        &availability_path,
        serde_json::to_string(&unavailable).unwrap(),
    )
    .unwrap();

    assert!(execution_gate_still_required(
        &cfg,
        "test",
        profile,
        &[],
        "#650",
        &gate,
    ));

    let wrong_grant = LedgerEntry::new_paid_route_approval(
        "test",
        profile,
        "#650",
        "opencode",
        Some("nous/other-model"),
        true,
    );
    assert!(execution_gate_still_required(
        &cfg,
        "test",
        profile,
        &[used_claude.clone(), wrong_grant],
        "#650",
        &gate,
    ));

    let exact_grant = LedgerEntry::new_paid_route_approval(
        "test",
        profile,
        "#650",
        "opencode",
        Some("nous/glm-5.2"),
        true,
    );
    assert!(!execution_gate_still_required(
        &cfg,
        "test",
        profile,
        &[used_claude.clone(), exact_grant],
        "#650",
        &gate,
    ));

    fs::write(
        &availability_path,
        serde_json::to_string(&AvailabilityState {
            version: 1,
            records: vec![],
        })
        .unwrap(),
    )
    .unwrap();
    let mut policy_entry = LedgerEntry::new("test", profile, "auto", "review", "#650", None, None);
    policy_entry.work_id = Some("#650".into());
    policy_entry.human_required = true;
    policy_entry.human_required_reason_code = Some("policy_approval".into());
    policy_entry.review_contract_version = Some(ledger::REVIEW_CONTRACT_VERSION);
    policy_entry.set_failure(
        crate::ledger::FailureClass::HumanBlocked,
        crate::ledger::FailureStage::Route,
    );
    policy_entry.routing_diagnostics = Some(crate::ledger::RoutingDiagnostics {
        candidates: vec![
            crate::ledger::RoutingCandidateDiagnostic {
                backend: "opencode".into(),
                model: Some("nous/glm-5.2".into()),
                skip_reason: Some("operator_approval_required".into()),
                ..Default::default()
            },
            crate::ledger::RoutingCandidateDiagnostic {
                backend: "claude".into(),
                model: Some("sonnet".into()),
                skip_reason: Some("model-specific quota_exhausted".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    let mut duplicate = LedgerEntry::new("test", profile, "auto", "review", "#650", None, None);
    duplicate.work_id = Some("#650".into());
    duplicate.validation_result = Some("skipped_duplicate_review".into());
    duplicate.review_source_sha = Some("same-sha".into());
    duplicate.reviewer_class = Some("escalatory:claude/sonnet".into());
    duplicate.review_contract_version = Some(ledger::REVIEW_CONTRACT_VERSION);
    let entries = vec![used_claude, policy_entry, duplicate];
    let mut blockers = Vec::new();
    project_effective_mr_gates(
        &cfg,
        "test",
        profile,
        &entries,
        &[test_mr(Some("#650"), "gah/review-650")],
        &mut blockers,
    );
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0].reason_code.as_deref(), Some("policy_approval"));
}

#[test]
fn structured_implementation_gate_rechecks_exact_task_route_and_respects_prior_failures() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("cfg.toml");
    fs::write(
        &path,
        format!(
            r#"
[defaults]
artifact_root = "{}"

[profiles.test]
display_name = "Test"
repo_id = "test/test"
provider = "github"
repo = "test/test"
local_path = "/tmp"
artifact_root = "{}"
default_target_branch = "main"
claude_path = "/bin/sh"
opencode_path = "/bin/sh"

[[profiles.test.routing.improve_candidates]]
backend = "opencode"
model = "nous/glm-5.2"
priority = 10
requires_approval = true

[[profiles.test.routing.task_routing_rules]]
difficulties = ["easy"]

[[profiles.test.routing.task_routing_rules.candidates]]
backend = "claude"
model = "sonnet"
priority = 0
included_in_quota = true
"#,
            tmp.path().display(),
            tmp.path().display()
        ),
    )
    .unwrap();
    let cfg = crate::config::load(Some(path.to_str().unwrap())).unwrap();
    let profile = &cfg.profiles["test"];
    let availability_path = tmp.path().join("availability.json");
    let _availability_guard = crate::test_support::AvailabilityEnvGuard::set(&availability_path);
    let gate = ledger::EffectiveHumanGate {
        reason_code: Some("policy_approval".into()),
        dispatch_reason: Some("initial".into()),
        message: None,
        mode: "fix".into(),
        timestamp: "2026-07-16T00:00:00Z".into(),
        routing_diagnostics: Some(crate::ledger::RoutingDiagnostics {
            candidates: vec![crate::ledger::RoutingCandidateDiagnostic {
                backend: "claude".into(),
                model: Some("sonnet".into()),
                skip_reason: Some("model-specific quota_exhausted".into()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        review_contract_version: None,
        review_generation: None,
    };

    assert!(!execution_gate_still_required(
        &cfg,
        "test",
        profile,
        &[],
        "#650",
        &gate,
    ));

    let mut failed = LedgerEntry::new("test", profile, "claude", "fix", "#650", None, None);
    failed.work_id = Some("#650".into());
    failed.effective_backend = "claude".into();
    failed.effective_model = Some("sonnet".into());
    failed.failure_class = Some("agent_failure".into());
    assert!(execution_gate_still_required(
        &cfg,
        "test",
        profile,
        &[failed],
        "#650",
        &gate,
    ));
}

#[test]
fn pre_bump_review_derived_policy_approval_gate_is_superseded() {
    let (cfg, profile) = policy_approval_test_cfg();
    let gate = ledger::EffectiveHumanGate {
        reason_code: Some("policy_approval".into()),
        dispatch_reason: Some("post_review_repair".into()),
        message: None,
        mode: "review".into(),
        timestamp: "2026-07-16T00:00:00Z".into(),
        review_contract_version: None, // Pre-bump
        review_generation: None,
        routing_diagnostics: None,
    };

    assert!(!execution_gate_still_required(
        &cfg,
        "test",
        &profile,
        &[],
        "#640",
        &gate,
    ));
}

#[test]
fn pre_bump_non_review_policy_approval_gate_still_required() {
    // TICKET-711 regression: a genuine implementation-task paid-route
    // hold (mode "fix", not raised during review or post-review repair)
    // predates the contract bump too, but it is not review-derived and
    // must remain blocked until an actual `gah route-approval grant` is
    // recorded -- not silently released just because it has no stamped
    // review_contract_version.
    let (cfg, profile) = policy_approval_test_cfg();
    let gate = ledger::EffectiveHumanGate {
        reason_code: Some("policy_approval".into()),
        dispatch_reason: Some("initial".into()),
        message: None,
        mode: "fix".into(),
        timestamp: "2026-07-16T00:00:00Z".into(),
        review_contract_version: None, // Pre-bump
        review_generation: None,
        routing_diagnostics: None,
    };

    assert!(execution_gate_still_required(
        &cfg,
        "test",
        &profile,
        &[],
        "#640",
        &gate,
    ));
}

fn policy_approval_test_cfg() -> (crate::config::GahConfig, crate::config::Profile) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("cfg.toml");
    fs::write(
        &path,
        format!(
            r#"
[defaults]
artifact_root = "{}"

[profiles.test]
display_name = "Test"
repo_id = "test/test"
provider = "github"
repo = "test/test"
local_path = "/tmp"
artifact_root = "{}"
default_target_branch = "main"
"#,
            tmp.path().display(),
            tmp.path().display()
        ),
    )
    .unwrap();
    let cfg = crate::config::load(Some(path.to_str().unwrap())).unwrap();
    let profile = cfg.profiles["test"].clone();
    (cfg, profile)
}
