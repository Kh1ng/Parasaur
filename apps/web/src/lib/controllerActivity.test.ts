import type { ControllerActivity } from '@git-agent-harness/contracts';
import { controllerActivityStatus } from './controllerActivity.js';

const run: ControllerActivity = {
  run_id: 'run', profile: 'gah', work_id: '#1525', action: 'review: branch',
  started_at: '2026-11-05T12:00:00Z', finished_at: '2026-11-05T12:01:00Z',
  status: 'failed', outcome: 'review: deferred_capacity: node admission deferred: memory reserve would be crossed'
};

test('historical capacity refusals display as deferred without rewriting the receipt', () => {
  expect(controllerActivityStatus(run)).toBe('deferred');
  expect(run.status).toBe('failed');
  expect(run.outcome).toContain('memory reserve');
});

test('a deferred fallback retains the evidence of real prior attempts', () => {
  const fallback = { ...run, outcome: 'fix_existing: deferred_capacity: fallback capacity deferred after 2 backend attempt(s): node admission deferred' };
  expect(controllerActivityStatus(fallback)).toBe('deferred');
  expect(fallback.outcome).toContain('after 2 backend attempt(s)');
});

test('an unfinished run remains running even if it contains an old capacity outcome', () => {
  expect(controllerActivityStatus({ ...run, status: 'running', finished_at: null })).toBe('running');
});

test.each([null, 'review: backend failed', 'review: backend error mentions deferred_capacity: in its output'])('ordinary and unknown failures stay failed: %s', outcome => {
  expect(controllerActivityStatus({ ...run, outcome })).toBe('failed');
});

test('ordinary completed dispatches remain finished', () => {
  expect(controllerActivityStatus({ ...run, status: 'finished', outcome: 'review: success' })).toBe('finished');
});
