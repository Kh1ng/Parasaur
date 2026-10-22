//! `gah diag export` saves the server's gzip bundle byte for byte and never
//! replaces an earlier export with an error response.
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
    thread,
};

#[path = "support/cli.rs"]
mod cli_support;

/// Serves one response, runs `gah diag export --out OUT`, and returns the
/// CLI output with the request line and headers the server saw.
fn export(
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    out: &Path,
) -> (Output, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let count = socket.read(&mut buf).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buf[..count]);
        }
        write!(
            socket,
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        socket.write_all(&body).unwrap();
        String::from_utf8(request).unwrap()
    });
    // Isolated HOME/XDG state, like every other `gah` child in the suite.
    let output = cli_support::gah_command()
        .args(["diag", "export", "--server", &url, "--out"])
        .arg(out)
        .env("GAH_SERVER_TOKEN", "test-owner-token")
        .env_remove("GAH_SERVER_URL")
        .output()
        .unwrap();
    (output, server.join().unwrap())
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut child = Command::new("gzip")
        .arg("-c")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    output.stdout
}

#[test]
fn diag_export_saves_the_gzip_bundle_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("bundle.json.gz");
    let bundle = serde_json::json!({
        "format": "parasaur-diagnostics/1",
        "node_role": "worker",
        "timeline": [{"t": "a"}, {"t": "b"}]
    });
    let served = gzip(bundle.to_string().as_bytes());
    let (output, request) = export("200 OK", "application/gzip", served.clone(), &out);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(request.starts_with("GET /api/diagnostics/export "));
    assert!(request.contains("Authorization: Bearer test-owner-token"));
    assert_eq!(fs::read(&out).unwrap(), served, "saved byte for byte");
    let unzipped = Command::new("gzip").arg("-dc").arg(&out).output().unwrap();
    assert!(
        unzipped.status.success(),
        "{}",
        String::from_utf8_lossy(&unzipped.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&unzipped.stdout).unwrap(),
        bundle
    );
}

#[test]
fn diag_export_rejection_keeps_the_existing_output_file() {
    let cases: [(&'static str, &'static str, &[u8], &str); 4] = [
        (
            "403 Forbidden",
            "application/json",
            br#"{"error":"owner_required"}"#,
            "HTTP 403",
        ),
        (
            "429 Too Many Requests",
            "application/json",
            br#"{"error":"rate_limited"}"#,
            "HTTP 429",
        ),
        (
            "500 Internal Server Error",
            "application/json",
            br#"{"error":"diagnostics_export_failed"}"#,
            "HTTP 500",
        ),
        // A success page that is not gzip (a proxy, an older server) is no bundle.
        (
            "200 OK",
            "text/html",
            b"<!doctype html><p>not a bundle</p>",
            "not a gzip diagnostics bundle",
        ),
    ];
    for (status, content_type, body, expected) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("bundle.json.gz");
        fs::write(&out, b"earlier export").unwrap();
        let (output, _) = export(status, content_type, body.to_vec(), &out);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{status} must fail the export");
        assert!(stderr.contains(expected), "{status}: {stderr}");
        assert_eq!(fs::read(&out).unwrap(), b"earlier export", "{status}");
        assert_eq!(
            fs::read_dir(tmp.path()).unwrap().count(),
            1,
            "{status} leaves no staged file"
        );
    }
}
