# Beta scope audit, 2026-11-04

This maps the owner's requested end state to current evidence. It is not a beta
sign-off. Desktop/shared web lead the work; MacBook execution remains deferred.
Earlier broad checks establish their recorded revisions, not every later edit.

| Requested capability | Evidence inspected | Remaining requirement |
| --- | --- | --- |
| Factory claim → implementation → validation → review/repair → publication | Current controller/claim projections; [forge gates](factory-forge-gates.md); 21 real-CLI fixture checks in [merged factory checks](merged-factory-checks.md) | Authenticate to the real central and inspect Michael's current receipts, decisions and jobs; complete one real lifecycle without bypassing gates. |
| Explain why queued work is stuck | Shared Factory blocker/receipt/claim UI and CLI admission tests; [broad web sweep](shared-web-workflows.md) | The live host is v0.2.0 and its assets fail; no evidence proves it runs the improved projections. |
| Keep unrelated work moving; recover stalled workers | Refill, pressure, shutdown, claim recovery and isolation regressions | Hosted CI's intermittent hang #1662 is unresolved; local timing does not meet its hosted target. |
| Multiple accounts of one provider | [Windows account execution](windows-account-execution.md), private source storage and named adapter resolution | Real owner-driven provider consent, independent account execution and revocation still need acceptance. Synthetic keys prove isolation, not subscription eligibility. |
| Select exact account and model | Session/full catalog binding and [exact factory node/account/model routing](factory-account-routing.md), including real CLI executable/state/ledger retry checks | Local fixtures now cover chat and factory binding. Deploy the matched central/worker/CLI versions and verify real account-advertised models, independent execution and billing. |
| Configurable providers/models | Backend-instance/config UI, account bindings, dynamic adapter model APIs; [model recovery](model-browser-recovery.md) | Per-provider current catalogs require the actual installed CLI/account. Configured-but-unimplemented providers remain explicitly unavailable. |
| Usage, limits and spend | Quota store, ledger attribution, `routing/decision.rs` weekly known-estimated/actual cost limits, issue budgets and local usage UI checks | Reconcile measured usage with actual provider billing; do not infer a comprehensive profile/chat budget guarantee from quota, concurrency or retry caps. |
| Unknown data stays unknown | Quota freshness and source identity tests; measured-vs-estimated UI and ledger contracts | Live account readings and reset behavior still require observation. |
| Windows native app, reconnect and recovery | Installed desktop `c1a55cf0`, native recovery tests, saved settings preservation | Current installed app remains closed. The real dashboard cannot render until its asset failure is repaired; owner rendering/sign-in remain unverified. |
| Shared UI usability | Broad recorded Windows workflows/components; newer focused model recovery/account tests | No broad visual redesign is claimed. Current full UI needs acceptance against a working real central, not only fixtures. |
| Workspace/environment configuration | Profile/environment/check editors and real-CLI local child-process evidence in the progress index | Other execution nodes own their configuration. A real remote-node/provider workspace workflow is missing. |
| Git with dirty-work preservation | Review-first commit/PR UI and recorded partial-selection/worktree checks | Fixture API clicks alone do not prove live dirty-checkout preservation or real forge publication. Complete the real workflow under the intended policy. |
| Installation and updates | Installed Windows app and user WSL CLI; [matched public release/install checks](current-public-release.md); checksummed central recovery package at `16e9ecc6` | Signed update feed/key is not configured (`tauri.conf.json` pubkey is empty). Fresh-OS and signed install/restart acceptance remain missing. Linux candidate needs glibc 2.39 and is not deployed. The existing WSL worker still uses its separate earlier release. |
| Live dashboard and pipeline access | [Live endpoint evidence](live-central-8443.md); port 8443 identified; public HTML/health succeeds, asset GETs fail, protected API rejects unauthenticated reads | Windows SSH key was denied and the private owner-token card timed out. No live service repair or deployment has occurred. |
| Enterprise/T3connect-style workspace access | [Combined build and earlier two-account checks](enterprise-current-pair.md), durable receipts, the [inference-side consumer](enterprise-inference-consumer.md), [sandboxed inference flow](enterprise-inference-relay.md) and [startup crash repair](enterprise-startup-authorization.md); [shared capacity/native supervision](enterprise-host-supervision.md); [durable compute start/result delivery](enterprise-compute-recovery.md); [bounded workspace/archive](enterprise-bounded-workspace.md); [opt-in queued compute and archive admission](enterprise-compute-queue.md); private `afd4cee4` paired with public core `8963ebb5` | Implement post-crash scope/slot reconciliation, journal/archive maintenance and review controls, production runtime/client and forge access, portable workspace provider aliases, execution identity/ledger integration, safe artifact review/publication, full simultaneous shared-capacity acceptance and staged config migration; apply/verify hosted configuration, connect real nodes and complete a provider-backed job. Logical local devices and a fixture HTTP provider do not prove two-machine or subscription acceptance. |
| iOS notifications and Live Activities | Tracked APNs server/tests, native attributes and widget sources, existing browser bridge checks | APNs credentials, signed iOS build and locked physical-device delivery/update/end/deep-link/revocation are unverified. This cannot be completed with the current Windows-only execution scope. |
| Suspected Mac memory leak | [Confirmed server retention fix and heap regression](memory-retention-1680.md); issue #1680 keeps the Mac report unconfirmed | Identify the actual Mac process and compare idle/normal-workflow memory samples after deployment. Mac RSS/physical footprint has not been measured. |

## Named-account catalog repair found during this audit

`ManagerChatPage` already requested its session models with the selected named
account, but constructed the full catalog source using only profile/node.
`ModelBrowser` therefore asked the default adapter for the same runner's models.
It now carries the session's runner/instance/label, scopes only that runner's
request to the named account, and labels its catalog choices. The existing
session PATCH retains the binding when selecting a model on the same runner;
switching runners still uses the existing explicit binding-clear behavior.

The stateful mock now retains optional instance identity on session creation
and update instead of dropping it, including source-seeded chats. Seven picker
component tests passed, including account/node forwarding and non-forwarding to
other runners. Fourteen surrounding chat/browser workflows passed, including
the real page's named-session catalog read and subsequent persisted selection.
All twelve mock-control-plane tests, web/mock typechecking and the web build
passed. All provider responses were fixtures; no real provider was contacted.

The repair is local and undeployed. The immutable recovery ZIP at `d86a59b4`
predates it and must not be described as containing this later UI change.

The audit found a concrete local defect and resolved it. It did not prove the
full objective complete or establish that all remaining work is external.
