import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createHeadlessBackend, agyBackendSpec } from './headlessAdapter.js';
import { createAgyModelOptions, parseAgyModels } from './agyModels.js';
import { agyGroupPool } from '../agyQuota.js';

const fixture = (name: string) => readFileSync(new URL(`../../../../tests/fixtures/agy/${name}`, import.meta.url), 'utf8');
const models = fixture('models.tsv');
const help = fixture('help.txt');

test('agy recorded inventory derives effort variants per model and shares provider bucket IDs with routing', () => {
  const parsed = parseAgyModels(models, help);
  assert.equal(parsed.models.length, 6);
  // The wire payload keeps per-model choices for session pickers that select
  // locally, without changing an ACP connection's current model.
  const wire = JSON.parse(JSON.stringify(parsed));
  assert.deepEqual(wire.models.find((model: { id: string }) => model.id === 'gemini-3.8-flash-high').reasoningEfforts.map((effort: { id: string }) => effort.id), ['low', 'medium', 'high']);
  assert.deepEqual(wire.models.find((model: { id: string }) => model.id === 'claude-sonnet-4-6').reasoningEfforts, []);
  assert.deepEqual(parsed.modelReasoningEfforts['gemini-3.8-flash-high'].map(e => e.id), ['low', 'medium', 'high']);
  assert.deepEqual(parsed.modelReasoningEfforts['gpt-oss-120b-medium'].map(e => e.id), ['medium']);
  assert.deepEqual(parsed.modelReasoningEfforts['claude-sonnet-4-6'], []);
  const usage = JSON.parse(fixture('usage.json'));
  assert.equal(usage.num_turns, 0);
  assert.deepEqual(usage.command.data.groups.map((group: { buckets: { id: string }[] }) => agyGroupPool(group.buckets.map(b => b.id))), ['gemini-weekly', '3p-weekly']);
  assert.equal(agyGroupPool(['future-weekly', 'future-short']), 'future-short');
  assert.equal(agyGroupPool(['invalid:pool']), undefined);
});

test('agy TTL refresh removes retired models and efforts, deduplicates probes, and warns once on malformed metadata', async t => {
  t.mock.timers.enable({ apis: ['setInterval'] });
  let now = 0;
  let inventory = models;
  let calls = 0;
  const warnings: string[] = [];
  const options = createAgyModelOptions(undefined, { now: () => now, warn: m => warnings.push(m), run: async args => {
    calls++;
    assert.ok(args[0] === 'models' || args[0] === '--help');
    return { stdout: args[0] === 'models' ? inventory : '', stderr: args[0] === '--help' ? help : '' };
  } });
  t.after(() => options.dispose());
  await Promise.all([options(), options()]);
  assert.equal(calls, 2);
  inventory = models.split('\n').filter(line => !line.startsWith('gemini-3.8-flash-low\t')).join('\n');
  await options();
  assert.equal(calls, 2);
  now = 60_000;
  t.mock.timers.tick(60_000);
  const next = await options();
  assert.equal(next.models.length, 5);
  assert.deepEqual(next.modelReasoningEfforts['gemini-3.8-flash-high'].map(e => e.id), ['medium', 'high']);
  assert.equal(next.modelReasoningEfforts['gemini-3.8-flash-low'], undefined);
  inventory = '{"unknown":"shape"}';
  for (now = 120_000; now < 300_000; now += 60_000) {
    assert.deepEqual((await options()).reasoningEfforts, []);
    assert.deepEqual((await options()).models, []);
  }
  assert.equal(warnings.length, 1);
  options.dispose();
  const count = calls;
  now += 60_000;
  t.mock.timers.tick(60_000);
  await options();
  assert.equal(calls, count);
});

test('unknown agy help keeps readable models with no efforts; empty inventory replaces the old one', async t => {
  assert.throws(() => parseAgyModels(models, 'changed help'));
  assert.deepEqual(parseAgyModels('', help).models, []);
  const options = createAgyModelOptions(undefined, { warn: () => {}, run: async args => ({ stdout: args[0] === 'models' ? models : 'changed help', stderr: '' }) });
  t.after(() => options.dispose());
  assert.equal((await options()).models.length, 6);
  assert.deepEqual((await options()).modelReasoningEfforts, {});
});

test('agy refresh clears a selected effort or model when the provider retires it', async t => {
  let now = 0;
  let inventory = models;
  const options = createAgyModelOptions(undefined, { now: () => now, run: async args => ({ stdout: args[0] === 'models' ? inventory : help, stderr: '' }) });
  const backend = createHeadlessBackend({ ...agyBackendSpec(), modelOptions: options, dispose: options.dispose });
  t.after(() => backend.dispose?.());
  const conversation = { key: 'retired', profile: 'p' };
  await backend.setModel(conversation, 'gemini-3.8-flash-high');
  await backend.setReasoningEffort(conversation, 'low');
  inventory = models.split('\n').filter(line => !line.startsWith('gemini-3.8-flash-low\t')).join('\n');
  now += 60_000;
  assert.equal((await backend.listModels(conversation)).currentReasoningEffortId, null);
  inventory = '';
  now += 60_000;
  const retired = await backend.listModels(conversation);
  assert.equal(retired.currentModelId, null);
  assert.deepEqual(retired.models, []);
  assert.deepEqual(retired.reasoningEfforts, []);
});

for (const failure of ['models exit', 'models spawn', 'help exit']) {
  test(`agy ${failure} failure keeps the last good inventory and selection until recovery`, async t => {
    let now = 0;
    let failing = false;
    let inventory = models;
    let calls = 0;
    const warnings: string[] = [];
    const options = createAgyModelOptions(undefined, { now: () => now, warn: message => warnings.push(message), run: async args => {
      calls++;
      if (failing && args[0] === (failure === 'help exit' ? '--help' : 'models')) {
        throw Object.assign(new Error('metadata invocation failed'), { code: failure === 'models spawn' ? 'ENOENT' : 1, stdout: '' });
      }
      return { stdout: args[0] === 'models' ? inventory : help, stderr: '' };
    } });
    const backend = createHeadlessBackend({ ...agyBackendSpec(), modelOptions: options, dispose: options.dispose });
    t.after(() => backend.dispose?.());
    const conversation = { key: 'transient-failure', profile: 'p' };
    await backend.setModel(conversation, 'gemini-3.8-flash-high');
    await backend.setReasoningEffort(conversation, 'low');
    const good = await options();
    failing = true;
    for (now = 60_000; now <= 120_000; now += 60_000) {
      const retained = await backend.listModels(conversation);
      assert.equal(retained.models.length, 6);
      assert.equal(retained.currentModelId, 'gemini-3.8-flash-high');
      assert.equal(retained.currentReasoningEffortId, 'low');
      assert.equal(await options(), good);
      const count = calls;
      await options();
      assert.equal(calls, count);
    }
    assert.equal(warnings.length, 1);
    failing = false;
    inventory = models.split('\n').filter(line => !line.startsWith('gemini-3.8-flash-low\t')).join('\n');
    const recovered = await backend.listModels(conversation);
    assert.equal(recovered.models.length, 5);
    assert.equal(recovered.currentModelId, 'gemini-3.8-flash-high');
    assert.equal(recovered.currentReasoningEffortId, null);
    failing = true;
    now += 60_000;
    await options();
    assert.equal(warnings.length, 2);
  });
}
