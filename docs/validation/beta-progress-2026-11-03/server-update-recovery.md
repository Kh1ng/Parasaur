# Shared dashboard server-update recovery — 2026-11-05

The update banner previously restored a persisted `running` state on mount but
never restarted its status polling. Its click-handler timer also continued
requesting status after the component was unmounted. Windows Chromium checks
reproduced both failures before the repair.

`GahUpdateBanner` now owns polling in an effect tied to the running state.
Opening the dashboard resumes observation without starting another install.
Cleanup clears the timer and ignores any late in-flight response, including
failure replies that would otherwise schedule another timer. A slow startup
status read cannot overwrite an update started in the meantime. A synchronous
start guard prevents duplicate submissions while the start request is pending.

Interrupted status reads preserve the previous observation and explicitly say
the status is unavailable while reconnecting. A terminal failure remains
visible even if the release feed offers no newer version, with Settings log
guidance and an explicit retry. Success and inferred restart retain their
existing dashboard reload behavior; inferred restart is not relabeled as a
verified successful install. The banner and controls name the server being
updated. This does not install a Windows desktop update.

Two approaches were considered: repairing the click-handler recursion or
making observation follow the persisted running state. The latter covers both
new and restored updates through one lifecycle and cancels it on unmount.
No new updater service, dependency or timer abstraction was added.

## Verification

Native Windows:

- `npm run --workspace=apps/web test:component -- GahUpdateBanner.spec.tsx --workers=1`:
  six passed. Cases cover restored updates without another POST, unmount
  cleanup, interrupted reads and terminal polling shutdown, late startup
  replies, and reload after success or inferred restart.
- `npm run --workspace=apps/web typecheck`: passed.
- `npm run --workspace=apps/web build`: passed, with the existing bundle-size
  warning. The final later source change only corrected the interface comment.
- Failure-state captures at 1100 px and 390 px were inspected; content and
  controls wrap without horizontal overflow.

The component checks route local fixture responses and do not execute the
updater or restart any server. Logs are retained locally at
`artifacts/update-banner-before.log`, `artifacts/update-banner-after.log`,
`artifacts/update-banner-typecheck.log` and `artifacts/update-banner-build.log`.

This source change follows the packaged `5e261e0e` candidate and its validated
enterprise pair; it is not included in those immutable artifacts. A matched
new package, private assembly and live acceptance remain required. Signed
desktop feeds, install/restart and fresh-host acceptance also remain open.
