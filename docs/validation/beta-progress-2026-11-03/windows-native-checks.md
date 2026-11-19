# Windows native checks and remaining transport gap

The earlier concurrent-window anomaly was investigated with a fresh, explicit
`WEBVIEW2_USER_DATA_FOLDER` for each fixture. Process command lines confirmed
the browser used that owned folder. Both the installed `b280a5e2` binary and a
debug candidate displayed the populated, indefinitely loading fixture correctly.
This does not establish a production startup bug or prove its root cause.
The speculative initial-navigation change was discarded. Future native fixture
checks must isolate WebView2 storage as well as Parasaur's `USERPROFILE`.

Code inspection identified a separate concrete responsiveness problem:
`repository_tools` was a synchronous Tauri command executing two WSL subprocess
probes. A slow WSL startup or shell profile therefore ran on the native command
thread. It now validates the local origin first and runs its probes through
Tauri's blocking worker pool, as credential reads already do. Replies and
installation detection are unchanged. This does not add a process timeout;
a stalled probe may still leave its own status check pending.

All 34 native Windows tests and the Settings browser script passed after this
change. Those checks do not simulate a real hung WSL service or prove native
input responsiveness under that condition. This source change has not yet
replaced the installed `b280a5e2` app. Fixture apps and the loopback server were
stopped; the installed user app remains open.

The same inspection found an important unfinished feature: native credential
management resolves `~/.cargo/bin/gah` on the host, while non-Unix credential
execution returns failure. Windows needs to invoke the selected WSL node's CLI
with private stdin, bounded execution, preserved account identity and fixed
errors. UI tests and Unix credential tests do not establish that Windows flow.
This remains required before accepting multiple provider accounts on Windows.
