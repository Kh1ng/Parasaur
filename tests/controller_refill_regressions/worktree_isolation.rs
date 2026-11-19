//! Scenario 3 (#1518): two improve dispatches running at once never see each
//! other's uncommitted edits, the main checkout stays untouched, and neither
//! exit path (clean publish, preserved WIP after a failed validation) leaves a
//! worktree attached to the other dispatch's branch.

use super::watchdog::wait_with_output;
use super::{
    make_fake_bin_with_body, serial, setup_fix_dispatch_repo, spawn_bin, test_tempdir,
    ProcessGroupGuard,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn worktree_list(repo: &Path) -> Vec<(PathBuf, Option<String>)> {
    let out = ProcessCommand::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let mut entries = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch = None;
    for line in text.lines().chain(std::iter::once("")) {
        if let Some(rest) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("branch ") {
            branch = Some(rest.to_string());
        } else if line.is_empty() {
            if let Some(p) = path.take() {
                entries.push((p, branch.take()));
            }
        }
    }
    entries
}

#[test]
fn sibling_worktrees_never_see_each_others_uncommitted_edits() {
    let _lock = serial();
    let tmp = test_tempdir();
    // Validation fails only where a worker left `fail-marker`, so the first
    // dispatch takes the clean publish path and the second the preserved-WIP
    // path, inside one loop pass.
    let (repo, home, cfg) = setup_fix_dispatch_repo(
        &tmp,
        "validation_commands = [\"test ! -f fail-marker\"]\nmax_fix_attempts = 1\n",
    );
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 801..=802 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-sibling.md")),
            format!("# TICKET-{id}: Sibling isolation\n\nGoal: edit without seeing the sibling.\nRecommended backend: codex\n"),
        )
        .unwrap();
    }
    let fake_bin = tmp.path().join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf 'https://github.com/owner/real/pull/1\\n'; exit 0; fi\nexit 0\n",
    );
    let calls = tmp.path().join("codex-calls");
    let cwds = tmp.path().join("worker-cwds");
    let start_edits = tmp.path().join("start-edits");
    let release = tmp.path().join("release");
    let lock = tmp.path().join("count.lock");
    // Each worker writes one uncommitted file named after its call number,
    // records its working directory, and blocks until released. The second
    // worker also plants the marker that fails validation.
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\nwhile ! mkdir '{lock}' 2>/dev/null; do sleep 0.01; done\n\
             n=$( [ -f '{calls}.count' ] && cat '{calls}.count' || echo 0 )\nn=$((n + 1))\necho \"$n\" > '{calls}.count'\nrmdir '{lock}'\n\
             printf '%s %s\\n' \"$n\" \"$PWD\" >> '{cwds}'\n\
             while [ ! -f '{start_edits}' ]; do sleep 0.05; done\n\
             printf 'sibling edit %s\\n' \"$n\" > \"sibling-$n.txt\"\n\
             if [ \"$n\" = 2 ]; then : > fail-marker; fi\n\
             while [ ! -f '{release}' ]; do sleep 0.05; done\nexit 0\n",
            lock = lock.display(),
            calls = calls.display(),
            cwds = cwds.display(),
            start_edits = start_edits.display(),
            release = release.display(),
        ),
    );

    let mut command = spawn_bin(tmp.path());
    let child = ProcessGroupGuard::new(
        command
            .args([
                "loop",
                "--profile",
                "real",
                "--config-path",
                cfg.to_str().unwrap(),
                "--once",
                "--parallel",
                "2",
            ])
            .env(
                "PATH",
                format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", &home)
            .env("GITHUB_TOKEN", "token")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let recorded = loop {
        let recorded = fs::read_to_string(&cwds).unwrap_or_default();
        if recorded.lines().count() == 2 {
            break recorded;
        }
        assert!(Instant::now() < deadline, "both workers did not start");
        thread::sleep(Duration::from_millis(20));
    };
    let mut dirs: Vec<(u32, PathBuf)> = recorded
        .lines()
        .map(|line| {
            let (n, dir) = line.split_once(' ').unwrap();
            (n.parse().unwrap(), PathBuf::from(dir))
        })
        .collect();
    dirs.sort();
    assert_eq!(dirs.len(), 2, "{recorded}");
    assert_ne!(
        dirs[0].1, dirs[1].1,
        "both workers ran in the same worktree"
    );
    // Both real dispatches hold claims and are still clean, with no completed
    // ledger record. Age alone must not make their checkouts disposable.
    let finished = tmp
        .path()
        .join("worktrees/gah-real-1730000000-11111111111111111111111111111111");
    assert!(ProcessCommand::new("git")
        .current_dir(&repo)
        .args(["worktree", "add", "--detach"])
        .arg(&finished)
        .status()
        .unwrap()
        .success());
    for path in [&finished, &dirs[0].1, &dirs[1].1] {
        assert!(ProcessCommand::new("touch")
            .args(["-t", "202401010000"])
            .arg(path)
            .status()
            .unwrap()
            .success());
    }
    let pruned = spawn_bin(tmp.path())
        .args([
            "prune",
            "--profile",
            "real",
            "--older-than",
            "1",
            "--config-path",
        ])
        .arg(&cfg)
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("GITHUB_TOKEN", "token")
        .output()
        .unwrap();
    assert!(
        pruned.status.success(),
        "{}",
        String::from_utf8_lossy(&pruned.stderr)
    );
    assert!(!finished.exists(), "idle checkout was not reclaimed");
    assert!(
        dirs.iter().all(|(_, dir)| dir.join(".git").is_file()),
        "prune removed an active clean checkout"
    );
    fs::write(start_edits, "go").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !dirs[0].1.join("sibling-1.txt").is_file() || !dirs[1].1.join("fail-marker").is_file() {
        assert!(
            Instant::now() < deadline,
            "workers could not write after prune"
        );
        thread::sleep(Duration::from_millis(20));
    }
    for (n, dir) in &dirs {
        let other = if *n == 1 { 2 } else { 1 };
        assert!(dir.join(format!("sibling-{n}.txt")).exists(), "{dir:?}");
        assert!(
            !dir.join(format!("sibling-{other}.txt")).exists(),
            "{dir:?} sees the sibling's edit"
        );
        assert!(dir.starts_with(tmp.path().join("worktrees")), "{dir:?}");
    }
    assert!(
        !repo.join("sibling-1.txt").exists()
            && !repo.join("sibling-2.txt").exists()
            && !repo.join("fail-marker").exists(),
        "main checkout was touched"
    );

    // Each dispatch branch is attached to exactly its own worktree, and the
    // attachment refuses to be reused for a second checkout of that branch.
    let attached: Vec<_> = worktree_list(&repo)
        .into_iter()
        .filter(|(path, _)| path != &repo)
        .collect();
    assert_eq!(attached.len(), 2, "{attached:?}");
    for (path, branch) in &attached {
        let branch = branch
            .as_deref()
            .unwrap()
            .trim_start_matches("refs/heads/")
            .to_string();
        assert!(branch.starts_with("gah/real-"), "{branch}");
        let attachment = git_agent_harness::worktree::branch_attachment(&repo, &branch)
            .unwrap()
            .expect("branch is attached");
        assert_eq!(
            attachment.path.canonicalize().unwrap(),
            path.canonicalize().unwrap()
        );
        assert!(
            !attachment.clean,
            "the worker's uncommitted edit must be visible as dirty"
        );
        let reuse = git_agent_harness::worktree::create_existing(
            &repo,
            &branch,
            &tmp.path().join("worktrees"),
        );
        assert!(
            reuse.is_err(),
            "a second checkout of {branch} must be refused while the worker owns it"
        );
    }

    fs::write(&release, "release").unwrap();
    let output = wait_with_output(child);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    // Clean publish for the first, preserved WIP after the failed validation for the second.
    assert!(
        stdout.contains("pull/1") || stdout.contains("Published") || stdout.contains("PR"),
        "{stdout}"
    );
    assert!(
        stdout.contains("validation") && (stdout.contains("failed") || stdout.contains("FAILED")),
        "{stdout}"
    );

    // Neither exit path leaves a worktree attached to any dispatch branch, and
    // the main checkout is still untouched.
    let remaining: Vec<_> = worktree_list(&repo)
        .into_iter()
        .filter(|(path, _)| path != &repo)
        .collect();
    assert!(remaining.is_empty(), "{remaining:?}");
    for (_, dir) in &dirs {
        assert!(
            !dir.exists(),
            "{dir:?} still exists after its dispatch ended"
        );
    }
    assert!(
        !repo.join("sibling-1.txt").exists()
            && !repo.join("sibling-2.txt").exists()
            && !repo.join("fail-marker").exists()
    );
}
