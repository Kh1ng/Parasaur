//! Routing and native observations must select the same provider quota group.
#![cfg(unix)]

#[test]
fn agy_nova_flash_route_sees_exhausted_provider_pool() {
    use git_agent_harness::{
        availability::AgyCatalog, execution_identity::ExecutionIdentity, quota_store, usage,
    };
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let output = include_str!("fixtures/agy/usage.json")
        .replace("Gemini Flash, Gemini Pro", "Nova Flash")
        .replace("gemini-weekly", "gemini-flash-weekly");
    let path = dir.path().join("usage.json");
    std::fs::write(&path, &output).unwrap();
    let exe = dir.path().join("agy");
    std::fs::write(&exe, format!(
        "#!/bin/sh\ncase \"$1\" in\n--help) echo --disable-slash-commands;;\n--print) cat \"{}\";;\nesac\n",
        path.display()
    )).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let records = usage::agy::refresh(exe.to_str().unwrap(), "agy", Some(dir.path())).unwrap();
    assert!(records.iter().any(
        |record| record.quota_pool.as_deref() == Some("agy:external")
            && record.quota_remaining_percent == Some(100.0)
    ));
    let catalog =
        AgyCatalog::parse("nova-4-flash-low\tNova 4 Flash (Low)\n", output.as_bytes()).unwrap();
    let pool = catalog.pool("agy", "nova-4-flash-low");
    assert_eq!(pool, "agy:gemini-flash-weekly");
    let identity =
        ExecutionIdentity::legacy_candidate("agy", Some("nova-4-flash-low"), Some(&pool));
    let windows = quota_store::latest_windows_for_identity(&records, &identity);
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].quota_remaining_percent, Some(0.0));
}
