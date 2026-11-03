# Windows provider credential transport

Windows native provider commands previously resolved a host-side Unix CLI path
and then used an unsupported execution branch. They now invoke the selected WSL
distribution's `$HOME/.cargo/bin/gah`. Linux credential storage and account
binding remain owned by that node's existing CLI.

The desktop passes fields as separate arguments to a fixed shell script using
`"$@"`. Secrets travel through an asynchronous stdin pipe; Windows does not
create temporary secret files. Stdout is limited to 256 KiB and stderr is
discarded in favor of the existing fixed errors. Pipe writing, reading and exit
waiting share a deadline, so a child that never reads stdin cannot block the
caller indefinitely. Existing local Settings origin checks remain unchanged.

Inside WSL, coreutils `timeout --kill-after=1s` bounds the Linux process group.
The Windows transport has an outer bound three seconds longer, and at most one
second for termination/reaping. Regular credential operations retain a ten
second Linux budget; usage refresh retains 95 seconds. WSL must have the CLI,
`/usr/bin/timeout` and `/bin/sh`; failure does not fall back to another node.

Actual instance creation also exposed a separate label mismatch: credential
display names permit spaces, while execution account labels are logical IDs.
New native instances now use the credential ID as their execution account
label, keeping the human-readable name in credential metadata and the UI.
The shared CLI argument helper has one fewer parameter. No core CLI identity
validation was relaxed, and existing instances are not rewritten.

## Checks

- Windows native suite: 35 passed, two ignored fixtures. One ignored entry is
  the subprocess fixture; the real WSL test was explicitly run and passed.
- The pipe test verifies stdin secrecy, bounded output, nonzero exit, and a
  child that waits without consuming a 32 KiB stdin payload.
- The opt-in Windows-to-WSL test uses Ubuntu-24.04 and the optimized Linux CLI
  built from `bbc37aa5`. It invokes the production Windows command builder and
  pipe runner with an isolated Linux home inserted after `--exec` for testing.
  It saves/lists two OpenAI API-key records, preserves quote/metacharacter
  display text literally, creates distinct instances, rebinds one, removes one
  credential, rejects a missing-account refresh, and verifies a timed-out shell's
  child is no longer executing. Keys are synthetic; no provider request occurs.
- The shared native-argument parser contract passed under WSL against the real
  core parser. Desktop Settings browser checks passed.

Run the opt-in test from Windows with `GAH_WINDOWS_WSL_TEST_GAH` set to a Linux
CLI path and `GAH_WINDOWS_WSL_TEST_DISTRIBUTION` set to the installed distro:

```powershell
cargo test --manifest-path apps/desktop/Cargo.toml --bin gah-desktop credentials::windows::tests::real_wsl_two_accounts_and_literal_arguments -- --ignored --nocapture
```

Initial fixture failures came from the test's incorrect config environment
variable and incomplete hand-written profile, then the real label mismatch.
The test now uses `GAH_CONFIG` and creates its profile through the actual CLI.
A discarded attempt to test the configured home through WSLENV confirmed WSL
sets Linux HOME itself; the committed integration fixture injects its isolated
environment explicitly. It does not change the user's WSL home or installation.
Successful runs remove their temporary homes. Automatic approval review rejected
cleanup of four failed synthetic Linux fixtures and one failed Windows fixture
profile with only “blocked by policy”; those task-owned temporary folders remain.

This proves local API-key transport and account configuration, not provider
authentication, billed usage, agent execution through each account, browser
OAuth on Windows, signed updates or live factory deployment. Native Unix tests
were not rerun: WSL lacks the GTK build prerequisites, and MacBook work is deferred.
