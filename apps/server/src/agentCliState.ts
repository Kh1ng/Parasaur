import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import type { AgentCliUpdate } from '@git-agent-harness/contracts';

export const AGENT_BACKENDS = ['claude', 'codex', 'opencode', 'vibe', 'agy', 'openhands', 'hermes'] as const;

/** Read the Rust ledger after a server restart too. A vanished updater is failed,
 * never inferred successful: provider updates do not restart this server. */
export function readAgentUpdates(): AgentCliUpdate[] {
  const directory = join(process.env.XDG_STATE_HOME ?? join(homedir(), '.local/state'), 'gah/agent-cli-updates');
  return AGENT_BACKENDS.flatMap(backend => {
    try {
      const record = JSON.parse(readFileSync(join(directory, `${backend}.json`), 'utf8'));
      if (record.backend !== backend || !['running', 'success', 'failed'].includes(record.status)
        || typeof record.started_at !== 'string') throw new Error('Invalid update record');
      if (record.status === 'running') {
        let alive = false;
        if (Number.isSafeInteger(record.pid) && record.pid > 0) {
          try { process.kill(record.pid, 0); alive = true; } catch { /* dead */ }
        }
        if (!alive) return [{ ...record, status: 'failed', error: 'Updater exited without recording a result.' }];
      }
      return [record as AgentCliUpdate];
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return [];
      // Unreadable state must prevent a second updater from being launched.
      throw new Error('Cannot read agent update state.');
    }
  });
}
