use super::telemetry_tests::create_test_ledger_entry;
use super::*;

#[test]
fn test_telemetry_aggregation_no_attempts() {
    use crate::telemetry::{generate_telemetry_report, AggregationDimension, AggregationParams};
    let temp_dir = tempdir().unwrap();

    // Write mock config
    let cfg_path = temp_dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        r#"
[profiles.test-profile]
display_name = "Test Profile"
repo_id = "test-repo"
provider = "github"
repo = "test/repo"
local_path = "/tmp"
artifact_root = "/tmp"
default_target_branch = "main"
"#,
    )
    .unwrap();

    let mut cfg = crate::config::load(Some(cfg_path.to_str().unwrap())).unwrap();
    cfg.defaults.artifact_root = temp_dir.path().to_string_lossy().into_owned();

    // Setup mock ledger entries
    let mut entry1 = create_test_ledger_entry();
    entry1.repo_id = "test-repo".to_string();
    entry1.work_id = Some("ticket-101".to_string());
    entry1.task_class = Some("fix".to_string());
    entry1.validation_result = Some("pass".to_string());
    entry1.effective_backend = "codex".to_string();
    entry1.effective_model = Some("gpt-4".to_string());
    entry1.usage = crate::ledger::LedgerUsage {
        usage_source: Some("test".to_string()),
        // `quota_backed` is the normalized value emitted by live
        // subscription-backed executions. It must never enter the API
        // spend total merely because a quota window is unavailable.
        usage_classification: Some("quota_backed".to_string()),
        behavior_metrics: None,
        backend_instance: Some("instance-1".to_string()),
        provider: Some("openai".to_string()),
        actual_model: Some("gpt-4".to_string()),
        account_label: Some("acct-1".to_string()),
        observed_at: Some("2026-07-10T12:00:00Z".to_string()),
        input_tokens: Some(100),
        output_tokens: Some(50),
        total_tokens: Some(150),
        actual_cost_usd: Some(0.01),
        ..Default::default()
    };

    let mut entry2 = create_test_ledger_entry();
    entry2.repo_id = "other-repo".to_string();
    entry2.work_id = Some("ticket-102".to_string());
    entry2.task_class = Some("improve".to_string());
    entry2.validation_result = Some("fail".to_string());
    entry2.failure_class = Some("agent_failure".to_string());
    entry2.effective_backend = "claude".to_string();
    entry2.effective_model = Some("claude-3-5".to_string());
    entry2.usage = crate::ledger::LedgerUsage {
        usage_source: Some("test".to_string()),
        usage_classification: Some("api_key_backed".to_string()),
        backend_instance: Some("instance-2".to_string()),
        provider: Some("anthropic".to_string()),
        actual_model: Some("claude-3-5".to_string()),
        account_label: Some("acct-2".to_string()),
        observed_at: Some("2026-07-10T13:00:00Z".to_string()),
        input_tokens: Some(200),
        output_tokens: Some(100),
        total_tokens: Some(300),
        actual_cost_usd: Some(0.05),
        ..Default::default()
    };

    let ledger_path = temp_dir.path().join("ledger.jsonl");
    let mut file = std::fs::File::create(&ledger_path).unwrap();
    use std::io::Write;
    writeln!(file, "{}", serde_json::to_string(&entry1).unwrap()).unwrap();
    writeln!(file, "{}", serde_json::to_string(&entry2).unwrap()).unwrap();

    // Aggregate without filters
    let params = AggregationParams {
        dimensions: vec![AggregationDimension::Model],
        since: None,
        until: None,
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts: true,
        project: None,
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: None,
        account: None,
    };

    let report = generate_telemetry_report(&cfg, params).unwrap();
    assert_eq!(report.total_entries, 2);
    assert_eq!(report.total_attempts, 2);
    assert_eq!(report.successful_attempts, 1);
    assert_eq!(report.failed_attempts, 1);
    assert!((report.total_cost_usd - 0.06).abs() < 1e-9);
    assert!((report.quota_backed_cost_usd - 0.01).abs() < 1e-9);
    assert!((report.api_cost_usd - 0.05).abs() < 1e-9);

    // Verify model aggregation details
    let gpt4_data = report
        .aggregated_data
        .iter()
        .find(|d| d.dimension_value == "gpt-4")
        .unwrap();
    assert_eq!(gpt4_data.attempts, 1);
    assert_eq!(gpt4_data.successful_attempts, 1);
    assert_eq!(gpt4_data.failed_attempts, 0);
    assert!((gpt4_data.actual_cost_usd - 0.01).abs() < 1e-9);
    assert!((gpt4_data.quota_backed_cost_usd - 0.01).abs() < 1e-9);
    assert!((gpt4_data.api_cost_usd - 0.0).abs() < 1e-9);

    // Filter by project (repo_id) = "test-repo"
    let params_project = AggregationParams {
        dimensions: vec![AggregationDimension::Model],
        since: None,
        until: None,
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts: true,
        project: Some("test-repo".to_string()),
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: None,
        account: None,
    };

    let report_project = generate_telemetry_report(&cfg, params_project).unwrap();
    assert_eq!(report_project.total_entries, 1);
    assert_eq!(report_project.total_attempts, 1);
    assert!((report_project.total_cost_usd - 0.01).abs() < 1e-9);

    // Filter by model = "claude-3-5"
    let params_model = AggregationParams {
        dimensions: vec![AggregationDimension::Model],
        since: None,
        until: None,
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts: true,
        project: None,
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: Some("claude-3-5".to_string()),
        account: None,
    };
    let report_model = generate_telemetry_report(&cfg, params_model).unwrap();
    assert_eq!(report_model.total_entries, 1);
    assert_eq!(report_model.total_attempts, 1);
    assert!((report_model.total_cost_usd - 0.05).abs() < 1e-9);
}

#[test]
fn test_telemetry_aggregation_with_attempts() {
    use crate::telemetry::{generate_telemetry_report, AggregationDimension, AggregationParams};
    let temp_dir = tempdir().unwrap();

    // Write mock config
    let cfg_path = temp_dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        r#"
[profiles.test-profile]
display_name = "Test Profile"
repo_id = "test-repo"
provider = "github"
repo = "test/repo"
local_path = "/tmp"
artifact_root = "/tmp"
default_target_branch = "main"
"#,
    )
    .unwrap();

    let mut cfg = crate::config::load(Some(cfg_path.to_str().unwrap())).unwrap();
    cfg.defaults.artifact_root = temp_dir.path().to_string_lossy().into_owned();

    // Setup mock ledger entries with 3 attempts:
    // Attempt 1: Codex gpt-4, failed/retried, API cost
    // Attempt 2: Codex gpt-4, failed/retried, API cost
    // Attempt 3: Claude claude-3-5, successful, Quota backed
    let attempt1 = crate::ledger::AttemptRecord {
        resources: None,
        attempt_number: 1,
        backend: "codex".to_string(),
        effective_model: Some("gpt-4".to_string()),
        exit_code: Some(1),
        validation_result: Some("fail".to_string()),
        failure_class: Some("agent_failure".to_string()),
        failure_stage: Some("agent_run".to_string()),
        duration_seconds: Some(50.0),
        diff_path: None,
        checkpoint_branch: None,
        checkpoint_sha: None,
        cli_version: None,
        usage: crate::ledger::LedgerUsage {
            usage_source: Some("test".to_string()),
            usage_classification: Some("api_key_backed".to_string()),
            backend_instance: Some("instance-api".to_string()),
            quota_pool: Some("api-pool".to_string()),
            provider: Some("openai".to_string()),
            actual_model: Some("gpt-4".to_string()),
            account_label: Some("api-acct".to_string()),
            observed_at: Some("2026-07-10T12:00:00Z".to_string()),
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
            actual_cost_usd: Some(0.01),
            ..Default::default()
        },
    };

    let attempt2 = crate::ledger::AttemptRecord {
        resources: None,
        attempt_number: 2,
        backend: "codex".to_string(),
        effective_model: Some("gpt-4".to_string()),
        exit_code: Some(1),
        validation_result: Some("fail".to_string()),
        failure_class: Some("agent_failure".to_string()),
        failure_stage: Some("agent_run".to_string()),
        duration_seconds: Some(60.0),
        diff_path: None,
        checkpoint_branch: None,
        checkpoint_sha: None,
        cli_version: None,
        usage: crate::ledger::LedgerUsage {
            usage_source: Some("test".to_string()),
            usage_classification: Some("api_key_backed".to_string()),
            backend_instance: Some("instance-api".to_string()),
            quota_pool: Some("api-pool".to_string()),
            provider: Some("openai".to_string()),
            actual_model: Some("gpt-4".to_string()),
            account_label: Some("api-acct".to_string()),
            observed_at: Some("2026-07-10T12:05:00Z".to_string()),
            input_tokens: Some(120),
            output_tokens: Some(60),
            total_tokens: Some(180),
            actual_cost_usd: Some(0.012),
            ..Default::default()
        },
    };

    let attempt3 = crate::ledger::AttemptRecord {
        resources: None,
        attempt_number: 3,
        backend: "claude".to_string(),
        effective_model: Some("claude-3-5".to_string()),
        exit_code: Some(0),
        validation_result: Some("pass".to_string()),
        failure_class: None,
        failure_stage: None,
        duration_seconds: Some(40.0),
        diff_path: None,
        checkpoint_branch: None,
        checkpoint_sha: None,
        cli_version: None,
        usage: crate::ledger::LedgerUsage {
            usage_source: Some("test".to_string()),
            usage_classification: Some("subscription".to_string()),
            backend_instance: Some("instance-sub".to_string()),
            quota_pool: Some("subscription-pool".to_string()),
            provider: Some("anthropic".to_string()),
            actual_model: Some("claude-3-5".to_string()),
            account_label: Some("sub-acct".to_string()),
            observed_at: Some("2026-07-10T12:10:00Z".to_string()),
            input_tokens: Some(200),
            output_tokens: Some(100),
            total_tokens: Some(300),
            actual_cost_usd: Some(0.05),
            ..Default::default()
        },
    };

    let mut entry = create_test_ledger_entry();
    entry.repo_id = "test-repo".to_string();
    entry.work_id = Some("ticket-101".to_string());
    entry.task_class = Some("fix".to_string());
    entry.validation_result = Some("pass".to_string());
    entry.effective_backend = "claude".to_string();
    entry.effective_model = Some("claude-3-5".to_string());
    entry.attempts = vec![attempt1, attempt2, attempt3];
    entry.attempt_routing = vec![
        crate::ledger::AttemptRoutingRecord {
            skills: Vec::new(),
            attempt_number: 1,
            backend_instance: "instance-api".into(),
            effective_model: Some("gpt-4".into()),
            identity: Some(crate::execution_identity::ExecutionIdentity::legacy_route(
                "auto",
                Some("gpt-4"),
                "codex",
                Some("gpt-4"),
                Some("api-pool"),
            )),
            routing_diagnostics: None,
        },
        crate::ledger::AttemptRoutingRecord {
            skills: Vec::new(),
            attempt_number: 2,
            backend_instance: "instance-api".into(),
            effective_model: Some("gpt-4".into()),
            identity: Some(crate::execution_identity::ExecutionIdentity::legacy_route(
                "auto",
                Some("gpt-4"),
                "codex",
                Some("gpt-4"),
                Some("api-pool"),
            )),
            routing_diagnostics: None,
        },
        crate::ledger::AttemptRoutingRecord {
            skills: Vec::new(),
            attempt_number: 3,
            backend_instance: "instance-sub".into(),
            effective_model: Some("claude-3-5".into()),
            identity: Some(crate::execution_identity::ExecutionIdentity::legacy_route(
                "auto",
                Some("gpt-4"),
                "claude",
                Some("claude-3-5"),
                Some("subscription-pool"),
            )),
            routing_diagnostics: None,
        },
    ];
    entry.attempts_started = Some(3);
    entry.attempts_completed = Some(3);
    // top-level usage would normally be aggregated sum of attempts
    entry.usage = crate::ledger::LedgerUsage {
        input_tokens: Some(420),
        output_tokens: Some(210),
        total_tokens: Some(630),
        actual_cost_usd: Some(0.072),
        ..Default::default()
    };

    let ledger_path = temp_dir.path().join("ledger.jsonl");
    let mut file = std::fs::File::create(&ledger_path).unwrap();
    use std::io::Write;
    writeln!(file, "{}", serde_json::to_string(&entry).unwrap()).unwrap();

    // Aggregate without filters - should include all 3 attempts
    let params = AggregationParams {
        dimensions: vec![
            AggregationDimension::Model,
            AggregationDimension::Runner,
            AggregationDimension::AuthClass,
            AggregationDimension::QuotaPool,
        ],
        since: None,
        until: None,
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts: true,
        project: None,
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: None,
        account: None,
    };

    let report = generate_telemetry_report(&cfg, params).unwrap();
    assert_eq!(report.total_entries, 1);
    assert_eq!(report.total_attempts, 3);
    assert_eq!(report.successful_attempts, 1);
    assert_eq!(report.failed_attempts, 2);
    // Cost should be sum of attempts: 0.01 + 0.012 + 0.05 = 0.072 (no double counting!)
    assert!((report.total_cost_usd - 0.072).abs() < 1e-9);
    assert!((report.quota_backed_cost_usd - 0.05).abs() < 1e-9);
    assert!((report.api_cost_usd - 0.022).abs() < 1e-9);
    assert!(report
        .aggregated_data
        .iter()
        .any(|bucket| { bucket.dimension_key == "runner" && bucket.dimension_value == "codex" }));
    assert!(report.aggregated_data.iter().any(|bucket| {
        bucket.dimension_key == "auth_class"
            && bucket.dimension_value == "api_key_backed"
            && bucket.attempts == 2
    }));
    assert!(report.aggregated_data.iter().any(|bucket| {
        bucket.dimension_key == "quota_pool"
            && bucket.dimension_value == "subscription-pool"
            && bucket.attempts == 1
    }));

    // Aggregate but exclude retried attempts - should only aggregate attempt 3 (the last one)
    let params_no_retries = AggregationParams {
        dimensions: vec![AggregationDimension::Model],
        since: None,
        until: None,
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts: false,
        project: None,
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: None,
        account: None,
    };
    let report_no_retries = generate_telemetry_report(&cfg, params_no_retries).unwrap();
    assert_eq!(report_no_retries.total_entries, 1);
    assert_eq!(report_no_retries.total_attempts, 1);
    assert_eq!(report_no_retries.successful_attempts, 1);
    assert_eq!(report_no_retries.failed_attempts, 0);
    assert!((report_no_retries.total_cost_usd - 0.05).abs() < 1e-9);

    // Filter by model = "gpt-4" - should only aggregate attempts 1 & 2
    let params_gpt4 = AggregationParams {
        dimensions: vec![AggregationDimension::Model],
        since: None,
        until: None,
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts: true,
        project: None,
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: Some("gpt-4".to_string()),
        account: None,
    };
    let report_gpt4 = generate_telemetry_report(&cfg, params_gpt4).unwrap();
    assert_eq!(report_gpt4.total_entries, 1);
    assert_eq!(report_gpt4.total_attempts, 2);
    assert_eq!(report_gpt4.successful_attempts, 0);
    assert_eq!(report_gpt4.failed_attempts, 2);
    assert!((report_gpt4.total_cost_usd - 0.022).abs() < 1e-9);
}

#[test]
fn test_telemetry_aggregation_date_range_is_strict_and_iso_month_bucketed() {
    use crate::telemetry::{generate_telemetry_report, AggregationDimension, AggregationParams};
    let temp_dir = tempdir().unwrap();
    let cfg_path = temp_dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        r#"
[profiles.test-profile]
display_name = "Test Profile"
repo_id = "test-repo"
provider = "github"
repo = "test/repo"
local_path = "/tmp"
artifact_root = "/tmp"
default_target_branch = "main"
"#,
    )
    .unwrap();
    let mut cfg = crate::config::load(Some(cfg_path.to_str().unwrap())).unwrap();
    cfg.defaults.artifact_root = temp_dir.path().to_string_lossy().into_owned();

    let valid = create_test_ledger_entry();
    let mut malformed = create_test_ledger_entry();
    malformed.timestamp = "not-a-timestamp".to_string();
    std::fs::write(
        temp_dir.path().join("ledger.jsonl"),
        format!(
            "{}\n{}\n",
            serde_json::to_string(&valid).unwrap(),
            serde_json::to_string(&malformed).unwrap()
        ),
    )
    .unwrap();

    let params = AggregationParams {
        dimensions: vec![AggregationDimension::DateRange],
        since: Some("2026-07-10".to_string()),
        until: Some("2026-07-10T23:59:59Z".to_string()),
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts: true,
        project: None,
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: None,
        account: None,
    };
    let report = generate_telemetry_report(&cfg, params).unwrap();
    assert_eq!(report.total_entries, 1);
    assert_eq!(report.aggregated_data[0].dimension_value, "2026-07");

    let invalid_since = AggregationParams {
        dimensions: vec![AggregationDimension::Date],
        since: Some("2026-july-10".to_string()),
        until: None,
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts: true,
        project: None,
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: None,
        account: None,
    };
    assert!(generate_telemetry_report(&cfg, invalid_since).is_err());
}
