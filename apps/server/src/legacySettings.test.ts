import assert from 'node:assert/strict';
import { test } from 'node:test';
import { adoptLegacySettings } from './legacySettings.js';

test('an old setting name is adopted unless the current name is set', () => {
  const env: NodeJS.ProcessEnv = {
    GAH_CONFIG: '/old/config.toml', GAH_BINARY: '/old/gah', PARASAUR_BINARY: '/new/parasaur',
    HOME: '/home/someone', AGAH_CONFIG: 'unrelated', GAH_EMPTY: '',
  };
  adoptLegacySettings(env);
  assert.deepEqual(env, {
    GAH_CONFIG: '/old/config.toml', GAH_BINARY: '/old/gah', PARASAUR_BINARY: '/new/parasaur',
    HOME: '/home/someone', AGAH_CONFIG: 'unrelated', GAH_EMPTY: '',
    PARASAUR_CONFIG: '/old/config.toml', PARASAUR_EMPTY: '',
  });
});
