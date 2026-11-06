import { quotaCheckProblem, usageWindowProblem, currentTightestWindow, formatUntil, projectedPercent, usageWindowIsCurrent, busySubscriptionIds, recordedCostLabel, subscriptionUsage } from './subscriptionUsage.js';

const candidate = (instance: string, model: string, cost: number) => ({
  backend: 'codex', backend_instance: instance, provider: 'openai', model,
  modes: ['improve'], eligible_now: true, usage: { actual_cost_usd: cost },
});
const spend = (instance: string | null, cost: number | null) => ({
  backend: 'codex', backend_instance: instance, actual_cost_usd: cost,
  estimated_cost_usd: 99, observations: 1, actual_cost_unknown_observations: cost === null ? 1 : 0,
});

test('account cost comes from recorded identity once, across repeated routing candidates', () => {
  const accounts = subscriptionUsage({ since: '7d', candidates: [candidate('work', 'model-a', 4), candidate('work', 'model-b', 4)], account_spend: [spend('work', 4)] } as never);
  expect(accounts[0].costUsd).toBe(4);
  expect(recordedCostLabel(accounts[0])).toBe('Recorded actual cost (7d): $4.00 · Estimated cost: $99.00');
});

test('same-provider accounts never inherit backend-wide candidate costs or unattributed costs', () => {
  const accounts = subscriptionUsage({ candidates: [candidate('personal', 'model-a', 12), candidate('work', 'model-a', 12)], account_spend: [spend('work', 4), spend('personal', 8), spend(null, 90)] } as never);
  expect(accounts.map(account => [account.id, account.costUsd])).toEqual([['personal', 8], ['work', 4]]);
});

test('legacy snapshots keep account cost unknown; a measured zero stays zero', () => {
  expect(subscriptionUsage({ candidates: [candidate('codex', 'model-a', 8)] } as never)[0].costUsd).toBeNull();
  const account = subscriptionUsage({ candidates: [candidate('codex', 'model-a', 8)], account_spend: [spend('codex', 0), spend(null, 90)] } as never)[0];
  expect(account.costUsd).toBe(0);
});

test('pool windows share an account but each canonical cost row contributes once', () => {
  const accounts = subscriptionUsage({ candidates: [candidate('work:external', 'a', 12), candidate('work:external', 'b', 12), candidate('work:google-native', 'c', 12)].map(row => ({ ...row, credential_id: 'work' })),
    account_spend: [spend('work:external', 2), { ...spend('work:google-native', null), estimated_cost_usd: null }] } as never);
  expect(accounts).toHaveLength(1);
  expect(accounts[0].costUsd).toBe(2);
  expect(accounts[0].costUnknownObservations).toBe(1);
  expect(recordedCostLabel(accounts[0])).toContain('Some recorded costs are unknown.');
});

test('claim ownership and a controller run cannot mark an account as working', () => {
  const subscriptions = subscriptionUsage({ candidates: [candidate('work', 'a', 0)] } as never);
  const busy = busySubscriptionIds({ subscriptions, sessions: [], controllerRuns: [{ status: 'running', work_id: '#1' }], claims: [{ work_id: '#1' }], recentLedger: { most_recent_work_id: '#1', most_recent_effective_backend: 'codex' } } as never);
  expect(busy.size).toBe(0);
});

test('session account identity wins over its provider transport and other same-provider accounts', () => {
  const subscriptions = subscriptionUsage({ candidates: [candidate('personal', 'a', 0), candidate('work', 'a', 0)] } as never);
  const session = { id: 's', providerKind: 'github', instanceId: 'personal', backend: 'codex', backendInstance: 'work', status: 'running' };
  expect([...busySubscriptionIds({ subscriptions, sessions: [session] } as never)]).toEqual(['work']);
  expect(busySubscriptionIds({ subscriptions, sessions: [{ ...session, backendInstance: undefined }] } as never).size).toBe(0);
  expect(busySubscriptionIds({ subscriptions, sessions: [{ ...session, status: 'starting' }] } as never).size).toBe(0);
});

test('another node cannot mark a local account with the same name as working', () => {
  const subscriptions = subscriptionUsage({ candidates: [candidate('work', 'a', 0)] } as never);
  const session = { id: 's', providerKind: 'github', instanceId: 'transport', backend: 'codex', backendInstance: 'work', nodeId: 'remote', status: 'running' };
  expect(busySubscriptionIds({ subscriptions, sessions: [session], nodeId: 'local' } as never).size).toBe(0);
  expect(busySubscriptionIds({ subscriptions, sessions: [session] } as never).size).toBe(0);
  expect([...busySubscriptionIds({ subscriptions, sessions: [session], nodeId: 'remote' } as never)]).toEqual(['work']);
});

test('pool identities credit their account once and ambiguous account mappings stay unconfirmed', () => {
  const subscriptions = subscriptionUsage({ candidates: [candidate('work:external', 'a', 0), candidate('work:google-native', 'b', 0)].map(row => ({ ...row, credential_id: 'work' })) } as never);
  const session = { id: 's', providerKind: 'github', instanceId: 'transport', backend: 'codex', backendInstance: 'work:external', status: 'running' };
  expect([...busySubscriptionIds({ subscriptions, sessions: [session] } as never)]).toEqual(['work']);
  const ambiguous = subscriptionUsage({ candidates: [{ ...candidate('work:external', 'a', 0), credential_id: 'one' }, { ...candidate('work:external', 'a', 0), credential_id: 'two' }] } as never);
  expect(busySubscriptionIds({ subscriptions: ambiguous, sessions: [session] } as never).size).toBe(0);
});

test('only fresh running worker observations identify factory account activity', () => {
  const subscriptions = subscriptionUsage({ candidates: [candidate('personal', 'a', 0), candidate('work', 'a', 0)] } as never);
  const now = Date.parse('2026-11-05T12:00:00Z');
  const worker = { backend: 'codex', backend_instance: 'work', state: 'running', node_id: 'local', last_activity_at: new Date(now).toISOString(), stale_after_seconds: 30 };
  const input = { subscriptions, sessions: [], runningWorkers: [worker], nodeId: 'local', now };
  expect([...busySubscriptionIds(input as never)]).toEqual(['work']);
  for (const patch of [{ state: 'stale' }, { node_id: 'remote' }, { last_activity_at: 'invalid' }, { last_activity_at: new Date(now - 31_000).toISOString() }, { backend_instance: 'missing' }]) {
    expect(busySubscriptionIds({ ...input, runningWorkers: [{ ...worker, ...patch }] } as never).size).toBe(0);
  }
});


test('expired monthly readings remain historical without claiming a reset or a new allowance', () => {
  const now = Date.parse('2026-11-05T20:00:00Z');
  const account = subscriptionUsage({ candidates: [{ ...candidate('opencode', 'mistral', 0), quota_observations: [
    { quota_window: 'monthly', quota_remaining_percent: 0, quota_reset_at: '2026-10-25T12:04:00Z', observed_at: '2026-10-24T12:00:00Z' },
    { quota_window: 'daily', quota_remaining_percent: 75, quota_reset_at: '2026-11-06T00:00:00Z', observed_at: new Date(now).toISOString() },
  ] }] } as never)[0];
  const expired = account.windows.find(window => window.label === 'Monthly')!;
  expect(expired.usedPercent).toBe(100);
  expect(usageWindowIsCurrent(expired, now)).toBe(false);
  expect(formatUntil(expired.resetAt, now)).toBeNull();
  expect(projectedPercent(expired, now)).toBeNull();
  expect(currentTightestWindow(account, now)?.usedPercent).toBe(25);
  expect(currentTightestWindow({ ...account, windows: [expired] }, now)).toBeNull();
  expect(usageWindowIsCurrent({ ...expired, resetAt: new Date(now).toISOString() }, now)).toBe(false);
  expect(usageWindowIsCurrent({ ...expired, resetAt: 'invalid' }, now)).toBe(false);
  expect(usageWindowIsCurrent({ ...expired, resetAt: null, observedAt: new Date(now).toISOString() }, now)).toBe(true);
  expect(formatUntil('2026-11-06T00:00:00Z', now)).toBe('4h 00m');
});


test('a generic OpenCode runner cannot relabel Nous quota as a Mistral allowance', () => {
  const now = '2026-11-06T05:00:00Z';
  const accounts = subscriptionUsage({ candidates: [{ ...candidate('opencode', 'tak-mistral-vibe/model', 99), backend: 'opencode', provider: 'mistral' }],
    quota_checks: [{ backend: 'opencode', backend_instance: 'opencode:nous-portal-api', provider: 'nous', quota_pool: 'nous-portal-api', status: 'data', checked_at: now,
      quota_observations: [{ backend: 'opencode', quota_window: 'monthly', quota_remaining_percent: 0, observed_at: now, quota_reset_at: '2026-11-25T12:00:00Z' }] }],
    account_spend: [{ ...spend('opencode', 4), backend: 'opencode' }],
  } as never);
  expect(accounts.map(account => [account.id, account.providerLabel, account.windows.length, account.costUsd])).toEqual([
    ['opencode', 'Mistral', 0, 4], ['opencode:nous-portal-api', 'Nous', 1, null],
  ]);
  expect([...busySubscriptionIds({ subscriptions: accounts, sessions: [{ backend: 'opencode', backendInstance: 'opencode', status: 'running' }] } as never)]).toEqual(['opencode']);
});

test('failed checks qualify only their exact pool and account while fresh sibling windows remain current', () => {
  const now = Date.parse('2026-11-06T05:00:00Z');
  const check = (instance: string, remaining: number, status: string) => ({ backend: 'agy', backend_instance: instance, provider: 'antigravity', quota_pool: instance, status, checked_at: new Date(now).toISOString(),
    quota_observations: [{ backend: 'agy', quota_window: 'weekly', quota_remaining_percent: remaining, observed_at: new Date(now - 60_000).toISOString(), quota_reset_at: '2026-11-13T00:00:00Z' }] });
  const account = subscriptionUsage({ quota_checks: [check('agy:external', 80, 'data'), check('agy:google-native', 0, 'failed')] } as never)[0];
  expect(account.id).toBe('agy');
  expect(account.windows).toHaveLength(2);
  expect(currentTightestWindow(account, now)?.usedPercent).toBe(20);
  const failed = account.windows.find(window => window.usedPercent === 100)!;
  expect(usageWindowProblem(failed, now)).toBe('Provider usage check failed.');
  expect(projectedPercent(failed, now)).toBeNull();
  const scope = { backend: 'agy', backend_instance: 'agy:external', quota_pool: 'agy:external', credential_id: 'one' };
  const observation = { backend: 'agy', observed_at: new Date(now).toISOString() };
  const failure = { ...scope, checked_at: new Date(now).toISOString(), status: 'auth_required' };
  expect(quotaCheckProblem(scope, observation, [{ ...failure, credential_id: 'two' }] as never)).toBeNull();
  expect(quotaCheckProblem(scope, observation, [{ ...failure, quota_pool: 'agy:google-native' }] as never)).toBeNull();
  expect(quotaCheckProblem(scope, observation, [failure] as never)).toBe('Provider sign-in required.');
  expect(quotaCheckProblem(scope, observation, [{ ...failure, checked_at: new Date(now - 1).toISOString() }] as never)).toBeNull();
  expect(quotaCheckProblem(scope, observation, [failure, { ...failure, status: 'data', checked_at: new Date(now + 1).toISOString() }] as never)).toBeNull();
});

test('a fresh successful check does not rejuvenate stale, undated or future observations', () => {
  const now = Date.parse('2026-11-06T05:00:00Z');
  const window = { resetAt: '2026-11-13T00:00:00Z', observedAt: new Date(now - 31 * 60_000).toISOString() };
  expect(usageWindowProblem(window, now)).toContain('older than 30 minutes');
  expect(usageWindowProblem({ ...window, observedAt: null }, now)).toBe('Observation time unavailable.');
  expect(usageWindowProblem({ ...window, observedAt: 'invalid' }, now)).toBe('Observation time unavailable.');
  expect(usageWindowProblem({ ...window, observedAt: new Date(now + 60_000).toISOString() }, now)).toContain('future');
  expect(usageWindowProblem({ ...window, observedAt: new Date(now - 30 * 60_000).toISOString() }, now)).toBeNull();
  const account = subscriptionUsage({ quota_checks: [{ backend: 'codex', backend_instance: 'work', status: 'data', checked_at: new Date(now).toISOString(), quota_observations: [
    { backend: 'codex', quota_window: 'weekly', quota_remaining_percent: 75, quota_reset_at: window.resetAt, observed_at: window.observedAt },
  ] }] } as never)[0];
  expect(account.windows[0].usedPercent).toBe(25);
  expect(currentTightestWindow(account, now)).toBeNull();
});
