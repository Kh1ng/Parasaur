import type { AvailableTicket, ControllerActivity, IssueIntakeRejection, RunningWorker, StatusSnapshot } from '@git-agent-harness/contracts';
import { buildKanban, managedIssues } from './kanbanBoard.js';

const rejection = (work_id: string | null, reason_code: string, reason: string): IssueIntakeRejection => ({
  ticket_path: work_id ?? 'issue', work_id, title: `Issue ${work_id}`, provider: 'github', author_login: 'owner', author_kind: 'human', reason_code, reason, labels: []
});
const ticket = (work_id: string, human_required_reason_code: string | null = null): AvailableTicket =>
  ({ work_id, ticket_path: work_id, title: `Ticket ${work_id}`, prior_attempt_count: 0, human_required_reason_code }) as unknown as AvailableTicket;
const status = (overrides: Partial<StatusSnapshot>): StatusSnapshot =>
  ({ profile: { max_fix_attempts_per_mr: 2, max_open_managed_mrs: 4 }, max_parallel_workers: 2, open_managed_mr_count: 0, implementation_intake_paused: false, blockers: [], available_tickets: [], merge_requests: [], active_claims: [], blocked_work_items: [], dependency_blockers: [], review_held_work_ids: [], running_workers: [], issue_intake_rejections: [], ...overrides }) as unknown as StatusSnapshot;
const board = (snapshot: StatusSnapshot) => buildKanban({ status: snapshot, quota: null, controllerRuns: [], factoryAgents: [], loopRunning: true, now: Date.now() });

const claim = { work_id: '#7', pid: 42, scope: 'gah', hostname: 'michaels-machine', claimed_at: '2026-11-05T10:00:00Z', age_seconds: 3600 };
const worker: RunningWorker = { work_id: '#7', run_id: 'run-7', mode: 'fix', backend: 'codex', runner: 'codex', backend_instance: 'codex:work', requested_model: 'configured-model', model: 'routed-model', actual_model: null, node_id: 'remote-node', branch: 'gah/7', started_at: '2026-11-05T10:01:00Z', attempt: 1, last_activity_at: '2026-11-05T10:02:00Z', stale_after_seconds: 900, state: 'running' };

test('a claim stays visible without intake and does not imply execution or permit duplicate assignment', () => {
  const card = board(status({ active_claims: [claim] })).cards[0];
  expect(card).toMatchObject({ workId: '#7', working: false, column: 'waiting', job: null, since: claim.claimed_at });
  expect(card.reason).toBe('Claimed by michaels-machine; execution is not confirmed');
  expect(card.blocks.join(' ')).toContain(claim.claimed_at);
  expect(card.assignHeldBy).toContain('already has a claim');
});

test('worker runtime evidence governs running cards and stale observations preserve uncertainty', () => {
  const snapshot = status({ active_claims: [claim], running_workers: [worker] });
  expect(board(snapshot).cards[0]).toMatchObject({ working: true, column: 'fixing', agent: 'Codex · routed-model', agentRole: 'working', since: worker.started_at });
  const stale = board({ ...snapshot, running_workers: [{ ...worker, state: 'stale' }] }).cards[0];
  expect(stale).toMatchObject({ working: false, column: 'waiting', agentRole: null });
  expect(stale.reason).toContain('stale');
  expect(stale.blocks.join(' ')).toContain('does not prove the process stopped');
  expect(stale.assignHeldBy).not.toBeNull();
  expect(board(status({ running_workers: [worker] })).cards[0]).toMatchObject({ workId: '#7', working: true });
});

test('retained worker and capacity records are not live evidence after a failed read', () => {
  const input = { status: status({ available_tickets: [ticket('#8')], running_workers: [worker] }), quota: null, controllerRuns: [], factoryAgents: [], loopRunning: true, now: Date.now() };
  const historical = buildKanban({ ...input, statusUnavailable: true });
  expect(historical.cards.find(card => card.workId === '#7')).toMatchObject({ working: false, column: 'waiting', reason: 'Factory state could not refresh; execution is not confirmed' });
  expect(historical.cards.find(card => card.workId === '#8')?.reason).toContain('Last recorded as ready');
  expect(historical.cards.every(card => card.assignHeldBy !== null)).toBe(true);
  expect(historical.gates.every(gate => gate.ok === null)).toBe(true);
  expect(buildKanban(input).cards.find(card => card.workId === '#7')?.working).toBe(true);
});

test('controller intent and a matching local tool do not override an empty worker roster', () => {
  const run: ControllerActivity = { run_id: 'run-7', profile: 'gah', work_id: '#7', started_at: worker.started_at, finished_at: null, action: 'fix_existing: gah/7', status: 'running', outcome: null };
  const result = buildKanban({ status: status({ active_claims: [claim] }), quota: null, controllerRuns: [run], factoryAgents: [{ pid: 99, tool: 'codex', cwd: '/worktrees/gah-7', started_at: worker.started_at }], loopRunning: true, now: Date.now() });
  expect(result.cards[0]).toMatchObject({ working: false, column: 'waiting' });
});

test('a claimed human gate keeps its explanation and cannot be manually assigned', () => {
  const card = board(status({ available_tickets: [{ ...ticket('#7'), human_required: true }], active_claims: [claim], blocked_work_items: [{ kind: 'human_required', source_reference: '#7', reason_code: 'stuck_loop_gate', message: 'Review step repeated without progress' }] })).cards[0];
  expect(card).toMatchObject({ working: false, column: 'needs_you' });
  expect(card.blocks).toContain('Review step repeated without progress');
  expect(card.blocks.join(' ')).toContain('michaels-machine');
  expect(card.assignHeldBy).toContain('already has a claim');
});

test('an unchanged reviewed head waits for a push instead of claiming a reviewer is next', () => {
  const review = {
    work_id: '#7', branch: 'gah/7', id: '42', classification: 'NEEDS_REVIEW', merged: false,
    source_sha: 'head-one', last_reviewed_head: 'head-one', ci_passed: true,
  } as StatusSnapshot['merge_requests'][number];
  const result = board(status({ merge_requests: [review] })).cards[0];
  expect(result.column).toBe('waiting');
  expect(result.job).toBeNull();
  expect(result.reason).toContain('paused until a new commit is pushed');
  expect(board(status({ merge_requests: [{ ...review, source_sha: 'head-two' }] })).cards[0].column).toBe('review');
});

test('the latest capacity refusal explains waiting work without overriding execution or human gates', () => {
  const review = { work_id: '#7', branch: 'gah/7', classification: 'NEEDS_REVIEW', merged: false } as StatusSnapshot['merge_requests'][number];
  const run: ControllerActivity = { run_id: 'deferred', profile: 'gah', work_id: '#7', action: 'review: gah/7', started_at: '2026-11-05T10:01:00Z', finished_at: '2026-11-05T10:02:00Z', status: 'failed', outcome: 'review: deferred_capacity: node admission deferred: 2527 MiB available; 2048 MiB safety floor route_state=abc' };
  const input = { status: status({ merge_requests: [review] }), quota: null, controllerRuns: [run], factoryAgents: [], loopRunning: true, now: Date.now() };
  const card = buildKanban(input).cards[0];
  expect(card).toMatchObject({ working: false, column: 'waiting', job: 'review', reason: 'Last attempt waited for node capacity' });
  expect(card.blocks.join(' ')).toContain('Last attempt at 2026-11-05T10:02:00Z: node admission deferred: 2527 MiB available');
  expect(card.blocks.join(' ')).toContain('worker reservations');
  expect(card.blocks.join(' ')).not.toContain('route_state');
  expect(buildKanban({ ...input, status: status({ merge_requests: [review], running_workers: [worker] }) }).cards[0].working).toBe(true);
  expect(buildKanban({ ...input, status: status({ merge_requests: [review], blocked_work_items: [{ kind: 'human_required', source_reference: '#7', reason_code: 'stuck_loop_gate' }] }) }).cards[0].column).toBe('needs_you');
  const recovered = { ...run, run_id: 'recovered', started_at: '2026-11-05T10:03:00Z', finished_at: '2026-11-05T10:04:00Z', status: 'finished' as const, outcome: 'review: success' };
  expect(buildKanban({ ...input, controllerRuns: [run, recovered] }).cards[0].reason).toBe('Waiting for a reviewer');
  const retrying = { ...recovered, run_id: 'retrying', finished_at: null, status: 'running' as const, outcome: null };
  expect(buildKanban({ ...input, controllerRuns: [run, retrying] }).cards[0].reason).toBe('Waiting for a reviewer');
});

test('managedIssues keys only the managed rejections by work key', () => {
  const managed = managedIssues(status({ issue_intake_rejections: [
    rejection('#7', 'managed', 'managed: assigned to colton, who is not this loop'),
    rejection('#8', 'untrusted_author', 'author is not trusted by this profile'),
    rejection(null, 'managed', 'managed: the managed label is on it')
  ] }));
  expect([...managed]).toEqual([['#7', 'Managed: assigned to colton, who is not this loop']]);
  expect(managedIssues(null).size).toBe(0);
});

test('a managed issue is marked on its card, turns Assign off and is flagged among the not-picked-up', () => {
  const result = board(status({
    available_tickets: [ticket('#7', 'stuck_loop_gate'), ticket('#9')],
    issue_intake_rejections: [rejection('TICKET-0007', 'managed', 'managed: the managed label is on it, so a manager owns this issue'), rejection('#8', 'blocked', 'blocked label present')]
  }));
  const seven = result.cards.find((card) => card.workId === '#7');
  const nine = result.cards.find((card) => card.workId === '#9');
  expect(seven?.managed).toBe('Managed: the managed label is on it, so a manager owns this issue');
  expect(seven?.assignHeldBy).toMatch(/^Assign is off: a manager owns this issue/);
  expect(seven?.blocks).toContain('Managed: the managed label is on it, so a manager owns this issue');
  expect(nine?.managed).toBeNull();
  expect(nine?.assignHeldBy).toBeNull();
  expect(result.notPickedUp.map((item) => [item.workId, item.managed])).toEqual([['TICKET-0007', true], ['#8', false]]);
});
