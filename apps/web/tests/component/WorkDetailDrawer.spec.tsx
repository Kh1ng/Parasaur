import { readFileSync } from 'node:fs';
import React from 'react';
import { expect, test } from '@playwright/experimental-ct-react';
import type { LedgerEntry, StatusSnapshot } from '@git-agent-harness/contracts';
import { WorkDetailDrawer } from '../../src/components/WorkDetailDrawer.js';
import { MockStoreProvider } from '../../src/test-utils/MockStoreProvider.js';
import { WebSocketProvider } from '../../src/ws/WebSocketContext.js';

const baseStatus: StatusSnapshot = JSON.parse(readFileSync(
  new URL('../../../server/tests/fixtures/gah/responses/status.json', import.meta.url), 'utf8',
));

const entry = {
  timestamp: '2026-09-26T12:00:00Z', profile: 'fixture', display_name: 'Drawer work', repo_id: 'repo', repo: 'Kh1ng/git-agent-harness', local_path: '/tmp/repo', provider: 'github',
  backend: 'codex', requested_backend: 'codex', effective_backend: 'codex', requested_model: 'gpt-5', effective_model: 'gpt-5', routing_reason: null, fallback_used: false,
  confidence_impact: null, human_required: false, mode: 'fix', target_summary: 'Drawer work', work_id: '#42', work_title: 'Drawer work', branch: 'feat/42', session_id: 'session-42',
  session_dir: '/tmp/session', duration_seconds: 12, backend_exit_code: 0, validation_result: 'passed', commit_attempted: true, commit_created: true, push_attempted: true,
  push_succeeded: true, mr_attempted: true, mr_created: true, mr_url: 'https://github.com/example/repo/pull/42', files_changed: 1, insertions: 2, deletions: 0,
  error_summary: null, dispatch_reason: 'initial', attempts: [], usage: { usage_source: 'backend_reported', input_tokens: 10, output_tokens: 5, cache_read_tokens: 0, cache_write_tokens: 0,
    total_tokens: 15, requests_count: 1, estimated_cost_usd: 0.01, actual_cost_usd: null,
    provider: 'openai', actual_model: 'gpt-5', actual_model_unknown_reason: null, provider_unknown_reason: null, account_label: null, auth_source_label: null,
    quota_pool: null, pricing_source: null, pricing_version: null, cost_unknown_reason: null, observed_at: null },
} as LedgerEntry;

function status(held = false): StatusSnapshot {
  return {
    ...baseStatus,
    review_held_work_ids: held ? ['#42'] : [],
    available_tickets: [{
      ticket_path: '42', normalized_work_identity: '#42', work_id: '#42', title: 'Drawer work', source: 'provider_issue', execution_policy: { dispatchable_now: true, reasons: [] },
      recommended_backend: 'codex', recommended_model: 'gpt-5', prior_attempt_count: 1, genuine_agent_failure_count: 0, last_failure_class: null,
      has_active_mr: true, has_active_claim: false, human_required: false,
    }],
    merge_requests: [{
      branch: 'feat/42', work_id: '#42', id: '42', url: 'https://github.com/example/repo/pull/42', title: 'Drawer work', state: 'open', draft: false,
      merge_status: 'mergeable', merged: false, ci_passed: true, ci_pending: false, review_contract_version: 1, classification: 'NEEDS_REVIEW', recommended_action: 'RUN_REVIEW',
    }],
  };
}

async function routes(page: import('@playwright/test').Page, snapshot = status()) {
  await page.route('**/api/events?*', route => route.fulfill({ json: [] }));
  await page.route('**/api/status?*', route => route.fulfill({ json: snapshot }));
  await page.route('**/api/work/**', (route) => route.fulfill({ json: [entry] }));
  await page.route('**/api/profiles', (route) => route.fulfill({ json: [{ name: 'fixture', repo: 'Kh1ng/git-agent-harness' }] }));
  // Mirror the server's contract: these routes read `workId` and require an Idempotency-Key.
  const mutation = (route: import('@playwright/test').Route) => {
    const request = route.request();
    if (typeof request.postDataJSON()?.workId !== 'string') return route.fulfill({ status: 400, json: { message: 'workId is required' } });
    if (!request.headers()['idempotency-key']) return route.fulfill({ status: 400, json: { message: 'idempotency_key_required' } });
    return route.fulfill({ json: { success: true } });
  };
  await page.route('**/api/hold/set', mutation);
  await page.route('**/api/hold/clear', mutation);
  await page.route('**/api/ledger/clear-attempts', mutation);
  await page.route('**/api/git/review**', (route) => route.fulfill({ json: {
    ownerNodeId: 'local', ownerNodeName: 'Local node', provider: 'github', providerLabel: 'pull request', branch: 'feat/42', base: 'main', upstream: 'origin/feat/42',
    ahead: 0, behind: 0, files: [], commits: [{ hash: 'abcdef1', short: 'abcdef1', subject: 'Drawer work' }], changedFiles: ['README.md'], patch: '',
    existing: { number: 42, title: 'Drawer work', url: 'https://github.com/example/repo/pull/42', draft: false },
  } }));
}

test('renders review and attempt evidence and runs hold, clear-attempts, and redispatch controls', async ({ mount, page }) => {
  await routes(page);
  page.on('dialog', (dialog) => dialog.accept());
  const component = await mount(
    <MockStoreProvider statusData={status()}><WebSocketProvider>
      <WorkDetailDrawer workId="#42" profile="fixture" connected sessions={[]} onClose={() => {}} onRedispatch={() => {}} />
    </WebSocketProvider></MockStoreProvider>,
  );
  await expect(component.getByRole('heading', { name: 'Drawer work' })).toBeVisible();
  await expect(component.getByText('Needs review', { exact: true })).toBeVisible();
  await expect(component.getByText('RUN REVIEW')).toBeVisible();
  await expect(component.getByText('codex/gpt-5', { exact: true }).first()).toBeVisible();
  await expect(component.getByText('$0.0100', { exact: true })).toBeVisible();

  await component.getByRole('button', { name: 'Set hold' }).click();
  await expect(component.getByRole('status')).toHaveText('Hold set.');
  await component.getByRole('button', { name: 'Clear attempts' }).click();
  await expect(component.getByRole('status')).toHaveText('Prior attempts cleared.');
  await component.getByRole('button', { name: 'Re-dispatch' }).click();
  await expect(component.getByRole('status')).toHaveText('Dispatch queued. The activity feed reports the outcome.');
  await component.getByRole('button', { name: 'Review in dashboard' }).click();
  await expect(page.getByRole('dialog', { name: 'Commit and pull request review' })).toBeVisible();
});

test('clears an active hold', async ({ mount, page }) => {
  await routes(page, status(true));
  const component = await mount(
    <MockStoreProvider statusData={status(true)}><WebSocketProvider>
      <WorkDetailDrawer workId="#42" profile="fixture" connected sessions={[]} onClose={() => {}} onRedispatch={() => {}} />
    </WebSocketProvider></MockStoreProvider>,
  );
  await component.getByRole('button', { name: 'Clear hold' }).click();
  await expect(component.getByRole('status')).toHaveText('Hold cleared.');
});

for (const width of [1440, 390]) {
  test(`shows every matching gate, review evidence and next steps at ${width}px`, async ({ mount, page }) => {
    await page.setViewportSize({ width, height: 900 });
    const snapshot = status(true);
    Object.assign(snapshot.merge_requests[0], {
      review_verdict: 'HUMAN_REVIEW',
      review_gate_reason: 'review_evidence_gate: acceptance criteria need a live device check',
      review_generation_status: 'source_changed',
    });
    snapshot.blocked_work_items = [
      {
        kind: 'human_required', source_reference: 'feat/42', reason_code: 'review_evidence_gate',
        message: 'The reviewer could not verify background notifications on a real iPhone.',
        remediation_plan: {
          result: 'plan', profile: 'fixture', work_id: '#42', reason_code: 'review_evidence_gate', required_authority: 'human_reviewer',
          safe_actions: [{ kind: 'manual_review', summary: 'Verify the notification on a locked iPhone and record the evidence.' }],
        },
      },
      { kind: 'dependency', source_reference: '#42', reason_code: 'dependency_open', message: 'Waiting for the APNs setup issue to close.' },
      { kind: 'human_required', source_reference: '#999', message: 'Unrelated gate must stay hidden.' },
      { kind: 'human_required', source_reference: 'https://github.com/example/repo/pull/42', message: 'A legacy provider review hold has no reason code.' },
      { kind: 'human_required', source_reference: 'feat/42', message: 'This branch is waiting for an operator.' },
    ];
    await routes(page, snapshot);
    const component = await mount(
      <MockStoreProvider statusData={snapshot}><WebSocketProvider>
        <WorkDetailDrawer workId="#42" profile="fixture" connected sessions={[]} onClose={() => {}} onRedispatch={() => {}} />
      </WebSocketProvider></MockStoreProvider>,
    );
    await expect(component.getByText('HUMAN REVIEW', { exact: true })).toBeVisible();
    await expect(component.getByText('source_changed', { exact: true })).toBeVisible();
    const gates = component.getByRole('region', { name: 'What is blocking this work' });
    await expect(gates).toContainText('The reviewer could not verify background notifications on a real iPhone.');
    await expect(gates).toContainText('Verify the notification on a locked iPhone and record the evidence.');
    await expect(gates).toContainText('human reviewer');
    await expect(gates).toContainText('Waiting for the APNs setup issue to close.');
    await expect(gates).toContainText('Unknown reason');
    await expect(gates).toContainText('This branch is waiting for an operator.');
    await expect(gates).not.toContainText('Unrelated gate');
    await expect(component.getByText('Automatic merging waits until this hold is cleared.')).toBeVisible();
    const content = page.getByRole('dialog', { name: 'Drawer work' });
    expect(await content.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
    await page.screenshot({ path: test.info().outputPath(`work-gates-${width}.png`) });
  });
}
