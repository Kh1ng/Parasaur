#!/usr/bin/env node

import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const args = process.argv.slice(2);
const command = args[0];
const commandArgs = args.slice(1);

// npm supplies its CLI path on every platform. Spawning `npm` directly fails
// on Windows, where the PATH entry is a .cmd shim rather than an executable.
function runScript(workspace, script, args = [], env = {}) {
  if (!process.env.npm_execpath) {
    console.error('Start this launcher with npm run dev (or another npm run dev:* command).');
    process.exit(1);
  }
  const child = spawn(process.execPath, [process.env.npm_execpath, 'run', script, ...(args.length ? ['--', ...args] : [])], {
    stdio: 'inherit',
    cwd: path.resolve(__dirname, '..', 'apps', workspace),
    env: { ...process.env, ...env }
  });
  child.on('error', error => { console.error(`Cannot start ${workspace}: ${error.message}`); process.exitCode = 1; });
  return child;
}

function runPair(serverScript, serverArgs = [], webEnv = {}) {
  const children = [runScript('server', serverScript, serverArgs), runScript('web', 'dev', [], webEnv)];

  let stopping = false;
  let requestedShutdown = false;
  let remaining = children.length;
  let exitCode = 0;
  const stop = signal => {
    if (stopping) return;
    stopping = true;
    for (const child of children) {
      if (child.exitCode !== null || !child.pid) continue;
      if (process.platform === 'win32') {
        // Stop only this launcher's npm subtree, including Vite/tsx children.
        spawn('taskkill', ['/pid', String(child.pid), '/t', '/f'], { stdio: 'ignore' });
      } else child.kill(signal);
    }
  };
  for (const signal of ['SIGINT', 'SIGTERM']) {
    process.on(signal, () => {
      requestedShutdown = true;
      stop(signal);
    });
  }
  for (const child of children) {
    child.on('close', code => {
      remaining -= 1;
      if (!stopping && !requestedShutdown) exitCode = code === 0 ? 0 : code && code > 0 ? code : 1;
      stop('SIGTERM');
      if (remaining === 0) process.exitCode = requestedShutdown ? 0 : exitCode;
    });
  }
}

const scripts = {
  'dev': () => {
    runPair('dev');
  },
  'dev:mock': () => {
    const portIndex = commandArgs.indexOf('--port');
    const port = portIndex < 0 ? 3774 : Number(commandArgs[portIndex + 1]);
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      console.error('Choose a mock port between 1 and 65535 with --port.');
      process.exitCode = 1;
      return;
    }
    runPair('dev:mock', portIndex < 0 ? ['--port', String(port), ...commandArgs] : commandArgs, {
      VITE_PROXY_TARGET: `http://127.0.0.1:${port}`,
      VITE_WS_PROXY_TARGET: `ws://127.0.0.1:${port}`
    });
  },
  'dev:server': () => {
    runScript('server', 'dev', commandArgs).on('close', code => { process.exitCode = code ?? 1; });
  },
  'dev:web': () => {
    runScript('web', 'dev', commandArgs).on('close', code => { process.exitCode = code ?? 1; });
  },
  'dev:desktop': () => {
    runScript('desktop', 'start', commandArgs).on('close', code => { process.exitCode = code ?? 1; });
  }
};

if (scripts[command]) {
  scripts[command]();
} else {
  console.error(`Unknown command: ${command}`);
  console.log('Available commands: dev, dev:mock, dev:server, dev:web, dev:desktop');
  process.exit(1);
}
