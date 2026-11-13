use super::*;

fn assert_claim_after_records(modes: &[&str], expected_active: bool) {
    let _exec_guard = crate::test_support::ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    setup_fake_gh(&bin_dir, "[]");
    let _guard = PathGuard::set(&bin_dir);
    let (cfg, prof, args) = super::repository_scope::claim_fixture(tmp.path());
    let mut entries = vec![LedgerEntry::new_claim("test", &prof, "TICKET-500")];
    for mode in modes {
        let mut entry = LedgerEntry::new("test", &prof, "codex", mode, "x", None, None);
        entry.work_id = Some("TICKET-500".into());
        entries.push(entry);
    }
    let lines: String = entries
        .iter()
        .map(|entry| format!("{}\n", serde_json::to_string(entry).unwrap()))
        .collect();
    fs::write(tmp.path().join("ledger.jsonl"), lines).unwrap();
    let result = check_duplicate_work(&cfg, &prof, &args, false);
    if expected_active {
        assert!(result
            .unwrap_err()
            .downcast_ref::<ActiveClaimError>()
            .is_some());
    } else {
        assert_eq!(result.unwrap().as_deref(), Some("TICKET-500"));
    }
    let index = crate::ledger::index_entries_by_work_id(&entries);
    let history = ledger_lookup_for_ticket(Some("TICKET-500"), &prof, &[], &index).unwrap();
    assert_eq!(history.5, expected_active);
    assert_eq!(
        history.0, 0,
        "control records and resets consume no attempts"
    );
    assert_eq!(history.1, 0);
}

#[test]
fn external_approval_request_keeps_claim_active() {
    assert_claim_after_records(&["external_approval_request"], true);
}

#[test]
fn paid_route_approval_grant_keeps_claim_active() {
    assert_claim_after_records(&["paid_route_approval_grant"], true);
}

#[test]
fn review_hold_and_release_keep_claim_active() {
    assert_claim_after_records(&["review_hold", "review_hold_release"], true);
}

#[test]
fn clear_attempts_resolves_claim() {
    assert_claim_after_records(&["clear_attempts"], false);
}

#[test]
fn control_record_modes_are_complete() {
    for mode in [
        "paid_route_approval_grant",
        "paid_route_approval_revoke",
        "external_approval_grant",
        "external_approval_request",
        "external_approval_consume",
        "external_approval_revoke",
        "external_approval_expire",
        "external_approval_deny",
        "review_hold",
        "review_hold_release",
    ] {
        assert!(is_control_record(mode), "{mode}");
    }
    for mode in ["claim", "fix", "review", "clear_attempts"] {
        assert!(!is_control_record(mode), "{mode}");
    }
}

fn duplicate_work_has_active_claim(outcome: Option<&str>, reclaim: bool) -> bool {
    let _exec_guard = crate::test_support::ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    setup_fake_gh(&bin_dir, "[]");
    let _guard = PathGuard::set(&bin_dir);

    let ticket_dir = tmp.path().join("docs/tickets");
    fs::create_dir_all(&ticket_dir).unwrap();
    let ticket_path = ticket_dir.join("TICKET-500-test.md");
    fs::write(
        &ticket_path,
        "# TICKET-500: Test\n\nGoal: test claim guard\n",
    )
    .unwrap();

    let cfg = crate::config::GahConfig {
        context: Default::default(),
        defaults: crate::config::Defaults {
            current_manager: None,
            artifact_root: tmp.path().to_string_lossy().into_owned(),
            worktree_base: tmp.path().to_string_lossy().into_owned(),
            llm_base_url: String::new(),
            llm_model_local: String::new(),
            llm_model_cloud: String::new(),
            routing: crate::config::RoutingPolicy::default(),

            ..Default::default()
        },
        profiles: std::collections::HashMap::new(),
    };
    let mut prof = profile(tmp.path());
    prof.provider = "github".to_string();
    prof.repo = "owner/repo".to_string();

    let args = super::DispatchArgs {
        profile: "test".to_string(),
        mode: "improve".to_string(),
        backend: "codex".to_string(),
        backend_instance: None,
        target: ticket_path.display().to_string(),
        branch: None,
        mr: None,
        current_branch: false,
        dry_run: false,
        oh_profile: None,
        model: None,
        retries: 0,
        allow_draft_fail: false,
        prod: false,
        issue_intake_override: false,
        allow_unknown_red_baseline: false,
        escalate: false,
        existing_branch: None,
        expected_review_generation: None,
        skip_validation_gate: false,
        dispatch_reason: None,
        prior_attempt_context: None,
        work_id: None,
        run_id: None,
        route_admission: None,
    };

    let mut entries = vec![LedgerEntry::new_claim("test", &prof, "TICKET-500")];
    if let Some(validation) = outcome {
        let mut ended = LedgerEntry::new("test", &prof, "codex", "fix", "x", None, None);
        ended.work_id = Some("TICKET-500".into());
        ended.failure_class = Some("backend_error".into());
        ended.validation_result = Some(validation.into());
        entries.push(ended);
    }
    if reclaim {
        entries.push(LedgerEntry::new_claim("test", &prof, "TICKET-500"));
    }
    let lines: String = entries
        .iter()
        .map(|e| format!("{}\n", serde_json::to_string(e).unwrap()))
        .collect();
    fs::write(tmp.path().join("ledger.jsonl"), lines).unwrap();
    match super::check_duplicate_work(&cfg, &prof, &args, false) {
        Ok(work_id) => {
            assert_eq!(work_id.as_deref(), Some("TICKET-500"));
            false
        }
        Err(error) => {
            assert!(error.downcast_ref::<ActiveClaimError>().is_some());
            true
        }
    }
}

#[test]
fn check_duplicate_work_allows_claim_then_failed_execution() {
    assert!(!duplicate_work_has_active_claim(Some("failed"), false));
}

#[test]
fn check_duplicate_work_allows_claim_then_capacity_deferral() {
    assert!(!duplicate_work_has_active_claim(
        Some("deferred_capacity"),
        false
    ));
}

#[test]
fn check_duplicate_work_blocks_claim_without_outcome() {
    assert!(duplicate_work_has_active_claim(None, false));
}

#[test]
fn check_duplicate_work_blocks_second_claim_after_execution() {
    assert!(duplicate_work_has_active_claim(Some("failed"), true));
}

#[test]
fn ticket_scan_capacity_deferral_resolves_claim_without_changing_attempt_history() {
    let tmp = tempfile::tempdir().unwrap();
    let prof = profile(tmp.path());
    let mut failed = LedgerEntry::new("test", &prof, "codex", "fix", "x", None, None);
    failed.work_id = Some("TICKET-500".into());
    failed.failure_class = Some("agent_no_progress".into());
    let claim = LedgerEntry::new_claim("test", &prof, "TICKET-500");
    let mut deferred = failed.clone();
    deferred.validation_result = Some("deferred_capacity".into());
    deferred.failure_class = Some("backend_error".into());
    let index_history = [failed.clone(), claim.clone()];
    let index = crate::ledger::index_entries_by_work_id(&index_history);
    let before = ledger_lookup_for_ticket(Some("TICKET-500"), &prof, &[], &index).unwrap();
    assert!(before.5);
    let index_history = [failed, claim, deferred];
    let index = crate::ledger::index_entries_by_work_id(&index_history);
    let after = ledger_lookup_for_ticket(Some("TICKET-500"), &prof, &[], &index).unwrap();
    assert!(!after.5);
    assert_eq!((after.0, after.1, after.2), (before.0, before.1, before.2));
    assert_eq!(after.0, 1);
    assert_eq!(after.1, 1);
}
