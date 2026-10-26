//! Strict registry metadata, kept separate from installed CLI banner parsing.
use super::{output, BackendKind, PROBE_TIMEOUT};
use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct PackageRelease {
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct PypiRelease {
    info: PackageRelease,
}

#[derive(Deserialize)]
struct AgyRelease {
    version: String,
    url: String,
    sha512: String,
}

fn strict_version(version: &str) -> Result<String> {
    // Full SemVer syntax, including numeric prerelease identifiers without leading zeros.
    let valid = regex::Regex::new(concat!(
        r"\A(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)",
        r"(?:-((?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)",
        r"(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?",
        r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\z"
    ))?;
    if !valid.is_match(version) {
        bail!("Invalid release version");
    }
    Ok(version.to_owned())
}

fn package_version(text: &str, expected: &str, pypi: bool) -> Result<String> {
    let release: PackageRelease = if pypi {
        serde_json::from_str::<PypiRelease>(text)?.info
    } else {
        serde_json::from_str(text)?
    };
    if release.name != expected {
        bail!("Release metadata names an unexpected package");
    }
    strict_version(&release.version)
}

fn agy_version(text: &str) -> Result<String> {
    let release: AgyRelease = serde_json::from_str(text)?;
    let url = url::Url::parse(&release.url).context("Invalid release URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || release.sha512.len() != 128
        || !release.sha512.bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("Invalid release manifest URL or SHA-512");
    }
    strict_version(&release.version)
}

pub(super) fn latest(kind: BackendKind, source: &str) -> Result<String> {
    if let Some(package) = source.strip_prefix("npm:") {
        let text = output(
            "npm",
            &["view", package, "name", "version", "--json"],
            PROBE_TIMEOUT,
        )?;
        return package_version(&text, package, false);
    }
    let text = output(
        "curl",
        &[
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "10",
            source,
        ],
        PROBE_TIMEOUT,
    )?;
    match kind {
        BackendKind::Vibe => package_version(&text, "mistral-vibe", true),
        BackendKind::Agy => agy_version(&text),
        _ => bail!("Unsupported release registry"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_metadata_requires_identity_schema_and_an_entire_semver() {
        assert_eq!(
            package_version(
                r#"{"name":"expected","version":"1.2.3-rc.1+build"}"#,
                "expected",
                false
            )
            .unwrap(),
            "1.2.3-rc.1+build"
        );
        for text in [
            r#"{"name":"wrong","version":"1.2.3"}"#,
            r#"{"version":"1.2.3"}"#,
            "junk",
            r#""1.2.3""#,
        ] {
            assert!(package_version(text, "expected", false).is_err());
        }
        for version in [
            "02.0.0",
            "1.2.3;ignored",
            "1.2.3 trailing",
            "1.2.3-01",
            "v1.2.3",
            "1.2.3\n",
            "1.2",
        ] {
            let text = serde_json::json!({"name":"expected", "version":version}).to_string();
            assert!(
                package_version(&text, "expected", false).is_err(),
                "{version}"
            );
        }
        assert!(package_version(
            r#"{"info":{"name":"wrong","version":"1.2.3"}}"#,
            "mistral-vibe",
            true
        )
        .is_err());
        let mut manifest = serde_json::json!({"version":"1.2.3", "url":"https://example.org/agy", "sha512":"a".repeat(128)});
        assert_eq!(agy_version(&manifest.to_string()).unwrap(), "1.2.3");
        manifest["url"] = "http://example.org/agy".into();
        assert!(agy_version(&manifest.to_string()).is_err());
        manifest["url"] = "https://example.org/agy".into();
        manifest["sha512"] = "junk".into();
        assert!(agy_version(&manifest.to_string()).is_err());
        assert!(agy_version(r#"{"version":"1.2.3"}"#).is_err());
    }
}
