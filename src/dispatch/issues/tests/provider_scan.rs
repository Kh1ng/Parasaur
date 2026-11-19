use super::*;

#[test]
fn scan_available_tickets_from_gitlab_intake_parses_priority_labels() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let issue_json = r#"[{"iid":88,"title":"GitLab priority check","description":"Priority: P4\n","labels":["P2","bug"],"author":{"username":"project-bot","bot":false},"state":"opened"}]"#;
    let glab_path = bin_dir.join("glab");
    fs::write(
        &glab_path,
        format!(
            "#!/bin/sh\ncase \"$*\" in\n  *projects/5/issues*--hostname*) printf '%s\\n' '{}'\n  ;;\nesac\n",
            issue_json.replace('\'', "'\\''")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&glab_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&glab_path, perms).unwrap();
    }
    let _guard = PathGuard::set(&bin_dir);

    let cfg = ticket_cfg(tmp.path());
    let mut prof = profile(tmp.path());
    prof.local_path = tmp.path().display().to_string();
    prof.provider = "gitlab".to_string();
    prof.repo = "group/project".to_string();
    prof.provider_api_base = Some("https://gitlab.example.com/api/v4".into());
    prof.provider_project_id = Some("5".into());
    prof.publishing.trusted_issue_human_authors = Some(vec!["project-bot".into()]);

    let candidates = scan_available_tickets(
        &prof,
        &[],
        &ledger::index_entries_by_work_id(&ledger::read_entries(&cfg).unwrap()),
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].priority, crate::models::TicketPriority::P2);
}

#[test]
fn scan_available_tickets_prefers_provider_priority_over_issue_metadata() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let issue_json = r#"[{"number":118,"title":"Priority override check","body":"Priority: P3\n","labels":[{"name":"p1"}],"author":{"login":"owner","type":"User","is_bot":false},"state":"OPEN"}]"#;
    let gh_path = bin_dir.join("gh");
    fs::write(
        &gh_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"api\" ]; then\n  printf '%s\\n' '{}'\nfi\n",
            issue_json.replace('\'', "'\\''")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&gh_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&gh_path, perms).unwrap();
    }
    let _guard = PathGuard::set(&bin_dir);

    let cfg = ticket_cfg(tmp.path());
    let mut prof = profile(tmp.path());
    prof.local_path = tmp.path().display().to_string();
    prof.provider = "github".to_string();
    prof.repo = "owner/repo".to_string();

    let candidates = scan_available_tickets(
        &prof,
        &[],
        &ledger::index_entries_by_work_id(&ledger::read_entries(&cfg).unwrap()),
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].priority, crate::models::TicketPriority::P1);
    assert_eq!(candidates[0].priority.to_sort_rank(), 1);
}

#[test]
fn scan_available_tickets_parses_github_issue_priority_when_missing_label() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let issue_json = r#"[{"number":119,"title":"Missing-label priority","body":"Priority: P3\n","labels":["bug"],"author":{"login":"owner","type":"User","is_bot":false},"state":"OPEN"}]"#;
    let gh_path = bin_dir.join("gh");
    fs::write(
        &gh_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"api\" ]; then\n  printf '%s\\n' '{}'\nfi\n",
            issue_json.replace('\'', "'\\''")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&gh_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&gh_path, perms).unwrap();
    }
    let _guard = PathGuard::set(&bin_dir);

    let cfg = ticket_cfg(tmp.path());
    let mut prof = profile(tmp.path());
    prof.local_path = tmp.path().display().to_string();
    prof.provider = "github".to_string();
    prof.repo = "owner/repo".to_string();

    let candidates = scan_available_tickets(
        &prof,
        &[],
        &ledger::index_entries_by_work_id(&ledger::read_entries(&cfg).unwrap()),
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].priority, crate::models::TicketPriority::P3);
}

#[test]
fn scan_available_tickets_prefers_issue_metadata_when_label_is_missing() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let issue_json = r#"[{"number":120,"title":"GitLab parity metadata parse","body":"Priority: P4\n","labels":[],"author":{"login":"owner","type":"User","is_bot":false},"state":"OPEN"}]"#;
    let gh_path = bin_dir.join("gh");
    fs::write(
        &gh_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"api\" ]; then\n  printf '%s\\n' '{}'\nfi\n",
            issue_json.replace('\'', "'\\''")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&gh_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&gh_path, perms).unwrap();
    }
    let _guard = PathGuard::set(&bin_dir);

    let cfg = ticket_cfg(tmp.path());
    let mut prof = profile(tmp.path());
    prof.local_path = tmp.path().display().to_string();
    prof.provider = "github".to_string();
    prof.repo = "owner/repo".to_string();

    let candidates = scan_available_tickets(
        &prof,
        &[],
        &ledger::index_entries_by_work_id(&ledger::read_entries(&cfg).unwrap()),
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].priority,
        crate::models::TicketPriority::Malformed
    );
}

#[test]
fn scan_available_tickets_excludes_owner_decision_github_issues() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let issue_json = r#"[{"number":92,"title":"MS-5: Fleet ledger","body":"","labels":[{"name":"EXEC:OWNER-DECISION"}],"author":{"login":"owner"}}]"#;
    let gh_path = bin_dir.join("gh");
    fs::write(
        &gh_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"api\" ]; then\n  printf '%s\\n' '{}'\nfi\n",
            issue_json.replace('\'', "'\\''")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&gh_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&gh_path, perms).unwrap();
    }
    let _guard = PathGuard::set(&bin_dir);
    let cfg = ticket_cfg(tmp.path());
    let mut prof = profile(tmp.path());
    prof.local_path = tmp.path().display().to_string();
    prof.provider = "github".to_string();
    prof.repo = "owner/repo".to_string();

    let candidates = scan_available_tickets(
        &prof,
        &[],
        &ledger::index_entries_by_work_id(&ledger::read_entries(&cfg).unwrap()),
    );

    assert!(candidates.is_empty());
}

#[test]
fn scan_available_tickets_excludes_issue_already_archived_locally() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let issue_json = r#"[{"number":118,"title":"TICKET-101-fail-closed-version-drift: TICKET-101 — Fail closed","body":"Recommended backend: agy\n","labels":[],"author":{"login":"owner"}}]"#;
    let gh_path = bin_dir.join("gh");
    fs::write(
        &gh_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"api\" ]; then\n  printf '%s\\n' '{}'\nfi\n",
            issue_json.replace('\'', "'\\''")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&gh_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&gh_path, perms).unwrap();
    }
    let _guard = PathGuard::set(&bin_dir);

    let closed_dir = tmp.path().join("docs/tickets/closed");
    fs::create_dir_all(&closed_dir).unwrap();
    fs::write(
        closed_dir.join("TICKET-101-fail-closed-version-drift.md"),
        "# TICKET-101: Fail closed\n\nGoal: test\n",
    )
    .unwrap();

    let cfg = ticket_cfg(tmp.path());
    let mut prof = profile(tmp.path());
    prof.local_path = tmp.path().display().to_string();
    prof.provider = "github".to_string();
    prof.repo = "owner/repo".to_string();

    let candidates = scan_available_tickets(
        &prof,
        &[],
        &ledger::index_entries_by_work_id(&ledger::read_entries(&cfg).unwrap()),
    );
    assert!(
        candidates.is_empty(),
        "expected locally-archived TICKET-101 issue to be excluded, got {candidates:?}"
    );
}
