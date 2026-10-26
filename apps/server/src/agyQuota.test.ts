import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { agyGroupPool, agyLabel, agyPool } from './agyQuota.js';

test('AGY quota warnings reset after good metadata', t => {
  const warnings: unknown[] = [];
  t.mock.method(console, 'warn', (message: unknown) => warnings.push(message));
  agyGroupPool(['good']);
  agyGroupPool([]);
  agyGroupPool(['bad:pool']);
  assert.equal(warnings.length, 1);
  agyGroupPool(['good']);
  agyGroupPool([]);
  assert.equal(warnings.length, 2);
});

test('AGY pool identity matches the shared Rust conformance fixture', () => {
  const cases: { model: string; buckets: string[]; pool: string }[] = JSON.parse(readFileSync(
    new URL('../../../tests/fixtures/agy/pool-identity.json', import.meta.url), 'utf8'));
  for (const { model, buckets, pool } of cases) {
    assert.equal(agyPool(model, buckets), pool);
    if (buckets.length) assert.equal(agyGroupPool(buckets), pool);
  }
});


test('AGY pool labels match the shared Rust conformance fixture', () => {
  const cases: { label: string; valid: boolean }[] = JSON.parse(readFileSync(
    new URL('../../../tests/fixtures/agy/pool-labels.json', import.meta.url), 'utf8'));
  for (const { label, valid } of cases) {
    assert.equal(agyLabel(label), valid, JSON.stringify(label));
    assert.equal(agyGroupPool([label]) !== undefined, valid, JSON.stringify(label));
  }
});
