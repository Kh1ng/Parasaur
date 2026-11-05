# Claimed work is not confirmed execution — 2026-11-05

The shared Kanban board previously set `working` when either an active claim or
a running controller event existed. A claim can remain while an issue is gated
or awaiting recovery, and a controller event records intent rather than an
independent dispatch-runtime observation. This could make stuck work look active.

Cards now follow the same dispatch worker roster as the navbar. A running worker
provides the current backend/model, job mode and start time. An explicitly stale
worker observation is not shown as confirmed running. Staleness does not prove
that the process stopped, and this change never releases a claim or starts work.

Claims and workers also create cards when the issue has disappeared from the
available intake and has no PR. A claim without execution evidence appears in
Waiting, with the claiming machine and timestamp in its explanation. Existing
human/review gates retain their reasons alongside the claim details. Assignment
is disabled while an existing claim or worker must be reconciled, including on
Needs you cards, so a manual fix cannot silently duplicate that work.

The alternative was to retain claim-based running status and add a warning.
Using the existing worker roster removes the conflicting source of truth and
keeps the board consistent with the navbar without another endpoint or timer.
Older servers without runtime observations conservatively show execution as
unconfirmed rather than infer it from ownership.

## Validation

On Windows:

- `npm run --workspace=apps/web test:unit -- --runTestsByPath src/lib/kanbanBoard.test.ts --coverage=false`: seven passed.
- `npm run --workspace=apps/web test:component -- tests/component/KanbanPage.spec.tsx --workers=1`: thirteen passed.
- `npm run --workspace=apps/web typecheck`: passed.
- `npm run --workspace=apps/web build`: passed.
- `git diff --check`: passed.

Regression coverage includes claim-only cards outside intake, runtime-only
cards, stale worker uncertainty, conflicting controller/process observations,
human-gate preservation, duplicate-assignment prevention, and the existing
review/merge lifecycle. One old component fixture inherited an unrelated live
worker while declaring its lifecycle scenario idle; it now explicitly supplies
an empty worker roster. This avoids asserting card order against an active job.

Desktop and 390px screenshots were inspected. The selected claim explanation
fits the narrow viewport; the existing board remains horizontally scrollable.
Screenshots are retained under `apps/web/test-results/component/` in the
`KanbanPage-a-claim-without-a67e3-dance-on-desktop-and-mobile` result directory.
These are isolated component checks, not a live factory or installed desktop run.

This public UI change is not in the earlier assembled enterprise source pair,
installed native executable, or public recovery ZIP. No private pin, installed
binary, provider session, live claim or remote service changed. Live node
recovery, failed central assets and authenticated pipeline acceptance remain open.
