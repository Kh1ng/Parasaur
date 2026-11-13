//! Telemetry Module Tests
//!
//! Comprehensive tests for the telemetry export functionality.

use super::exporter::*;
use super::extractor::*;
use super::records::*;
use crate::ledger::LedgerEntry;
use tempfile::tempdir;

mod aggregation;

#[cfg(test)]
pub(crate) mod telemetry_tests {
    use super::*;

    #[test]
    fn streamed_ledger_export_matches_bulk_dedup_and_rejects_partial_export() {
        let (_tmp, cfg) = crate::ledger::test_util::test_config();
        let dir = tempdir().unwrap();
        let config = |name: &str| TelemetryConfig {
            telemetry_repo_path: dir.path().join(name),
            format: ExportFormat::Jsonl,
            generate_manifests: false,
            commit_batch_size: None,
        };
        let mut newer = create_test_ledger_entry();
        newer.timestamp = "2026-11-05T12:00:00Z".into();
        let mut older = newer.clone();
        older.timestamp = "2026-11-04T12:00:00Z".into();
        let entries = [newer, older];
        let journal = entries
            .iter()
            .map(|entry| serde_json::to_string(entry).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(cfg.defaults.ledger_path(), &journal).unwrap();
        let mut bulk = TelemetryExporter::new(config("bulk")).unwrap();
        bulk.export_from_entries(&entries).unwrap();
        let mut streamed = TelemetryExporter::new(config("streamed")).unwrap();
        assert_eq!(
            streamed.export_from_ledger(&cfg).unwrap().as_deref(),
            Some(entries[0].timestamp.as_str())
        );
        assert!(bulk.records_exported() > 0);
        assert_eq!(bulk.records_exported(), streamed.records_exported());
        // Persisted IDs must match as well as counts; reloading also exercises
        // the streamed partition reader and prevents duplicate exports.
        let mut bulk_reloaded = TelemetryExporter::new(config("bulk")).unwrap();
        bulk_reloaded.load_exported_ids().unwrap();
        bulk_reloaded.export_from_ledger(&cfg).unwrap();
        assert_eq!(bulk_reloaded.records_exported(), 0);
        let mut reloaded = TelemetryExporter::new(config("streamed")).unwrap();
        reloaded.load_exported_ids().unwrap();
        reloaded.export_from_entries(&entries).unwrap();
        assert_eq!(reloaded.records_exported(), 0);
        std::fs::write(cfg.defaults.ledger_path(), format!("{journal}\n{{broken\n")).unwrap();
        let mut invalid = TelemetryExporter::new(config("invalid")).unwrap();
        assert!(invalid.export_from_ledger(&cfg).is_err());
        assert_eq!(invalid.records_exported(), 0);
        let mut after_failure = TelemetryExporter::new(config("invalid")).unwrap();
        after_failure.load_exported_ids().unwrap();
        after_failure.export_from_entries(&entries).unwrap();
        assert_eq!(after_failure.records_exported(), bulk.records_exported());
    }

    #[test]
    fn test_partition_key_parsing() {
        let key = PartitionKey::from_date_str("2026-07-10T12:34:56Z").unwrap();
        assert_eq!(key.year, 2026);
        assert_eq!(key.month, 7);
        assert_eq!(key.day, 10);
        assert_eq!(key.to_path_string(), "2026/07/10");
        assert_eq!(key.to_date_string(), "2026-07-10");
    }

    #[test]
    fn test_partition_key_from_date_only() {
        let key = PartitionKey::from_date_str("2026-07-10").unwrap();
        assert_eq!(key.year, 2026);
        assert_eq!(key.month, 7);
        assert_eq!(key.day, 10);
        assert_eq!(key.to_date_string(), "2026-07-10");
    }

    #[test]
    fn test_generate_record_ids() {
        let id1 = generate_attempt_usage_id(
            "2026-07-10T12:00:00Z",
            Some("work-123"),
            1,
            "backend-a",
            Some("model-x"),
        );
        let id2 = generate_attempt_usage_id(
            "2026-07-10T12:00:00Z",
            Some("work-123"),
            1,
            "backend-a",
            Some("model-x"),
        );

        // Same inputs should produce same ID
        assert_eq!(id1, id2);

        // Different attempt number should produce different ID
        let id3 = generate_attempt_usage_id(
            "2026-07-10T12:00:00Z",
            Some("work-123"),
            2,
            "backend-a",
            Some("model-x"),
        );
        assert_ne!(id1, id3);

        // Different work ID should produce different ID
        let id4 = generate_attempt_usage_id(
            "2026-07-10T12:00:00Z",
            Some("work-456"),
            1,
            "backend-a",
            Some("model-x"),
        );
        assert_ne!(id1, id4);
    }

    #[test]
    fn test_schema_version_in_record() {
        assert_eq!(
            SCHEMA_VERSION, 13,
            "skill provenance exports must be distinguishable from schema v12"
        );
        let base = TelemetryRecord {
            schema_version: SCHEMA_VERSION,
            record_id: "test-123".to_string(),
            exported_at: "2026-07-10T12:00:00Z".to_string(),
            observed_at: "2026-07-10T12:00:00Z".to_string(),
        };

        let record = AttemptUsageRecord {
            base: base.clone(),
            profile: "test".to_string(),
            repo_id: "test".to_string(),
            repo: "test".to_string(),
            provider: "test".to_string(),
            work_id: None,
            target_summary: None,
            mode: "test".to_string(),
            attempt_number: 0,
            skills: Vec::new(),
            backend: "test".to_string(),
            runner_kind: None,
            effective_backend: "test".to_string(),
            requested_backend: "test".to_string(),
            effective_model: None,
            requested_model: None,
            actual_model: None,
            actual_model_unknown_reason: None,
            exit_code: None,
            duration_seconds: None,
            validation_result: None,
            failure_class: None,
            failure_stage: None,
            fallback_used: false,
            human_required: false,
            routing_reason: None,
            cpu_time: None,
            peak_rss: None,
            usage_source: None,
            usage_classification: None,
            backend_instance: None,
            model_provider: None,
            provider_attribution_source: None,
            model_provider_unknown_reason: None,
            account_label: None,
            auth_source_label: None,
            quota_pool: None,
            pricing_source: None,
            pricing_version: None,
            cost_unknown_reason: None,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: None,
            requests_count: None,
            estimated_cost_usd: None,
            actual_cost_usd: None,
            token_usage_unknown_reason: None,
            tool_calls: None,
            shell_calls: None,
            file_edits: None,
            test_runs: None,
        };

        let exported = ExportedTelemetryRecord::AttemptUsage(Box::new(record));
        let json = serde_json::to_string(&exported).unwrap();
        // println!("JSON: {}", json); // Debug output
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Check that schema_version is present (inside data due to enum tag)
        assert_eq!(parsed["record_type"], "attempt_usage");
        assert_eq!(parsed["data"]["schema_version"], SCHEMA_VERSION);
    }

    #[test]
    fn attempt_usage_deserializes_before_reasoning_and_unknown_reason_fields() {
        let value = serde_json::from_str::<serde_json::Value>(
            r#"{
            "schema_version": 5,
            "record_id": "legacy-attempt",
            "exported_at": "2026-07-10T12:00:00Z",
            "observed_at": "2026-07-10T12:00:00Z",
            "profile": "test",
            "repo_id": "test",
            "repo": "o/r",
            "provider": "github",
            "work_id": null,
            "target_summary": null,
            "mode": "review",
            "attempt_number": 1,
            "backend": "codex",
            "effective_backend": "codex",
            "requested_backend": "codex",
            "effective_model": null,
            "requested_model": null,
            "actual_model": null,
            "actual_model_unknown_reason": null,
            "exit_code": 0,
            "duration_seconds": 1.0,
            "validation_result": null,
            "failure_class": null,
            "failure_stage": null,
            "fallback_used": false,
            "human_required": false,
            "routing_reason": null,
            "usage_source": null,
            "usage_classification": "quota_backed",
            "backend_instance": "codex",
            "model_provider": "openai",
            "model_provider_unknown_reason": null,
            "account_label": null,
            "pricing_source": null,
            "pricing_version": null,
            "cost_unknown_reason": null,
            "input_tokens": 10,
            "output_tokens": 2,
            "cache_read_tokens": null,
            "cache_write_tokens": null,
            "total_tokens": 12,
            "requests_count": 1,
            "estimated_cost_usd": null,
            "actual_cost_usd": null,
            "quota_window": "weekly",
            "quota_used_percent": 25,
            "quota_remaining_percent": 75,
            "quota_reset_at": "2026-01-12T00:00:00Z",
            "quota_unknown_reason": null
        }"#,
        )
        .unwrap();

        let record: AttemptUsageRecord = serde_json::from_value(value).unwrap();
        assert_eq!(record.reasoning_tokens, None);
        assert_eq!(record.token_usage_unknown_reason, None);
        let written = serde_json::to_value(record).unwrap();
        for key in [
            "quota_window",
            "quota_used_percent",
            "quota_remaining_percent",
            "quota_reset_at",
            "quota_unknown_reason",
        ] {
            assert!(
                written.get(key).is_none(),
                "retired attempt key {key} must not be exported"
            );
        }
    }

    #[test]
    fn test_telemetry_record_serialization() {
        let base = TelemetryRecord {
            schema_version: SCHEMA_VERSION,
            record_id: "test-task-123".to_string(),
            exported_at: "2026-07-10T12:00:00Z".to_string(),
            observed_at: "2026-07-10T12:00:00Z".to_string(),
        };

        let record = TaskOutcomeRecord {
            skills: Vec::new(),
            base,
            profile: "test-profile".to_string(),
            repo_id: "test-repo".to_string(),
            repo: "test/repo".to_string(),
            provider: "github".to_string(),
            work_id: "work-123".to_string(),
            target_summary: Some("test target".to_string()),
            mode: "fix".to_string(),
            branch: Some("main".to_string()),
            dispatch_reason: None,
            attempts_started: Some(1),
            attempts_completed: Some(1),
            duration_seconds: Some(100.0),
            backend_exit_code: Some(0),
            validation_result: Some("pass".to_string()),
            review_verdict: None,
            review_confidence: None,
            reviewer_backend: None,
            reviewer_model: None,
            review_gate_reason: None,
            review_actionable_findings: vec![],
            commit_attempted: true,
            commit_created: true,
            push_attempted: true,
            push_succeeded: true,
            mr_attempted: false,
            mr_created: false,
            mr_url: None,
            files_changed: Some(5),
            insertions: Some(100),
            deletions: Some(50),
            failure_class: None,
            failure_stage: None,
            error_summary: None,
            usage_source: None,
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_tokens: Some(100),
            cache_write_tokens: Some(50),
            total_tokens: Some(1500),
            requests_count: Some(10),
            estimated_cost_usd: Some(0.1),
            actual_cost_usd: Some(0.15),
            final_outcome: Some("SUCCESS".to_string()),
            merge_status: None,
        };

        let exported = ExportedTelemetryRecord::TaskOutcome(Box::new(record));
        let json = serde_json::to_string(&exported).unwrap();

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("record_type").is_some());
        assert_eq!(parsed["record_type"], "task_outcome");

        // Check that the work_id is preserved
        assert_eq!(parsed["data"]["work_id"], "work-123");
        assert_eq!(parsed["data"]["mode"], "fix");

        // Historical exports predate the optional gate-reason field. Tagged
        // enum deserialization must keep accepting them after schema v4.
        let mut legacy = serde_json::to_value(&exported).unwrap();
        legacy["data"]
            .as_object_mut()
            .unwrap()
            .remove("review_gate_reason");
        let restored: ExportedTelemetryRecord = serde_json::from_value(legacy).unwrap();
        match restored {
            ExportedTelemetryRecord::TaskOutcome(record) => {
                assert_eq!(record.review_gate_reason, None)
            }
            other => panic!("expected task outcome, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_telemetry_records_basic() {
        let entry = create_test_ledger_entry();
        let exported_at = "2026-07-10T13:00:00Z";
        let records = extract_telemetry_records(&entry, exported_at);

        // Should have at least task outcome record
        assert!(!records.is_empty());

        // Check that schema version is set correctly
        for record in &records {
            match record {
                ExportedTelemetryRecord::AttemptUsage(r) => {
                    assert_eq!(r.base.schema_version, SCHEMA_VERSION);
                }
                ExportedTelemetryRecord::QuotaObservation(r) => {
                    assert_eq!(r.base.schema_version, SCHEMA_VERSION);
                }
                ExportedTelemetryRecord::TaskOutcome(r) => {
                    assert_eq!(r.base.schema_version, SCHEMA_VERSION);
                }
                ExportedTelemetryRecord::ReviewOutcome(r) => {
                    assert_eq!(r.base.schema_version, SCHEMA_VERSION);
                }
            }
        }
    }

    #[test]
    fn test_unknown_values_remain_unknown() {
        let entry = create_test_ledger_entry();
        // Modify the entry to have unknown values
        let mut entry = entry;
        entry.requested_model = None;
        entry.effective_model = None;
        entry.duration_seconds = None;
        entry.backend_exit_code = None;
        entry.validation_result = None;
        entry.usage.input_tokens = None;
        entry.usage.output_tokens = None;
        entry.usage.estimated_cost_usd = None;
        entry.usage.actual_cost_usd = None;

        let exported_at = "2026-07-10T13:00:00Z";
        let records = extract_telemetry_records(&entry, exported_at);

        for record in &records {
            if let ExportedTelemetryRecord::TaskOutcome(r) = record {
                // Check that unknown values remain None
                assert!(r.duration_seconds.is_none());
                assert!(r.backend_exit_code.is_none());
                assert!(r.validation_result.is_none());
                assert!(r.input_tokens.is_none());
                assert!(r.output_tokens.is_none());
                assert!(r.estimated_cost_usd.is_none());
                assert!(r.actual_cost_usd.is_none());
            }
        }
    }

    #[test]
    fn test_no_double_counting_of_usage() {
        use crate::ledger::{AttemptRecord, LedgerUsage};

        let _attempt = AttemptRecord {
            resources: None,
            attempt_number: 1,
            backend: "test-backend".to_string(),
            effective_model: Some("test-model".to_string()),
            exit_code: Some(0),
            validation_result: Some("pass".to_string()),
            failure_class: None,
            failure_stage: None,
            duration_seconds: Some(50.0),
            diff_path: None,
            checkpoint_branch: None,
            checkpoint_sha: None,
            cli_version: None,
            usage: LedgerUsage {
                usage_unknown_reason: None,
                usage_source: Some("attempt".to_string()),
                behavior_metrics: None,
                usage_classification: None,
                backend_instance: None,
                provider: None,
                actual_model: None,
                actual_model_unknown_reason: None,
                provider_unknown_reason: None,
                account_label: None,
                auth_source_label: None,
                quota_pool: None,
                provider_attribution_source: None,
                pricing_source: None,
                pricing_version: None,
                cost_unknown_reason: None,
                observed_at: Some("2026-07-10T12:30:00Z".to_string()),
                input_tokens: Some(500),
                output_tokens: Some(250),
                reasoning_tokens: None,
                cache_read_tokens: Some(50),
                cache_write_tokens: Some(25),
                total_tokens: Some(750),
                requests_count: Some(5),
                estimated_cost_usd: Some(0.05),
                actual_cost_usd: Some(0.06),
                token_usage_unknown_reason: None,
            },
        };

        let mut entry = create_test_ledger_entry();
        // Add an attempt with different usage
        let attempt = AttemptRecord {
            resources: None,
            attempt_number: 1,
            backend: "test-backend".to_string(),
            effective_model: Some("test-model".to_string()),
            exit_code: Some(0),
            validation_result: Some("pass".to_string()),
            failure_class: None,
            failure_stage: None,
            duration_seconds: Some(50.0),
            diff_path: None,
            checkpoint_branch: None,
            checkpoint_sha: None,
            cli_version: None,
            usage: LedgerUsage {
                usage_unknown_reason: None,
                usage_source: Some("attempt".to_string()),
                behavior_metrics: None,
                usage_classification: None,
                backend_instance: None,
                provider: None,
                actual_model: None,
                actual_model_unknown_reason: None,
                provider_unknown_reason: None,
                account_label: None,
                auth_source_label: None,
                quota_pool: None,
                provider_attribution_source: None,
                pricing_source: None,
                pricing_version: None,
                cost_unknown_reason: None,
                observed_at: Some("2026-07-10T12:30:00Z".to_string()),
                input_tokens: Some(500),
                output_tokens: Some(250),
                reasoning_tokens: None,
                cache_read_tokens: Some(50),
                cache_write_tokens: Some(25),
                total_tokens: Some(750),
                requests_count: Some(5),
                estimated_cost_usd: Some(0.05),
                actual_cost_usd: Some(0.06),
                token_usage_unknown_reason: None,
            },
        };
        entry.attempts = vec![attempt];
        // Update entry-level usage to be different from attempt
        entry.usage = LedgerUsage {
            usage_unknown_reason: None,
            usage_source: Some("entry".to_string()),
            behavior_metrics: None,
            usage_classification: None,
            backend_instance: None,
            provider: None,
            actual_model: None,
            actual_model_unknown_reason: None,
            provider_unknown_reason: None,
            account_label: None,
            auth_source_label: None,
            quota_pool: None,
            provider_attribution_source: None,
            pricing_source: None,
            pricing_version: None,
            cost_unknown_reason: None,
            observed_at: Some("2026-07-10T12:00:00Z".to_string()),
            input_tokens: Some(1000), // Different from attempt
            output_tokens: Some(500),
            reasoning_tokens: None,
            cache_read_tokens: Some(100),
            cache_write_tokens: Some(50),
            total_tokens: Some(1500),
            requests_count: Some(10),
            estimated_cost_usd: Some(0.10),
            actual_cost_usd: Some(0.12),
            token_usage_unknown_reason: None,
        };

        let exported_at = "2026-07-10T13:00:00Z";
        let records = extract_telemetry_records(&entry, exported_at);

        // Count attempt usage records vs task outcome records
        let attempt_usage_count = records
            .iter()
            .filter(|r| matches!(r, ExportedTelemetryRecord::AttemptUsage(_)))
            .count();
        let task_outcome_count = records
            .iter()
            .filter(|r| matches!(r, ExportedTelemetryRecord::TaskOutcome(_)))
            .count();

        // Should have 1 attempt usage record and 1 task outcome record
        assert!(attempt_usage_count >= 1);
        assert!(task_outcome_count >= 1);

        // Check that the task outcome record doesn't include the attempt's specific usage
        for record in &records {
            if let ExportedTelemetryRecord::TaskOutcome(task_record) = record {
                // Task outcome should have the entry-level usage, not the attempt-level
                assert_eq!(task_record.input_tokens, Some(1000));
                assert_ne!(task_record.input_tokens, Some(500)); // Not the attempt's tokens
            }
        }
    }

    #[test]
    fn attempt_export_uses_canonical_route_identity() {
        let mut entry = create_test_ledger_entry();
        entry.requested_backend = "stale-entry-value".into();
        entry.attempts = vec![crate::ledger::AttemptRecord {
            resources: None,
            attempt_number: 1,
            backend: "agy-second".into(),
            effective_model: Some("gemini-pro".into()),
            usage: crate::ledger::LedgerUsage {
                backend_instance: Some("agy-second:google".into()),
                account_label: Some("google-secondary".into()),
                auth_source_label: Some("agy-secondary-session".into()),
                quota_pool: Some("google".into()),
                provider: Some("google".into()),
                provider_attribution_source: Some("backend_reported".into()),
                ..Default::default()
            },
            ..Default::default()
        }];
        let mut identity = crate::execution_identity::ExecutionIdentity::legacy_route(
            "auto",
            Some("requested-gemini"),
            "agy-second",
            Some("gemini-pro"),
            Some("google"),
        );
        identity.account_label = Some("google-secondary".into());
        identity.auth_source_label = Some("agy-secondary-session".into());
        entry.attempt_routing = vec![crate::ledger::AttemptRoutingRecord {
            skills: Vec::new(),
            attempt_number: 1,
            backend_instance: identity.backend_instance.clone(),
            effective_model: identity.effective_model.clone(),
            identity: Some(identity),
            routing_diagnostics: None,
        }];

        let records = crate::telemetry::extractor::extract_attempt_usage_records(
            &entry,
            "2026-07-10T13:00:00Z",
        );
        let record = &records[0];
        assert_eq!(record.runner_kind.as_deref(), Some("agy"));
        assert_eq!(record.requested_backend, "auto");
        assert_eq!(record.requested_model.as_deref(), Some("requested-gemini"));
        assert_eq!(
            record.backend_instance.as_deref(),
            Some("agy-second:google")
        );
        assert_eq!(
            record.auth_source_label.as_deref(),
            Some("agy-secondary-session")
        );
        assert_eq!(record.quota_pool.as_deref(), Some("google"));
        assert_eq!(
            record.provider_attribution_source.as_deref(),
            Some("backend_reported")
        );
    }

    #[test]
    fn historical_attempt_export_does_not_invent_identity_or_provider() {
        let mut entry = create_test_ledger_entry();
        entry.provider = "github".into();
        entry.usage = crate::ledger::LedgerUsage {
            backend_instance: Some("different-summary-instance".into()),
            provider: Some("different-summary-provider".into()),
            usage_classification: Some("api_key_backed".into()),
            account_label: Some("different-summary-account".into()),
            ..Default::default()
        };
        entry.attempts = vec![crate::ledger::AttemptRecord {
            resources: None,
            attempt_number: 1,
            backend: "legacy-backend".into(),
            effective_model: None,
            usage: crate::ledger::LedgerUsage::default(),
            ..Default::default()
        }];

        let records = crate::telemetry::extractor::extract_attempt_usage_records(
            &entry,
            "2026-07-10T13:00:00Z",
        );
        let record = &records[0];
        assert_eq!(record.runner_kind, None);
        assert_eq!(record.backend_instance, None);
        assert_eq!(record.model_provider, None);
        assert_eq!(record.usage_classification, None);
        assert_eq!(record.account_label, None);
        assert_eq!(record.auth_source_label, None);
        assert_eq!(record.quota_pool, None);
    }

    #[test]
    fn test_export_idempotency() {
        let entry = create_test_ledger_entry();
        let exported_at = "2026-07-10T13:00:00Z";
        let records1 = extract_telemetry_records(&entry, exported_at);
        let records2 = extract_telemetry_records(&entry, exported_at);

        // Same entry should produce same records with same IDs
        assert_eq!(records1.len(), records2.len());

        for (r1, r2) in records1.iter().zip(records2.iter()) {
            assert_eq!(r1.get_id(), r2.get_id());
        }
    }

    #[test]
    fn test_final_outcome_determination() {
        // Test APPROVE
        let mut entry = create_test_ledger_entry();
        entry.review_verdict = Some("APPROVE".to_string());
        let outcome = determine_final_outcome(&entry);
        assert_eq!(outcome, Some("APPROVE".to_string()));

        // Test NEEDS_FIX
        entry.review_verdict = Some("NEEDS_FIX".to_string());
        let outcome = determine_final_outcome(&entry);
        assert_eq!(outcome, Some("NEEDS_FIX".to_string()));

        // Test validation passed
        entry.review_verdict = None;
        entry.validation_result = Some("pass".to_string());
        entry.commit_created = false;
        entry.push_succeeded = false;
        let outcome = determine_final_outcome(&entry);
        assert_eq!(outcome, Some("VALIDATION_PASSED".to_string()));

        // Dispatch records a pass as "passed"; "pass" above is the legacy spelling.
        entry.validation_result = Some("passed".to_string());
        let outcome = determine_final_outcome(&entry);
        assert_eq!(outcome, Some("VALIDATION_PASSED".to_string()));

        // FailureClass is stored in its snake_case wire form.
        entry.validation_result = None;
        entry.failure_class = Some(
            crate::ledger::FailureClass::HumanBlocked
                .as_str()
                .to_string(),
        );
        let outcome = determine_final_outcome(&entry);
        assert_eq!(outcome, Some("HUMAN_BLOCKED".to_string()));
        entry.failure_class = None;

        // Test success
        entry.validation_result = None;
        entry.backend_exit_code = Some(0);
        let outcome = determine_final_outcome(&entry);
        assert_eq!(outcome, Some("SUCCESS".to_string()));
    }

    #[test]
    fn test_telemetry_exporter_basic() {
        let temp_dir = tempdir().unwrap();
        let telemetry_path = temp_dir.path().join("telemetry");

        let config = TelemetryConfig {
            telemetry_repo_path: telemetry_path.clone(),
            format: ExportFormat::Jsonl,
            generate_manifests: false,
            commit_batch_size: None,
        };

        let mut exporter = TelemetryExporter::new(config).unwrap();

        // Test with empty entries
        let entries: Vec<LedgerEntry> = vec![];
        exporter.export_from_entries(&entries).unwrap();

        assert_eq!(exporter.records_exported(), 0);

        // Test with one entry
        let entry = create_test_ledger_entry();
        exporter.export_from_entries(&[entry]).unwrap();

        // Should have exported some records
        assert!(exporter.records_exported() > 0);

        // Verify repository structure was created
        assert!(telemetry_path.join("raw").join("usage").exists());
        assert!(telemetry_path.join("raw").join("quota").exists());
        assert!(telemetry_path.join("raw").join("outcomes").exists());
        assert!(telemetry_path.join("exports").exists());
        assert!(telemetry_path.join("manifests").exists());
        assert!(telemetry_path.join("schemas").exists());
    }

    #[test]
    fn test_deduplication() {
        let temp_dir = tempdir().unwrap();
        let telemetry_path = temp_dir.path().join("telemetry");

        let config = TelemetryConfig {
            telemetry_repo_path: telemetry_path.clone(),
            format: ExportFormat::Jsonl,
            generate_manifests: false,
            commit_batch_size: None,
        };

        let mut exporter = TelemetryExporter::new(config).unwrap();

        // Create test entries
        let entry = create_test_ledger_entry();

        // First export
        exporter
            .export_from_entries(std::slice::from_ref(&entry))
            .unwrap();
        let first_export_count = exporter.records_exported();
        assert!(first_export_count > 0);

        // Second export of the same entry should not export duplicates
        exporter.export_from_entries(&[entry]).unwrap();
        let second_export_count = exporter.records_exported();

        // Should be the same count (no new records exported)
        assert_eq!(first_export_count, second_export_count);
    }

    #[test]
    fn test_manifest_generation() {
        let temp_dir = tempdir().unwrap();
        let telemetry_path = temp_dir.path().join("telemetry");

        let config = TelemetryConfig {
            telemetry_repo_path: telemetry_path.clone(),
            format: ExportFormat::Jsonl,
            generate_manifests: true,
            commit_batch_size: None,
        };

        let mut exporter = TelemetryExporter::new(config).unwrap();

        // Export some entries
        let entry = create_test_ledger_entry();
        exporter.export_from_entries(&[entry]).unwrap();

        // Check that manifests directory was created and has files
        let manifests_dir = telemetry_path.join("manifests");
        assert!(manifests_dir.exists());

        // Count manifest files
        let manifest_files: Vec<_> = std::fs::read_dir(&manifests_dir)
            .unwrap()
            .filter_map(|entry| {
                let entry = entry.unwrap();
                if entry
                    .path()
                    .extension()
                    .map(|e| e == "json")
                    .unwrap_or(false)
                {
                    Some(entry.path())
                } else {
                    None
                }
            })
            .collect();

        // Should have at least one manifest file
        assert!(!manifest_files.is_empty());
    }

    // Helper function to create a test ledger entry
    pub(crate) fn create_test_ledger_entry() -> LedgerEntry {
        LedgerEntry {
            remote_execution: None,
            skills: Vec::new(),
            timestamp: "2026-07-10T12:00:00Z".to_string(),
            schema_version: crate::ledger::LEDGER_SCHEMA_VERSION,
            session_id: None,
            profile: "test-profile".to_string(),
            display_name: "Test Profile".to_string(),
            repo_id: "test-repo".to_string(),
            repo: "test/repo".to_string(),
            local_path: "/tmp/test/repo".to_string(),
            provider: "github".to_string(),
            backend: "test-backend".to_string(),
            requested_backend: "test-backend".to_string(),
            effective_backend: "test-backend".to_string(),
            requested_model: Some("test-model".to_string()),
            effective_model: Some("test-model".to_string()),
            routing_reason: None,
            fallback_used: false,
            confidence_impact: None,
            human_required: false,
            human_required_reason_code: None,
            routing_diagnostics: None,
            mode: "fix".to_string(),
            target_summary: Some("test target".to_string()),
            work_id: Some("work-123".to_string()),
            source_issue_number: None,
            work_title: None,
            task_class: None,
            difficulty: None,
            predicted_difficulty: None,
            predicted_cost_usd: None,
            predicted_duration_seconds: None,
            branch: Some("main".to_string()),
            session_dir: None,
            duration_seconds: Some(100.0),
            backend_exit_code: Some(0),
            validation_result: Some("pass".to_string()),
            review_verdict: None,
            review_confidence: None,
            reviewer_backend: None,
            reviewer_model: None,
            reviewer_tier: None,
            review_source_sha: None,
            review_metadata_fingerprint: None,
            review_contract_version: None,
            review_generation: None,
            reviewer_class: None,
            review_gate_reason: None,
            review_blocking_findings: vec![],
            review_actionable_findings: vec![],
            review_non_blocking_findings: vec![],
            review_risk_notes: vec![],
            review_evidence: vec![],
            review_compatibility_evidence: vec![],
            review_timeout_class: None,
            review_idle_timeout_seconds: None,
            review_hard_timeout_seconds: None,
            review_last_progress_secs: None,
            external_approval: None,
            commit_attempted: true,
            commit_created: true,
            push_attempted: true,
            push_succeeded: true,
            mr_attempted: false,
            mr_created: false,
            mr_url: None,
            provider_mutation_kind: None,
            provider_mutation_status: None,
            provider_mutation_url: None,
            pm_plan_fingerprint: None,
            pm_publication_status: None,
            pm_child_issue_numbers: vec![],
            pm_decomposition_depth: None,
            pm_publication_state_path: None,
            files_changed: Some(5),
            insertions: Some(100),
            deletions: Some(50),
            error_summary: None,
            failure_class: None,
            failure_stage: None,
            attempts_started: Some(1),
            attempts_completed: Some(1),
            resumable_checkpoint_branch: None,
            resumable_checkpoint_sha: None,
            attempts: vec![],
            attempt_routing: vec![],
            routing_runtime: Default::default(),
            dispatch_reason: None,
            context_phase: None,
            context_estimated_tokens_before: None,
            context_estimated_tokens_after: None,
            context_compacted: false,
            usage: crate::ledger::LedgerUsage::default(),
            memory_gateway_recall_hit: false,
            memory_gateway_context_byte_size: None,
            memory_gateway_capture_l0_recorded: None,
        }
    }

    /// Issue #240 acceptance #3: when a ledger entry lacks attempt counters
    /// (pre-tracking), the extracted telemetry `TaskOutcomeRecord` carries
    /// `None` (unknown) rather than `0` — so the telemetry export separates
    /// unknown from a real zero.
    #[test]
    fn telemetry_extract_separates_unknown_attempts_from_zero() {
        let raw_legacy = r#"{
            "timestamp": "2026-07-10T00:00:00Z",
            "profile": "test-profile",
            "display_name": "Test Profile",
            "repo_id": "test-repo",
            "repo": "test/repo",
            "local_path": "/tmp",
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
        let legacy: LedgerEntry = serde_json::from_str(raw_legacy).unwrap();
        let legacy_records = extract_task_outcome_records(&legacy, "2026-07-10T00:00:00Z");
        assert_eq!(legacy_records.len(), 1);
        assert_eq!(
            legacy_records[0].attempts_started, None,
            "pre-tracking entry must extract as unknown, not 0"
        );
        assert_eq!(legacy_records[0].attempts_completed, None);

        let mut known = create_test_ledger_entry();
        known.attempts_started = Some(2);
        known.attempts_completed = Some(1);
        let known_records = extract_task_outcome_records(&known, "2026-07-10T00:00:00Z");
        assert_eq!(known_records[0].attempts_started, Some(2));
        assert_eq!(known_records[0].attempts_completed, Some(1));
    }
}
