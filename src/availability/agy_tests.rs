use super::*;

const MODELS: &str = include_str!("../../tests/fixtures/agy/models.tsv");
const USAGE: &[u8] = include_bytes!("../../tests/fixtures/agy/usage.json");

thread_local! {
    pub(super) static FIXTURE: std::cell::RefCell<Option<Catalog>> = const { std::cell::RefCell::new(None) };
}

pub(crate) struct Fixture {
    pub catalog: Catalog,
}
pub(crate) fn fixture() -> Fixture {
    FIXTURE.with(|cell| *cell.borrow_mut() = Some(Catalog::parse(MODELS, USAGE).unwrap()));
    Fixture {
        catalog: Catalog::parse(MODELS, USAGE).unwrap(),
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        FIXTURE.with(|cell| *cell.borrow_mut() = None);
    }
}

#[test]
fn agy_fixture_maps_native_models_and_accounts_to_reported_buckets() {
    let catalog = Catalog::parse(MODELS, USAGE).unwrap();
    assert_eq!(
        catalog.pool("agy", "gemini-3.8-flash-low"),
        "agy:google-native"
    );
    assert_eq!(
        catalog.pool("agy", "Gemini 3.8 Flash (High)"),
        "agy:google-native"
    );
    assert_eq!(
        catalog.pool("agy-second", "gemini-3.8-flash-low"),
        "agy-second:google-native"
    );
    assert_eq!(catalog.pool("agy", "claude-sonnet-4-6"), "agy:external");
    assert_eq!(catalog.pool("agy", "gpt-oss-120b-medium"), "agy:external");
    assert_eq!(catalog.pool("agy", "not-listed"), "agy:external");
}

#[test]
fn agy_new_family_uses_a_policy_pool_from_provider_metadata() {
    let usage = String::from_utf8(USAGE.to_vec())
        .unwrap()
        .replace("Gemini Flash, Gemini Pro", "Nova Flash");
    let catalog =
        Catalog::parse("nova-4-flash-low\tNova 4 Flash (Low)\n", usage.as_bytes()).unwrap();
    assert_eq!(catalog.pool("agy", "nova-4-flash-low"), "agy:google-native");
}

#[test]
fn agy_retired_model_disappears_when_inventory_is_replaced() {
    let models = MODELS
        .lines()
        .filter(|line| !line.starts_with("gemini-3.8-flash-low\t"))
        .collect::<Vec<_>>()
        .join("\n");
    let catalog = Catalog::parse(&models, USAGE).unwrap();
    assert_eq!(
        catalog.pool("agy", "gemini-3.8-flash-low"),
        "agy:google-native"
    );
    assert_eq!(
        catalog.pool("agy", "gemini-3.8-flash-high"),
        "agy:google-native"
    );
}

#[test]
fn agy_unknown_shapes_and_ambiguous_membership_use_family_fallback() {
    for usage in [
        b"{}".as_slice(),
        b"not json",
        br#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[]}}}"#,
    ] {
        assert!(Catalog::parse(MODELS, usage).is_err());
    }
    let mut ambiguous: serde_json::Value = serde_json::from_slice(USAGE).unwrap();
    ambiguous["command"]["data"]["groups"][1]["description"] =
        "Models within this group: Gemini Flash".into();
    let catalog = Catalog::parse(MODELS, &serde_json::to_vec(&ambiguous).unwrap()).unwrap();
    assert_eq!(
        catalog.pool("agy", "gemini-3.8-flash-high"),
        "agy:google-native"
    );
    ambiguous["command"]["data"]["groups"][1]["buckets"][0]["id"] = "gemini-weekly".into();
    assert!(Catalog::parse(MODELS, &serde_json::to_vec(&ambiguous).unwrap()).is_err());
    assert!(Catalog::parse("changed format", USAGE).is_err());
    let changed = String::from_utf8(USAGE.to_vec())
        .unwrap()
        .replace("Models within this group:", "Unknown membership:");
    assert!(Catalog::parse(MODELS, changed.as_bytes()).is_err());
    assert_eq!(
        Catalog::default().pool("agy-two", "anything"),
        "agy-two:external"
    );
}

#[cfg(unix)]
#[test]
fn agy_routing_cache_refreshes_per_installation_and_home_without_model_calls() {
    use std::os::unix::fs::PermissionsExt;
    let _exec = crate::test_support::ExecGuard::new();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("agy");
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join("models"), MODELS).unwrap();
    std::fs::write(home.join("usage"), USAGE).unwrap();
    std::fs::write(&script, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/calls\"\ncase \"$*\" in\nmodels) cat \"$HOME/models\";;\n--help) echo --disable-slash-commands;;\n'--print /usage --output-format json') cat \"$HOME/usage\";;\n*) exit 99;;\nesac\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let resolved = script.canonicalize().unwrap();
    let executable = resolved.to_str().unwrap();
    assert_eq!(
        settled_pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-one:google-native"
    );
    // Keep an exhausted reading attached to the same pool through failures.
    let readings = crate::usage::agy::refresh(executable, "agy-one", Some(&home)).unwrap();
    let key = (executable.to_string(), Some(home.clone()));
    let cache = CACHE.get().unwrap();
    let expire = || {
        let mut entries = cache.lock().unwrap();
        *entries
            .get_mut(&key)
            .unwrap()
            .refreshed_at
            .as_mut()
            .unwrap() -= TTL;
    };
    let retry = || {
        let mut entries = cache.lock().unwrap();
        *entries.get_mut(&key).unwrap().retry_at.as_mut().unwrap() -= RETRY_BACKOFF;
    };
    expire();
    let original = std::fs::read(&script).unwrap();
    std::fs::write(
        &script,
        "#!/bin/sh\necho models >> \"$HOME/calls\"\nexit 1\n",
    )
    .unwrap();
    let retained = settled_pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home));
    assert_eq!(retained, "agy-one:google-native");
    let identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
        "agy-one",
        Some("gemini-3.8-flash-high"),
        Some(retained),
    );
    let windows = crate::quota_store::latest_windows_for_identity(&readings, &identity);
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].quota_remaining_percent, Some(0.0));
    let calls = std::fs::read_to_string(home.join("calls")).unwrap();
    assert_eq!(
        settled_pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-one:google-native"
    );
    assert_eq!(std::fs::read_to_string(home.join("calls")).unwrap(), calls);
    retry();
    assert_eq!(
        settled_pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-one:google-native"
    );
    assert_eq!(
        std::fs::read_to_string(home.join("calls"))
            .unwrap()
            .lines()
            .count(),
        7
    );
    retry();
    std::fs::remove_file(&script).unwrap();
    assert_eq!(
        settled_pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-one:google-native"
    );
    std::fs::write(&script, original).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let recovered = String::from_utf8(USAGE.to_vec())
        .unwrap()
        .replace("gemini-weekly", "3p-5h");
    std::fs::write(home.join("usage"), recovered).unwrap();
    retry();
    assert_eq!(
        settled_pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-one:external"
    );
    assert!(CACHE
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .get(&key)
        .unwrap()
        .retry_at
        .is_none());
    std::fs::write(home.join("usage"), USAGE).unwrap();
    std::fs::write(home.join("models"), "").unwrap();
    assert_eq!(
        settled_pool("agy-two", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-two:external"
    );
    assert_eq!(
        std::fs::read_to_string(home.join("calls"))
            .unwrap()
            .lines()
            .count(),
        10
    );
    expire();
    assert_eq!(
        settled_pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-one:google-native"
    );
    // The candidate identity carries the installation that actually executes,
    // even when several accounts share the logical backend and binary.
    let mut routing = crate::config::RoutingPolicy::default();
    for name in ["agy-primary", "agy-secondary"] {
        routing.backend_instances.insert(
            name.into(),
            crate::config::BackendInstanceConfig {
                runner_kind: "agy".into(),
                executable: Some(executable.into()),
                state_root: Some(home.to_string_lossy().into_owned()),
                ..Default::default()
            },
        );
    }
    std::fs::write(home.join("models"), MODELS).unwrap();
    expire();
    let identity = |instance: &str| {
        routing.execution_identity_for_candidate(&crate::config::CandidateConfig {
            backend: "agy".into(),
            instance: Some(instance.into()),
            model: Some("gemini-3.8-flash-high".into()),
            ..Default::default()
        })
    };
    settled_pool(
        "agy-primary",
        "gemini-3.8-flash-high",
        executable,
        Some(&home),
    );
    assert_eq!(
        identity("agy-primary").quota_pool.as_deref(),
        Some("agy-primary:google-native")
    );
    assert_eq!(
        identity("agy-secondary").quota_pool.as_deref(),
        Some("agy-secondary:google-native")
    );
    let mut profile = crate::config::tests::test_profile_for_notifications();
    profile.agy_path = Some(executable.into());
    profile.agy_second_home = Some(home.to_string_lossy().into_owned());
    let mut routing = profile.effective_routing(&crate::config::Defaults::default());
    let candidate = crate::config::CandidateConfig {
        backend: "agy-second".into(),
        model: Some("gemini-3.8-flash-high".into()),
        ..Default::default()
    };
    assert_eq!(
        routing
            .execution_identity_for_candidate(&candidate)
            .quota_pool
            .as_deref(),
        Some("agy-second:google-native")
    );
    routing.agy_path = Some("/missing-legacy-agy".into());
    routing.backend_instances.insert(
        "agy-second".into(),
        crate::config::BackendInstanceConfig {
            runner_kind: "agy".into(),
            executable: Some(executable.into()),
            state_root: Some(home.to_string_lossy().into_owned()),
            ..Default::default()
        },
    );
    assert_eq!(
        routing
            .find_quota_pool("improve", "agy-second", Some("gemini-3.8-flash-high"))
            .as_deref(),
        Some("agy-second:google-native")
    );
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("models"), MODELS).unwrap();
    std::fs::write(other.join("usage"), USAGE).unwrap();
    assert_eq!(
        settled_pool("agy-two", "gemini-3.8-flash-high", executable, Some(&other)),
        "agy-two:google-native"
    );
}

#[cfg(unix)]
fn settled_pool(account: &str, model: &str, executable: &str, home: Option<&Path>) -> String {
    let _ = pool(account, model, executable, home);
    let key = (resolved_executable(executable), home.map(Path::to_path_buf));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let cache = CACHE.get().unwrap().lock().unwrap();
        let cached = cache.get(&key).unwrap();
        if !cached.refreshing {
            return cached.catalog.pool(account, model);
        }
        drop(cache);
        assert!(Instant::now() < deadline, "metadata refresh did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn agy_missing_metadata_keeps_gemini_and_third_party_pools_separate() {
    for (model, pool) in [
        ("Gemini 9 Flash", "google-native"),
        ("gemini-new", "google-native"),
        ("Nova", "external"),
        ("Claude", "external"),
    ] {
        assert_eq!(
            super::super::derive_quota_pool("agy-second", Some(model), None),
            Some(format!("agy-second:{pool}"))
        );
    }
}

#[cfg(unix)]
#[test]
fn agy_slow_probe_never_blocks_routing_or_another_installation() {
    use std::os::unix::fs::PermissionsExt;
    let _exec = crate::test_support::ExecGuard::new();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("slow-agy");
    std::fs::write(
        &script,
        "#!/bin/sh\ntouch \"$HOME/started\"\nsleep 10\ntouch \"$HOME/completed\"\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let resolved = script.canonicalize().unwrap();
    let executable = resolved.to_str().unwrap();
    let start = Instant::now();
    assert_eq!(
        pool("agy", "Gemini future", executable, Some(dir.path())),
        "agy:google-native"
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(dir.path().join("started").exists());
    let start = Instant::now();
    assert_eq!(
        pool("agy", "Gemini future", executable, Some(dir.path())),
        "agy:google-native"
    );
    assert_eq!(
        pool("agy-second", "Nova", "/missing-agy", None),
        "agy-second:external"
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(
        !dir.path().join("completed").exists(),
        "timed out probe child must be killed"
    );
}

#[cfg(unix)]
#[test]
fn agy_fresh_process_uses_persisted_catalog_membership() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let executable = dir.path().join("slow-agy");
    std::fs::write(
        &executable,
        "#!/bin/sh\ntouch \"$HOME/started\"\nsleep 10\ntouch \"$HOME/completed\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let usage = String::from_utf8(USAGE.to_vec())
        .unwrap()
        .replace("Gemini Flash, Gemini Pro", "Nova Flash");
    let catalog =
        Catalog::parse("nova-4-flash-low\tNova 4 Flash (Low)\n", usage.as_bytes()).unwrap();
    store_catalog(executable.to_str().unwrap(), Some(&home), &catalog);

    assert_eq!(
        pool(
            "agy",
            "nova-4-flash-low",
            executable.to_str().unwrap(),
            Some(&home)
        ),
        "agy:google-native"
    );
    join_background_refreshes();
    assert!(home.join("started").exists());
    assert!(
        !home.join("completed").exists(),
        "CLI shutdown join must wait for the bounded probe to kill its child"
    );
    let path = cache_path(executable.to_str().unwrap(), Some(&home)).unwrap();
    assert!(path.is_file());
    let _ = std::fs::remove_file(path);
}

#[test]
fn agy_pool_identity_matches_shared_server_fixture_with_and_without_local_probe() {
    #[derive(Deserialize)]
    struct Case {
        model: String,
        buckets: Vec<String>,
        pool: String,
    }
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../../tests/fixtures/agy/pool-identity.json")).unwrap();
    for case in cases {
        let ids = case.buckets.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(pool_identity(&case.model, &ids), case.pool);
        if !case.model.is_empty() {
            for catalog in [Catalog::default(), Catalog::parse(MODELS, USAGE).unwrap()] {
                assert_eq!(
                    catalog.pool("agy-second", &case.model),
                    format!("agy-second:{}", case.pool)
                );
            }
        }
    }
}

#[test]
fn agy_cold_start_does_not_warn_about_unrecognized_metadata() {
    let _exec = crate::test_support::ExecGuard::new();
    // No successful metadata response has arrived; an empty cache is normal.
    // Failed process startup is handled by the separate refresh-failure warning.
    let before = WARNED.load(Ordering::Relaxed);
    let _ = catalog("/missing-cold-start-agy", None);
    assert_eq!(WARNED.load(Ordering::Relaxed), before);
}

#[cfg(unix)]
fn metadata_binary(path: &Path, usage: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!(
        "#!/bin/sh\ncase \"$1\" in\nmodels) printf 'nova-4-flash-low\\tNova 4 Flash (Low)\\n';;\n--help) echo --disable-slash-commands;;\n--print) printf '%s' '{usage}';;\n*) exit 99;;\nesac\n"
    )).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
#[test]
fn agy_path_installations_have_separate_canonical_cache_files() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let mut paths = Vec::new();
    for (installation, bucket, expected) in [
        ("one", "gemini-weekly", "agy:google-native"),
        ("two", "3p-5h", "agy:external"),
    ] {
        let bin = dir.path().join(installation);
        std::fs::create_dir(&bin).unwrap();
        let script = bin.join("agy");
        let usage = String::from_utf8(USAGE.to_vec())
            .unwrap()
            .replace("Gemini Flash, Gemini Pro", "Nova Flash")
            .replace("gemini-weekly", bucket);
        metadata_binary(&script, &usage);
        let _path = crate::test_support::PathGuard::set(&bin);
        assert_eq!(
            pool("agy", "nova-4-flash-low", "agy", Some(&home)),
            expected
        );
        let path = cache_path("agy", Some(&home)).unwrap();
        assert!(path.is_file());
        assert_eq!(
            path,
            cache_path(script.to_str().unwrap(), Some(&home)).unwrap()
        );
        let alias = bin.join("alias");
        std::os::unix::fs::symlink(&script, &alias).unwrap();
        assert_eq!(
            path,
            cache_path(alias.to_str().unwrap(), Some(&home)).unwrap()
        );
        paths.push(path);
    }
    assert_ne!(paths[0], paths[1]);
    for path in paths {
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn agy_corrupt_cache_falls_back_and_successful_probe_rewrites_it() {
    use std::os::unix::fs::PermissionsExt;
    let _exec = crate::test_support::ExecGuard::new();
    let dir = tempfile::tempdir().unwrap();
    let usage = String::from_utf8(USAGE.to_vec())
        .unwrap()
        .replace("Gemini Flash, Gemini Pro", "Nova Flash");
    for (index, invalid) in ["", "unknown-pool", "google-native:bad"].iter().enumerate() {
        let script = dir.path().join(format!("agy-{index}"));
        std::fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable = resolved_executable(script.to_str().unwrap());
        let home = dir.path();
        let path = cache_path(&executable, Some(home)).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "pools": {"nova-4-flash-low": invalid}
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            pool("agy", "nova-4-flash-low", &executable, Some(home)),
            "agy:external"
        );
        assert!(
            !path.exists(),
            "invalid persisted catalog must be discarded"
        );
        metadata_binary(&script, &usage);
        let key = (executable.clone(), Some(home.to_path_buf()));
        CACHE
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .get_mut(&key)
            .unwrap()
            .retry_at = None;
        assert_eq!(
            settled_pool("agy", "nova-4-flash-low", &executable, Some(home)),
            "agy:google-native"
        );
        assert_eq!(
            read_persisted(&executable, Some(home))
                .unwrap()
                .pool("agy", "nova-4-flash-low"),
            "agy:google-native"
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn agy_parsed_catalog_rejects_unknown_pool_identities() {
    let usage = String::from_utf8(USAGE.to_vec())
        .unwrap()
        .replace("gemini-weekly", "unknown-pool");
    let catalog = Catalog::parse(MODELS, usage.as_bytes());
    assert!(catalog.is_err());
    assert_eq!(
        catalog
            .unwrap_or_default()
            .pool("agy", "gemini-3.8-flash-high"),
        "agy:google-native"
    );
}

#[cfg(unix)]
#[test]
fn agy_routing_uses_the_usage_capability_gate_and_shared_deadline() {
    use std::os::unix::fs::PermissionsExt;
    let _exec = crate::test_support::ExecGuard::new();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("agy");
    for help in [
        "echo unsupported",
        "sleep 10; echo --disable-slash-commands",
    ] {
        std::fs::write(&script, format!(
            "#!/bin/sh\ncase \"$1\" in\nmodels) echo;;\n--help) {help};;\n*) touch \"$HOME/inference\";;\nesac\n"
        )).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let start = Instant::now();
        assert!(load(
            script.to_str().unwrap(),
            Some(dir.path()),
            Duration::from_millis(100)
        )
        .is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(!dir.path().join("inference").exists());
    }
}
