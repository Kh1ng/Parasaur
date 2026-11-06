import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { ServerMessage } from '@git-agent-harness/contracts';
import { createSessionManager } from './SessionManager.js';
import { MAX_DIAGNOSTIC_CHARS, DIAGNOSTIC_TRUNCATION } from '../diagnosticTail.js';

test('session diagnostics retain a bounded tail and retired sessions ignore late output', async () => {
  const published: ServerMessage[] = [];
  let finish!: (result: { exitCode: number; stderr: string }) => void;
  const manager = createSessionManager({ disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: { publish: message => { published.push(message); } },
    dispatchRunner: () => ({ promise: new Promise(resolve => { finish = resolve; }), cancel: async () => ({ cancelled: true }) })
  });
  const session = await manager.startSession({ profile: 'fixture', providerKind: 'github', instanceId: 'github-0', repo: 'fixture/repo', mode: 'fix' });
  await new Promise(resolve => setImmediate(resolve));
  manager.addSessionOutput(session.id, 'live stdout');
  manager.addSessionOutput(session.id, 'live stderr', true);
  assert.ok(published.some(message => message.type === 'session.stdout' && message.data === 'live stdout'));
  assert.ok(published.some(message => message.type === 'session.stderr' && message.data === 'live stderr'));
  finish({ exitCode: 1, stderr: 'old prefix\n' + 'x'.repeat(MAX_DIAGNOSTIC_CHARS * 3) + '\nfinal cause' });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(session.status, 'error');
  assert.ok(session.error?.startsWith(DIAGNOSTIC_TRUNCATION));
  assert.ok(session.error?.endsWith('final cause'));
  assert.ok(session.error!.length <= MAX_DIAGNOSTIC_CHARS + DIAGNOSTIC_TRUNCATION.length);
  const count = published.length;
  manager.addSessionOutput(session.id, 'late output');
  manager.addSessionOutput('missing' as typeof session.id, 'unknown session');
  assert.equal(published.length, count);
});

test('an expired cancelled session cannot launch after its queued metadata is cleaned up', async t => {
  t.mock.timers.enable({ apis: ['setInterval', 'Date'], now: Date.now() });
  const targets: string[] = [];
  let finish!: (result: { exitCode: number; stderr: string }) => void;
  const manager = createSessionManager({
    providerRegistry: { isProviderAvailable: () => true }, pushBus: { publish() {} },
    dispatchRunner: options => {
      targets.push(options.target!);
      return { promise: new Promise(resolve => { finish = resolve; }), cancel: async () => ({ cancelled: true }) };
    }
  });
  const base = { profile: 'fixture', providerKind: 'github' as const, instanceId: 'github-0', repo: 'fixture/repo', mode: 'fix' };
  await manager.startSession({ ...base, target: 'first' });
  await new Promise(resolve => setImmediate(resolve));
  const queued = await manager.startSession({ ...base, target: 'cancelled' });
  await manager.stopSession(queued.id);
  queued.endedAt = new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString();
  t.mock.timers.tick(60001);
  assert.equal(manager.getSession(queued.id), undefined);
  finish({ exitCode: 0, stderr: '' });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(targets, ['first']);
});

test('explicit manual workers launch alongside an active same-profile session and retain their reasoning', async () => {
  const dispatched: { manualWorker?: boolean; reasoningEffort?: string; target?: string }[] = [];
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: { publish() {} },
    dispatchRunner: (options) => {
      dispatched.push(options);
      return { promise: new Promise(() => {}), cancel: async () => ({ cancelled: true }) };
    },
  });
  const base = { profile: 'repo', providerKind: 'github' as const, instanceId: 'github-0', repo: 'owner/repo', mode: 'improve' };
  await manager.startSession({ ...base, target: 'first.md' });
  await new Promise((resolve) => setImmediate(resolve));
  await manager.startSession({ ...base, target: 'second.md', manualWorker: true, reasoningEffort: 'high', backend: 'codex', model: 'gpt-6.1-sol' });
  assert.equal(dispatched.length, 2);
  assert.equal(dispatched[1].manualWorker, true);
  assert.equal(dispatched[1].reasoningEffort, 'high');
  assert.equal(dispatched[1].target, 'second.md');
});

test('explicit review and improve routes reach dispatch unchanged and do not fall back on failure', async () => {
  for (const mode of ['review', 'improve']) {
    const dispatched: Array<{ mode: string; backend?: string; backendInstance?: string; model?: string }> = [];
    const manager = createSessionManager({
      disableCleanupTimer: true,
      providerRegistry: { isProviderAvailable: () => true },
      pushBus: { publish() {} },
      dispatchRunner: (options) => {
        dispatched.push({ mode: options.mode, backend: options.backend, backendInstance: options.backendInstance, model: options.model });
        return {
          promise: Promise.resolve({
            exitCode: 1, stdout: '',
            stderr: 'no eligible backend available for preferred vibe/mistral-medium-3.5',
          }),
          cancel: async () => ({ cancelled: false }),
        };
      },
    });
    const session = await manager.startSession({
      profile: 'gah', providerKind: 'github', instanceId: 'github-0',
      repo: 'owner/repo', mode, backend: 'vibe', backendInstance: 'work', model: 'mistral-medium-3.5', requestId: 'exact-route',
    });
    await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(dispatched, [{ mode, backend: 'vibe', backendInstance: 'work', model: 'mistral-medium-3.5' }]);
    assert.equal(manager.getSession(session.id)?.backendInstance, 'work');
    assert.equal(manager.getSession(session.id)?.requestId, 'exact-route');
    assert.equal(manager.getSession(session.id)?.status, 'error');
    assert.equal(manager.getSession(session.id)?.backend, 'vibe');
    assert.equal(manager.getSession(session.id)?.model, 'mistral-medium-3.5');
  }
});

test('session.start request ids are idempotent and emit one start event', async () => {
  const published: ServerMessage[] = [];
  let dispatchCalls = 0;
  let dispatchedMr: string | undefined;
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: {
      isProviderAvailable: () => true
    },
    pushBus: {
      publish(message) {
        published.push(message);
      }
    },
    dispatchRunner: (options) => {
      dispatchCalls += 1;
      dispatchedMr = options.mr;
      return {
        promise: new Promise(() => {
          // Keep the dispatch active so the second request simulates a retry
          // against an in-flight run rather than a completed one.
        }),
        cancel: async () => ({ cancelled: true }),
      };
    }
  });

  const session = await manager.startSession({
    requestId: 'dispatch-req-1',
    profile: 'gah',
    providerKind: 'codex',
    instanceId: 'codex-0',
    repo: 'owner/repo',
    mr: '1100',
    mode: 'improve'
  });

  const retry = await manager.startSession({
    requestId: 'dispatch-req-1',
    profile: 'gah',
    providerKind: 'codex',
    instanceId: 'codex-0',
    repo: 'owner/repo',
    mr: '1100',
    mode: 'improve'
  });

  assert.equal(retry.id, session.id);
  assert.equal(dispatchCalls, 1);
  assert.equal(dispatchedMr, '1100');
  assert.equal(manager.getAllSessions().length, 1);
  assert.equal(manager.getActiveSessions().length, 1);
  assert.equal(
    published.filter((message) => message.type === 'session.started').length,
    1
  );

  await manager.stopSession(session.id);

  assert.equal(manager.getActiveSessions().length, 0);
  assert.equal(
    published.filter((message) => message.type === 'session.stopped').length,
    1
  );
});

test('same-profile sessions queue instead of racing the CLI profile lock', async () => {
  const releases: Array<() => void> = [];
  let dispatchCalls = 0;
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: { publish() {} },
    dispatchRunner: () => {
      dispatchCalls += 1;
      let resolvePromise: (value: { exitCode: number; stdout: string; stderr: string }) => void;
      const promise = new Promise<{ exitCode: number; stdout: string; stderr: string }>((resolve) => {
        resolvePromise = resolve;
      });
      
      // Add this resolve function to the releases array for the test
      releases.push(() => resolvePromise!({ exitCode: 0, stdout: '', stderr: '' }));
      
      return {
        promise,
        cancel: async () => {
          resolvePromise!({ exitCode: -1, stdout: '', stderr: 'cancelled' });
          return { cancelled: true };
        },
      };
    }
  });
  const options = {
    profile: 'gah',
    providerKind: 'github' as const,
    instanceId: 'github-0',
    repo: 'owner/repo',
    mode: 'fix'
  };

  await Promise.all([
    manager.startSession({ ...options, target: '#1' }),
    manager.startSession({ ...options, target: '#2' })
  ]);
  assert.equal(dispatchCalls, 1);

  releases.shift()?.();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(dispatchCalls, 2);
  releases.shift()?.();
});

test('stopSession terminates running dispatch and awaits completion', async () => {
  let cancelCalled = false;
  let cancelResolve: () => void;
  const cancelPromise = new Promise<void>((resolve) => { cancelResolve = resolve; });
  
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: { publish() {} },
    dispatchRunner: () => ({
      promise: new Promise(() => {}), // Never completes
      cancel: async () => {
        cancelCalled = true;
        cancelResolve();
        return { cancelled: true };
      },
    })
  });

  const session = await manager.startSession({
    profile: 'gah',
    providerKind: 'github',
    instanceId: 'github-0',
    repo: 'owner/repo',
    mode: 'fix'
  });

  assert.equal(session.status, 'running');
  assert.equal(cancelCalled, false);

  // Stop the session - should call cancel and await completion
  const stopPromise = manager.stopSession(session.id);
  
  // Wait for cancel to be called
  await cancelPromise;
  
  const stoppedSession = await stopPromise;
  
  assert.equal(cancelCalled, true);
  assert.equal(stoppedSession.status, 'stopped');
});

test('stopSession prevents queued same-profile session from starting', async () => {
  let dispatchCalls = 0;
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: { publish() {} },
    dispatchRunner: () => {
      dispatchCalls += 1;
      return {
        promise: new Promise(() => {}), // Never completes
        cancel: async () => ({ cancelled: true }),
      };
    }
  });

  const options = {
    profile: 'gah',
    providerKind: 'github' as const,
    instanceId: 'github-0',
    repo: 'owner/repo',
    mode: 'fix'
  };

  // Start first session -- occupies the profile slot indefinitely, since its
  // dispatch never resolves.
  const session1 = await manager.startSession({ ...options, target: '#1' });
  assert.equal(dispatchCalls, 1);

  // Queue second session behind the first (same profile). startSession
  // resolves immediately with a session even though the underlying dispatch
  // is still waiting for session1's slot.
  const session2 = await manager.startSession({ ...options, target: '#2' });
  assert.equal(dispatchCalls, 1);

  // Stop the QUEUED session itself -- not the running one. This is the
  // scenario AC2 actually describes: cancelling a session that hasn't
  // started must prevent its own dispatchRunner call.
  await manager.stopSession(session2.id);

  // Now let session1 vacate the profile slot so the queue can advance to
  // where session2 would have started.
  await manager.stopSession(session1.id);

  // Give the queue a moment to process the (cancelled) second slot.
  await new Promise(resolve => setTimeout(resolve, 10));

  // Only the first dispatch should ever have been called -- session2 was
  // cancelled before its turn came up.
  assert.equal(dispatchCalls, 1);
});

test('stopping a running session does not cancel an unrelated queued session on the same profile', async () => {
  let dispatchCalls = 0;
  const releases: Array<(result: { exitCode: number; stderr: string }) => void> = [];
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: { publish() {} },
    dispatchRunner: () => {
      dispatchCalls += 1;
      let resolvePromise: (result: { exitCode: number; stderr: string }) => void;
      const promise = new Promise<{ exitCode: number; stderr: string }>((resolve) => {
        resolvePromise = resolve;
      });
      releases.push((result) => resolvePromise(result));
      return {
        promise,
        cancel: async () => {
          resolvePromise({ exitCode: -1, stderr: 'cancelled' });
          return { cancelled: true };
        },
      };
    }
  });

  const options = {
    profile: 'gah',
    providerKind: 'github' as const,
    instanceId: 'github-0',
    repo: 'owner/repo',
    mode: 'fix'
  };

  const session1 = await manager.startSession({ ...options, target: '#1' });
  const session2 = await manager.startSession({ ...options, target: '#2' }); // queued behind session1
  assert.equal(dispatchCalls, 1);

  // Stop the RUNNING session (session1). This must not poison the profile
  // for session2, which never asked to be cancelled -- it should still get
  // its turn to dispatch.
  await manager.stopSession(session1.id);

  // Give the queue a moment to advance now that session1's slot released.
  await new Promise(resolve => setTimeout(resolve, 10));

  assert.equal(dispatchCalls, 2);
  assert.notEqual(manager.getSession(session2.id)?.status, 'error');

  // Drain both dispatch promises so nothing is left dangling.
  releases.shift()?.({ exitCode: 0, stderr: '' });
  releases.shift()?.({ exitCode: 0, stderr: '' });
});

test('stopSession does not publish stopped until cancellation completes', async () => {
  const published: ServerMessage[] = [];
  let cancelResolve: () => void;
  const cancelPromise = new Promise<void>((resolve) => { cancelResolve = resolve; });
  
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: {
      publish(message) {
        published.push(message);
      }
    },
    dispatchRunner: () => ({
      promise: new Promise(() => {}),
      cancel: async () => {
        // Delay the cancellation to test timing
        await new Promise(resolve => setTimeout(resolve, 50));
        cancelResolve();
        return { cancelled: true };
      },
    })
  });

  const session = await manager.startSession({
    profile: 'gah',
    providerKind: 'github',
    instanceId: 'github-0',
    repo: 'owner/repo',
    mode: 'fix'
  });

  // Start stopping the session
  const stopPromise = manager.stopSession(session.id);
  
  // Check that we have the stopping status message but not the stopped message yet
  const statusMessagesBefore = published.filter(m => m.type === 'session.status');
  const stoppedMessagesBefore = published.filter(m => m.type === 'session.stopped');
  
  assert.equal(statusMessagesBefore.length >= 1, true);
  assert.equal(stoppedMessagesBefore.length, 0);
  
  // Wait for cancellation to complete
  await cancelPromise;
  
  // Now the stopped message should be published
  await stopPromise;
  
  const stoppedMessagesAfter = published.filter(m => m.type === 'session.stopped');
  assert.equal(stoppedMessagesAfter.length, 1);
});

test('late process completion cannot overwrite cancelled terminal state', async () => {
  // Captured directly from the dispatchRunner closure that backs this
  // session -- not recovered by reaching into manager internals after
  // stopSession() has already removed the activeDispatches entry, which
  // would make the "late completion" branch below unreachable.
  let simulateLateCompletion: (() => void) | undefined;

  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: { publish() {} },
    dispatchRunner: () => {
      let resolveDispatch: (result: { exitCode: number; stderr: string }) => void;
      const promise = new Promise<{ exitCode: number; stderr: string }>((resolve) => {
        resolveDispatch = resolve;
      });
      simulateLateCompletion = () => resolveDispatch({ exitCode: 0, stderr: '' });

      return {
        promise,
        // cancel() reports success without settling `promise` -- modeling a
        // process whose cancellation was confirmed but whose result
        // callback fires on a separate, later tick.
        cancel: async () => ({ cancelled: true })
      };
    }
  });

  const session = await manager.startSession({
    profile: 'gah',
    providerKind: 'github',
    instanceId: 'github-0',
    repo: 'owner/repo',
    mode: 'fix'
  });

  await manager.stopSession(session.id);
  assert.equal(manager.getSession(session.id)?.status, 'stopped');

  // Now fire the late completion -- this must hit handleDispatchComplete's
  // terminal-status guard and not overwrite the already-stopped session.
  assert.equal(typeof simulateLateCompletion, 'function');
  simulateLateCompletion?.();

  await new Promise(resolve => setTimeout(resolve, 10));

  const finalSession = manager.getSession(session.id);
  assert.equal(finalSession?.status, 'stopped');
});

test('stopSession times out if cancellation takes too long', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const published: ServerMessage[] = [];
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: {
      publish(message) {
        published.push(message);
      }
    },
    dispatchRunner: () => ({
      promise: new Promise(() => {}),
      cancel: async () => {
        // This will never resolve, simulating a hung process
        await new Promise(() => {});
        return { cancelled: true };
      },
    })
  });

  const session = await manager.startSession({
    profile: 'gah',
    providerKind: 'github',
    instanceId: 'github-0',
    repo: 'owner/repo',
    mode: 'fix'
  });

  const stopPromise = manager.stopSession(session.id);
  t.mock.timers.tick(29_999);
  assert.equal(manager.getSession(session.id)?.status, 'stopping');
  t.mock.timers.tick(1);
  const stoppedSession = await stopPromise;

  // A deadline is not termination proof; retain the nonterminal session.
  assert.equal(stoppedSession.status, 'stopping');
  assert.equal(stoppedSession.endedAt, undefined);
  assert.match(stoppedSession.error ?? '', /Cancellation timed out/);
});

test('a GitLab dispatch context is recorded on the session and survives fleet listing', async () => {
  const manager = createSessionManager({
    disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true },
    pushBus: { publish() {} },
    dispatchRunner: () => ({ promise: new Promise(() => {}), cancel: async () => ({ cancelled: true }) })
  });
  const gitlab = { project: 'parasaur-test/group/project', kind: 'merge_request' as const, iid: 7, webUrl: 'https://gitlab.example.test/parasaur-test/group/project' };
  const session = await manager.startSession({
    profile: 'gl-profile', providerKind: 'gitlab', instanceId: 'gitlab-0',
    repo: 'parasaur-test/group/project', mode: 'review', mr: '7', gitlab
  });
  assert.deepEqual(manager.getSession(session.id)?.gitlab, gitlab);
  assert.deepEqual(manager.getAllSessions().find(candidate => candidate.id === session.id)?.gitlab, gitlab);

  // Sessions without a context (every other start path) stay unchanged.
  const plain = await manager.startSession({
    profile: 'gah', providerKind: 'github', instanceId: 'github-0', repo: 'owner/repo', mode: 'improve'
  });
  assert.equal(manager.getSession(plain.id)?.gitlab, undefined);
});


test('unconfirmed cancellation keeps the profile reserved until execution really settles', async () => {
  let launches = 0;
  let finish!: (value: { exitCode: number; stderr: string }) => void;
  const manager = createSessionManager({ disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true }, pushBus: { publish() {} },
    dispatchRunner: () => {
      launches += 1;
      return { promise: new Promise(resolve => { finish = resolve; }), cancel: async () => ({ cancelled: false, error: 'termination not confirmed' }) };
    }
  });
  const options = { profile: 'fixture', providerKind: 'github' as const, instanceId: 'github-0', repo: 'fixture/repo', mode: 'fix' };
  const first = await manager.startSession({ ...options, target: '#1' });
  const second = await manager.startSession({ ...options, target: '#2' });
  const stopped = await manager.stopSession(first.id);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(stopped.status, 'stopping');
  assert.equal(stopped.endedAt, undefined);
  assert.equal(launches, 1, 'the unconfirmed process must retain its profile slot');
  assert.ok(manager.getActiveSessions().some(session => session.id === first.id));
  finish({ exitCode: 3, stderr: 'observed final exit' });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(launches, 2, 'observed process completion releases the slot');
  assert.equal(manager.getSession(first.id)?.status, 'stopped');
  finish({ exitCode: 0, stderr: '' });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(manager.getSession(second.id)?.status, 'stopped');
});


test('cancelling the newest queued session cannot let a new sibling bypass its live predecessor', async () => {
  let launches = 0;
  const finishes: Array<(result: { exitCode: number; stderr: string }) => void> = [];
  const manager = createSessionManager({ disableCleanupTimer: true,
    providerRegistry: { isProviderAvailable: () => true }, pushBus: { publish() {} },
    dispatchRunner: () => { launches += 1; return { promise: new Promise(resolve => finishes.push(resolve)), cancel: async () => ({ cancelled: true }) }; }
  });
  const options = { profile: 'fixture', providerKind: 'github' as const, instanceId: 'github-0', repo: 'fixture/repo', mode: 'fix' };
  await manager.startSession({ ...options, target: '#1' });
  const second = await manager.startSession({ ...options, target: '#2' });
  assert.equal(second.status, 'starting');
  await manager.stopSession(second.id);
  assert.deepEqual(second.execution, { reason: 'not_started', exitCode: null, signal: null });
  await manager.startSession({ ...options, target: '#3' });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(launches, 1);
  finishes.shift()?.({ exitCode: 0, stderr: '' });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(launches, 2);
  finishes.shift()?.({ exitCode: 0, stderr: '' });
  await new Promise(resolve => setImmediate(resolve));
});

for (const [exitCode, signal, spawnFailed, reason] of [[0, null, false, 'exited'], [7, null, false, 'exited'], [-1, 'SIGTERM', false, 'exited'], [-1, null, true, 'launch_failed']] as const) {
  test(`execution result preserves exit ${exitCode}, signal ${signal}, launch failure ${spawnFailed}`, async () => {
    const published: ServerMessage[] = [];
    const manager = createSessionManager({ disableCleanupTimer: true, providerRegistry: { isProviderAvailable: () => true },
      pushBus: { publish: message => published.push(message) },
      dispatchRunner: () => ({ promise: Promise.resolve({ exitCode, signal, spawnFailed, stderr: 'fixture diagnostic' }), cancel: async () => ({ cancelled: false }) })
    });
    const session = await manager.startSession({ profile: 'fixture', providerKind: 'github', instanceId: 'github-0', repo: 'fixture/repo', mode: 'fix' });
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(session.execution, { reason, exitCode: exitCode < 0 ? null : exitCode, signal });
    assert.ok(session.endedAt);
    assert.equal(session.status, exitCode === 0 ? 'stopped' : 'error');
    const terminal = published.find(message => message.type === 'session.stopped');
    assert.ok(terminal && terminal.type === 'session.stopped');
    assert.deepEqual(terminal.session.execution, session.execution);
    assert.deepEqual(manager.getAllSessions()[0]?.execution, session.execution);
  });
}
