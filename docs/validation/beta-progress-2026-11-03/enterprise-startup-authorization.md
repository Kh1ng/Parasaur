# Controller startup authorization repair — 2026-11-04

Verified private `5046d2c0271198347d8afcc440648ab5d5c235ca` with public core
`f7294e34f5684a9b4f7ac2cee43234062b98b24d`. This corrects the isolation boundary
before enabling automatic compute execution. No deployed worker or app was
using this private compute library; node startup still does not enable it.

## Reproduced failures

Earlier checks covered explicit cancellation and controller death after the job
started. New crash probes killed the controller immediately after launcher
creation and after namespaces were ready while authorization was still pending.

Both failed against the previous implementation. During early startup an owned
scope could remain active. At the namespace-ready boundary, the unauthorized
payload actually wrote its execution marker. Bubblewrap's blocking descriptor
unblocks on EOF as well as data, so closing the owner's descriptors could release
the program without affirmative authorization. A descriptor wait was not a
sufficient authorization gate.

## Repair

A host Python guard registers a kernel parent-death signal before systemd-run
starts. It checks the expected parent both before and after registration, so an
owner that has already died cannot leave an unbound launcher. It refuses set-ID
and file-capability executables that would clear that tracking. These semantics
are documented in the [Linux parent-death signal manual](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html).

Inside the prepared read-only runtime, a trusted Python guard requires an
explicit approval byte on private stdin before executing the requested job.
EOF exits with failure. Isolated imports and disabled site initialization keep
repository Python files and environment configuration out of that gate. After
approval, stdin becomes `/dev/null`, preserving the previous job input behavior.
The host requires Python 3 with ctypes; the image requires trusted Python 3.12
and its library. Missing prerequisites fail without host execution fallback.

The original namespace, cgroup, resource, socket and cleanup checks remain.
Claim revalidation still occurs after namespace setup and before approval.

## Final revision checks

- Official combined build: 1,027 server passes, 14 explicit skips, zero failures;
  13 mock passes; server/web typechecks and production builds; artifact/input
  guards and transformed transport checks.
- Dedicated real WSL suite: 12 passes, with only the unsupported-native-host
  check skipped. Both new crash probes now pass, alongside authorized execution,
  denial, late approval, cancellation, deadline, memory/process ceilings, socket
  isolation, detached descendant and running-controller-death checks.
- Supplemental startup crashes at 0, 2, 5, 10, 20, 40 and 80 ms after launcher
  creation all left no unauthorized marker and an inactive scope.
- Exact-revision SQL replay and real Auth/PostgREST-to-inference-worker checks
  passed with an actual sandboxed client. Repeated receipt reads retained one
  fixture-provider attempt. Active-provider revocation, relay identity, HTTP and
  WebSocket behavior, entitlement and removal checks passed again.

The clean verification source is
`/home/khing/.cache/parasaur-enterprise-current._ygwl569/source`. Its sibling
`build.log`, `isolation-verification.log`, `auth-verification.log` and
`startup-crash-matrix.json` retain the results. The release manifest identifies
the exact revisions above. Provider credentials and devices were disposable
fixtures; no actual subscription or remote machine was used.

## Still required

The automatic compute consumer, durable execution journal, existing host capacity
integration, production runtime/client, forge access, aliases/skills and ledger
identity remain implementation work. A paused but living controller also needs
an independent cloud-lease deadline; a parent-death signal addresses death, not
a stopped event loop. Capacity must remain reserved until execution cleanup is
verified, and uncertain start RPCs must not become automatic reruns.

Bounded workspace storage, simultaneous local/cloud admission, staged migration,
hosted rollout and real provider/node acceptance remain open. Installed desktop,
CLI and public recovery ZIP bytes were not changed. Mac work remains deferred.
