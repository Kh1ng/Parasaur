# Persistent GitLab panel approval

Source `acf34437` changes the owner-approved panel handoff from returning the
shared coordinator token to issuing an ordinary paired controller cookie.
The panel receives only device metadata. The cookie is HttpOnly, Secure over
TLS, SameSite=Strict, and expires after 30 days; the existing device store
persists only its hash. The owner can revoke it through paired devices.
The panel clears any old tab-scoped coordinator credential after pairing.

The one-use approval remains bound to the initiating verifier, exact origin,
owner approval and a 60-second lifetime. Owner-token rotation invalidates an
unclaimed approval. The paired panel cannot perform owner-only administration.
No GitLab cookie is forwarded to Parasaur, and the change does not implement
enterprise GitLab user attribution or GitLab SSO.

Validation:

- Seven browser end-to-end tests pass, including the real handoff router,
  a panel reload, another tab, an HttpOnly persistent cookie and no owner
  token in the panel's session/local storage.
- All four focused pairing/handoff HTTP tests pass in the public Linux
  checkout. They cover transport/origin checks, verifier theft, one claim,
  durable reload, controller authority, revocation and expiry.
- On Windows, the new handoff, prefix and trusted-device tests pass. The
  broader existing pairing test fails at its push-subscription assertion
  (503 versus 201); it passes in the Linux public checkout. This platform
  difference was not hidden by weakening its assertion.
- Server/web typechecks pass. Private assembly `9a357ff`, pinned to
  `acf34437`, passes 1,106 server tests with 38 explicit skips, 16 mock
  tests, production build/typechecks, private packaging/input checks and
  cloud transport checks. Its Supabase build values are validation fixtures;
  this does not establish live SSO.
- The public package passes the actual optimized release installer in an
  isolated home/checkout and cold HTTP asset checks, including inherited
  Windows npm selection, unchanged checkout manifests and exact binaries.

The VM candidate at `~/.local/share/gah/releases/acf34437/runtime-ready`
also passes import and compiled HTTP handoff/session checks: one claim,
controller authority, no owner token response, durable device-store reload,
owner-administration denial and revocation. Node ABI 127 matches. This is an
isolated fixture probe, not a production sign-in. `STAGE_CHECK.json` records
the staged runtime; the local `PANEL_SESSION_CHECK.json` captures the probe.
An initial root dependency install missed the bundle's managed runtime; its
replacement npm install used over 1 GiB RAM, was stopped, and is not a pass.
Dependencies were instead prepared by the verified installer in WSL, then
transferred and checked on the VM. The live API was never replaced during
staging. The incomplete VM runtime was removed after verifying that no
process used it and that all ready-runtime links stay inside the ready tree.
The ready candidate and evidence were preserved. No package was transferred to the low-space MacBook.

Deployment is pending. The public candidate is
`artifacts/parasaur-central-acf34437/`; the live central API has not received
this change. Keep the completed memory observation separate from a service
rollout and verify no active chat/dispatch would be interrupted first.

The same-origin GitLab `/parasaur/` installation requires cookie forwarding
as described below to support this flow.
Cross-origin extension installs still depend on browser third-party cookie
policy; do not promise persistence when the browser blocks those cookies.
A real signed-in GitLab-to-Parasaur acceptance run remains required.


## GitLab proxy prerequisite

The companion repository's current Omnibus snippet (base blob
`d4fd79ce9e5d2d1ff44b7147649f67ba1e9045a1`) sets `Cookie` to an empty value.
That default would discard both the controller session and browser-bound
pairing-request cookie. The installed GitLab config has not been inspected;
its management login is not available yet. The central's configured GitLab
origin is `https://gitlab.coltonspurgin.tech`, and its running Caddy gate uses
GitLab upstream `192.168.5.150:80`. The gate/API/Hermes services were not changed.

[Cookie forwarding patch](gitlab-cookie-forwarding.patch) selectively forwards
`gah_device` and `gah_pairing_request`, preserves empty invalid credentials,
omits absent credentials, rejects duplicate credential names and continues to
send the GitLab session only as the gate header. Ten isolated native-nginx
transport cases and syntax validation pass with Debian nginx 1.26.3. These
are fixture-cookie transport tests, not a real signed-in GitLab run. Docker
validation initially timed out before creating a container; the native package
was extracted into an owned test directory instead. The fixture listeners were
stopped after testing; no system nginx was installed or started.

The auth API rollout remains pending a check/update of that proxy. Live web
`99f8cca1` accepts both the older tab-only response and the new device-cookie
response, allowing the quota correction to deploy independently. Existing
sign-in behavior is preserved until the staged API is rolled out. No owner
credential is newly persisted beyond its existing tab-scoped storage.

The live API has since received the notification-only backport described in
[capacity notifications](capacity-notifications.md). The staged `acf34437`
runtime predates that fix: rebuild from current public source and repeat the
cookie/runtime checks before deploying persistent auth, so it cannot regress
the notification correction. The actual GitLab proxy prerequisite remains.
