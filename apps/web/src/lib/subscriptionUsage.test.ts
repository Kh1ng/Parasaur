import { busySubscriptionIds, recordedCostLabel, subscriptionUsage } from './subscriptionUsage.js';

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
  const accounts = subscriptionUsage({ candidates: [candidate('work:external', 'a', 12), candidate('work:external', 'b', 12), candidate('work:google-native', 'c', 12)],
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
  const subscriptions = subscriptionUsage({ candidates: [candidate('work:external', 'a', 0), candidate('work:google-native', 'b', 0)] } as never);
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
