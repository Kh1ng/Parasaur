import { expect, test } from '@playwright/experimental-ct-react';
import type { Session } from '@git-agent-harness/contracts';
import { SessionCard } from '../../src/components/SessionCard.js';

const session: Session = {
  id: 'session-123',
  providerKind: 'claude',
  instanceId: 'claude-0',
  status: 'running',
  repo: 'Kh1ng/git-agent-harness',
  mode: 'improve',
  target: '#1112',
  backend: 'claude'
};

test('names a dispatch by its work instead of its repository', async ({ mount }) => {
  const component = await mount(<SessionCard session={session} onClick={() => undefined} />);

  await expect(component.getByRole('heading', { name: 'Improve #1112' })).toBeVisible();
  await expect(component.getByText('Kh1ng/git-agent-harness', { exact: true })).toBeVisible();
});


test('execution details distinguish observed exit, cancellation and missing legacy results', async ({ mount, page }, testInfo) => {
  const component = await mount(<SessionCard session={{ ...session, status: 'stopped', execution: { reason: 'exited', exitCode: 0, signal: null } }} onClick={() => {}} />);
  await expect(component.getByText('Process exited \u00b7 exit code 0', { exact: true })).toBeVisible();
  await component.update(<SessionCard session={{ ...session, status: 'stopped', execution: { reason: 'cancelled', exitCode: null, signal: 'SIGTERM' } }} onClick={() => {}} />);
  await expect(component.getByText('Cancelled \u00b7 SIGTERM', { exact: true })).toBeVisible();
  await component.update(<SessionCard session={{ ...session, status: 'stopped' }} onClick={() => {}} />);
  await expect(component.getByText('Exit result not reported by this node', { exact: true })).toBeVisible();
  await component.update(<SessionCard session={{ ...session, status: 'starting', leaseState: 'uncertain_reconciling', error: 'Worker connection lost; execution is unconfirmed' }} onClick={() => {}} />);
  await expect(component.getByText('Execution unconfirmed', { exact: true })).toBeVisible();
  await expect(component.getByText('Process exited', { exact: false })).toHaveCount(0);
  await page.setViewportSize({ width: 1100, height: 500 });
  await page.screenshot({ path: testInfo.outputPath('execution-unconfirmed-desktop.png') });
  await page.setViewportSize({ width: 390, height: 650 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('execution-unconfirmed-narrow.png') });
});
