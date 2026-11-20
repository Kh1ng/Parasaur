//! Linux x64 release bundles already contain the server's locked production
//! packages in the WSL kit. Extract them into the disposable release staging tree.
use anyhow::{ensure, Context, Result};
use std::{fs, path::Path, process::Command};

/// Check the server entry point and resolve packages from the staged output.
pub(super) fn probe(node: &Path, dist: &Path) -> Result<()> {
    let imports = "await import('express'); await import('@git-agent-harness/contracts')";
    for args in [
        vec!["--check", "bin.js"],
        vec!["--input-type=module", "-e", imports],
    ] {
        ensure!(
            Command::new(node)
                .args(args)
                .current_dir(dist)
                .status()?
                .success(),
            "Bundled server runtime probe failed"
        );
    }
    Ok(())
}

pub(super) fn unpack_if_compatible(staging: &Path, revision: Option<&str>) -> Result<bool> {
    if !cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        return Ok(false);
    }
    let dist = staging.join("apps/server/dist");
    let kit = dist.join("worker-artifacts");
    let metadata = kit.join("worker-artifact.json");
    if !metadata.try_exists()? {
        return Ok(false);
    }
    let metadata: serde_json::Value = serde_json::from_slice(&fs::read(metadata)?)?;
    if metadata["runtime"] != "linux-x64-gnu" {
        return Ok(false);
    }
    let revision = revision.context("Packaged Linux runtime requires a release revision")?;
    ensure!(
        metadata["schema_version"] == 1 && metadata["revision"] == revision,
        "Packaged Linux runtime does not match the release revision"
    );
    let archive = kit.join("source.tar.gz");
    ensure!(
        metadata["files"]["source.tar.gz"] == super::sha256_of(&archive)?,
        "Packaged Linux runtime checksum mismatch"
    );

    let unpack = tempfile::tempdir_in(staging)?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(unpack.path())
        .args(["./REVISION", "./apps/server/dist/.release-runtime"])
        .status()
        .context("extracting packaged Linux server dependencies")?;
    ensure!(
        status.success(),
        "Packaged Linux server dependency extraction failed"
    );
    let runtime = unpack.path().join("apps/server/dist/.release-runtime");
    ensure!(
        fs::read_to_string(unpack.path().join("REVISION"))?.trim() == revision
            && runtime.join("node_modules").is_dir()
            && fs::read(runtime.join("package-lock.json"))?
                == fs::read(staging.join("package-lock.json"))?,
        "Packaged Linux server dependencies do not match the release"
    );
    let destination = dist.join(".release-runtime");
    fs::rename(&destination, unpack.path().join("original-metadata"))?;
    fs::rename(runtime, destination)?;
    Ok(true)
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod tests {
    use super::*;

    #[test]
    fn only_matching_prebuilt_packages_replace_staged_metadata() {
        let outer = tempfile::tempdir().unwrap();
        let inner = tempfile::tempdir().unwrap();
        let relative = Path::new("apps/server/dist/.release-runtime");
        let staged = outer.path().join(relative);
        let packed = inner.path().join(relative);
        let kit = outer.path().join("apps/server/dist/worker-artifacts");
        let revision = "a".repeat(40);
        fs::create_dir_all(&staged).unwrap();
        fs::create_dir_all(&kit).unwrap();
        fs::create_dir_all(packed.join("node_modules")).unwrap();
        fs::write(staged.join("original"), "before").unwrap();
        fs::write(packed.join("node_modules/example"), "installed").unwrap();
        fs::write(packed.join("package-lock.json"), "lock").unwrap();
        fs::write(inner.path().join("REVISION"), &revision).unwrap();
        let archive = kit.join("source.tar.gz");
        assert!(Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(inner.path())
            .arg(".")
            .status()
            .unwrap()
            .success());
        let manifest = kit.join("worker-artifact.json");
        let mut info = serde_json::json!({"schema_version":1,"revision":revision,"files":{}});
        fs::write(&manifest, info.to_string()).unwrap();
        assert!(!unpack_if_compatible(outer.path(), Some(&revision)).unwrap());
        info["runtime"] = "linux-x64-gnu".into();
        fs::write(&manifest, info.to_string()).unwrap();
        assert!(unpack_if_compatible(outer.path(), Some("wrong"))
            .unwrap_err()
            .to_string()
            .contains("revision"));
        assert!(unpack_if_compatible(outer.path(), Some(&revision))
            .unwrap_err()
            .to_string()
            .contains("checksum"));
        info["files"]["source.tar.gz"] =
            crate::update::release::sha256_of(&archive).unwrap().into();
        fs::write(manifest, info.to_string()).unwrap();
        fs::write(outer.path().join("package-lock.json"), "another lock").unwrap();
        assert!(unpack_if_compatible(outer.path(), Some(&revision)).is_err());
        assert!(staged.join("original").is_file());
        fs::write(outer.path().join("package-lock.json"), "lock").unwrap();
        assert!(unpack_if_compatible(outer.path(), Some(&revision)).unwrap());
        assert_eq!(
            fs::read(staged.join("node_modules/example")).unwrap(),
            b"installed"
        );
    }
}
