use super::super::*;
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

struct Token(Option<std::ffi::OsString>);
impl Drop for Token {
    fn drop(&mut self) {
        match &self.0 {
            Some(value) => std::env::set_var("COORDINATOR_TOKEN", value),
            None => std::env::remove_var("COORDINATOR_TOKEN"),
        }
    }
}

fn receive(listener: &TcpListener) -> (TcpStream, String, serde_json::Value) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "missing controller request"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = std::io::BufReader::new(&mut stream);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let mut length = None;
    let mut authorized = false;
    for _ in 0..32 {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
            if name.eq_ignore_ascii_case("authorization") {
                authorized = value.trim() == "Bearer native-fixture";
            }
        }
    }
    assert!(authorized);
    let length = length.unwrap();
    assert!(length <= 64 * 1024);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (stream, first, serde_json::from_slice(&body).unwrap())
}

fn respond(mut stream: TcpStream, body: &serde_json::Value) {
    let body = serde_json::to_vec(body).unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(&body).unwrap();
}

#[test]
fn ownership_transfer_and_lost_replies_keep_the_original_run() {
    let _exec = crate::test_support::ExecGuard::new();
    let _token = Token(std::env::var_os("COORDINATOR_TOKEN"));
    std::env::set_var("COORDINATOR_TOKEN", "native-fixture");
    let (directory, mut cfg) = crate::ledger::test_util::test_config();
    let _claims =
        crate::test_support::ClaimStateEnvGuard::set(directory.path().join("claims.json"));
    let profile = crate::ledger::test_util::profile();
    let scope = crate::work_claim::canonical_claim_scope("test", &profile.repo_id);
    cfg.profiles.insert("test".into(), profile);
    let args: DispatchArgs = serde_json::from_value(serde_json::json!({
        "profile":"test", "mode":"review", "backend":"auto", "target":"gah/review",
        "branch":"gah/review", "expected_review_generation":"selected-generation",
        "current_branch":false, "dry_run":false, "retries":2, "allow_draft_fail":false,
        "prod":false, "issue_intake_override":false, "allow_unknown_red_baseline":false,
        "escalate":false, "skip_validation_gate":false, "work_id":"#71",
        "dispatch_reason":"review", "run_id":uuid::Uuid::new_v4().to_string()
    }))
    .unwrap();
    // Normal handoff, lost reservation reply, lost activation reply, wrong-work reply.
    for case in 0..4 {
        assert!(crate::work_claim::try_claim_work(&scope, "TICKET-071").unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        cfg.defaults.registry_central_url =
            Some(format!("http://{}", listener.local_addr().unwrap()));
        let profile = &cfg.profiles["test"];
        let run = uuid::Uuid::new_v4().to_string();
        let view = serde_json::json!({"binding":{
            "run_id":run, "context_sha256":"a".repeat(64), "node_id":"worker",
            "profile":"test", "repo_id":profile.repo_id, "repo":profile.repo,
            "work_id":if case == 3 { "#72" } else { "71" }, "mode":"review",
            "branch":"gah/review", "review_generation":"selected-generation",
            "dispatch_reason":"review", "backend":"codex", "backend_instance":"work", "model":"fixture"
        }, "phase":"reserved", "activated":false});
        let server = std::thread::spawn({
            let scope = scope.clone();
            move || {
                let (stream, path, request) = receive(&listener);
                assert!(path.starts_with("POST /api/native-dispatch/reserve "));
                assert_eq!(
                    request["args"]["expected_review_generation"],
                    "selected-generation"
                );
                assert!(crate::work_claim::is_claimed(&scope, "#71").unwrap());
                if case == 1 {
                    return;
                } // Caller never receives the durable reservation.
                respond(stream, &view);
                if case == 3 {
                    return;
                }
                let (stream, path, request) = receive(&listener);
                assert!(path.starts_with("POST /api/native-dispatch/offer "));
                assert!(!crate::work_claim::is_claimed(&scope, "#71").unwrap());
                assert_eq!(request["run_id"], view["binding"]["run_id"]);
                assert_eq!(request["context_sha256"], view["binding"]["context_sha256"]);
                if case == 2 {
                    return;
                } // Activation may have committed; never invent a new run.
                let mut active = view;
                active["phase"] = "stage_offered".into();
                active["activated"] = true.into();
                respond(stream, &active);
            }
        });
        let result = submit(&cfg, &args);
        server.join().unwrap();
        if matches!(case, 1 | 3) {
            assert!(result.is_err());
            assert!(crate::work_claim::is_claimed(&scope, "#71").unwrap());
        } else {
            let offered = result.unwrap();
            assert_eq!(offered.run_id, run);
            assert!(offered.detail.contains(if case == 2 {
                "activation_unconfirmed"
            } else {
                "stage_offered"
            }));
            assert!(!crate::work_claim::is_claimed(&scope, "#71").unwrap());
        }
        crate::work_claim::release_owned_work(&scope, "#71").unwrap();
    }
}
