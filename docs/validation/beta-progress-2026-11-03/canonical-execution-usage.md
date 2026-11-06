# Native reporting foundation for combined usage

This work is staged locally, not deployed. It advances #1538's reporting
contract; the combined HTTP endpoint and Telemetry view remain unfinished.

Public `d765484c` adds `project` and `work-item` to the report/ledger-summary
grouping vocabulary and its shared TypeScript contract. Group keys are JSON
tuples containing profile, forge/provider and repository, plus nullable work id
for work-item groups. An identical `#42` in another project or forge cannot
merge. A missing work id stays an explicit unknown bucket rather than borrowing
the target text. Existing grouped accounting is reused over borrowed entry
references, without cloning ledger payloads into each scope.

Public `e29a9eee`, corrected by `76833d3a`, adds the machine-readable source:

```sh
gah report --usage --json --profile gah --since 7d
```

The export visits validated ledger records under the existing shared read lock
and retains grouped measurements only. It does not load the entire journal in
Node or keep a second history copy. It groups by project/work identity, exact
backend/account, recorded requested/effective/actual model facts, model provider,
usage classification and UTC record date. Actual cost and API-equivalent
estimates remain separate. Token categories are independent; no sum derives
total tokens or infers cache usage.

Per-attempt usage supersedes its mirrored dispatch summary, including modern
attempts that explicitly record unavailable usage. Historical records that
never captured per-attempt usage retain their dispatch-level readings using
the same canonical observation boundary as the existing ledger reports. Those
rows identify a legacy dispatch summary and retain any known attempt count.
The legacy-summary count must not be added to the attempt count as if it were
another set of attempts.

Each numeric sum requires a valid reading from every included observation.
Missing/invalid values remain null; observed zero remains zero. Only proven
unstarted dispatches are excluded as preflight: contradictory usage, route,
review or validation evidence prevents that exclusion. Control records are not
execution usage. Invalid execution timestamps fail the source instead of
returning an empty successful report, and offsets are normalized for UTC days.
The date belongs to the dispatch record; no per-attempt timestamp is invented.

`DispatchUsageReport` defines schema version 1. The server's typed
`runDispatchUsageReport` wrapper checks that version and rejects an older CLI
response. It is the source for the forthcoming combined endpoint, not an
already-connected dashboard feature. The existing chat-only endpoint remains
unchanged by this stage.

## Evidence

- Project/work grouping passed 30 ledger-summary unit cases, twelve existing
  report cases, three real CLI report cases and 60 HTTP query/argument cases.
- The usage stage passed 123 ledger, 200 controller and 48 quota cases, the
  existing report/real-CLI cases, and 93 HTTP/CLI-wrapper cases. The final five
  streaming usage regressions and all-target/all-feature Clippy passed.
- A legacy regression was first run against the earlier implementation and
  failed on the expected lost `Some(70)` token measurement. After correction,
  it retains 70 once, two recorded attempts and one legacy dispatch summary.
- Contracts build, server typecheck and formatting checks passed. Native logs
  and the verified source are in
  `/home/khing/.cache/parasaur-project-report.qy1ih6c9/`. The earlier grouping
  run is `/home/khing/.cache/parasaur-project-report.rq76r2xm/`.

The optimized `76833d3a` candidate passed its release build, version/flag checks
and comparison of all 591 tracked Rust/Cargo input files. Its SHA-256 is
`c262aeab36e576453e36a9c82faef4e19682c0e82bb3225dcdd34ce27750c318`;
its maximum required glibc version is 2.39. It is retained under ignored
`artifacts/parasaur-canonical-usage-76833d3a/`, not installed. The earlier
`e29a9eee` artifact is superseded by the legacy correction.

Private `2b7c96e`, pinned to `76833d3a`, passes the combined private production
build: 1,136 server cases with 38 explicit skips, 16 mock cases, production
builds and private input/artifact/browser credential guards. Its log is
`/home/khing/.cache/parasaur-enterprise-current.mlc1vq_k/build.log`.

No central/worker service, installed CLI, MacBook or Hermes service was changed
in this stage. At 12:02 UTC the existing native controller PID 666974 and memory
observer PID 667504 remained live; the observer had 70 samples and zero gaps.
That is still an incomplete ten-hour observation.

Remaining #1538 acceptance includes the combined chat/dispatch endpoint and
page, same-day cross-source counting evidence, project/day/model presentation,
and export integration. Fleet-wide usage needs explicit node/source coverage;
the new native source describes its local ledger only. No issue was closed.
