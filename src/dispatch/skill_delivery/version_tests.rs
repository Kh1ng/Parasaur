use crate::ledger::{AttemptRoutingRecord, LedgerEntry};

struct BankEnv(Option<std::ffi::OsString>);
impl Drop for BankEnv {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("GAH_SKILL_BANK_PATH", value),
            None => std::env::remove_var("GAH_SKILL_BANK_PATH"),
        }
    }
}

#[test]
fn pinned_skill_dispatch_records_and_delivers_the_exact_version() {
    let _exec = crate::test_support::ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bank = tmp.path().join("skills.json");
    let _bank_env = BankEnv(std::env::var_os("GAH_SKILL_BANK_PATH"));
    std::env::set_var("GAH_SKILL_BANK_PATH", &bank);
    let bank_content = |pin| {
        serde_json::json!({
            "skills": [
                { "id": "review", "version": "1.0.0", "content": "old instructions", "backends": ["codex"] },
                { "id": "review", "version": "2.0.0", "content": "new instructions", "backends": ["codex"] }
            ],
            "bindings": { "review": [{"label": "backend:codex", "version": pin}] },
            "bindingOverrides": ["backend:codex"]
        })
    };
    std::fs::write(&bank, bank_content("1.0.0").to_string()).unwrap();
    let fake = tmp.path().join("codex");
    std::fs::write(&fake, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    crate::dispatch::test_util::init_repo(tmp.path());
    let mut profile = crate::dispatch::test_util::profile(tmp.path());
    profile.codex_path = Some(fake.display().to_string());
    let cfg = crate::dispatch::test_util::gah_config_with_ledger(tmp.path(), Default::default());
    let identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
        "codex",
        None::<String>,
        None::<String>,
    );
    let mut ledger = LedgerEntry::new("test", &profile, "codex", "improve", "task", None, None);
    let cargo_target =
        crate::build_cache::ScopedCargoTarget::acquire(tmp.path().to_str().unwrap()).unwrap();
    let llm = crate::runner::LlmConfig {
        base_url: String::new(),
        api_key: String::new(),
        model: "default".into(),
    };
    for (number, version) in [(1, "1.0.0"), (2, "2.0.0")] {
        if number == 2 {
            let mut unpinned = bank_content("1.0.0");
            unpinned["bindings"]["review"] = serde_json::json!(["backend:codex"]);
            std::fs::write(&bank, unpinned.to_string()).unwrap();
        }
        ledger.attempt_routing.push(AttemptRoutingRecord {
            attempt_number: number,
            ..Default::default()
        });
        let session = tmp.path().join(format!("attempt-{number}"));
        std::fs::create_dir(&session).unwrap();
        let result = crate::dispatch::attempts::run_backend_for_identity(
            &cfg,
            "test",
            &identity,
            &profile,
            tmp.path(),
            "task",
            &session,
            &cargo_target,
            &llm,
            None,
            None,
            None,
            crate::runner::WriteIntent::Implementation,
            Some(&mut ledger),
        )
        .unwrap();
        assert_eq!(ledger.skills_for_attempt(number)[0].version, version);
        let delivered = std::fs::read_to_string(result.log_path).unwrap();
        assert!(
            delivered.contains(&format!("review@{version}")),
            "{delivered}"
        );
        let review = super::review_prompt(
            &cfg,
            "test",
            &profile,
            &identity,
            &mut ledger,
            &session,
            "review task",
        )
        .unwrap();
        assert!(review.contains(&format!("review@{version}")));
    }
    assert_eq!(ledger.skills_for_attempt(1)[0].version, "1.0.0");
    assert_eq!(ledger.skills.len(), 2);
    // A missing pin must never silently follow newest.
    std::fs::write(&bank, bank_content("9.0.0").to_string()).unwrap();
    assert!(crate::skill_bindings::resolve(&cfg.defaults, "test", "codex", None).is_err());
}
