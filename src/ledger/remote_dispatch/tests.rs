use super::*;
use crate::ledger::{test_util, AttemptRoutingRecord};

fn receive(
    cfg: &GahConfig,
    binding: &DispatchBinding,
    receipt: &DispatchReceipt,
) -> anyhow::Result<ReceiveOutcome> {
    super::receive(cfg, binding, &serde_json::to_vec(receipt)?)
}

fn fixture() -> (
    tempfile::TempDir,
    GahConfig,
    DispatchBinding,
    DispatchReceipt,
) {
    let (tmp, mut cfg) = test_util::test_config();
    let profile = test_util::profile();
    let binding = DispatchBinding {
        run_id: uuid::Uuid::new_v4().to_string(),
        context_sha256: "a".repeat(64),
        node_id: "worker-one".into(),
        profile: "test".into(),
        repo_id: profile.repo_id.clone(),
        repo: profile.repo.clone(),
        work_id: "#1525".into(),
        mode: "review".into(),
        branch: Some("feature".into()),
        review_generation: Some("review-v1:source:metadata".into()),
        dispatch_reason: Some("review".into()),
        backend: "codex".into(),
        backend_instance: Some("work".into()),
        model: Some("fixture-model".into()),
    };
    let mut entry = LedgerEntry::new(
        "test",
        &profile,
        "codex",
        "review",
        "feature",
        Some(binding.run_id.to_string()),
        None,
    );
    entry.work_id = Some(binding.work_id.clone());
    entry.branch = binding.branch.clone();
    entry.review_generation = binding.review_generation.clone();
    entry.review_contract_version = Some(super::super::REVIEW_CONTRACT_VERSION);
    entry.dispatch_reason = binding.dispatch_reason.clone();
    entry.requested_model = binding.model.clone();
    entry.attempts_started = Some(1);
    entry.attempts_completed = Some(1);
    let mut identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
        "codex",
        Some("fixture-model"),
        None::<String>,
    );
    identity.backend_instance = "work".into();
    entry.attempt_routing.push(AttemptRoutingRecord {
        attempt_number: 1,
        identity: Some(identity),
        ..Default::default()
    });
    let receipt = DispatchReceipt {
        context_sha256: binding.context_sha256.clone(),
        execution: RemoteExecution {
            node_id: binding.node_id.clone(),
            termination: ProcessTermination {
                reason: TerminationReason::Exited,
                exit_code: Some(0),
                signal: None,
            },
        },
        ledger: entry,
    };
    cfg.profiles.insert("test".into(), profile);
    (tmp, cfg, binding, receipt)
}

#[test]
fn concurrent_terminal_delivery_counts_once_and_retains_unknown_usage_and_job_failure() {
    let (_tmp, cfg, binding, mut receipt) = fixture();
    receipt.ledger.validation_result = Some("failed".into());
    receipt.ledger.failure_class = Some("validation_failure".into());
    let outcomes = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| receive(&cfg, &binding, &receipt).unwrap()))
            .collect();
        jobs.into_iter()
            .map(|job| job.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ReceiveOutcome::Recorded)
            .count(),
        1
    );
    let rows = super::super::read_entries(&cfg).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].validation_result.as_deref(), Some("failed"));
    assert_eq!(rows[0].failure_class.as_deref(), Some("validation_failure"));
    assert_eq!(rows[0].usage.actual_cost_usd, None);
    assert_eq!(rows[0].usage.input_tokens, None);
    assert_eq!(
        rows[0]
            .remote_execution
            .as_ref()
            .unwrap()
            .termination
            .exit_code,
        Some(0)
    );
}

#[test]
fn mismatched_identity_or_unconfirmed_exit_never_changes_central_history() {
    let (_tmp, cfg, binding, receipt) = fixture();
    for change in 0..13 {
        let mut changed = receipt.clone();
        match change {
            0 => changed.execution.node_id = "other-worker".into(),
            1 => changed.ledger.session_id = Some(uuid::Uuid::new_v4().to_string()),
            2 => changed.ledger.profile = "other".into(),
            3 => changed.ledger.work_id = Some("#other".into()),
            4 => changed.ledger.review_generation = Some("review-v1:old:metadata".into()),
            5 => changed.ledger.branch = Some("another-branch".into()),
            6 => {
                changed.ledger.attempt_routing[0]
                    .identity
                    .as_mut()
                    .unwrap()
                    .backend_instance = "personal".into()
            }
            7 => changed.execution.termination.exit_code = None,
            8 => changed.execution.termination.signal = Some("SIGTERM".into()),
            9 => changed.ledger.schema_version = LEDGER_SCHEMA_VERSION + 1,
            10 => changed.context_sha256 = "b".repeat(64),
            11 => changed.ledger.review_contract_version = Some(0),
            _ => changed.ledger.provider = "other-forge".into(),
        }
        assert!(
            receive(&cfg, &binding, &changed).is_err(),
            "change {change}"
        );
        assert!(super::super::read_entries(&cfg).unwrap().is_empty());
    }
}

#[test]
fn changed_terminal_result_is_refused_but_original_replay_survives_central_backfill() {
    let (_tmp, cfg, binding, receipt) = fixture();
    assert_eq!(
        receive(&cfg, &binding, &receipt).unwrap(),
        ReceiveOutcome::Recorded
    );
    let path = cfg.defaults.ledger_path();
    let mut changed = receipt.clone();
    changed.ledger.usage.actual_cost_usd = Some(10.0);
    let before = std::fs::read(&path).unwrap();
    assert!(receive(&cfg, &binding, &changed).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut row = super::super::read_entries(&cfg).unwrap().remove(0);
    row.review_verdict = Some("pass".into());
    std::fs::write(&path, format!("{}\n", serde_json::to_string(&row).unwrap())).unwrap();
    let backfilled = std::fs::read(&path).unwrap();
    assert_eq!(
        receive(&cfg, &binding, &receipt).unwrap(),
        ReceiveOutcome::AlreadyRecorded
    );
    assert_eq!(std::fs::read(&path).unwrap(), backfilled);
}

#[test]
fn corruption_anywhere_in_history_refuses_import_and_leaves_the_file_intact() {
    let (_tmp, cfg, binding, receipt) = fixture();
    let path = cfg.defaults.ledger_path();
    std::fs::write(&path, b"not-json\n").unwrap();
    assert!(receive(&cfg, &binding, &receipt).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"not-json\n");
}

#[test]
fn the_original_local_claim_can_share_a_run_but_a_local_terminal_result_cannot() {
    let (_tmp, cfg, binding, receipt) = fixture();
    let mut claim = receipt.ledger.clone();
    claim.mode = "claim".into();
    super::super::append(&cfg, &claim).unwrap();
    assert_eq!(
        receive(&cfg, &binding, &receipt).unwrap(),
        ReceiveOutcome::Recorded
    );
    assert_eq!(super::super::read_entries(&cfg).unwrap().len(), 2);
    let (_other_tmp, other_cfg, other_binding, other_receipt) = fixture();
    super::super::append(&other_cfg, &other_receipt.ledger).unwrap();
    let before = std::fs::read(other_cfg.defaults.ledger_path()).unwrap();
    assert!(receive(&other_cfg, &other_binding, &other_receipt).is_err());
    assert_eq!(
        std::fs::read(other_cfg.defaults.ledger_path()).unwrap(),
        before
    );
}

#[test]
fn incoming_schema_keeps_negative_exit_codes_and_nonterminal_reasons_out() {
    let (_tmp, _cfg, _binding, receipt) = fixture();
    let mut value = serde_json::to_value(receipt).unwrap();
    value["execution"]["termination"]["exit_code"] = serde_json::json!(-1);
    assert!(serde_json::from_value::<DispatchReceipt>(value.clone()).is_err());
    value["execution"]["termination"]["exit_code"] = serde_json::Value::Null;
    value["execution"]["termination"]["reason"] = serde_json::json!("uncertain_reconciling");
    assert!(serde_json::from_value::<DispatchReceipt>(value).is_err());
}

#[test]
fn read_only_delivery_retains_its_handoff_result_without_inventing_publication() {
    let (_tmp, mut cfg, mut binding, mut receipt) = fixture();
    cfg.profiles.get_mut("test").unwrap().delivery_mode = crate::config::DeliveryMode::Handoff;
    binding.mode = "fix".into();
    binding.review_generation = None;
    receipt.ledger.mode = "handoff".into();
    receipt.ledger.review_generation = None;
    assert_eq!(
        receive(&cfg, &binding, &receipt).unwrap(),
        ReceiveOutcome::Recorded
    );
    let row = super::super::read_entries(&cfg).unwrap().remove(0);
    assert_eq!(row.mode, "handoff");
    assert!(!row.mr_created);
    assert_eq!(row.mr_url, None);
    assert_eq!(
        receive(&cfg, &binding, &receipt).unwrap(),
        ReceiveOutcome::AlreadyRecorded
    );
}

#[test]
fn receiving_through_a_ledger_symlink_preserves_the_link_and_the_actual_history_file() {
    let (tmp, cfg, binding, receipt) = fixture();
    let parent = tmp.path().join("canonical");
    std::fs::create_dir(&parent).unwrap();
    let target = parent.join("history.jsonl");
    let alias = cfg.defaults.ledger_path();
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    assert_eq!(
        receive(&cfg, &binding, &receipt).unwrap(),
        ReceiveOutcome::Recorded
    );
    assert!(alias.is_symlink());
    assert_eq!(std::fs::read_to_string(&target).unwrap().lines().count(), 1);
    assert_eq!(
        receive(&cfg, &binding, &receipt).unwrap(),
        ReceiveOutcome::AlreadyRecorded
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap().lines().count(), 1);
}

#[test]
fn receipt_identity_uses_original_json_before_receiver_defaults_or_schema_upgrade() {
    let (_tmp, cfg, binding, receipt) = fixture();
    let mut original = serde_json::to_value(receipt).unwrap();
    original["ledger"].as_object_mut().unwrap().remove("skills");
    original["ledger"]["schema_version"] = serde_json::json!(11);
    let bytes = serde_json::to_vec(&original).unwrap();
    assert_eq!(
        super::receive(&cfg, &binding, &bytes).unwrap(),
        ReceiveOutcome::Recorded
    );
    let row = super::super::read_entries(&cfg).unwrap().remove(0);
    assert_eq!(row.schema_version, LEDGER_SCHEMA_VERSION);
    assert_eq!(
        row.remote_execution.unwrap().receipt_sha256,
        format!("{:x}", Sha256::digest(&bytes))
    );
    let pretty = serde_json::to_vec_pretty(&original).unwrap();
    assert_eq!(
        super::receive(&cfg, &binding, &pretty).unwrap(),
        ReceiveOutcome::AlreadyRecorded
    );
}
