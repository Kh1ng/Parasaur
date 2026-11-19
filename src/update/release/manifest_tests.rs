use super::*;
use std::fs;

fn manifest_asset(name: &str, kind: ReleaseAssetKind, sha256: &str) -> ReleaseAsset {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "kind": match kind {
            ReleaseAssetKind::Cli => "cli",
            ReleaseAssetKind::McpServer => "mcp-server",
            ReleaseAssetKind::ServerBundle => "server-bundle",
        },
        "sha256": sha256,
    }))
    .unwrap()
}

#[test]
fn manifest_parses_schema_one_and_rejects_other_schemas() {
    let manifest: ReleaseManifest = serde_json::from_value(serde_json::json!({
        "schema": 1,
        "version": "0.1.3",
        "channel": "edge",
        "commit": "abc123",
        "assets": [
            { "name": "gah-linux-x86_64", "kind": "cli", "sha256": "00" },
            { "name": "gah-server-bundle.tar.gz", "kind": "server-bundle", "sha256": "11" },
        ]
    }))
    .unwrap();
    assert_eq!(manifest.assets.len(), 2);
    assert_eq!(manifest.assets[0].kind, ReleaseAssetKind::Cli);
    assert_eq!(manifest.assets[1].kind, ReleaseAssetKind::ServerBundle);
    assert_eq!(manifest.commit.as_deref(), Some("abc123"));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    fs::write(
        &path,
        r#"{"schema":9,"version":"9.9.9","channel":"edge","assets":[]}"#,
    )
    .unwrap();
    let error = checked_manifest(&ManifestLocation::Path(path)).unwrap_err();
    assert!(error
        .to_string()
        .contains("unsupported release manifest schema 9"));
}

#[test]
fn github_slug_accepts_https_and_ssh_remotes_only() {
    assert_eq!(
        github_slug("https://github.com/Kh1ng/git-agent-harness.git"),
        Some("Kh1ng/git-agent-harness".to_string())
    );
    assert_eq!(
        github_slug("git@github.com:Kh1ng/git-agent-harness.git"),
        Some("Kh1ng/git-agent-harness".to_string())
    );
    assert_eq!(github_slug("git@gitlab.com:group/repo.git"), None);
    assert_eq!(github_slug("https://github.com/Kh1ng"), None);
}

#[test]
fn release_version_ordering_is_numeric_and_downgrade_detected() {
    assert_eq!(
        compare_release_versions("0.1.4", "0.1.3"),
        Ordering::Greater
    );
    assert_eq!(compare_release_versions("0.1.3", "0.1.3"), Ordering::Equal);
    assert_eq!(compare_release_versions("0.1.2", "0.1.3"), Ordering::Less);
    assert_eq!(compare_release_versions("0.2.0", "0.10.0"), Ordering::Less);
    // Malformed components sort as 0: never above a real version.
    assert_eq!(
        compare_release_versions("not-a-version", "0.0.1"),
        Ordering::Less
    );
}

#[test]
fn cli_asset_selection_matches_platform_naming() {
    let assets = vec![
        manifest_asset("gah-macos-universal", ReleaseAssetKind::Cli, "00"),
        manifest_asset("gah-linux-x86_64", ReleaseAssetKind::Cli, "11"),
        manifest_asset(
            "gah-server-bundle.tar.gz",
            ReleaseAssetKind::ServerBundle,
            "22",
        ),
    ];
    let wanted = if cfg!(target_os = "macos") {
        "gah-macos-universal"
    } else if cfg!(target_arch = "x86_64") {
        "gah-linux-x86_64"
    } else {
        "gah-linux-aarch64"
    };
    let selected = asset_for_current_platform(&assets, ReleaseAssetKind::Cli).unwrap();
    assert_eq!(selected.name, wanted);
    // The MCP server is its own kind: a CLI asset never stands in for it.
    assert!(asset_for_current_platform(&assets, ReleaseAssetKind::McpServer).is_err());

    let bundle_only = vec![manifest_asset(
        "gah-server-bundle.tar.gz",
        ReleaseAssetKind::ServerBundle,
        "22",
    )];
    assert!(asset_for_current_platform(&bundle_only, ReleaseAssetKind::Cli).is_err());
}

#[test]
fn asset_locator_resolves_local_and_release_url_assets() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("edge-manifest.json");
    fs::write(&manifest_path, "{}").unwrap();
    let local = AssetLocator::new(ManifestLocation::Path(manifest_path.clone()));
    let asset = manifest_asset("gah-linux-x86_64", ReleaseAssetKind::Cli, "00");
    match local.source(&asset).unwrap() {
        // The asset does not exist next to the manifest, so it must
        // resolve against the manifest-relative base URL instead.
        AssetSource::Path(path) => {
            unreachable!("no local asset exists: {path:?}");
        }
        AssetSource::Url(url) => {
            assert!(url.ends_with("gah-linux-x86_64"), "{url}");
        }
    }
    let asset_file = dir.path().join("gah-linux-x86_64");
    fs::write(&asset_file, "binary").unwrap();
    match local.source(&asset).unwrap() {
        AssetSource::Path(path) => assert_eq!(path, asset_file),
        AssetSource::Url(_) => unreachable!("local asset must resolve locally"),
    }

    let remote = AssetLocator::new(ManifestLocation::Path(manifest_path.clone()));
    let named = serde_json::from_value::<ReleaseAsset>(serde_json::json!({
        "name": "gah-linux-x86_64",
        "kind": "cli",
    }))
    .unwrap();
    match remote.source(&named).unwrap() {
        AssetSource::Path(path) => assert_eq!(path, asset_file),
        AssetSource::Url(_) => unreachable!("local asset exists and must be used"),
    }

    let github = AssetLocator::new(ManifestLocation::Url(
        "https://github.com/Kh1ng/git-agent-harness/releases/download/edge/edge-manifest.json"
            .to_string(),
    ));
    match github.source(&named).unwrap() {
        AssetSource::Url(url) => assert_eq!(
            url,
            "https://github.com/Kh1ng/git-agent-harness/releases/download/edge/gah-linux-x86_64"
        ),
        AssetSource::Path(_) => unreachable!("a URL manifest resolves remotely"),
    }
    let absolute = serde_json::from_value::<ReleaseAsset>(serde_json::json!({
        "name": "gah-linux-x86_64",
        "kind": "cli",
        "url": "https://example.com/gah",
    }))
    .unwrap();
    match github.source(&absolute).unwrap() {
        AssetSource::Url(url) => assert_eq!(url, "https://example.com/gah"),
        AssetSource::Path(_) => unreachable!(),
    }
}

#[test]
fn checksum_verification_accepts_match_and_rejects_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = dir.path().join("gah-fake");
    fs::write(&artifact, "release bytes").unwrap();
    let digest = sha256_of(&artifact).unwrap();
    let asset = manifest_asset("gah-fake", ReleaseAssetKind::Cli, &digest);
    verify_asset(&asset, &artifact).unwrap();

    let wrong = manifest_asset("gah-fake", ReleaseAssetKind::Cli, "deadbeef");
    let error = verify_asset(&wrong, &artifact).unwrap_err();
    assert!(error.to_string().contains("checksum mismatch"));

    let unsigned = serde_json::from_value::<ReleaseAsset>(serde_json::json!({
        "name": "gah-fake",
        "kind": "cli",
    }))
    .unwrap();
    verify_asset(&unsigned, &artifact).unwrap();
}
