//! Exercise the real CLI hook; cloud failure must preserve central success.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

#[path = "support/cli.rs"]
mod cli_support;

#[test]
fn node_register_cloud_cli_preserves_local_success() {
    for cloud_status in [Some(204), Some(401), None] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let central_url = format!("http://{}", listener.local_addr().unwrap());
        let cloud_url = if cloud_status.is_some() {
            central_url.clone()
        } else {
            let refused = TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", refused.local_addr().unwrap())
        };
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut statuses = vec![201];
            if let Some(status) = cloud_status {
                statuses.push(status);
                if status == 204 {
                    statuses.push(200);
                }
            }
            for status in statuses {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline, "missing CLI request");
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(&mut socket);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                reader.read_exact(&mut vec![0; length]).unwrap();
                write!(
                    socket,
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "[defaults]\norchestration_mode = 'hosted'\n").unwrap();
        let output = cli_support::gah_command()
            .args(["node", "register", "--config"])
            .arg(&config)
            .args([
                "--central-url",
                &central_url,
                "--advertised-url",
                "http://127.0.0.1:3773",
            ])
            .env(
                "GAH_COORDINATOR_IDENTITY_PATH",
                dir.path().join("identity.json"),
            )
            .env("GAH_SUPABASE_URL", cloud_url)
            .env("GAH_SUPABASE_ANON_KEY", "fake-anon")
            .env("GAH_SUPABASE_ACCESS_TOKEN", "fake-user-jwt")
            .env("GAH_SUPABASE_DEVICE_TOKEN", "fake-device-token")
            .env(
                "GAH_SUPABASE_WORKSPACE_ID",
                "11111111-1111-4111-8111-111111111111",
            )
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Registered node against"));
        assert_eq!(
            stderr
                .matches("WARNING cloud node registration degraded")
                .count(),
            usize::from(cloud_status != Some(204))
        );
        for secret in ["fake-anon", "fake-user-jwt", "fake-device-token"] {
            assert!(!stderr.contains(secret), "{stderr}");
        }
        server.join().unwrap();
    }
}
