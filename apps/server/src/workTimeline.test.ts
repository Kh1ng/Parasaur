import assert from 'node:assert/strict';
import { test } from 'node:test';
import { withFixtureServer } from './fixtureGahHarness.js';

test('work history filters the selected project and preserves legacy aggregate reads', async () => {
  await withFixtureServer(async origin => {
    const endpoint = `${origin}/api/work/%2342`;
    const all = await (await fetch(endpoint)).json() as { profile: string }[];
    assert.deepEqual(all.map(entry => entry.profile), ['alpha', 'beta']);
    const selected = await (await fetch(`${endpoint}?profile=beta`)).json() as { work_title: string }[];
    assert.deepEqual(selected.map(entry => entry.work_title), ['Beta issue']);
    assert.deepEqual(await (await fetch(`${endpoint}?profile=missing`)).json(), []);
    assert.equal((await fetch(`${endpoint}?profile[]=beta`)).status, 400);
    assert.equal((await fetch(`${endpoint}?profile=`)).status, 400);
  });
});
