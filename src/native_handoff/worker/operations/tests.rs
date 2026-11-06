use super::*;

fn running() -> (tempfile::TempDir, DispatchBinding, WorkerRun) {
    let directory = crate::native_handoff::worker::tests::directory();
    let binding = crate::native_handoff::worker::tests::binding();
    let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    run.ready().unwrap();
    let grant = crate::native_handoff::worker::tests::grant(&binding);
    run.offer_start_checked(&grant, || Ok(()), |_, _| Ok(()))
        .unwrap();
    run.journal.execution = Some(Execution {
        invocation: "d".repeat(32),
        pid: std::process::id(),
        start_ticks: "1".into(),
        consumed_at: crate::clock::now_utc().format(&Rfc3339).unwrap(),
    });
    run.journal.launch = Some(super::super::super::host::JobIdentity {
        invocation: "d".repeat(32),
        pid: std::process::id(),
        start_ticks: "1".into(),
    });
    run.journal.consumption_offered = true;
    run.persist().unwrap();
    (directory, binding, run)
}

fn permission(request: &OperationRequest) -> OperationPermission {
    OperationPermission {
        allowed: true,
        run_id: request.run_id.clone(),
        node_id: request.node_id.clone(),
        context_sha256: request.context_sha256.clone(),
        reservation_id: request.reservation_id.clone(),
        sequence: request.sequence,
        purpose: request.purpose,
        authorized_at: crate::clock::now_utc().format(&Rfc3339).unwrap(),
        expires_at: (crate::clock::now_utc() + time::Duration::seconds(9))
            .format(&Rfc3339)
            .unwrap(),
    }
}

#[test]
fn pending_operation_survives_lost_reply_and_reopen_without_reissuing_http() {
    let (directory, binding, mut run) = running();
    let mut calls = 0;
    assert!(run
        .authorize_checked(
            OperationPurpose::Attempt,
            |_| {
                calls += 1;
                anyhow::bail!("lost response")
            },
            |_| Ok(())
        )
        .is_err());
    assert_eq!(calls, 1);
    drop(run);
    let mut run = WorkerRun::open(directory.path(), &binding).unwrap();
    assert_eq!(run.observation().unwrap()["operation"]["state"], "pending");
    assert!(run
        .authorize_checked(
            OperationPurpose::Attempt,
            |_| panic!("replayed HTTP"),
            |_| Ok(())
        )
        .is_err());
}

#[test]
fn permission_is_scoped_fresh_and_explicit_completion_advances_one_counter() {
    let (directory, binding, mut run) = running();
    let allowed = run
        .authorize_checked(
            OperationPurpose::Attempt,
            |request| Ok(permission(request)),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(allowed.sequence, 1);
    assert!(run
        .authorize_checked(
            OperationPurpose::Publish,
            |_| panic!("started before confirmed exit"),
            |_| Ok(())
        )
        .is_err());
    drop(run);
    let mut run = WorkerRun::open(directory.path(), &binding).unwrap();
    let mut wrong = allowed.clone();
    wrong.sequence = 2;
    assert!(run.complete_checked(&wrong, |_| Ok(())).is_err());
    run.complete_checked(&allowed, |_| Ok(())).unwrap();
    assert!(run.complete_checked(&allowed, |_| Ok(())).is_err());
    let second = run
        .authorize_checked(
            OperationPurpose::Publish,
            |request| Ok(permission(request)),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(second.sequence, 2);
}

#[test]
fn wrong_or_expired_permissions_leave_the_operation_held() {
    for field in [
        "allowed",
        "node_id",
        "reservation_id",
        "context_sha256",
        "sequence",
        "purpose",
        "authorized_at",
        "expires_at",
    ] {
        let (_directory, _binding, mut run) = running();
        assert!(
            run.authorize_checked(
                OperationPurpose::Attempt,
                |request| {
                    let mut value = serde_json::to_value(permission(request)).unwrap();
                    value[field] = match field {
                        "allowed" => false.into(),
                        "sequence" => 2.into(),
                        "purpose" => "publish".into(),
                        "authorized_at" | "expires_at" => "2000-01-01T00:00:00Z".into(),
                        _ => "wrong".into(),
                    };
                    Ok(serde_json::from_value(value).unwrap())
                },
                |_| Ok(())
            )
            .is_err(),
            "accepted incorrect {field}"
        );
        assert_eq!(run.observation().unwrap()["operation"]["state"], "pending");
    }
}

#[test]
fn lost_holder_after_http_and_unconfirmed_job_never_deliver_permission() {
    let (_directory, _binding, mut run) = running();
    let checks = std::cell::Cell::new(0);
    assert!(run
        .authorize_checked(
            OperationPurpose::Attempt,
            |request| Ok(permission(request)),
            |_| {
                checks.set(checks.get() + 1);
                ensure!(checks.get() == 1, "holder disappeared");
                Ok(())
            }
        )
        .is_err());
    assert_eq!(run.observation().unwrap()["operation"]["state"], "pending");
    assert!(run
        .authorize_operation(OperationPurpose::Attempt, |_| panic!(
            "unknown job sent HTTP"
        ))
        .is_err());
}

#[test]
fn uncertain_intent_write_and_malformed_execution_never_authorize() {
    let (directory, binding, mut run) = running();
    let path = directory.path().join("worker.json");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(run
        .authorize_checked(
            OperationPurpose::Attempt,
            |_| panic!("uncommitted HTTP"),
            |_| Ok(())
        )
        .is_err());
    assert!(run.observation().is_err());
    drop(run);
    assert!(WorkerRun::open(directory.path(), &binding).is_err());
    let (directory, binding, run) = running();
    drop(run);
    let path = directory.path().join("worker.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["execution"]["start_ticks"] = "unknown".into();
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(WorkerRun::open(directory.path(), &binding).is_err());
}

#[test]
fn public_worker_methods_reject_a_missing_job_and_source_scope_change() {
    let (_directory, _binding, mut run) = running();
    assert!(run
        .authorize_operation(OperationPurpose::Attempt, |_| panic!(
            "unobserved job reached central"
        ))
        .is_err());
    let (_temp, _central, worker, request) = crate::native_handoff::tests::fixture();
    let mut context = WorkerContext {
        config: worker,
        request,
        binding: run.journal.binding.clone(),
        history_entries: 0,
    };
    context.binding.model = Some("different".into());
    assert!(run
        .authorize_central(&context, OperationPurpose::Attempt, "fixture-owner")
        .is_err());
    assert!(run
        .enter(&StartConsumption {
            allowed: true,
            run_id: run.journal.binding.run_id.clone(),
            node_id: run.journal.binding.node_id.clone(),
            context_sha256: run.journal.binding.context_sha256.clone(),
            reservation_id: "a".repeat(32),
            consumed_at: crate::clock::now_utc().format(&Rfc3339).unwrap(),
            expires_at: run.journal.start.as_ref().unwrap().expires_at.clone()
        })
        .is_err());
}

#[test]
fn central_denials_keep_known_reasons_without_echoing_remote_data() {
    let denied = crate::curl_http::CurlResponse {
        status: 409,
        body: br#"{"allowed":false,"reason":"budget_held","detail":"PRIVATE_REMOTE_PAYLOAD"}"#
            .to_vec(),
    };
    let error = response_body(denied).unwrap_err().to_string();
    assert!(error.contains("budget_held"));
    assert!(!error.contains("PRIVATE_REMOTE_PAYLOAD"));
    let unknown = crate::curl_http::CurlResponse {
        status: 503,
        body: br#"{"allowed":false,"reason":"PRIVATE_REMOTE_PAYLOAD"}"#.to_vec(),
    };
    let error = response_body(unknown).unwrap_err().to_string();
    assert!(error.contains("authority_unavailable"));
    assert!(!error.contains("PRIVATE_REMOTE_PAYLOAD"));
}

#[test]
fn legacy_unstarted_journal_upgrades_on_write_without_discarding_its_fence() {
    let directory = crate::native_handoff::worker::tests::directory();
    let binding = crate::native_handoff::worker::tests::binding();
    let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    run.ready().unwrap();
    drop(run);
    let path = directory.path().join("worker.json");
    let mut legacy: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    legacy["version"] = 1.into();
    for key in ["execution", "operation", "consumption_offered", "launch"] {
        legacy.as_object_mut().unwrap().remove(key);
    }
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut run = WorkerRun::open(directory.path(), &binding).unwrap();
    let grant = crate::native_handoff::worker::tests::grant(&binding);
    run.offer_start_checked(&grant, || Ok(()), |_, _| Ok(()))
        .unwrap();
    drop(run);
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["version"], 3);
    assert_eq!(stored["phase"], "start_intent");
    let mut reopened = WorkerRun::open(directory.path(), &binding).unwrap();
    assert!(reopened
        .offer_start_checked(&grant, || panic!("legacy fence was reset"), |_, _| Ok(()))
        .is_err());
}

#[test]
fn version_two_consumed_identity_survives_upgrade_without_new_authority() {
    let (directory, binding, run) = running();
    drop(run);
    let path = directory.path().join("worker.json");
    let mut legacy: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    legacy["version"] = 2.into();
    legacy.as_object_mut().unwrap().remove("launch");
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut run = WorkerRun::open(directory.path(), &binding).unwrap();
    run.authorize_checked(
        OperationPurpose::Attempt,
        |request| Ok(permission(request)),
        |_| Ok(()),
    )
    .unwrap();
    drop(run);
    let upgraded: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(upgraded["version"], 3);
    for field in ["invocation", "pid", "start_ticks"] {
        assert_eq!(upgraded["launch"][field], legacy["execution"][field]);
    }
    assert!(WorkerRun::open(directory.path(), &binding).is_ok());
}

#[test]
fn execution_cannot_reopen_with_a_different_recorded_launch() {
    let (directory, binding, run) = running();
    drop(run);
    let path = directory.path().join("worker.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["launch"]["pid"] = (std::process::id() + 1).into();
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(WorkerRun::open(directory.path(), &binding).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
