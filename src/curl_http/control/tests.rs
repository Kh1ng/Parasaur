use super::*;
use std::{
    io::Read,
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
};

fn request_bytes(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 32 * 1024);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..index]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                })
                .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                .unwrap();
            if bytes.len() >= index + 4 + length {
                return bytes;
            }
        }
    }
}

fn server(
    reply: impl FnOnce(&mut TcpStream) + Send + 'static,
) -> (String, mpsc::Receiver<Vec<u8>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        tx.send(request_bytes(&mut stream)).unwrap();
        reply(&mut stream);
    });
    (
        format!("http://{address}/api/native-dispatch/operation"),
        rx,
        handle,
    )
}

#[test]
fn native_control_posts_private_config_and_keeps_wire_json_intact() {
    let _guard = crate::test_support::ExecGuard::new();
    let (url, received, handle) = server(|stream| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .unwrap()
    });
    let body =
        serde_json::to_string(&serde_json::json!({"line":"one\ntwo","quoted":"a\"b\\c"})).unwrap();
    let response = request_control(&url, &body, "fixture-private-credential").unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"{}");
    let request =
        String::from_utf8(received.recv_timeout(Duration::from_secs(2)).unwrap()).unwrap();
    handle.join().unwrap();
    assert!(request.contains("Authorization: Bearer fixture-private-credential\r\n"));
    assert_eq!(request.split_once("\r\n\r\n").unwrap().1, body);
}

#[test]
fn unknown_length_response_cannot_grow_capture_or_echo_remote_secrets() {
    let _guard = crate::test_support::ExecGuard::new();
    let (url, received, handle) = server(|stream| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
        let _ = stream.write_all("REMOTE_PRIVATE_DATA".repeat(16 * 1024).as_bytes());
    });
    let error = request_control(&url, "{}", "fixture-private-credential")
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("REMOTE_PRIVATE_DATA"));
    assert!(!error.contains("fixture-private-credential"));
    assert!(error.contains("transport failed") || error.contains("response limit"));
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    handle.join().unwrap();
}

#[test]
fn unresponsive_control_peer_has_a_hard_deadline() {
    let _guard = crate::test_support::ExecGuard::new();
    let (stop_tx, stop_rx) = mpsc::channel();
    let (url, received, handle) = server(move |stream| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n")
            .unwrap();
        let _ = stop_rx.recv_timeout(Duration::from_secs(8));
    });
    let start = std::time::Instant::now();
    let result = request_control(&url, "{}", "fixture-private-credential");
    assert!(result.is_err());
    assert!(start.elapsed() < Duration::from_secs(7));
    let _ = stop_tx.send(());
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    handle.join().unwrap();
}

#[test]
fn unsafe_control_destinations_and_credentials_fail_before_connecting() {
    for (url, token) in [
        ("file:///tmp/not-a-network", "fixture"),
        ("http://user:password@127.0.0.1/", "fixture"),
        ("http://127.0.0.1/?token=hidden", "fixture"),
        ("http://127.0.0.1/", "fixture\r\nInjected: header"),
    ] {
        let error = request_control(url, "{}", token).err().unwrap().to_string();
        assert!(!error.contains("password"));
        assert!(!error.contains("Injected"));
        assert!(!error.contains("hidden"));
    }
}
