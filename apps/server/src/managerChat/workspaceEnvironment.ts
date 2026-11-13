import { execFile } from 'node:child_process';
import { findGahBinary, getSpawnOptions } from '../gahCli.js';
import type { ManagerConversation } from './registry.js';

/** Private CLI pipe, never a dashboard endpoint or durable cache. Rust owns
 * file parsing and the exclusion of provider/control-plane credentials. */
export function workspaceEnvironment({ profile }: ManagerConversation, signal?: AbortSignal): Promise<Record<string, string>> {
  return new Promise((resolve, reject) => {
    const fail = () => reject(new Error('Cannot load the workspace environment. Check the development file on the execution node and update its Parasaur CLI.'));
    const options = getSpawnOptions();
    execFile(findGahBinary(), ['config', 'show-chat-environment', '--profile', profile], {
      cwd: options.cwd, env: options.env, timeout: 10_000, maxBuffer: 256 * 1024, encoding: 'utf8', signal, windowsHide: true,
    }, (error, stdout) => {
      if (error) { fail(); return; }
      try {
        const value: unknown = JSON.parse(stdout);
        if (!value || typeof value !== 'object' || Array.isArray(value)
          || Object.entries(value).some(([name, value]) => !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) || typeof value !== 'string' || value.includes('\0'))) {
          fail(); return;
        }
        resolve(value as Record<string, string>);
      } catch { fail(); }
    });
  });
}
