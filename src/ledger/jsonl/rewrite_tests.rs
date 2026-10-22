use super::{append, backfill_review_verdict, read_entries, rewrite_locked, ReviewVerdictBackfill};
use crate::ledger::test_util as ledger_tests;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

fn implementation_entry(branch: &str) -> crate::ledger::LedgerEntry {
    let mut entry = crate::ledger::LedgerEntry::new(
        "test",
        &ledger_tests::profile(),
        "vibe",
        "improve",
        "rewrite",
        None,
        None,
    );
    entry.branch = Some(branch.to_string());
    entry
}

fn approve() -> ReviewVerdictBackfill<'static> {
    ReviewVerdictBackfill {
        verdict: "APPROVE",
        confidence: "high",
        reviewer_backend: "claude",
        reviewer_model: None,
        reviewer_tier: None,
        review_gate_reason: None,
        review_source_sha: None,
        review_metadata_fingerprint: None,
        review_contract_version: None,
        review_generation: None,
        blocking_findings: &[],
        actionable_findings: &[],
        non_blocking_findings: &[],
        risk_notes: &[],
        evidence: &[],
        compatibility_evidence: &[],
    }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

/// Files a rewrite could leave behind: anything beside the ledger that is
/// not the ledger, its lock, or its sqlite mirror.
fn stray_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with("ledger.") || name.contains("tmp"))
        .collect();
    names.sort();
    names
}

#[test]
fn failed_rewrite_leaves_the_ledger_bytes_and_no_staged_file() {
    let (tmp, cfg) = ledger_tests::test_config();
    let path = cfg.defaults.ledger_path();
    append(&cfg, &implementation_entry("gah/a")).unwrap();
    append(&cfg, &implementation_entry("gah/b")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let before = fs::read(&path).unwrap();

    let _lock = crate::ledger::locking::exclusive(&path).unwrap();
    let err = rewrite_locked(&path, |out| {
        out.write_all(b"{\"timestamp\":\"partial")?;
        out.flush()?;
        anyhow::bail!("injected failure mid-write")
    })
    .unwrap_err();

    assert!(format!("{err:#}").contains("injected failure mid-write"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(mode(&path), 0o640);
    assert!(
        stray_files(tmp.path()).is_empty(),
        "{:?}",
        stray_files(tmp.path())
    );
}

#[test]
fn backfill_replaces_the_ledger_by_rename_and_keeps_its_mode() {
    let (tmp, cfg) = ledger_tests::test_config();
    let path = cfg.defaults.ledger_path();
    append(&cfg, &implementation_entry("gah/a")).unwrap();
    append(&cfg, &implementation_entry("gah/b")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let inode_before = fs::metadata(&path).unwrap().ino();

    assert!(backfill_review_verdict(&cfg, "gah/a", approve()).unwrap());

    assert_ne!(
        fs::metadata(&path).unwrap().ino(),
        inode_before,
        "the ledger must be replaced, not truncated and rewritten in place"
    );
    assert_eq!(mode(&path), 0o640);
    assert!(
        stray_files(tmp.path()).is_empty(),
        "{:?}",
        stray_files(tmp.path())
    );
    let entries = read_entries(&cfg).unwrap();
    let verdicts: Vec<_> = entries
        .iter()
        .map(|entry| (entry.branch.as_deref(), entry.review_verdict.as_deref()))
        .collect();
    assert_eq!(
        verdicts,
        [(Some("gah/a"), Some("APPROVE")), (Some("gah/b"), None)]
    );
    assert!(fs::read(&path).unwrap().ends_with(b"}\n"));
}

#[test]
fn backfill_of_a_symlinked_ledger_replaces_the_link_target() {
    let (tmp, cfg) = ledger_tests::test_config();
    let path = cfg.defaults.ledger_path();
    let real_dir = tempfile::tempdir().unwrap();
    let real = real_dir.path().join("ledger.jsonl");
    fs::write(&real, b"").unwrap();
    std::os::unix::fs::symlink(&real, &path).unwrap();
    append(&cfg, &implementation_entry("gah/a")).unwrap();

    assert!(backfill_review_verdict(&cfg, "gah/a", approve()).unwrap());

    assert!(fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    let entries = read_entries(&cfg).unwrap();
    assert_eq!(entries[0].review_verdict.as_deref(), Some("APPROVE"));
    assert!(stray_files(tmp.path()).is_empty());
    assert!(stray_files(real_dir.path()).is_empty());
}
