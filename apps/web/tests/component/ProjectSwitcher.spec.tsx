import { expect, test } from '@playwright/experimental-ct-react';
import React from 'react';
import { Navbar } from '../../src/components/Navbar.js';
import { WebSocketProvider } from '../../src/ws/WebSocketContext.js';

test('a failed project read leaves loading, offers connection repair, and retries successfully', async ({ mount, page }, testInfo) => {
  let available = false;
  let reads = 0;
  await page.routeWebSocket('**/ws', () => { /* No welcome: a cloud machine has not been selected. */ });
  await page.route('**/api/profiles', route => {
    reads++;
    return available
      ? route.fulfill({ json: [{ name: 'beta', display_name: 'Beta workspace', repo: 'owner/beta' }] })
      : route.fulfill({ status: 503, json: { message: 'Choose a workspace machine in Settings.' } });
  });
  const selected: string[] = [];
  await mount(<WebSocketProvider><Navbar currentPage="overview" sideView={null} chatOpen={false} onChatToggle={() => {}} onPageChange={value => selected.push(value)} onImportProject={() => {}} onCreateProject={() => {}} /></WebSocketProvider>);
  const opener = page.getByRole('button', { name: /Project: unavailable/ });
  await expect(opener).toBeVisible();
  await opener.click();
  await expect(page.getByRole('alert')).toContainText('Choose a workspace machine in Settings.');
  await page.screenshot({ path: testInfo.outputPath('projects-unavailable-desktop.png') });
  await page.getByRole('menuitem', { name: 'Connection settings', exact: true }).click();
  expect(selected).toEqual(['settings']);
  await expect(page.getByRole('menu', { name: 'Projects' })).not.toBeVisible();

  await opener.click();
  available = true;
  const before = reads;
  await page.getByRole('menuitem', { name: 'Retry projects', exact: true }).click();
  await expect(page.getByRole('menuitemradio', { name: /Beta workspace/ })).toBeVisible();
  expect(reads).toBeGreaterThan(before);
  await page.getByRole('menuitemradio', { name: /Beta workspace/ }).click();
  await expect(page.getByRole('button', { name: /Project: Beta workspace/ })).toBeVisible();
  await expect(page.getByRole('menu', { name: 'Projects' })).not.toBeVisible();
});

test('a saved project remains recognizable during an outage on a narrow screen', async ({ mount, page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.routeWebSocket('**/ws', () => {});
  await page.route('**/api/profiles', route => route.fulfill({ status: 503, json: { message: 'Server unavailable.' } }));
  await page.evaluate(() => history.replaceState(null, '', '/?profile=saved-project'));
  await mount(<WebSocketProvider><Navbar currentPage="overview" sideView={null} chatOpen={false} onChatToggle={() => {}} onPageChange={() => {}} onImportProject={() => {}} onCreateProject={() => {}} /></WebSocketProvider>);
  await page.getByRole('button', { name: /saved-project/ }).click();
  await expect(page.getByRole('alert')).toContainText('Server unavailable.');
  await expect(page.getByRole('menuitem', { name: 'Connection settings', exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('projects-unavailable-mobile.png') });
  const menu = await page.getByRole('menu', { name: 'Projects' }).boundingBox();
  expect(menu!.x).toBeGreaterThanOrEqual(0);
  expect(menu!.x + menu!.width).toBeLessThanOrEqual(390);
});
