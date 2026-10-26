//! In-process central API with per-instance facts and a complete request log.

use super::ScenarioHarness;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct FakeCentral {
    pub url: String,
    state: Arc<Mutex<Value>>,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl FakeCentral {
    /// Bind before returning. State changes and snapshots are serialized with requests.
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(json!({
            "nodes": {}, "claims": {}, "resolutions": {}, "quotas": {}, "calls": []
        })));
        let stop = Arc::new(AtomicBool::new(false));
        let server = {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((socket, _)) => serve(socket, &state),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("fake central accept: {error}"),
                    }
                }
            })
        };
        Self {
            url,
            state,
            stop,
            server: Some(server),
        }
    }

    pub fn state(&self) -> Value {
        self.state.lock().unwrap().clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Value)) {
        change(&mut self.state.lock().unwrap());
    }

    /// Close the listener and join its thread; subsequent connects fail immediately.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            let result = server.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

impl Drop for FakeCentral {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve(mut socket: TcpStream, state: &Mutex<Value>) {
    socket.set_nonblocking(false).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(&mut socket);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap();
    let target = parts.next().unwrap();
    let mut length = 0;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
    }
    assert!(length < 1024 * 1024, "unexpected fake central request size");
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    let url = url::Url::parse(&format!("http://localhost{target}")).unwrap();
    let mut state = state.lock().unwrap();
    state["calls"].as_array_mut().unwrap().push(json!({
        "method": method, "target": target, "body": body
    }));
    let (status, response) = respond(&mut state, method, &url, body);
    let response = response.to_string();
    write!(socket, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
}

fn respond(state: &mut Value, method: &str, url: &url::Url, body: Value) -> (u16, Value) {
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let profile = query.get("profile").map(String::as_str).unwrap_or("");
    match (method, url.path()) {
        ("POST", "/api/registry/nodes") => {
            state["nodes"][body["node_id"].as_str().unwrap()] = body.clone();
            (201, body)
        }
        ("GET", "/api/registry/fleet") => (200, json!([])),
        ("GET", "/api/registry/quota") => state["quotas"]
            .get(profile)
            .map(|quota| (200, quota.clone()))
            .unwrap_or((404, json!({"error":"quota_project_not_found"}))),
        ("GET", "/api/skills/resolve") => {
            let key = format!(
                "{profile}/{}/{}",
                query.get("backend").map(String::as_str).unwrap_or(""),
                query.get("instance").map(String::as_str).unwrap_or("")
            );
            state["resolutions"]
                .get(&key)
                .map(|resolution| (200, resolution.clone()))
                .unwrap_or((404, json!({"error":"resolution_not_seeded"})))
        }
        ("POST", "/api/claims/acquire" | "/api/claims/renew" | "/api/claims/release") => {
            let key = format!(
                "{}/{}",
                body["profile"].as_str().unwrap(),
                body["work_id"].as_str().unwrap()
            );
            let held = state["claims"].get(&key);
            if let Some(held) = held {
                if held["node_id"] != body["node_id"] {
                    return (409, json!({"message":"already claimed", "held_by":held}));
                }
            } else if !url.path().ends_with("acquire") {
                return (404, json!({"error":"claim_not_found"}));
            }
            if url.path().ends_with("release") {
                state["claims"].as_object_mut().unwrap().remove(&key);
                (200, json!({"released":true}))
            } else {
                state["claims"][&key] = body;
                (200, json!({"expires_at":"2099-01-01T00:00:00Z"}))
            }
        }
        _ => (404, json!({"error":"route_not_seeded"})),
    }
}

impl ScenarioHarness {
    /// Use a fake central while preserving the real worker registration and HTTP clients.
    pub fn with_central(self, central: &FakeCentral) -> Self {
        // Advisory loop registration needs an existing advertised endpoint.
        std::fs::write(
            self._temp.path().join("identity.json"),
            json!({
                "node_id":"scenario-worker", "display_name":"Scenario worker",
                "advertised_url":"http://127.0.0.1:3773",
                "version":"0.1.2", "schema_digest":"scenario"
            })
            .to_string(),
        )
        .unwrap();
        self.with_defaults_append(&format!(
            "node_role = \"worker\"\nregistry_central_url = {:?}",
            central.url
        ))
    }

    /// Start a central-profile CLI with separate config/state, without another ExecGuard.
    pub fn central_status(&mut self) -> Value {
        self.setup_env();
        self.install_fakes();
        let mut config: toml::Value =
            toml::from_str(&std::fs::read_to_string(&self.config_path).unwrap()).unwrap();
        config["defaults"]
            .as_table_mut()
            .unwrap()
            .remove("registry_central_url");
        config["defaults"]["node_role"] = toml::Value::String("central".into());
        let path = self._temp.path().join("central.toml");
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let output = self
            .gah_command()
            .env("GAH_CONFIG", path)
            .env("XDG_STATE_HOME", self._temp.path().join("central-state"))
            .args(["status", "--profile", &self.profile_name, "--json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Explicit operator transition: clear the shared central URL and install warmed skills locally.
    pub fn use_local_skills(&mut self, resolution: &Value) {
        self.defaults_append = "node_role = \"standalone\"\n".into();
        let path = self._temp.path().join("home/.config/gah/skills.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bindings: serde_json::Map<String, Value> = resolution["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|skill| {
                (
                    skill["id"].as_str().unwrap().to_string(),
                    json!([format!(
                        "backend:{}",
                        resolution["backend"].as_str().unwrap()
                    )]),
                )
            })
            .collect();
        std::fs::write(
            path,
            json!({"skills":resolution["skills"], "bindings":bindings}).to_string(),
        )
        .unwrap();
    }
}
