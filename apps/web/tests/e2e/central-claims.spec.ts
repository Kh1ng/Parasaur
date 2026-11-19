import { expect, test } from '@playwright/test';
import type { CentralClaimObservation } from '@git-agent-harness/contracts';

test('Factory distinguishes a refused claim, later admission, and missing evidence', async ({ page }) => {
  let observation: CentralClaimObservation | undefined = {
    state: 'observed', outcome: 'refused', observed_at: '2026-11-03T12:00:00Z', work_id: '#1516'
  };
  let sourceState = 'healthy';
  await page.route('**/api/status**', async route => {
    const snapshot = await (await route.fetch()).json();
    await route.fulfill({ json: { ...snapshot, node: { role: 'worker', central_url: 'https://central.example', central_claim_observation: observation }, nodes: [{
      node_id: 'remote-machine', display_name: 'Remote machine', central_claim_observations: [
        { profile: 'fixture', central_url: 'https://central.example', observation, source_state: sourceState },
        { profile: 'other', central_url: 'https://must-not-leak.example', observation, source_state: 'healthy' }
      ]
    }] } });
  });
  await page.goto('/?page=work&profile=fixture');
  const refused = page.getByRole('region', { name: 'Latest central claim refused', exact: true });
  await expect(refused).toContainText('That dispatch did not start.');
  await expect(refused).toContainText('#1516');
  const remote = page.getByRole('region', { name: 'Remote machine · remote-machine: Latest central claim refused' });
  await expect(remote).toContainText('https://central.example');
  await expect(page.getByText('https://must-not-leak.example', { exact: false })).toHaveCount(0);
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 900 });
    expect(await refused.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
    await page.screenshot({ path: test.info().outputPath(`central-claim-refused-${width}.png`) });
  }
  observation = { state: 'observed', outcome: 'acquired', observed_at: '2026-11-03T12:01:00Z', work_id: '#1516' };
  sourceState = 'unreachable';
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Latest central claim acquired', exact: true })).toContainText('This does not establish a current lease or current service health.');
  await expect(refused).toHaveCount(0);
  await expect(page.getByRole('region', { name: 'Remote machine · remote-machine: Latest central claim acquired' })).toContainText('Node observation: unreachable. Showing the last reported claim evidence.');
  observation = undefined;
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Central claim evidence unavailable', exact: true })).toContainText('has not reported a recognized claim result');
  observation = { state: 'unavailable' };
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Central claim evidence unavailable', exact: true })).toContainText('local claim history could not be read');
});
