use super::*;
use std::os::unix::fs::PermissionsExt;

pub(super) fn directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

fn offer(run: &mut WorkerRun, grant: &StartGrant, send: impl FnOnce() -> Result<()>) -> Result<()> {
    run.offer_start_checked(grant, send, |_, _| Ok(()))
}

pub(super) fn binding() -> DispatchBinding {
    DispatchBinding {
        run_id: uuid::Uuid::new_v4().to_string(),
        context_sha256: "b".repeat(64),
        node_id: "fixture-worker".into(),
        profile: "fixture".into(),
        repo_id: "fixture".into(),
        repo: "fixture/repo".into(),
        work_id: "#1".into(),
        mode: "fix".into(),
        branch: None,
        review_generation: None,
        dispatch_reason: Some("initial".into()),
        backend: "codex".into(),
        backend_instance: None,
        model: Some("fixture-model".into()),
    }
}

pub(super) fn grant(binding: &DispatchBinding) -> StartGrant {
    StartGrant {
        run_id: binding.run_id.clone(),
        node_id: binding.node_id.clone(),
        context_sha256: binding.context_sha256.clone(),
        reservation_id: "a".repeat(32),
        token: "c".repeat(64),
        expires_at: (crate::clock::now_utc() + time::Duration::seconds(10))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    }
}

#[test]
fn writer_drop_releases_the_lock_even_with_an_inherited_descriptor_copy() {
    let directory = directory();
    let binding = binding();
    let run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    let inherited = run._lock.try_clone().unwrap();
    drop(run);
    let resumed = WorkerRun::open(directory.path(), &binding).unwrap();
    drop(inherited);
    assert!(WorkerRun::open(directory.path(), &binding).is_err());
    drop(resumed);
    assert!(WorkerRun::open(directory.path(), &binding).is_ok());
}

#[test]
fn lost_start_reply_is_never_reoffered_after_reopening() {
    let directory = directory();
    let binding = binding();
    let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    assert!(
        WorkerRun::open(directory.path(), &binding).is_err(),
        "separate writer must be excluded"
    );
    run.ready().unwrap();
    let grant = grant(&binding);
    let mut calls = 0;
    assert!(offer(&mut run, &grant, || {
        calls += 1;
        anyhow::bail!("lost acknowledgement")
    })
    .is_err());
    assert_eq!(calls, 1);
    drop(run);
    let mut resumed = WorkerRun::open(directory.path(), &binding).unwrap();
    assert_eq!(resumed.observation().unwrap()["phase"], "start_intent");
    let mut replacement = grant.clone();
    replacement.token = "d".repeat(64);
    for retry in [&grant, &replacement] {
        assert!(offer(&mut resumed, retry, || {
            calls += 1;
            Ok(())
        })
        .is_err());
    }
    assert_eq!(calls, 1);
    let encoded = fs::read_to_string(directory.path().join("worker.json")).unwrap();
    assert!(!encoded.contains(&grant.token));
    assert!(encoded.contains(&format!("{:x}", Sha256::digest(grant.token.as_bytes()))));
    assert!(!resumed.observation().unwrap().to_string().contains("token"));
}

#[test]
fn wrong_reservation_or_expired_grant_never_reaches_transport() {
    let directory = directory();
    let binding = binding();
    let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    run.ready().unwrap();
    for field in [
        "run_id",
        "node_id",
        "context_sha256",
        "reservation_id",
        "token",
        "expires_at",
    ] {
        let mut value = serde_json::to_value(grant(&binding)).unwrap();
        value[field] = if field == "expires_at" {
            "2000-01-01T00:00:00Z".into()
        } else {
            "wrong".into()
        };
        let rejected = serde_json::from_value(value).unwrap();
        assert!(offer(&mut run, &rejected, || panic!(
            "invalid grant reached transport"
        ))
        .is_err());
        assert_eq!(run.observation().unwrap()["phase"], "ready");
    }
    offer(&mut run, &grant(&binding), || Ok(())).unwrap();
    assert!(offer(&mut run, &grant(&binding), || panic!("duplicate start")).is_err());
}

#[test]
fn reservation_intent_and_exact_binding_survive_writer_exit() {
    let directory = directory();
    let binding = binding();
    let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    assert!(offer(&mut run, &grant(&binding), || panic!(
        "unconfirmed capacity started"
    ))
    .is_err());
    assert_eq!(run.observation().unwrap()["phase"], "reservation_intent");
    drop(run);
    assert!(WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).is_err());
    let mut wrong = binding.clone();
    wrong.model = Some("different".into());
    assert!(WorkerRun::open(directory.path(), &wrong).is_err());
    run = WorkerRun::open(directory.path(), &binding).unwrap();
    assert_eq!(run.observation().unwrap()["phase"], "reservation_intent");
}

#[test]
fn uncertain_start_write_never_calls_transport_or_allows_in_memory_retry() {
    let directory = directory();
    let binding = binding();
    let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    run.ready().unwrap();
    let journal = directory.path().join("worker.json");
    fs::remove_file(&journal).unwrap();
    fs::create_dir(&journal).unwrap();
    assert!(offer(&mut run, &grant(&binding), || panic!(
        "uncommitted start reached transport"
    ))
    .is_err());
    assert!(run.observation().is_err());
    assert!(offer(&mut run, &grant(&binding), || panic!(
        "uncertain writer retried"
    ))
    .is_err());
    drop(run);
    assert!(WorkerRun::open(directory.path(), &binding).is_err());
}

#[test]
fn malformed_state_and_symlink_lock_fail_closed() {
    let directory = directory();
    let binding = binding();
    let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    run.ready().unwrap();
    drop(run);
    let path = directory.path().join("worker.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["phase"] = "start_intent".into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(WorkerRun::open(directory.path(), &binding).is_err());
    let lock = directory.path().join("worker.lock");
    fs::remove_file(&lock).unwrap();
    std::os::unix::fs::symlink(&path, lock).unwrap();
    assert!(WorkerRun::open(directory.path(), &binding).is_err());
}

#[test]
fn ready_file_without_a_live_holder_never_offers_a_start_or_reports_free_capacity() {
    let directory = directory();
    let binding = binding();
    let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
    run.ready().unwrap();
    assert!(run
        .offer_start(&grant(&binding), || panic!(
            "missing holder started a wrapper"
        ))
        .is_err());
    let observed = run.inspect().unwrap();
    assert_eq!(observed["phase"], "ready");
    assert_eq!(observed["state"], "unknown");
    assert!(observed["capacity_held"].is_null());
    assert_eq!(observed["start_offered"], false);
}
