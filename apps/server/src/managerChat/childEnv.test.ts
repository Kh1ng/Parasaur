import assert from 'node:assert/strict';
import { test } from 'node:test';
import { agentChildEnv, grantAgentMemoryGateway } from './childEnv.js';

const parent = {
  PATH: '/usr/bin',
  HOME: '/home/owner',
  LC_ALL: 'C.UTF-8',
  XDG_CONFIG_HOME: '/home/owner/.config',
  HTTPS_PROXY: 'http://proxy:3128',
  NODE_EXTRA_CA_CERTS: '/etc/ca.pem',
  MISTRAL_API_KEY: 'backend-key',
  COORDINATOR_TOKEN: 'owner-token',
  TDAI_GATEWAY_API_KEY: 'gateway-key',
  TELEGRAM_BOT_TOKEN: 'bot-token',
  GAH_TELEGRAM_WEBHOOK_SECRET: 'webhook-secret',
  GAH_GATEWAY_LLM_API_KEY: 'gateway-llm-key',
  MISTRAL_ADMIN_API_KEY: 'admin-key'
};

test('agent children inherit the runtime allow-list but no control-plane secret', () => {
  const env = agentChildEnv({}, parent, 'linux');
  assert.equal(env.PATH, '/usr/bin');
  assert.equal(env.HOME, '/home/owner');
  assert.equal(env.LC_ALL, 'C.UTF-8');
  assert.equal(env.XDG_CONFIG_HOME, '/home/owner/.config');
  assert.equal(env.HTTPS_PROXY, 'http://proxy:3128');
  assert.equal(env.NODE_EXTRA_CA_CERTS, '/etc/ca.pem');
  assert.equal(env.MISTRAL_API_KEY, 'backend-key');
  for (const name of ['COORDINATOR_TOKEN', 'TDAI_GATEWAY_API_KEY', 'TELEGRAM_BOT_TOKEN',
    'GAH_TELEGRAM_WEBHOOK_SECRET', 'GAH_GATEWAY_LLM_API_KEY', 'MISTRAL_ADMIN_API_KEY']) {
    assert.equal(env[name], undefined, name);
  }
});

test('adapter env is layered over the inherited allow-list', () => {
  const env = agentChildEnv({ HOME: '/isolated', LLM_API_KEY: 'adapter-key' }, parent, 'linux');
  assert.equal(env.HOME, '/isolated');
  assert.equal(env.LLM_API_KEY, 'adapter-key');
  assert.equal(env.PATH, '/usr/bin');
});

test('Windows environment names match the allow-list case-insensitively', () => {
  const env = agentChildEnv({}, { Path: 'C:\\bin', SystemRoot: 'C:\\Windows', Coordinator_Token: 'owner-token' }, 'win32');
  assert.deepEqual(env, { Path: 'C:\\bin', SystemRoot: 'C:\\Windows' });
});

test('a worker memory grant passes only the gateway variables', () => {
  try {
    grantAgentMemoryGateway({ TDAI_GATEWAY_URL: 'https://central/api/worker-memory', TDAI_GATEWAY_API_KEY: 'relay-key', COORDINATOR_TOKEN: 'owner-token' });
    const env = agentChildEnv({}, parent, 'linux');
    assert.equal(env.TDAI_GATEWAY_URL, 'https://central/api/worker-memory');
    assert.equal(env.TDAI_GATEWAY_API_KEY, 'relay-key');
    assert.equal(env.COORDINATOR_TOKEN, undefined);
  } finally {
    grantAgentMemoryGateway({});
  }
  assert.equal(agentChildEnv({}, parent, 'linux').TDAI_GATEWAY_API_KEY, undefined);
});
