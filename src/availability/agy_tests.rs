use super::*;

const MODELS: &str = include_str!("../../tests/fixtures/agy/models.tsv");
const USAGE: &[u8] = include_bytes!("../../tests/fixtures/agy/usage.json");

thread_local! {
    pub(super) static FIXTURE: std::cell::RefCell<Option<Catalog>> = const { std::cell::RefCell::new(None) };
}

pub(crate) struct Fixture;
pub(crate) fn fixture() -> Fixture {
    FIXTURE.with(|cell| *cell.borrow_mut() = Some(Catalog::parse(MODELS, USAGE).unwrap()));
    Fixture
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
        "agy:gemini-weekly"
    );
    assert_eq!(
        catalog.pool("agy", "Gemini 3.8 Flash (High)"),
        "agy:gemini-weekly"
    );
    assert_eq!(
        catalog.pool("agy-second", "gemini-3.8-flash-low"),
        "agy-second:gemini-weekly"
    );
    assert_eq!(catalog.pool("agy", "claude-sonnet-4-6"), "agy:3p-weekly");
    assert_eq!(catalog.pool("agy", "gpt-oss-120b-medium"), "agy:3p-weekly");
    assert_eq!(catalog.pool("agy", "not-listed"), "agy:default");
}

#[test]
fn agy_new_family_and_pool_are_provider_metadata() {
    let usage = String::from_utf8(USAGE.to_vec())
        .unwrap()
        .replace("Gemini Flash, Gemini Pro", "Nova Flash")
        .replace("gemini-weekly", "new-provider-pool");
    let catalog =
        Catalog::parse("nova-4-flash-low\tNova 4 Flash (Low)\n", usage.as_bytes()).unwrap();
    assert_eq!(
        catalog.pool("agy", "nova-4-flash-low"),
        "agy:new-provider-pool"
    );
}

#[test]
fn agy_retired_model_disappears_when_inventory_is_replaced() {
    let models = MODELS
        .lines()
        .filter(|line| !line.starts_with("gemini-3.8-flash-low\t"))
        .collect::<Vec<_>>()
        .join("\n");
    let catalog = Catalog::parse(&models, USAGE).unwrap();
    assert_eq!(catalog.pool("agy", "gemini-3.8-flash-low"), "agy:default");
    assert_eq!(
        catalog.pool("agy", "gemini-3.8-flash-high"),
        "agy:gemini-weekly"
    );
}

#[test]
fn agy_unknown_shapes_and_ambiguous_membership_use_default() {
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
    assert_eq!(catalog.pool("agy", "gemini-3.8-flash-high"), "agy:default");
    ambiguous["command"]["data"]["groups"][1]["buckets"][0]["id"] = "gemini-weekly".into();
    assert!(Catalog::parse(MODELS, &serde_json::to_vec(&ambiguous).unwrap()).is_err());
    assert!(Catalog::parse("changed format", USAGE).is_err());
    let changed = String::from_utf8(USAGE.to_vec())
        .unwrap()
        .replace("Models within this group:", "Unknown membership:");
    assert!(Catalog::parse(MODELS, changed.as_bytes()).is_err());
    assert_eq!(
        Catalog::default().pool("agy-two", "anything"),
        "agy-two:default"
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
    let executable = script.to_str().unwrap();
    assert_eq!(
        pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-one:gemini-weekly"
    );
    std::fs::write(home.join("models"), "").unwrap();
    assert_eq!(
        pool("agy-two", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-two:gemini-weekly"
    );
    assert_eq!(
        std::fs::read_to_string(home.join("calls"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    let cache = CACHE.get().unwrap();
    cache
        .lock()
        .unwrap()
        .get_mut(&(executable.to_string(), Some(home.clone())))
        .unwrap()
        .at -= TTL;
    assert_eq!(
        pool("agy-one", "gemini-3.8-flash-high", executable, Some(&home)),
        "agy-one:default"
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
    cache
        .lock()
        .unwrap()
        .get_mut(&(executable.to_string(), Some(home.clone())))
        .unwrap()
        .at -= TTL;
    let identity = |instance: &str| {
        routing.execution_identity_for_candidate(&crate::config::CandidateConfig {
            backend: "agy".into(),
            instance: Some(instance.into()),
            model: Some("gemini-3.8-flash-high".into()),
            ..Default::default()
        })
    };
    assert_eq!(
        identity("agy-primary").quota_pool.as_deref(),
        Some("agy-primary:gemini-weekly")
    );
    assert_eq!(
        identity("agy-secondary").quota_pool.as_deref(),
        Some("agy-secondary:gemini-weekly")
    );
    let mut profile = crate::config::tests::test_profile_for_notifications();
    profile.agy_path = Some(executable.into());
    profile.agy_second_home = Some(home.to_string_lossy().into_owned());
    let routing = profile.effective_routing(&crate::config::Defaults::default());
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
        Some("agy-second:gemini-weekly")
    );
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("models"), MODELS).unwrap();
    std::fs::write(other.join("usage"), USAGE).unwrap();
    assert_eq!(
        pool("agy-two", "gemini-3.8-flash-high", executable, Some(&other)),
        "agy-two:gemini-weekly"
    );
}
