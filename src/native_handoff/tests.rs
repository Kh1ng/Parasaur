use super::*;
use crate::config::{BackendInstanceConfig, CandidateConfig, SpendBudget};

pub(super) fn fixture() -> (tempfile::TempDir, GahConfig, GahConfig, JobRequest) {
    let (tmp, mut central) = crate::ledger::test_util::test_config();
    let mut profile = crate::ledger::test_util::profile();
    profile.local_path = "/central/repo".into();
    profile.env_file = Some("/central/private.env".into());
    profile.codex_path = Some("/central/codex".into());
    profile.codex_args = vec!["--max-turns".into(), "8".into()];
    profile.publishing.allow_pull_request_creation = false;
    profile.routing.max_review_cycles_per_ticket = Some(2);
    profile.routing.max_paid_reviews_per_ticket = Some(1);
    profile.routing.spend_budget = Some(SpendBudget {
        daily_usd: Some(1.0),
        ..Default::default()
    });
    profile.routing.improve_candidates = Some(vec![CandidateConfig {
        backend: "codex".into(),
        model: Some("fixture-model".into()),
        ..Default::default()
    }]);
    let instance: BackendInstanceConfig = serde_json::from_value(serde_json::json!({"runner_kind":"codex", "logical_backend":"codex",
        "account_label":"work", "auth_source_label":"local-credential", "quota_pool":"work", "executable":"/central/provider",
        "credential_id":"central-reference", "state_root":"/central/state", "supported_models":["fixture-model"]})).unwrap();
    profile
        .routing
        .backend_instances
        .insert("work".into(), instance);
    central.profiles.insert("test".into(), profile.clone());
    let mut worker = central.clone();
    worker.defaults.artifact_root = tmp
        .path()
        .join("worker-history")
        .to_string_lossy()
        .into_owned();
    worker.defaults.registry_central_url = Some("https://central.invalid".into());
    worker.defaults.node_capacity.worker_memory_mib = 8192;
    let local = worker.profiles.get_mut("test").unwrap();
    local.local_path = "/worker/repo".into();
    local.artifact_root = "/worker/artifacts".into();
    local.env_file = Some("/worker/private.env".into());
    local.codex_path = Some("/worker/codex".into());
    local.codex_args = vec!["--max-turns".into(), "8".into()];
    local.routing.max_paid_reviews_per_ticket = Some(99);
    local.routing.spend_budget = None;
    local.publishing.allow_pull_request_creation = true;
    let account = local.routing.backend_instances.get_mut("work").unwrap();
    account.executable = Some("/worker/provider".into());
    account.credential_id = Some("worker-reference".into());
    account.state_root = Some("/worker/state".into());
    let request: JobRequest = serde_json::from_value(serde_json::json!({"args":{
        "profile":"test", "mode":"fix", "backend":"codex", "backend_instance":"work", "model":"fixture-model",
        "target":"/central/repo/docs/task.md", "current_branch":false, "dry_run":false, "retries":2, "allow_draft_fail":false,
        "prod":false, "issue_intake_override":false, "allow_unknown_red_baseline":false, "escalate":false, "skip_validation_gate":false,
        "dispatch_reason":"retry", "prior_attempt_context":"retained retry evidence", "work_id":"#1525", "run_id":uuid::Uuid::new_v4().to_string()},
        } )).unwrap();
    (tmp, central, worker, request)
}

#[test]
fn current_central_policy_refuses_disabled_factory_changed_policy_and_wrong_identity() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, central, mut worker, request) = fixture();
    worker.defaults.node_role = crate::node_role::NodeRole::Worker;
    let input = tmp.path().join("current-policy");
    let bound = prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input,
    )
    .unwrap();
    let check = |cfg: &GahConfig, node: &str, origin: &str| {
        check_current_policy(cfg, &input, &bound.context_sha256, node, origin)
    };
    assert_eq!(
        serde_json::to_value(check(&central, "worker", "https://central.invalid").unwrap())
            .unwrap(),
        serde_json::to_value(&bound).unwrap()
    );
    assert!(check(&central, "another-worker", "https://central.invalid").is_err());
    assert!(check(&central, "worker", "https://other.invalid").is_err());
    assert!(check(&worker, "worker", "https://central.invalid").is_err());
    let mut changed = central.clone();
    changed.defaults.factory_enabled = Some(false);
    assert!(check(&changed, "worker", "https://central.invalid").is_err());
    changed = central.clone();
    changed
        .profiles
        .get_mut("test")
        .unwrap()
        .publishing
        .allow_pull_request_creation = true;
    assert!(check(&changed, "worker", "https://central.invalid")
        .unwrap_err()
        .to_string()
        .contains("policy changed"));
    changed = central.clone();
    changed
        .profiles
        .get_mut("test")
        .unwrap()
        .codex_args
        .push("--dangerously-bypass-approvals-and-sandbox".into());
    assert!(check(&changed, "worker", "https://central.invalid").is_err());
    fs::write(input.join("request.json"), b"{}").unwrap();
    assert!(check(&central, "worker", "https://central.invalid").is_err());
}

#[test]
fn current_policy_checks_new_central_spend_instead_of_prepared_history() {
    let _guard = crate::test_support::ExecGuard::new();
    for observed_cost in [Some(1.0), None] {
        let (tmp, central, _, request) = fixture();
        let input = tmp.path().join("current-spend");
        let bound = prepare(
            &central,
            &request,
            "worker",
            "https://central.invalid",
            &input,
        )
        .unwrap();
        let check = || {
            check_current_policy(
                &central,
                &input,
                &bound.context_sha256,
                "worker",
                "https://central.invalid",
            )
        };
        assert!(check().is_ok());
        let mut row = crate::ledger::LedgerEntry::new(
            "test",
            &central.profiles["test"],
            "codex",
            "fix",
            "another-work",
            Some(uuid::Uuid::new_v4().to_string()),
            None,
        );
        row.work_id = Some("#another-work".into());
        row.usage.actual_cost_usd = observed_cost;
        row.attempts_started = Some(1);
        crate::ledger::append(&central, &row).unwrap();
        assert!(check()
            .unwrap_err()
            .downcast_ref::<crate::spend_budget::SpendBudgetHeld>()
            .is_some());
        let manifest: Manifest =
            read_json(&fs::read(input.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            manifest.history_entries, 0,
            "The prepared snapshot itself has not changed"
        );
    }
}

#[test]
fn current_policy_refuses_repository_permission_changes_and_removal() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, mut central, _, request) = fixture();
    let path = tmp.path().join("repository-policy.toml");
    fs::write(&path, "[repo]\ntrust_mode = 'draft_pr_allowed'\nallow_provider_mutation = true\nallow_push = false\nallow_draft_pr = true\nallow_issue_write = false\nallow_project_write = false\n").unwrap();
    central.profiles.get_mut("test").unwrap().policy_path =
        Some(path.to_string_lossy().into_owned());
    let input = tmp.path().join("current-repository-policy");
    let bound = prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input,
    )
    .unwrap();
    let check = |cfg: &GahConfig| {
        check_current_policy(
            cfg,
            &input,
            &bound.context_sha256,
            "worker",
            "https://central.invalid",
        )
    };
    assert!(check(&central).is_ok());
    fs::write(&path, "[repo]\ntrust_mode = 'draft_pr_allowed'\nallow_provider_mutation = true\nallow_push = true\nallow_draft_pr = true\nallow_issue_write = false\nallow_project_write = false\n").unwrap();
    assert!(check(&central).is_err());
    central.profiles.get_mut("test").unwrap().policy_path = None;
    assert!(check(&central).is_err());
}

#[test]
fn worker_reassembles_all_chunks_and_refuses_a_missing_middle_chunk() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, central, worker, request) = fixture();
    let input = tmp.path().join("input");
    prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input,
    )
    .unwrap();
    let mut manifest: Manifest =
        serde_json::from_slice(&fs::read(input.join("manifest.json")).unwrap()).unwrap();
    fs::remove_file(input.join(history::name(0))).unwrap();
    manifest.files.remove(&history::name(0));

    let mut row = crate::ledger::LedgerEntry::new(
        "test",
        &central.profiles["test"],
        "codex",
        "review",
        "branch",
        Some(uuid::Uuid::new_v4().to_string()),
        None,
    );
    row.work_id = Some("#1525".into());
    row.usage.actual_cost_usd = Some(0.6);
    row.attempts_started = Some(1);
    let mut rows = [row.clone(), row.clone(), row];
    rows[1].mode = "paid_route_approval_grant".into();
    rows[2].mode = "paid_route_approval_revoke".into();
    let encoded: Vec<Vec<u8>> = rows
        .iter()
        .map(|row| serde_json::to_vec(row).unwrap())
        .collect();
    let limit = encoded
        .iter()
        .map(|bytes| bytes.len() as u64 + 1)
        .max()
        .unwrap();
    let mut writer = history::Writer::new(&input, limit, MAX_HISTORY_CHUNKS);
    for bytes in &encoded {
        writer.append(bytes).unwrap();
    }
    let chunks = writer.finish().unwrap();
    assert_eq!(chunks.len(), 3);
    manifest.history_chunks = 3;
    manifest.history_entries = 3;
    manifest.files.extend(chunks);
    let expected = write_blob(
        &input,
        "manifest.json",
        &serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap()
    .sha256;
    let loaded = load(
        &worker,
        &input,
        &expected,
        "worker",
        &tmp.path().join("job"),
    )
    .unwrap();
    let actual = crate::ledger::read_entries(&loaded.config).unwrap();
    assert_eq!(loaded.history_entries, 3);
    assert_eq!(
        actual
            .iter()
            .map(|row| row.mode.as_str())
            .collect::<Vec<_>>(),
        vec![
            "review",
            "paid_route_approval_grant",
            "paid_route_approval_revoke"
        ]
    );
    assert!(actual.iter().all(
        |row| row.work_id.as_deref() == Some("#1525") && row.usage.actual_cost_usd == Some(0.6)
    ));

    let first = fs::read(input.join(history::name(0))).unwrap();
    let middle = fs::read(input.join(history::name(1))).unwrap();
    fs::write(input.join(history::name(0)), &middle).unwrap();
    fs::write(input.join(history::name(1)), &first).unwrap();
    let reordered = tmp.path().join("reordered");
    assert!(load(&worker, &input, &expected, "worker", &reordered).is_err());
    assert!(!reordered.join("ledger.jsonl").exists());
    fs::write(input.join(history::name(0)), &first).unwrap();
    fs::write(input.join(history::name(1)), &middle).unwrap();

    // Even an intact first/last chunk cannot hide a missing middle row.
    fs::remove_file(input.join(history::name(1))).unwrap();
    let rejected = tmp.path().join("missing-middle");
    assert!(load(&worker, &input, &expected, "worker", &rejected).is_err());
    assert!(!rejected.join("ledger.jsonl").exists());

    // A claimed prefix, excess chunks or noncontiguous names are not complete history.
    for count in [0, 2, MAX_HISTORY_CHUNKS + 1] {
        manifest.history_chunks = count;
        let expected = write_blob(
            &input,
            "manifest.json",
            &serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap()
        .sha256;
        let rejected = tmp.path().join(format!("bad-count-{count}"));
        assert!(load(&worker, &input, &expected, "worker", &rejected).is_err());
        assert!(!rejected.exists());
    }
}

#[test]
fn history_storage_cannot_consume_the_dispatch_free_space_floor() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("not-created");
    let error = crate::dispatch::ensure_minimum_free_space(&missing, "native history", u64::MAX)
        .unwrap_err();
    assert!(error.to_string().contains("insufficient free space"));
    assert!(!missing.exists());
}

#[test]
fn policy_and_complete_profile_history_survive_without_central_credentials_or_capacity_overrides() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, central, worker, request) = fixture();
    let mut row = crate::ledger::LedgerEntry::new(
        "test",
        &central.profiles["test"],
        "codex",
        "review",
        "branch",
        Some(uuid::Uuid::new_v4().to_string()),
        None,
    );
    row.work_id = Some("#1525".into());
    row.usage.actual_cost_usd = Some(1.0);
    row.attempts_started = Some(1);
    crate::ledger::append(&central, &row).unwrap();
    let mut approval = row.clone();
    approval.mode = "paid_route_approval_grant".into();
    crate::ledger::append(&central, &approval).unwrap();
    let mut unrelated = row.clone();
    unrelated.profile = "other".into();
    crate::ledger::append(&central, &unrelated).unwrap();
    let input = tmp.path().join("input");
    let output = tmp.path().join("job");
    let expected = prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid/",
        &input,
    )
    .unwrap();
    let portable = fs::read_to_string(input.join("policy.json")).unwrap();
    for secret in ["/central/", "--max-turns", "central-reference"] {
        assert!(!portable.contains(secret));
    }
    let loaded = load(&worker, &input, &expected.context_sha256, "worker", &output).unwrap();
    assert_eq!(loaded.history_entries, 2);
    assert_eq!(loaded.request.args.target, "docs/task.md");
    assert_eq!(
        loaded.request.args.prior_attempt_context,
        request.args.prior_attempt_context
    );
    let profile = &loaded.config.profiles["test"];
    assert_eq!(profile.local_path, "/worker/repo");
    assert_eq!(profile.env_file.as_deref(), Some("/worker/private.env"));
    assert_eq!(profile.codex_args, ["--max-turns", "8"]);
    assert_eq!(profile.routing.max_paid_reviews_per_ticket(), 1);
    assert_eq!(profile.routing.max_review_cycles_per_ticket(), 2);
    assert!(!profile.publishing.allow_pull_request_creation);
    assert_eq!(
        profile.routing.backend_instances["work"]
            .credential_id
            .as_deref(),
        Some("worker-reference")
    );
    assert_eq!(
        loaded.config.defaults.node_capacity,
        worker.defaults.node_capacity
    );
    let rows: Vec<crate::ledger::LedgerEntry> = fs::read_to_string(output.join("ledger.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows[1].mode, "paid_route_approval_grant");
    let spend = crate::spend_budget::evaluate(
        profile.routing.spend_budget.as_ref(),
        "test",
        &rows,
        crate::clock::now_utc(),
    )
    .unwrap();
    assert!(
        spend.intake_held,
        "worker must see the central recorded spend instead of its empty local ledger"
    );
    assert!(prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input
    )
    .is_err());
    assert!(load(&worker, &input, &expected.context_sha256, "worker", &output).is_err());
}

#[test]
fn altered_missing_or_misdirected_context_never_becomes_an_empty_history() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, central, mut worker, request) = fixture();
    let input = tmp.path().join("input");
    let expected = prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input,
    )
    .unwrap();
    assert!(load(
        &worker,
        &input,
        &"0".repeat(64),
        "worker",
        &tmp.path().join("wrong-digest")
    )
    .is_err());
    assert!(load(
        &worker,
        &input,
        &expected.context_sha256,
        "other-node",
        &tmp.path().join("wrong-node")
    )
    .is_err());
    worker.defaults.registry_central_url = Some("https://other.invalid".into());
    assert!(load(
        &worker,
        &input,
        &expected.context_sha256,
        "worker",
        &tmp.path().join("wrong-origin")
    )
    .is_err());
    worker.defaults.registry_central_url = Some("https://central.invalid".into());
    fs::write(input.join("history-0000.jsonl"), "altered").unwrap();
    assert!(load(
        &worker,
        &input,
        &expected.context_sha256,
        "worker",
        &tmp.path().join("altered")
    )
    .is_err());
    fs::remove_file(input.join("history-0000.jsonl")).unwrap();
    assert!(load(
        &worker,
        &input,
        &expected.context_sha256,
        "worker",
        &tmp.path().join("missing")
    )
    .is_err());
}

#[test]
fn worker_disabled_accounts_and_repository_permissions_are_not_relaxed() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, mut central, mut worker, request) = fixture();
    let policy_text = |push: bool| {
        format!("[repo]\ntrust_mode = 'draft_pr_allowed'\nallow_provider_mutation = true\nallow_push = {push}\nallow_draft_pr = true\nallow_issue_write = false\nallow_project_write = false\n")
    };
    let central_policy = tmp.path().join("central-policy.toml");
    fs::write(&central_policy, policy_text(true)).unwrap();
    let local_policy = tmp.path().join("local-policy.toml");
    fs::write(&local_policy, policy_text(false)).unwrap();
    central.profiles.get_mut("test").unwrap().policy_path =
        Some(central_policy.to_string_lossy().into_owned());
    worker.profiles.get_mut("test").unwrap().policy_path =
        Some(local_policy.to_string_lossy().into_owned());
    worker
        .profiles
        .get_mut("test")
        .unwrap()
        .routing
        .backend_instances
        .get_mut("work")
        .unwrap()
        .enabled = Some(false);
    let input = tmp.path().join("input");
    let expected = prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input,
    )
    .unwrap();
    let loaded = load(
        &worker,
        &input,
        &expected.context_sha256,
        "worker",
        &tmp.path().join("job"),
    )
    .unwrap();
    assert!(!loaded.config.profiles["test"].routing.backend_instances["work"].enabled());
    let path = Path::new(loaded.config.profiles["test"].policy_path.as_ref().unwrap());
    assert!(!crate::policy::config_allows_action(path, "git-push").unwrap());
    assert!(crate::policy::config_allows_action(path, "review").unwrap());
}

#[test]
fn a_worker_can_use_its_matching_account_without_inheriting_other_nodes_accounts() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, mut central, worker, request) = fixture();
    let mut unavailable = central.profiles["test"].routing.backend_instances["work"].clone();
    unavailable.account_label = Some("another-node-account".into());
    central
        .profiles
        .get_mut("test")
        .unwrap()
        .routing
        .backend_instances
        .insert("other".into(), unavailable);
    let input = tmp.path().join("input");
    let expected = prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input,
    )
    .unwrap();
    let loaded = load(
        &worker,
        &input,
        &expected.context_sha256,
        "worker",
        &tmp.path().join("job"),
    )
    .unwrap();
    let accounts = &loaded.config.profiles["test"].routing.backend_instances;
    assert!(accounts["work"].enabled());
    assert!(!accounts["other"].enabled());
    assert_eq!(accounts["other"].credential_id, None);
    assert_eq!(accounts["other"].executable, None);
}

#[test]
fn unsupported_or_escaping_requests_fail_before_publishing_a_context() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, central, _worker, mut request) = fixture();
    request.args.target = "/central/repo/../private.env".into();
    let output = tmp.path().join("escaping");
    assert!(prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &output
    )
    .is_err());
    assert!(!output.exists());
    request.args.target = "feature".into();
    request.args.mode = "review".into();
    request.args.branch = Some("feature".into());
    assert!(prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &tmp.path().join("no-generation")
    )
    .is_err());
    request.args.expected_review_generation = Some("review-v1:source:metadata".into());
    assert!(prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &tmp.path().join("selected-generation")
    )
    .is_ok());
}

#[test]
fn a_worker_with_different_permission_arguments_cannot_silently_replace_central_rules() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, central, mut worker, request) = fixture();
    let input = tmp.path().join("input");
    let expected = prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input,
    )
    .unwrap();
    worker.profiles.get_mut("test").unwrap().codex_args =
        vec!["--dangerously-bypass-approvals-and-sandbox".into()];
    let work = tmp.path().join("job");
    let error = load(&worker, &input, &expected.context_sha256, "worker", &work)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("codex_args"));
    assert!(!error.contains("dangerously"));
    assert!(!work.exists());
}

#[test]
fn ignored_opencode_agent_overrides_do_not_block_a_worker_but_model_and_runtime_rules_do() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, mut central, mut worker, mut request) = fixture();
    request.args.backend = "opencode".into();
    request.args.backend_instance = None;
    central.profiles.get_mut("test").unwrap().opencode_args = vec![
        "--agent".into(),
        "gah-reviewer".into(),
        "--agent=ignored".into(),
    ];
    let input = tmp.path().join("input");
    let expected = prepare(
        &central,
        &request,
        "worker",
        "https://central.invalid",
        &input,
    )
    .unwrap();
    load(
        &worker,
        &input,
        &expected.context_sha256,
        "worker",
        &tmp.path().join("job"),
    )
    .unwrap();
    for (index, args) in [
        vec!["--model", "other-model"],
        vec!["--permission", "changed"],
        vec!["--format", "json"],
    ]
    .into_iter()
    .enumerate()
    {
        worker.profiles.get_mut("test").unwrap().opencode_args =
            args.into_iter().map(String::from).collect();
        let work = tmp.path().join(format!("changed-{index}"));
        let error = load(&worker, &input, &expected.context_sha256, "worker", &work)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("opencode_args"));
        assert!(!work.exists());
    }
}

#[test]
fn prelaunch_policy_refusal_does_not_invent_spend_but_possible_invocation_still_holds_budget() {
    let _guard = crate::test_support::ExecGuard::new();
    let (_tmp, central, _worker, _request) = fixture();
    let mut entry = crate::ledger::LedgerEntry::new(
        "test",
        &central.profiles["test"],
        "codex",
        "fix",
        "target",
        None,
        None,
    );
    entry.validation_result = Some("policy_blocked".into());
    entry.failure_class = Some("human_blocked".into());
    entry = entry.normalized_for_persistence();
    let budget = central.profiles["test"].routing.spend_budget.as_ref();
    let status =
        crate::spend_budget::evaluate(budget, "test", &[entry.clone()], crate::clock::now_utc())
            .unwrap();
    assert!(!status.intake_held);
    assert_eq!(status.windows[0].unmeasured_observations, 0);
    entry.attempts_started = Some(1);
    entry.usage.usage_unknown_reason =
        Some(crate::ledger::UsageUnknownReason::UsageArtifactMissing);
    assert!(
        crate::spend_budget::evaluate(budget, "test", &[entry], crate::clock::now_utc())
            .unwrap()
            .intake_held
    );
}

#[test]
fn pinned_jobs_match_their_runtime_while_auto_or_unpinned_jobs_require_all_runtimes() {
    let _guard = crate::test_support::ExecGuard::new();
    let (tmp, mut central, worker, request) = fixture();
    central.profiles.get_mut("test").unwrap().oh_profile = Some("central-only".into());
    central.profiles.get_mut("test").unwrap().claude_args =
        vec!["--permission-mode".into(), "plan".into()];
    for (index, (backend, model, succeeds)) in [
        ("codex", Some("fixture-model"), true),
        ("auto", Some("fixture-model"), false),
        ("codex", None, false),
        ("codex", Some("auto"), false),
        ("codex", Some("default"), false),
        ("codex", Some(" "), false),
        ("unknown", Some("fixture-model"), false),
        ("openhands", Some("fixture-model"), false),
        ("cloud-coder", Some("fixture-model"), false),
        ("claude", Some("fixture-model"), false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = request.clone();
        request.args.backend = backend.into();
        request.args.model = model.map(String::from);
        request.args.backend_instance = None;
        let input = tmp.path().join(format!("input-{index}"));
        let expected = prepare(
            &central,
            &request,
            "worker",
            "https://central.invalid",
            &input,
        )
        .unwrap();
        let work = tmp.path().join(format!("job-{index}"));
        let loaded = load(&worker, &input, &expected.context_sha256, "worker", &work);
        assert_eq!(
            loaded.is_ok(),
            succeeds,
            "backend {backend}, model {model:?}"
        );
        if !succeeds {
            assert!(!work.exists());
        }
    }
}
