import { recordedCostLabel, subscriptionUsage } from './subscriptionUsage.js';

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
