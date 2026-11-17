//! `scripts/macos-launchd.sh` registers a worker with the settings the worker
//! itself runs with. The script's own functions run here against stand-ins;
//! nothing is installed, loaded or contacted.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// Runs the script's startup adoption and its `register_worker` with
/// `inherited` in the environment and `env_file` as `gah-loop.env`, and
/// returns the config path the registration command was given.
fn registration_config(inherited: &[(&str, &str)], env_file: &str) -> String {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    std::fs::create_dir_all(home.join(".config/gah")).unwrap();
    std::fs::write(home.join(".config/gah/gah-loop.env"), env_file).unwrap();
    let seen = home.join("seen");
    let gah = home.join("gah");
    std::fs::write(
        &gah,
        "#!/bin/sh\nprintf %s \"$PARASAUR_CONFIG\" > \"$SEEN\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&gah, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/macos-launchd.sh");
    // The same two parts of the script, in its order and under its options,
    // with the plist, the health wait and the registration command replaced.
    let probe = r#"
        eval "$(sed -n '/^adopted_settings=()$/,/^adopt_old_setting_names$/p' "$1")"
        set -euo pipefail
        eval "$(sed -n '/^register_worker() {$/,/^}$/p' "$1")"
        registration_command="$2"
        plist_value() {
            if [ "$1" = PARASAUR_BINARY ]; then echo "$registration_command"; else echo stand-in; fi
        }
        wait_for_worker_health() { :; }
        register_worker 2>/dev/null || true
    "#;
    let output = Command::new("bash")
        .args(["-c", probe, "probe"])
        .arg(&script)
        .arg(&gah)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("SEEN", &seen)
        .envs(inherited.iter().copied())
        .output()
        .unwrap();
    assert!(
        seen.exists(),
        "registration did not run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(seen).unwrap()
}

#[test]
fn worker_registration_reads_the_env_file_before_adopting_old_names() {
    let old = [("GAH_CONFIG", "/inherited-old.toml")];
    let new = [("PARASAUR_CONFIG", "/inherited-new.toml")];
    for (inherited, env_file, expected) in [
        (&old[..], "", "/inherited-old.toml"),
        // An install from before the rename: the file's value is the worker's.
        (&old[..], "GAH_CONFIG=/file-old.toml\n", "/file-old.toml"),
        (
            &old[..],
            "PARASAUR_CONFIG=/file-new.toml\n",
            "/file-new.toml",
        ),
        (
            &old[..],
            "GAH_CONFIG=/file-old.toml\nPARASAUR_CONFIG=/file-new.toml\n",
            "/file-new.toml",
        ),
        // A current name the caller set is not an adopted one: it stays.
        (
            &new[..],
            "GAH_CONFIG=/file-old.toml\n",
            "/inherited-new.toml",
        ),
        (&[][..], "GAH_CONFIG=/file-old.toml\n", "/file-old.toml"),
    ] {
        assert_eq!(
            registration_config(inherited, env_file),
            expected,
            "inherited {inherited:?}, file {env_file:?}"
        );
    }
}
