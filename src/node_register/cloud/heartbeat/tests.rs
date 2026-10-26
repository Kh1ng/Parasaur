use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const LEASE: Duration = Duration::from_millis(90);
const INTERVAL: Duration = Duration::from_millis(20);

#[derive(Default)]
struct Lease {
    renewals: usize,
    last: Option<Instant>,
    expired: bool,
}

struct FakeCloud {
    url: String,
    lease: Arc<Mutex<Lease>>,
    ready: mpsc::Receiver<()>,
    stop: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl FakeCloud {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let lease = Arc::new(Mutex::new(Lease::default()));
        let observed = Arc::clone(&lease);
        let (ready_tx, ready) = mpsc::channel();
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || loop {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut socket);
                    let mut request = String::new();
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        assert!(reader.read_line(&mut line).unwrap() > 0);
                        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                            length = value.trim().parse().unwrap();
                        }
                        request.push_str(&line);
                        if line == "\r\n" {
                            break;
                        }
                    }
                    reader.read_exact(&mut vec![0; length]).unwrap();
                    if request.starts_with("POST /rest/v1/rpc/cloud_node_heartbeat ") {
                        let mut lease = observed.lock().unwrap();
                        let now = Instant::now();
                        lease.expired |= lease.last.is_some_and(|last| now - last >= LEASE);
                        lease.last = Some(now);
                        lease.renewals += 1;
                        ready_tx.send(()).unwrap();
                    }
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("{error}"),
            }
            match stopped.recv_timeout(Duration::from_millis(1)) {
                Err(RecvTimeoutError::Timeout) => {}
                Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            }
        });
        Self {
            url,
            lease,
            ready,
            stop: Some(stop),
            worker: Some(worker),
        }
    }

    fn start(
        &self,
        shutdown: Arc<AtomicBool>,
        interval: Duration,
    ) -> (tempfile::TempDir, CloudHeartbeat) {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity.json");
        let url = self.url.clone();
        let cfg = toml::from_str("[defaults]\norchestration_mode = 'hosted'").unwrap();
        let heartbeat = CloudHeartbeat::start_with(
            cfg,
            None,
            move |key| match key {
                "GAH_SUPABASE_URL" => Some(url.clone()),
                "GAH_SUPABASE_ANON_KEY" => Some("fake-anon".into()),
                "GAH_SUPABASE_ACCESS_TOKEN" => Some("fake-user".into()),
                "GAH_SUPABASE_DEVICE_TOKEN" => Some("fake-device".into()),
                "GAH_SUPABASE_WORKSPACE_ID" => Some("11111111-1111-4111-8111-111111111111".into()),
                "GAH_COORDINATOR_IDENTITY_PATH" => Some(identity.to_string_lossy().into_owned()),
                "GAH_NODE_ADVERTISED_URL" => Some("http://127.0.0.1:3773".into()),
                _ => None,
            },
            move || shutdown.load(Ordering::SeqCst),
            interval,
        );
        self.ready.recv_timeout(Duration::from_secs(5)).unwrap();
        (dir, heartbeat)
    }
}

impl Drop for FakeCloud {
    fn drop(&mut self) {
        drop(self.stop.take());
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn cloud_heartbeat_renews_during_a_tick_longer_than_the_lease() {
    let cloud = FakeCloud::new();
    let (_dir, heartbeat) = cloud.start(Arc::new(AtomicBool::new(false)), INTERVAL);

    // The controller thread cannot heartbeat while this slow tick is executing.
    let slow_tick = || thread::sleep(LEASE * 3);
    slow_tick();

    let lease = cloud.lease.lock().unwrap();
    assert!(lease.renewals >= 4, "slow tick prevented lease renewals");
    assert!(!lease.expired, "lease expired between heartbeats");
    assert!(lease.last.unwrap().elapsed() < LEASE, "lease is offline");
    drop(lease);
    drop(heartbeat);
}

#[test]
fn cloud_heartbeat_stops_on_shutdown_and_drop_without_waiting_for_interval() {
    for signal_shutdown in [false, true] {
        let cloud = FakeCloud::new();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (_dir, heartbeat) = cloud.start(Arc::clone(&shutdown), HEARTBEAT_INTERVAL);
        let start = Instant::now();
        if signal_shutdown {
            shutdown.store(true, Ordering::SeqCst);
            while !heartbeat.worker.as_ref().unwrap().is_finished() {
                assert!(start.elapsed() < Duration::from_secs(1));
                thread::sleep(Duration::from_millis(5));
            }
        }
        drop(heartbeat);
        assert!(start.elapsed() < Duration::from_secs(1));
        let count = cloud.lease.lock().unwrap().renewals;
        thread::sleep(INTERVAL * 2);
        assert_eq!(cloud.lease.lock().unwrap().renewals, count);
    }
}

#[test]
fn cloud_heartbeat_local_mode_has_no_thread_or_http_and_stops_a_cloud_worker() {
    let cfg = toml::from_str("").unwrap();
    let mut heartbeat = CloudHeartbeat::default();
    heartbeat.refresh(&cfg);
    assert!(heartbeat.worker.is_none());
    assert!(heartbeat.config.is_none());

    let cloud = FakeCloud::new();
    let (_dir, mut heartbeat) = cloud.start(Arc::new(AtomicBool::new(false)), INTERVAL);
    heartbeat.refresh(&cfg);
    assert!(heartbeat.worker.is_none());
    assert!(heartbeat.config.is_none());
    let count = cloud.lease.lock().unwrap().renewals;
    thread::sleep(INTERVAL * 2);
    assert_eq!(cloud.lease.lock().unwrap().renewals, count);
}
