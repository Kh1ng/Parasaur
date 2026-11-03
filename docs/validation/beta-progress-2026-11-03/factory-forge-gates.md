# Recorded forge gates

Read-only GitHub inspection on 2026-11-03 found four open draft PRs authored by `Ram-Rod6198`. This is forge evidence, not the running controller's state. No PR, review, issue, assignment, or policy was changed.

| PR / issue | Current recorded gate | Evidence needed to advance |
| --- | --- | --- |
| [#1679](https://github.com/Kh1ng/Parasaur/pull/1679) / #1531 | `NEEDS_FIX`, Rust CI failed, draft. Head `3671d73a9d4bea86d3403adcf16a5e065bdf8b88`. | Repair the scenario without changing every controller ticket/retry/escalation from `fix` to `improve`; validate production routing and the failing regression, then obtain a new review. |
| [#1678](https://github.com/Kh1ng/Parasaur/pull/1678) / #1525 | `NEEDS_FIX`, checks green, draft. Head `d46ae6e1605de4ade7c8a5095dc03bb5f5ef61a4`. | Status must expose an actual central-outage refusal, rather than the same static policy in healthy and failed cases; the test must distinguish those states. |
| [#1648](https://github.com/Kh1ng/Parasaur/pull/1648) / #1606 | `HUMAN_REVIEW`, checks green, draft. Head `945bac6056e4b3bdf7205c61dc3b455d83ffb6e6`. | The runbook is partial progress. Hosted invitation, remote node, real dispatched job, events/health and recorded findings are still required. |
| [#1647](https://github.com/Kh1ng/Parasaur/pull/1647) / #1605 | `HUMAN_REVIEW`, checks green, draft. Head `15c3f2319c40a5ab7d65457b5440c4a7cf5d1c54`. | Inspect the hosted migration ledger and deployed node-health behavior, then record the live result. The doc alone proves no hosted change. |

The recorded blocking findings are in [#1679's review comment](https://github.com/Kh1ng/Parasaur/pull/1679#issuecomment-6513391124) and [#1678's review comment](https://github.com/Kh1ng/Parasaur/pull/1678#issuecomment-6507074925). The human evidence requirements are in [#1648's review comment](https://github.com/Kh1ng/Parasaur/pull/1648#issuecomment-6503604348) and [#1647's review comment](https://github.com/Kh1ng/Parasaur/pull/1647#issuecomment-6503587249). GitHub's formal review arrays and review-decision fields were empty: these verdicts were posted as issue comments by the factory, so formal review status alone would miss them.

The finishing branch retains controller `fix` mode and does not include the draft's controller-wide mode change. Its local enterprise tests now cover real disposable authentication/relay and a two-account browser invitation/removal flow. Those local results do not close either hosted acceptance issue.

The finishing branch now also records actual central claim admission results in the existing event stream and projects the latest matching attempt into CLI/HTTP status and Factory. A real worker CLI regression distinguishes acquired admission from an outage refusal and confirms no backend/local claim on refusal. This addresses the observability gap independently; it does not change #1678's draft head or establish that Michael is running the change.

To explain why work has not advanced, inspect Michael's current controller receipt, repair retry budget, selected route, claim, and latest decision for these exact heads. Without those runtime facts, a review finding is evidence of required work, not proof that a review gate is malfunctioning. The UI now surfaces available blocker/receipt/decision evidence; it still needs a real Parasaur endpoint for this check.
