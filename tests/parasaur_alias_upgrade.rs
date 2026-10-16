//! #1364: a release upgrade from an older `gah` runs that older updater, which
//! replaces `gah` but never creates `parasaur`. Start from that state and check
//! the new binary's first run completes the layout.
#![cfg(unix)]

use std::process::Command;

#[test]
fn first_run_after_an_old_updater_adds_parasaur() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".cargo/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gah = bin.join("gah");
    std::fs::copy(env!("CARGO_BIN_EXE_gah"), &gah).unwrap();
    assert!(bin.join("parasaur").symlink_metadata().is_err());

    let status = Command::new(&gah)
        .arg("--version")
        .env("HOME", home.path())
        .env_remove("CARGO_HOME")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read_link(bin.join("parasaur")).unwrap(),
        std::path::Path::new("gah")
    );
    let alias = Command::new(bin.join("parasaur"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(alias.status.success());
}
