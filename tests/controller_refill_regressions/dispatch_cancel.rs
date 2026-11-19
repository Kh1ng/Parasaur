use super::*;
use serde_json::Value;
use std::path::Path;

fn entries(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[test]
fn cancel_dispatch_preserves_only_the_selected_workers_changes_and_releases_its_claim() {
    cancel_while_running(false);
}

#[test]
fn cancel_during_validation_checkpoints_before_publication_and_preserves_the_sibling() {
    cancel_while_running(true);
}

fn cancel_while_running(during_validation: bool) {
    let _serial = serial();
    let tmp = test_tempdir();
    let root = tmp.path();
    let check = root.join("bin/check");
    let (repo, home, cfg) = setup_fix_dispatch_repo(
        &tmp,
        &format!("validation_commands = [\"{}\"]\n", check.display()),
    );
    let tickets = repo.join("docs/tickets");
    let markers = root.join("markers");
    let bin = root.join("bin");
    for dir in [&tickets, &markers, &bin] {
        fs::create_dir_all(dir).unwrap();
    }
    for id in [601, 602] {
        fs::write(tickets.join(format!("TICKET-{id}-cancel.md")), format!(
            "# TICKET-{id}: Scoped worker cancellation\n\nGoal: update work.txt.\nRecommended backend: codex\n"
        )).unwrap();
    }

    make_fake_bin_with_body(&bin, "gh", "#!/bin/sh\nif [ \"$1 $2\" = 'pr create' ]; then echo 'https://github.com/owner/real/pull/1'; else echo '[]'; fi\n");
    let blocking = r#"#!/bin/sh
[ -f work.txt ] || exit 0
sleep 300 &
descendant=$!
branch=$(git branch --show-current)
marker="$FIXTURE/markers/$$"
printf '%s\n%s\n%s\n' "$branch" "$PWD" "$descendant" > "$marker.tmp"
mv "$marker.tmp" "$marker"
while [ -d "$FIXTURE" ] && [ ! -f "$FIXTURE/release" ]; do sleep 0.05; done
kill "$descendant"
wait "$descendant" 2>/dev/null || :
printf 'completed\n' > work.txt
printf '%s\n' "$branch" >> "$FIXTURE/completed"
"#
    .replace("$FIXTURE", root.to_str().unwrap());
    let edit = "printf 'in progress\\n' > work.txt\n";
    if during_validation {
        make_fake_bin_with_body(&bin, "codex", &format!("#!/bin/sh\n{edit}"));
        make_fake_bin_with_body(&bin, "check", &blocking);
    } else {
        make_fake_bin_with_body(
            &bin,
            "codex",
            &blocking.replace("[ -f work.txt ] || exit 0", edit),
        );
        make_fake_bin_with_body(&bin, "check", "#!/bin/sh\nexit 0\n");
    }

    if during_validation {
        make_fake_bin_with_body(&root.join("github-root/owner/real.git/hooks"), "pre-receive",
            &r#"#!/bin/sh
read old new reference
if [ -f "$FIXTURE/watch-push" ] && [ "$reference" = "refs/heads/$(cat "$FIXTURE/watch-push")" ]; then
  touch "$FIXTURE/publishing"
  while [ -d "$FIXTURE" ] && [ ! -f "$FIXTURE/release-push" ]; do sleep 0.05; done
fi
"#.replace("$FIXTURE", root.to_str().unwrap()));
    }

    let claims = root.join("claims.json");
    let ledger = root.join("ledger.jsonl");
    let release = root.join("release");
    let artifacts = root.join("artifacts/real");
    let command = || {
        let mut cmd = spawn_bin(root);
        cmd.env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("HOME", &home)
        .env("GITHUB_TOKEN", "fixture")
        .env("PARASAUR_NODE_ROLE", "standalone")
        .env("PARASAUR_CLAIM_STATE_PATH", &claims)
        .env("PARASAUR_LEDGER_PATH", &ledger)
        .env("PARASAUR_EVENTS_PATH", root.join("events.jsonl"));
        cmd
    };
    let config = cfg.to_str().unwrap();
    let controller = ProcessGroupGuard::new(
        command()
            .args([
                "loop",
                "--once",
                "--parallel",
                "2",
                "--profile",
                "real",
                "--config-path",
                config,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let workers =
        || git_agent_harness::running_workers::observe(&artifacts, time::OffsetDateTime::now_utc());
    let controller = wait_for_worker_event(
        controller,
        &release,
        || {
            workers().iter().filter(|row| row.cancellable).count() == 2
                && fs::read_dir(&markers)
                    .unwrap()
                    .flatten()
                    .filter(|m| m.path().extension().is_none())
                    .count()
                    == 2
        },
        "two independent backends hold claims",
    );
    let observed = workers();
    let selected = observed
        .iter()
        .find(|w| w.work_id.as_deref() == Some("TICKET-601"))
        .unwrap();
    let sibling = observed
        .iter()
        .find(|w| w.work_id.as_deref() == Some("TICKET-602"))
        .unwrap();
    let marker = fs::read_dir(&markers)
        .unwrap()
        .flatten()
        .map(|p| fs::read_to_string(p.path()).unwrap())
        .find(|text| text.lines().next() == selected.branch.as_deref())
        .unwrap();
    let mut lines = marker.lines().skip(1);
    let worktree = Path::new(lines.next().unwrap());
    let descendant = lines.next().unwrap().parse::<u32>().unwrap();

    let wrong = command()
        .args([
            "cancel",
            "601",
            "--profile",
            "real",
            "--run-id",
            &sibling.run_id,
            "--config-path",
            config,
        ])
        .output()
        .unwrap();
    assert!(!wrong.status.success());
    let cancelled = command()
        .args(["cancel", "#0601", "--json", "--config-path", config])
        .output()
        .unwrap();
    assert!(
        cancelled.status.success(),
        "{}",
        String::from_utf8_lossy(&cancelled.stderr)
    );
    let receipt: Value = serde_json::from_slice(&cancelled.stdout).unwrap();
    assert_eq!(receipt["run_id"], selected.run_id);
    assert_eq!(receipt["work_id"], "TICKET-601");

    let only_sibling_claimed = || {
        let state: Value = fs::read(&claims)
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default();
        state["claims"]["real@real"]
            .as_array()
            .is_some_and(|left| left.len() == 1 && left[0]["work_id"] == "#602")
    };
    let controller = wait_for_worker_event(
        controller,
        &release,
        || {
            only_sibling_claimed()
                && entries(&ledger)
                    .iter()
                    .any(|e| e["validation_result"] == "cancelled_operator")
        },
        "the selected worker checkpoints and releases its claim while its sibling runs",
    );
    let terminal = entries(&ledger)
        .into_iter()
        .find(|e| e["validation_result"] == "cancelled_operator")
        .unwrap();
    assert_eq!(terminal["work_id"], "TICKET-601");
    assert_eq!(
        terminal["attempts"][0]["validation_result"],
        "cancelled_operator"
    );
    assert_eq!(
        terminal["failure_stage"],
        if during_validation {
            "post_validation"
        } else {
            "agent_run"
        }
    );
    let sha = terminal["resumable_checkpoint_sha"].as_str().unwrap();
    let saved = ProcessCommand::new("git")
        .args(["show", &format!("{sha}:work.txt")])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&saved.stdout), "in progress\n");
    assert!(!worktree.exists(), "cancelled worktree remains on disk");
    let stat = fs::read_to_string(format!("/proc/{descendant}/stat")).unwrap_or_default();
    assert!(
        stat.is_empty() || stat.split(") ").nth(1).unwrap().starts_with('Z'),
        "descendant still running: {stat}"
    );
    assert!(workers().iter().any(|w| w.run_id == sibling.run_id));

    fs::write(root.join("watch-push"), sibling.branch.as_deref().unwrap()).unwrap();
    fs::write(&release, "finish").unwrap();
    let controller = if during_validation {
        let release_push = root.join("release-push");
        let controller = wait_for_worker_event(
            controller,
            &release_push,
            || root.join("publishing").exists(),
            "sibling entered git publication",
        );
        assert!(!workers()
            .iter()
            .any(|row| row.run_id == sibling.run_id && row.cancellable));
        let too_late = command()
            .args([
                "cancel",
                "602",
                "--profile",
                "real",
                "--run-id",
                &sibling.run_id,
                "--config-path",
                config,
            ])
            .output()
            .unwrap();
        assert!(!too_late.status.success());
        assert!(
            String::from_utf8_lossy(&too_late.stderr).contains("No cancellable running dispatch")
        );
        fs::write(release_push, "finish").unwrap();
        controller
    } else {
        controller
    };
    let output = watchdog::wait_with_output(controller);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let branch = selected.branch.as_ref().unwrap();
    let published = ProcessCommand::new("git")
        .arg("--git-dir")
        .arg(root.join("github-root/owner/real.git"))
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .status()
        .unwrap();
    assert!(
        !published.success(),
        "cancelled dispatch branch was published"
    );
    let completed = fs::read_to_string(root.join("completed")).unwrap();
    assert_eq!(
        completed.trim(),
        sibling.branch.as_deref().unwrap(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        entries(&ledger)
            .iter()
            .any(|e| e["work_id"] == "TICKET-602" && e["validation_result"] == "passed"),
        "ledger: {}",
        fs::read_to_string(&ledger).unwrap()
    );
    assert!(workers().is_empty());
    let events = entries(&root.join("events.jsonl"));
    for run in [&selected.run_id, &sibling.run_id] {
        for event in ["dispatch_started", "dispatch_finished"] {
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e["run_id"] == **run && e["event_type"] == event)
                    .count(),
                1,
                "missing or duplicate {event} for {run}"
            );
        }
    }
    let reconcile = command()
        .args([
            "ledger",
            "reconcile",
            "--profile",
            "real",
            "--config-path",
            config,
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        reconcile.status.success(),
        "{}",
        String::from_utf8_lossy(&reconcile.stderr)
    );
    let report: Value = serde_json::from_slice(&reconcile.stdout).unwrap();
    assert!(
        report["new_entries"].as_array().unwrap().is_empty(),
        "{report}"
    );
}

#[test]
fn central_cancel_cli_sends_the_pinned_work_and_a_mutation_key() {
    use std::io::{BufRead, BufReader, Read, Write};
    let _serial = serial();
    let tmp = test_tempdir();
    let (_, _, cfg) = setup_fix_dispatch_repo(&tmp, "");
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut cmd = spawn_bin(tmp.path());
    let run = "7c271c72-f180-4c76-b0a0-ee24ed627bda";
    let child = ProcessGroupGuard::new(
        cmd.args([
            "cancel",
            "#601",
            "--json",
            "--profile",
            "real",
            "--run-id",
            run,
            "--config-path",
            cfg.to_str().unwrap(),
        ])
        .env("PARASAUR_NODE_ROLE", "central")
        .env("PARASAUR_SERVER_URL", origin)
        .env("PARASAUR_SERVER_TOKEN", "fixture-token")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut socket = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => panic!("CLI failed to contact the fixture: {e}"),
        }
    };
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(&socket);
    let mut request = String::new();
    while !request.ends_with("\r\n\r\n") {
        assert!(reader.read_line(&mut request).unwrap() > 0);
    }
    let body_size = request
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|size| size.parse::<usize>().ok())
        })
        .unwrap();
    let mut body = vec![0; body_size];
    reader.read_exact(&mut body).unwrap();
    assert!(request.starts_with("POST /api/dispatch/cancel HTTP/1.1\r\n"));
    assert!(request.contains("Authorization: Bearer fixture-token\r\n"));
    assert!(request.lines().any(|h| h
        .strip_prefix("Idempotency-Key: ")
        .is_some_and(|key| uuid::Uuid::parse_str(key).is_ok())));
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        serde_json::json!({"work_id":"#601","profile":"real","run_id":run})
    );
    let receipt =
        serde_json::json!({"work_id":"TICKET-601","profile":"real","run_id":run}).to_string();
    write!(
        socket,
        "HTTP/1.1 202 Accepted\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        receipt.len(),
        receipt
    )
    .unwrap();
    drop(socket);
    let output = watchdog::wait_with_output(child);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["run_id"],
        run
    );
}
