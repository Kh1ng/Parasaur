#![cfg(unix)]
use super::*;

fn info() -> super::super::CredentialInfo {
    super::super::CredentialInfo {
        id: "work".into(),
        provider: "mistral".into(),
        kind: super::super::CredentialKind::MistralDashboard,
        account_label: "Work".into(),
        env_var: None,
    }
}

#[test]
fn named_publication_rejects_rotation_deletion_and_create_delete_cycles() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("credentials");
    let absent = snapshot_at(&root, Some("work")).unwrap();
    assert_eq!(absent, snapshot_at(&root, Some("work")).unwrap());
    super::super::save_at(&root, info(), "session=first").unwrap();
    assert!(!save_at(
        &root,
        Some("work"),
        Some("Work"),
        &absent,
        "session=stale",
        None
    )
    .unwrap());
    let first = snapshot_at(&root, Some("work")).unwrap();
    let quota = home.path().join("existing-quota.jsonl");
    std::fs::write(&quota, "existing-observation\n").unwrap();
    super::super::save_at(&root, info(), "session=second").unwrap();
    assert!(!save_at(
        &root,
        Some("work"),
        Some("Work"),
        &first,
        "session=stale",
        None
    )
    .unwrap());
    assert!(!save_at(
        &root,
        Some("work"),
        Some("Work"),
        &first,
        "session=stale",
        Some(&quota)
    )
    .unwrap());
    assert_eq!(
        std::fs::read_to_string(&quota).unwrap(),
        "existing-observation\n"
    );
    let second = snapshot_at(&root, Some("work")).unwrap();
    super::super::remove_at(&root, "work", &home.path().join("quota.jsonl")).unwrap();
    assert!(!save_at(
        &root,
        Some("work"),
        Some("Work"),
        &second,
        "session=stale",
        None
    )
    .unwrap());
    assert!(!save_at(
        &root,
        Some("work"),
        Some("Work"),
        &absent,
        "session=stale",
        None
    )
    .unwrap());
    let fresh = snapshot_at(&root, Some("work")).unwrap();
    assert!(save_at(
        &root,
        Some("work"),
        Some("Work"),
        &fresh,
        "session=verified",
        None
    )
    .unwrap());
    assert_eq!(
        super::super::read_at(&root, "work").unwrap().secret,
        "session=verified"
    );
    assert!(!save_at(
        &root,
        Some("work"),
        Some("Work"),
        &fresh,
        "session=duplicate",
        None
    )
    .unwrap());
}

#[test]
fn interrupted_publication_metadata_invalidates_old_checks_without_exposing_secrets() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("credentials");
    super::super::save_at(&root, info(), "session=unchanged").unwrap();
    let before = snapshot_at(&root, Some("work")).unwrap();
    assert!(!std::fs::read_to_string(root.join(".work.lock"))
        .unwrap()
        .contains("session="));
    std::fs::write(root.join(".work.lock"), "partial interrupted marker").unwrap();
    assert!(!save_at(
        &root,
        Some("work"),
        Some("Work"),
        &before,
        "session=stale",
        None
    )
    .unwrap());
    let current = snapshot_at(&root, Some("work")).unwrap();
    super::super::save_at(&root, info(), "session=unchanged").unwrap();
    assert!(!save_at(
        &root,
        Some("work"),
        Some("Work"),
        &current,
        "session=stale",
        None
    )
    .unwrap());
    assert_eq!(
        super::super::read_at(&root, "work").unwrap().secret,
        "session=unchanged"
    );
}

#[test]
fn default_publication_rejects_replaced_files_and_older_successes() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("credentials");
    let absent = snapshot_at(&root, None).unwrap();
    assert!(save_at(&root, None, None, &absent, "session=first", None).unwrap());
    let first = snapshot_at(&root, None).unwrap();
    let current = destination(&root).unwrap();
    let mut replacement = tempfile::NamedTempFile::new_in(home.path()).unwrap();
    replacement.write_all(b"session=external").unwrap();
    replacement.persist(&current).unwrap();
    assert!(!save_at(&root, None, None, &first, "session=stale", None).unwrap());
    assert_eq!(
        std::fs::read_to_string(&current).unwrap(),
        "session=external"
    );
    let changed = snapshot_at(&root, None).unwrap();
    assert!(save_at(&root, None, None, &changed, "session=latest", None).unwrap());
    std::fs::remove_file(&current).unwrap();
    assert!(!save_at(&root, None, None, &absent, "session=stale", None).unwrap());
    assert!(!save_at(&root, None, None, &changed, "session=stale", None).unwrap());
    assert!(!current.exists());
}

#[test]
fn unsafe_state_and_invalid_input_are_not_missing_connections() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("credentials");
    let token = snapshot_at(&root, None).unwrap();
    assert!(save_at(&root, None, None, &token, "session=x\nInjected: x", None).is_err());
    std::fs::write(destination(&root).unwrap(), "public-cookie").unwrap();
    std::fs::set_permissions(
        destination(&root).unwrap(),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(snapshot_at(&root, None).is_err());
    std::fs::remove_file(destination(&root).unwrap()).unwrap();
    std::os::unix::fs::symlink(home.path().join("other"), destination(&root).unwrap()).unwrap();
    assert!(snapshot_at(&root, None).is_err());
}
