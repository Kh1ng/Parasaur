use super::*;
use std::io::{BufRead, BufReader, Write};

pub(super) fn readmits_deferred_item(complete_first: bool) {
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 605..=606 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-pressure.md")),
            format!("# TICKET-{id}: Recover node capacity\n\nGoal: admit this same ticket after recovery.\nRecommended backend: codex\n"),
        ).unwrap();
    }
    let fake_bin = tmp.path().join("bin");
    let calls = tmp.path().join("calls");
    let release = tmp.path().join("release");
    let first = tmp.path().join("first");
    let second_started = tmp.path().join("second-started");
    fs::create_dir_all(&fake_bin).unwrap();
    for path in [&release, &second_started] {
        let path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    }
    let mut second_started = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&second_started)
        .unwrap();
    let mut release = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&release)
        .unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then echo https://github.com/owner/real/pull/1; else echo '[]'; fi\n",
    );
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\ngit branch --show-current >> '{calls}'\nif ( set -C; : > '{first}' ) 2>/dev/null; then read -r release < '{release}'; else echo started > '{second_started}'; fi\nprintf 'agent edit\\n' > recovered.txt\n",
            calls = calls.display(), first = first.display(), release = tmp.path().join("release").display(), second_started = tmp.path().join("second-started").display(),
        ),
    );
    let mut command = spawn_bin(tmp.path());
    let pressure = tmp.path().join("node-pressure.json");
    write_node_pressure_with_load(&pressure, 16, 32, 50.0);
    let mut child = ProcessGroupGuard::new(
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
                "--json",
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
    let stderr = child.stderr.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = thread::spawn(move || {
        let mut output = String::new();
        for line in BufReader::new(stderr).lines() {
            let line = line.unwrap();
            if line.contains("route is ready but node admission deferred") {
                let _ = send.send(());
            }
            output.push_str(&line);
            output.push('\n');
        }
        output
    });
    receive
        .recv_timeout(WORKER_EVENT_TIMEOUT)
        .expect("must actually defer a dispatch before recovery");
    write_node_pressure_with_load(&pressure, 16, 32, 0.5);
    if !complete_first {
        // A successful reprobe must refill while the first backend is still
        // blocked. Wait for the second backend's event, never elapsed time.
        let reader = second_started.try_clone().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let started = thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(reader).read_line(&mut line).unwrap();
            let _ = send.send(line);
        });
        let event = receive.recv_timeout(WORKER_EVENT_TIMEOUT);
        if event.is_err() {
            writeln!(second_started, "cancel").unwrap();
        }
        started.join().unwrap();
        assert_eq!(
            event.expect(
                "recovered node must admit the deferred backend while its sibling is active"
            ),
            "started\n"
        );
    }
    // Finishing the first backend also exercises the idle refill path.
    writeln!(release, "release").unwrap();
    let output = child.wait_with_output().unwrap();
    let stderr = reader.join().unwrap();
    assert!(output.status.success(), "{stderr}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let start = stdout.rfind("\n[\n").map_or(0, |index| index + 1);
    let results: serde_json::Value = serde_json::from_str(&stdout[start..]).expect(&stdout);
    let results = results.as_array().unwrap();
    let deferred = results
        .iter()
        .find(|result| result["outcome"].as_str().unwrap().starts_with("Deferred "))
        .expect("node deferral must remain observable");
    let work_id = &deferred["action"]["work_id"];
    assert!(work_id.is_string(), "{deferred}");
    assert!(results.iter().any(|result| result["action"]["work_id"] == *work_id
        && result["outcome"].as_str().unwrap().starts_with("Dispatched ticket")),
        "the same node-deferred item must be admitted on the next refill tick: {results:?}\n{stderr}");
    assert_eq!(fs::read_to_string(calls).unwrap().lines().count(), 2);
}
