//! Native WebView2 fixture; no provider page or real credentials are opened.
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::Manager;

#[test]
#[ignore = "opens two synthetic native WebView2 windows; PARASAUR_WINDOWS_BROWSER_TESTS=1"]
fn named_windows_keep_http_only_sessions_in_separate_stores() {
    assert_eq!(
        std::env::var("PARASAUR_WINDOWS_BROWSER_TESTS").as_deref(),
        Ok("1")
    );
    let root = std::env::temp_dir().join(format!(
        "parasaur-webview-cookie-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url: tauri::Url = format!("http://{}/", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let serving = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let stop = serving.clone();
    let server = std::thread::spawn(move || {
        while serving.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                let body = "<!doctype html><title>Synthetic account store</title><p>Cookie isolation fixture</p>";
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });
    let ready = Arc::new(AtomicUsize::new(0));
    let profiles_root = root.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let app = tauri::Builder::default()
        .any_thread()
        .setup(move |app| {
            for label in ["mistral-login-fixture-one", "mistral-login-fixture-two"] {
                let ready = ready.clone();
                let sender = sender.clone();
                let builder = tauri::WebviewWindowBuilder::new(
                    app,
                    label,
                    tauri::WebviewUrl::External(url.clone()),
                )
                .title("Parasaur synthetic cookie isolation")
                .inner_size(500.0, 280.0)
                .on_page_load(move |window, payload| {
                    if payload.event() != tauri::webview::PageLoadEvent::Finished
                        || ready.fetch_add(1, Ordering::SeqCst) != 1
                    {
                        return;
                    }
                    let app = window.app_handle().clone();
                    let sender = sender.clone();
                    std::thread::spawn(move || {
                        let result = (|| -> Result<(), String> {
                            for (label, value) in [
                                ("mistral-login-fixture-one", "synthetic-one"),
                                ("mistral-login-fixture-two", "synthetic-two"),
                            ] {
                                let window =
                                    app.get_webview_window(label).ok_or("fixture missing")?;
                                window
                                    .set_cookie(
                                        tauri::webview::Cookie::build(("session", value))
                                            .domain("127.0.0.1")
                                            .path("/")
                                            .http_only(true)
                                            .secure(true)
                                            .build(),
                                    )
                                    .map_err(|_| "cookie write failed")?;
                            }
                            for (label, expected) in [
                                ("mistral-login-fixture-one", "synthetic-one"),
                                ("mistral-login-fixture-two", "synthetic-two"),
                            ] {
                                let cookies = app
                                    .get_webview_window(label)
                                    .ok_or("fixture missing")?
                                    .cookies()
                                    .map_err(|_| "cookie read failed")?;
                                let sessions: Vec<_> = cookies
                                    .iter()
                                    .filter(|cookie| cookie.name() == "session")
                                    .collect();
                                if sessions.len() != 1
                                    || sessions[0].value() != expected
                                    || sessions[0].http_only() != Some(true)
                                    || sessions[0].secure() != Some(true)
                                {
                                    return Err(
                                        "account cookies crossed stores or lost HttpOnly/Secure"
                                            .into(),
                                    );
                                }
                            }
                            Ok(())
                        })();
                        let _ = sender.send(result);
                        app.exit(0);
                    });
                });
                let target = super::super::LoginTarget::new(
                    Some(label.strip_prefix("mistral-login-").unwrap().into()),
                    Some("Synthetic account".into()),
                )
                .unwrap();
                super::super::browser_store(builder, &target, &root).build()?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap();
    let watchdog = app.handle().clone();
    let (cancel, deadline) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if deadline.recv_timeout(Duration::from_secs(25)).is_err() {
            watchdog.exit(1);
        }
    });
    let exit = app.run_return(|_, _| {});
    let _ = cancel.send(());
    watchdog.join().unwrap();
    stop.store(false, Ordering::Relaxed);
    server.join().unwrap();
    assert_eq!(exit, 0, "native fixture timed out");
    receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let resolved = profiles_root.canonicalize().unwrap();
    assert_eq!(
        resolved.parent(),
        Some(std::env::temp_dir().canonicalize().unwrap().as_path())
    );
    if let Err(error) = std::fs::remove_dir_all(&resolved) {
        eprintln!(
            "Synthetic WebView2 cache cleanup deferred ({error}): {}",
            resolved.display()
        );
    }
}
