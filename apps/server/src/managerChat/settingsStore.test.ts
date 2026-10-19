import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { readSettings, setBackendForProfile } from './settingsStore.js';
import { DEFAULT_BACKEND_ID } from './registry.js';

test('a corrupt settings file is set aside and the conservative defaults apply', (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-chat-settings-'));
  const previous = process.env.GAH_MANAGER_CHAT_SETTINGS_PATH;
  const path = join(dir, 'manager-chat-settings.json');
  process.env.GAH_MANAGER_CHAT_SETTINGS_PATH = path;
  const logged = t.mock.method(console, 'error', () => {});
  try {
    const torn = '{"defaultBackend":"hermes","permissionMode":"bypass","profileOverrides":{"gah":';
    writeFileSync(path, torn);
    const settings = readSettings();
    assert.equal(settings.permissionMode, 'ask');
    assert.equal(settings.defaultBackend, DEFAULT_BACKEND_ID);
    assert.deepEqual(settings.profileOverrides, {});
    assert.equal(logged.mock.callCount(), 1);
    const preserved = readdirSync(dir).filter(name => name.startsWith('manager-chat-settings.json.corrupt-'));
    assert.equal(preserved.length, 1);
    assert.equal(readFileSync(join(dir, preserved[0]!), 'utf8'), torn, 'the corrupt file is kept for recovery');

    setBackendForProfile('gah', 'hermes');
    assert.equal(readSettings().profileOverrides.gah, 'hermes');
    assert.equal(logged.mock.callCount(), 1, 'a repaired file is not reported again');
    assert.deepEqual(readdirSync(dir).sort(), [preserved[0], 'manager-chat-settings.json'].sort(), 'no temporary file is left behind');
  } finally {
    if (previous === undefined) delete process.env.GAH_MANAGER_CHAT_SETTINGS_PATH;
    else process.env.GAH_MANAGER_CHAT_SETTINGS_PATH = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});
