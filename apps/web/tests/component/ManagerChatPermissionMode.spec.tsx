import { test, expect } from '@playwright/experimental-ct-react';
import React from 'react';
import { ManagerChatSettingsSection } from '../../src/pages/SettingsPage.js';
import { WebSocketProvider } from '../../src/ws/WebSocketContext.js';

// Issue #1382: the Settings control defaults to asking and posts only the
// permission mode when the owner opts in to bypass.
test('the permission prompt setting defaults to ask and saves bypass', async ({ mount, page }) => {
  await page.routeWebSocket('**/ws*', (socket) => socket.close());
  await page.route('**/api/**', (route) => route.fulfill({ json: { backend_instances: [], models: [] } }));
  const posted: unknown[] = [];
  await page.route('**/api/manager-chat/settings', (route) => {
    if (route.request().method() === 'POST') {
      posted.push(route.request().postDataJSON());
      return route.fulfill({ json: { success: true } });
    }
    // An older server omits permissionMode; the control must still read "ask".
    return route.fulfill({
      json: {
        defaultBackend: 'codex',
        profileOverrides: {},
        availableBackends: [{ id: 'codex', displayName: 'Codex', implemented: true }],
        helperRoutes: [],
      },
    });
  });
  const component = await mount(<WebSocketProvider><ManagerChatSettingsSection configuredProfiles={[]} /></WebSocketProvider>);
  const control = component.getByRole('combobox', { name: 'Permission prompts' });
  await expect(control).toHaveValue('ask');
  await expect(control).toHaveAccessibleDescription(/Pushes, actions outside the chat's worktree/);
  await control.selectOption('bypass');
  await expect.poll(() => posted).toEqual([{ permissionMode: 'bypass' }]);
});
