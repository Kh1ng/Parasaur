use super::*;

fn execution() -> (tempfile::TempDir, Execution) {
    let (directory, central, worker, request) = crate::native_handoff::tests::fixture();
    let policy = crate::native_handoff::policy::prepare(&central, &request.args.profile).unwrap();
    let binding =
        crate::native_handoff::binding(&request, "worker", &policy, &"a".repeat(64)).unwrap();
    let work = directory.path().join("scope");
    crate::native_handoff::create_directory(&work).unwrap();
    (
        directory,
        Execution {
            context: WorkerContext {
                config: worker,
                request,
                binding,
                history_entries: 0,
            },
            work,
            token: "fixture-owner".into(),
            unconfirmed: AtomicBool::new(false),
            provider_gate: Mutex::new(()),
        },
    )
}

fn permission(execution: &Execution) -> OperationPermission {
    let binding = &execution.context.binding;
    let now = crate::clock::now_utc();
    let format = &time::format_description::well_known::Rfc3339;
    OperationPermission {
        allowed: true,
        run_id: binding.run_id.clone(),
        node_id: binding.node_id.clone(),
        context_sha256: binding.context_sha256.clone(),
        reservation_id: "b".repeat(32),
        sequence: 1,
        purpose: OperationPurpose::Attempt,
        authorized_at: now.format(format).unwrap(),
        expires_at: (now + time::Duration::seconds(9)).format(format).unwrap(),
    }
}

#[test]
fn refused_authority_never_enters_attempt_bookkeeping_or_backend() {
    let (_directory, execution) = execution();
    let mut starts = 0;
    let result: Result<()> = operate(
        &execution,
        || anyhow::bail!("factory_paused"),
        || {
            starts += 1;
            Ok(())
        },
        |_| true,
    );
    assert!(result.is_err());
    assert!(authority_held(result.as_ref().unwrap_err()));
    assert_eq!(starts, 0);
    assert!(execution.unconfirmed.load(Ordering::SeqCst));
}

#[test]
fn completion_uncertainty_preserves_real_exit_and_usage_evidence() {
    let (_directory, execution) = execution();
    // No matching real job exists in this unit fixture, so completion must be
    // held. The actual operation result must still reach ledger bookkeeping.
    let result = operate(
        &execution,
        || Ok(permission(&execution)),
        || Ok((0, 1234u64)),
        |_| true,
    )
    .unwrap();
    assert_eq!(result, (0, 1234));
    assert!(execution.unconfirmed.load(Ordering::SeqCst));
}

#[test]
fn wrong_route_cannot_replace_last_executed_identity() {
    let (_directory, execution) = execution();
    let bound = &execution.context.binding;
    let mut identity =
        ExecutionIdentity::legacy_candidate(&bound.backend, bound.model.as_deref(), None::<String>);
    identity.backend_instance = bound
        .backend_instance
        .clone()
        .unwrap_or_else(|| bound.backend.clone());
    execution.verify_identity(&identity).unwrap();
    identity.effective_model = Some("different".into());
    assert!(execution.verify_identity(&identity).is_err());
    assert_eq!(
        execution.context.binding.model.as_deref(),
        Some("fixture-model")
    );
}

#[test]
fn caught_nested_failure_cannot_complete_the_outer_operation() {
    let (_directory, execution) = execution();
    execution.unconfirmed.store(true, Ordering::SeqCst);
    let error = execution.finish(&permission(&execution)).unwrap_err();
    assert!(error.to_string().contains("retain its operation fence"));
}

#[test]
fn process_authority_requires_an_entered_real_job_and_never_installs_from_env() {
    let (_directory, execution) = execution();
    let mut run =
        WorkerRun::reserve(&execution.work, &execution.context.binding, &"b".repeat(32)).unwrap();
    run.ready().unwrap();
    drop(run);
    assert!(install(execution.context, execution.work, execution.token).is_err());
    assert!(!active());
}
