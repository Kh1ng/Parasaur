import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chooseDefaultProfile, defaultProfile } from './defaultProfile.js';

test('the default profile follows the configured profiles', () => {
  assert.equal(defaultProfile(), 'gah', 'an unconfigured server keeps the legacy name');
  assert.equal(chooseDefaultProfile([]), 'gah');
  assert.equal(chooseDefaultProfile(['web', 'api']), 'api', 'first by name when gah is absent');
  assert.equal(chooseDefaultProfile(['web', 'api', 'gah']), 'api', 'stays while it still exists');
  assert.equal(chooseDefaultProfile(['web', 'gah']), 'gah', 'prefers gah once the choice is gone');
  assert.equal(chooseDefaultProfile([{ name: 'web' }, { name: 'api' }]), 'api', 'reads profile summaries');
  for (const notAListing of [null, {}, 'web', [{}, 7, { name: '' }]]) {
    assert.equal(chooseDefaultProfile(notAListing), 'api', 'an unusable listing changes nothing');
  }
  assert.equal(defaultProfile(), 'api');
});
