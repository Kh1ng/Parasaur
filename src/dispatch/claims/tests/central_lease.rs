use super::*;
use std::cell::RefCell;

/// Issue #1541: the claim fixture with central claims configured.
fn central_fixture(tmp: &Path) -> (GahConfig, Profile, DispatchArgs) {
    let (mut cfg, prof, args) = super::repository_scope::claim_fixture(tmp);
    cfg.defaults.registry_central_url = Some("http://central:3773".into());
    (cfg, prof, args)
}

/// Runs `acquire_claim` with a fake central lease call and returns its
/// result alongside the lease ids that call was asked for.
fn acquire_recording(
    cfg: &GahConfig,
    prof: &Profile,
    args: &DispatchArgs,
    tmp: &Path,
) -> (Result<Option<String>>, Vec<String>) {
    let leases = RefCell::new(Vec::new());
    let result = acquire_claim_with(
        cfg,
        prof,
        args,
        &tmp.join("sessions/run"),
        |central_url, lease_id| {
            assert_eq!(central_url, "http://central:3773");
            leases.borrow_mut().push(lease_id.to_string());
            Ok(())
        },
    )
    .map(|(guard, claimed)| {
        assert_eq!(guard.is_some(), claimed.is_some());
        claimed
    });
    (result, leases.into_inner())
}

/// The ledger entry of the run that opened the pull request on `branch`
/// for issue #123, and the open pull request itself.
fn open_pull_request_for_issue(cfg: &GahConfig, prof: &Profile, bin_dir: &Path, branch: &str) {
    let updated_at = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
    setup_fake_gh(
        bin_dir,
        &format!(
            r##"[{{"title":"#123: Fix","body":null,"head":{{"ref":"{branch}","sha":"source-sha"}},"html_url":"https://github.com/owner/repo/pull/9","labels":[],"number":9,"state":"open","draft":false,"updated_at":"{updated_at}"}}]"##
        ),
    );
    let mut entry = LedgerEntry::new("test", prof, "codex", "fix", "123", None, None);
    entry.work_id = Some("#123".into());
    entry.branch = Some(branch.into());
    entry.mr_url = Some("https://github.com/owner/repo/pull/9".into());
    entry.attempts_completed = Some(1);
    crate::ledger::append(cfg, &entry).unwrap();
}

#[test]
fn acquire_claim_takes_central_lease_for_issue_number_target() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, prof, mut args) = central_fixture(tmp.path());
    for target in ["123", "#123", " #0123 "] {
        args.target = target.into();
        let (claimed, leases) = acquire_recording(&cfg, &prof, &args, tmp.path());
        assert_eq!(claimed.unwrap().as_deref(), Some("#123"), "{target:?}");
        assert_eq!(leases, ["#123"], "{target:?}");
    }
}

/// A ticket file keeps its own id for the ledger; its central lease uses
/// the normalised id, the same key an issue-backed dispatch of it takes.
#[test]
fn ticket_file_lease_uses_the_normalised_work_id() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, prof, args) = central_fixture(tmp.path());
    let (claimed, leases) = acquire_recording(&cfg, &prof, &args, tmp.path());
    assert_eq!(claimed.unwrap().as_deref(), Some("TICKET-500"));
    assert_eq!(leases, ["#500"]);
}

/// `FixMr` passes the pull request's branch as its target and the work id
/// separately. The open pull request on that branch is the one it repairs,
/// so it does not refuse the repair; it does refuse a fresh dispatch.
#[test]
fn fix_existing_branch_dispatch_takes_lease_under_its_work_id() {
    let _exec_guard = crate::test_support::ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let _guard = PathGuard::set(&bin_dir);
    let (cfg, prof, mut args) = central_fixture(tmp.path());
    let branch = "gah/repo-issue-123";
    open_pull_request_for_issue(&cfg, &prof, &bin_dir, branch);

    args.mode = "fix".into();
    args.target = branch.into();
    args.existing_branch = Some(branch.into());
    args.work_id = Some("#123".into());
    args.dispatch_reason = Some("post_review_repair".into());
    let (claimed, leases) = acquire_recording(&cfg, &prof, &args, tmp.path());
    assert_eq!(claimed.unwrap().as_deref(), Some("#123"));
    assert_eq!(leases, ["#123"]);

    args.target = "123".into();
    args.existing_branch = None;
    args.dispatch_reason = Some("initial".into());
    let (refused, leases) = acquire_recording(&cfg, &prof, &args, tmp.path());
    let error = refused.unwrap_err();
    let duplicate = duplicate_work_error(&error).expect("open pull request refuses");
    assert_eq!(duplicate.branch.as_deref(), Some(branch));
    assert!(leases.is_empty());
}

/// A repair of a pull request whose title carries no work id is claimed
/// under its branch.
#[test]
fn fix_existing_branch_without_work_id_leases_its_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, prof, mut args) = central_fixture(tmp.path());
    args.mode = "fix".into();
    args.target = "gah/legacy-fix".into();
    args.existing_branch = Some("gah/legacy-fix".into());
    args.dispatch_reason = Some("post_review_repair".into());
    let (claimed, leases) = acquire_recording(&cfg, &prof, &args, tmp.path());
    assert_eq!(claimed.unwrap().as_deref(), Some("gah/legacy-fix"));
    assert_eq!(leases, ["gah/legacy-fix"]);
}

/// A controller improve or fix dispatch that names no work fails closed; a
/// direct free-text dispatch and an experiment still run unclaimed.
#[test]
fn unresolvable_target_refuses_only_controller_improve_and_fix() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, prof, mut args) = central_fixture(tmp.path());
    args.target = "fix the flaky test".into();

    for mode in ["improve", "fix", "experiment"] {
        args.mode = mode.into();
        args.dispatch_reason = None;
        let (claimed, leases) = acquire_recording(&cfg, &prof, &args, tmp.path());
        assert_eq!(claimed.unwrap(), None, "direct {mode}");
        assert!(leases.is_empty());

        args.dispatch_reason = Some("initial".into());
        let (result, leases) = acquire_recording(&cfg, &prof, &args, tmp.path());
        assert!(leases.is_empty());
        match mode {
            "experiment" => assert_eq!(result.unwrap(), None),
            _ => assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("resolves to no work id"),
                "controller {mode}"
            ),
        }
    }

    // Without central claims nothing changes: the dispatch runs unclaimed.
    let mut local = cfg.clone();
    local.defaults.registry_central_url = None;
    let (guard, claimed) =
        acquire_claim(&local, &prof, &args, &tmp.path().join("sessions/run")).unwrap();
    assert!(guard.is_none());
    assert_eq!(claimed, None);
}

/// Without central claims an issue number or existing-branch target still
/// writes no ledger claim: only target files are claimed locally.
#[test]
fn local_claims_still_cover_only_target_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut cfg, prof, mut args) = central_fixture(tmp.path());
    cfg.defaults.registry_central_url = None;
    args.mode = "fix".into();
    args.target = "123".into();
    args.work_id = Some("#123".into());
    args.existing_branch = Some("gah/repo-issue-123".into());
    args.dispatch_reason = Some("post_review_repair".into());
    let (guard, claimed) =
        acquire_claim(&cfg, &prof, &args, &tmp.path().join("sessions/run")).unwrap();
    assert!(guard.is_none());
    assert_eq!(claimed, None);
    assert!(crate::ledger::read_entries(&cfg).unwrap().is_empty());
}
