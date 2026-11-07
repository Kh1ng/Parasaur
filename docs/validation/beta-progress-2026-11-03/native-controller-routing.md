# Central controller native dispatch

The controller now prepares remote work through the central API instead of
requiring the selected provider executable on the controller. Each worker exposes
an authenticated account catalog containing readiness and supported models, with
no credential values or executable paths. Central applies its ordinary routing
rules, exact account selection, history, approvals and budgets to that catalog.
Unknown authentication readiness remains unknown.

Preparation reserves canonical work ownership without staging or starting the
worker. Only after receiving that durable reservation does the controller release
its own local claim and activate the original run. A lost reservation reply keeps
the local claim; a lost activation reply retains the remote identity and reports
unconfirmed activation. Neither case authorizes a local backend fallback.

A private per-work index recovers a completed preparation after a lost reply
without another history copy. Partial preparations remain inspectable and refuse
replacement. Preparation is serialized to bound transient history memory.
Scheduling includes native claims in intake counts and excludes their work from
the selectable queue. Fresh start authority uses that same canonical claim scope,
excluding the run being authorized so its own reservation cannot block it.

Focused verification includes a real local HTTP controller test for successful
claim transfer, dropped reservation and activation replies, and a wrong-work
reservation. The server suite covers dormant reservations across reload, explicit
activation, worker selection, account metadata boundaries, partial preparation and
lost-reply recovery. These checks use controlled credentials and do not dispatch
production issues or prove a production controller-to-worker run.

Verification: 55 native handoff tests, 577 dispatch tests (including the three
controller native tests), 76 provider tests, three CLI handoff tests and 12
capability tests pass under Linux/WSL. Formatting and all-target/all-feature
Clippy with warnings denied pass. Contract/server typechecks and all 60 focused
server handoff, producer, transport and context tests pass. The shared review
account-pin rule also has a regression test for backend/model substitution.

Deployment remains pending. Provider-assignee claim renewal, remote PM artifact
publication and uncertain-start reconciliation without a terminal ledger receipt
remain incomplete. Existing worker memory/capacity guards are unchanged. Real
end-to-end acceptance must use the matched newly built API and Linux CLI before
installing this controller path.
