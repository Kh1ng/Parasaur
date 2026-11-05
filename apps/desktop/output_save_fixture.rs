//! Opt-in hidden WebView2 acceptance. The fixture uses an isolated browser
//! profile and origin-private files; it never opens a picker or a user file.
use std::{sync::mpsc, time::Duration};

#[test]
#[ignore = "hidden native fixture; requires GAH_WINDOWS_OUTPUT_FIXTURE_URL"]
fn native_output_stream_preserves_files_on_failure_and_cancel() {
    let url: tauri::Url = std::env::var("GAH_WINDOWS_OUTPUT_FIXTURE_URL")
        .expect("start the synthetic output server first")
        .parse()
        .unwrap();
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert!(url.port().is_some());
    assert!(url.username().is_empty() && url.password().is_none());
    assert_eq!(url.path(), "/native-output-fixture.html");
    let origin = url.origin();
    let root = std::env::temp_dir().join(format!(
        "parasaur-output-webview-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    println!("Fixture profile root: {}", root.display());
    let profile = root.join("profile");
    let (completed, result) = mpsc::channel();
    let app = tauri::Builder::default()
        .any_thread()
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::WebviewWindowBuilder::new(
                app,
                "output-save-fixture",
                tauri::WebviewUrl::External(url),
            )
            .title("Parasaur hidden output fixture")
            .visible(false)
            .data_directory(profile)
            .on_navigation(move |url| {
                if url.scheme() == "gah-output-fixture" {
                    let ok = url.host_str() == Some("complete")
                        && url
                            .query_pairs()
                            .any(|(key, value)| key == "ok" && value == "1");
                    let _ = completed.send(ok);
                    handle.exit(if ok { 0 } else { 1 });
                    return false;
                }
                url.origin() == origin
            })
            .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap();
    let watchdog = app.handle().clone();
    let (stop, deadline) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if deadline.recv_timeout(Duration::from_secs(60)).is_err() {
            watchdog.exit(1);
        }
    });
    let exit = app.run_return(|_, _| {});
    let _ = stop.send(());
    watchdog.join().unwrap();
    let ok = result.recv_timeout(Duration::from_secs(1)).unwrap_or(false);
    // Delete only the unique test directory created directly below Temp.
    let resolved = root.canonicalize().unwrap();
    assert_eq!(
        resolved.parent(),
        Some(std::env::temp_dir().canonicalize().unwrap().as_path())
    );
    if let Err(error) = std::fs::remove_dir_all(&resolved) {
        eprintln!(
            "Fixture profile cleanup deferred ({error}): {}",
            resolved.display()
        );
    }
    assert_eq!(exit, 0, "hidden output fixture failed or timed out");
    assert!(ok, "see the fixture server's native-output-result.json");
}
