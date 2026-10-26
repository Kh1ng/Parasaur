use super::*;

#[test]
fn agents_version_parses_banners_and_compares_without_downgrades() {
    assert_eq!(version("codex-cli 0.120.0").unwrap(), "0.120.0");
    assert_eq!(version("2.1.301 (Claude Code)").unwrap(), "2.1.301");
    assert_eq!(version("v3.0.1").unwrap(), "3.0.1");
    assert!(version("not installed").is_err());
    assert!(newer("1.9.0", "1.10.0"));
    assert!(!newer("2.0.0", "1.9.0"));
    assert!(!newer("1.0.0+build", "1.0.0"));
    assert!(newer("1.0.0-beta.1", "1.0.0"));
}

#[test]
fn agents_opencode_registry_tracks_the_installed_major_without_forcing_migration() {
    assert_eq!(
        source(BackendKind::Opencode, Some("1.19.4")).as_deref(),
        Some("npm:opencode-ai")
    );
    assert_eq!(
        source(BackendKind::Opencode, Some("2.0.21")).as_deref(),
        Some("npm:@opencode/cli")
    );
}
