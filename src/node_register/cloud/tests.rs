use super::*;
use crate::node_register::{register_with, CurlRegisterTransport};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn server(statuses: Vec<u16>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        let mut requests = Vec::new();
        for status in statuses {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline, "missing HTTP request");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            request.push_str(std::str::from_utf8(&body).unwrap());
            requests.push(request);
            write!(
                socket,
                "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        }
        requests
    });
    (url, worker)
}

struct Fixture {
    _dir: tempfile::TempDir,
    opts: RegisterOptions,
    cfg: GahConfig,
    cloud_url: String,
}

impl Fixture {
    fn new(central_url: String, cloud_url: String) -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            opts: RegisterOptions {
                central_url,
                advertised_url: Some("http://127.0.0.1:3773".into()),
                transport_mode: "trusted_lan".into(),
                secret_ref: "env:LOCAL_ONLY_SECRET".into(),
                profiles: Some(vec!["test-profile".into()]),
                token: Some("local-only-token".into()),
                config_path: None,
                identity_path: Some(dir.path().join("identity.json")),
            },
            _dir: dir,
            cfg: toml::from_str("[defaults]\norchestration_mode = 'hosted'").unwrap(),
            cloud_url,
        }
    }

    fn env(&self, key: &str) -> Option<String> {
        match key {
            "GAH_SUPABASE_URL" => Some(self.cloud_url.clone()),
            "GAH_SUPABASE_ANON_KEY" => Some("fake-anon".into()),
            "GAH_SUPABASE_ACCESS_TOKEN" => Some("fake-user-jwt".into()),
            "GAH_SUPABASE_DEVICE_TOKEN" => Some("fake-device-token".into()),
            "GAH_SUPABASE_WORKSPACE_ID" => Some("11111111-1111-4111-8111-111111111111".into()),
            _ => None,
        }
    }

    fn sync(&self, cloud: &mut CloudRegistration) -> bool {
        cloud.sync(&self.cfg, Some(&self.opts), |key| self.env(key))
    }

    fn register_locally(&self) {
        register_with(&CurlRegisterTransport, &self.opts).unwrap();
    }
}

#[test]
fn node_register_cloud_upserts_then_heartbeats_on_subsequent_passes() {
    let (url, server) = server(vec![201, 204, 200, 200, 200]);
    let mut fixture = Fixture::new(url.clone(), url);
    fixture.cfg.defaults.orchestration_mode = OrchestrationMode::SelfHosted;
    fixture.register_locally();
    let mut cloud = CloudRegistration::default();
    fixture.sync(&mut cloud);
    fixture.sync(&mut cloud);
    fixture.sync(&mut cloud);
    assert_eq!(cloud.state, Some(CloudState::Healthy));
    assert_eq!(cloud.failures, 0);
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("POST /api/registry/nodes "));
    assert!(requests[1].starts_with("POST /rest/v1/rpc/register_cloud_node "));
    assert!(requests[1].contains("apikey: fake-anon\r\n"));
    assert!(requests[1].contains("Authorization: Bearer fake-user-jwt\r\n"));
    let payload: serde_json::Value =
        serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        payload["p_workspace_id"],
        "11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(payload["p_profiles"][0], "test-profile");
    let identity =
        super::super::read_identity(fixture.opts.identity_path.as_ref().unwrap()).unwrap();
    assert_eq!(payload["p_node_id"], identity.node_id);
    assert_eq!(payload["p_display_name"], identity.display_name);
    assert_eq!(payload["p_advertised_url"], "http://127.0.0.1:3773");
    for request in &requests[2..] {
        assert!(request.starts_with("POST /rest/v1/rpc/cloud_node_heartbeat "));
        assert!(request.contains("Authorization: Bearer fake-anon\r\n"));
        assert!(request.contains(r#"{"p_device_token":"fake-device-token"}"#));
    }
    for request in &requests[1..] {
        assert!(!request.contains("LOCAL_ONLY_SECRET"));
        assert!(!request.contains("local-only-token"));
        assert!(!request.contains("secret_ref"));
    }
}

#[test]
fn node_register_cloud_401_does_not_fail_local_registration_and_retries() {
    let (url, server) = server(vec![201, 401, 401, 204, 200, 401, 401, 200, 401, 201]);
    let fixture = Fixture::new(url.clone(), url);
    fixture.register_locally();
    let mut cloud = CloudRegistration::default();
    for (expected, transition_logged) in [
        (CloudState::Http(401), true),
        (CloudState::Http(401), false),
        (CloudState::Healthy, true),
        (CloudState::Http(401), true),
        (CloudState::Http(401), false),
        (CloudState::Healthy, true),
        (CloudState::Http(401), true),
    ] {
        assert_eq!(fixture.sync(&mut cloud), transition_logged);
        assert_eq!(cloud.state, Some(expected));
    }
    assert_eq!(cloud.failures, 5);
    fixture.register_locally();
    let requests = server.join().unwrap();
    assert!(requests[1..4]
        .iter()
        .all(|r| r.contains("/rpc/register_cloud_node ")));
    assert!(requests[4..9]
        .iter()
        .all(|r| r.contains("/rpc/cloud_node_heartbeat ")));
}

#[test]
fn node_register_cloud_connection_refused_still_registers_locally() {
    let refused = TcpListener::bind("127.0.0.1:0").unwrap();
    let cloud_url = format!("http://{}", refused.local_addr().unwrap());
    drop(refused);
    let (url, server) = server(vec![201, 201]);
    let fixture = Fixture::new(url, cloud_url);
    fixture.register_locally();
    let mut cloud = CloudRegistration::default();
    fixture.sync(&mut cloud);
    fixture.sync(&mut cloud);
    assert_eq!(cloud.state, Some(CloudState::Transport));
    assert_eq!(cloud.failures, 2);
    assert!(cloud.registered.is_none());
    fixture.register_locally();
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn node_register_local_mode_never_reads_cloud_environment_or_identity() {
    let cfg = toml::from_str("").unwrap();
    let mut cloud = CloudRegistration::default();
    cloud.sync(&cfg, None, |_| panic!("local mode must stay offline"));
    assert!(cloud.state.is_none());
}

#[test]
fn node_register_cloud_missing_credentials_degrade_without_becoming_fatal() {
    let cfg = toml::from_str("[defaults]\norchestration_mode = 'hosted'").unwrap();
    let mut cloud = CloudRegistration::default();
    cloud.sync(&cfg, None, |_| None);
    assert_eq!(
        cloud.state,
        Some(CloudState::Configuration("GAH_SUPABASE_URL"))
    );
    assert_eq!(cloud.failures, 1);
}
