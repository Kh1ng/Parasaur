import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
// js-yaml is already installed by `npm ci` (a dev dependency of the web
// workspace's test tooling); JSON_SCHEMA loads plain data only.
import yaml from 'js-yaml';

const workflow = yaml.load(
  readFileSync(new URL('../.github/workflows/release-edge.yml', import.meta.url), 'utf8'),
  { schema: yaml.JSON_SCHEMA },
);
const jobs = workflow.jobs;
const GATE = 'trusted-source';
const REPO = 'Kh1ng/git-agent-harness';

// A strict evaluator for exactly the expression grammar release-edge.yml
// uses: an optional `${{ }}` wrapper, `||`, `&&`, `!`, `==`, parentheses,
// single-quoted strings, dotted context paths and the job status functions.
// Any other token, a path missing from the context or a non-boolean operand
// throws, so a changed expression fails here instead of passing unchecked.
function tokenize(source) {
  let text = source.trim();
  if (text.startsWith('${{')) {
    if (!text.endsWith('}}')) throw new Error(`unterminated expression: ${source}`);
    text = text.slice(3, -2);
  }
  if (text.includes('${{') || text.includes('}}')) throw new Error(`nested expression: ${source}`);
  const tokens = [];
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    let match;
    if ((match = /^\s+/.exec(rest))) {
      i += match[0].length;
    } else if ((match = /^(\|\||&&|==|\(|\))/.exec(rest))) {
      tokens.push({ kind: match[0] });
      i += match[0].length;
    } else if (rest[0] === '!' && rest[1] !== '=') {
      tokens.push({ kind: '!' });
      i += 1;
    } else if ((match = /^'((?:[^']|'')*)'/.exec(rest))) {
      tokens.push({ kind: 'string', value: match[1].replaceAll("''", "'") });
      i += match[0].length;
    } else if ((match = /^(always|success|failure|cancelled)\(\)/.exec(rest))) {
      tokens.push({ kind: 'call', name: match[1] });
      i += match[0].length;
    } else if ((match = /^[A-Za-z_][A-Za-z0-9_-]*(\.[A-Za-z_][A-Za-z0-9_-]*)*(?![A-Za-z0-9_.(\[-])/.exec(rest))) {
      tokens.push({ kind: 'path', segments: match[0].split('.') });
      i += match[0].length;
    } else {
      throw new Error(`unsupported token at ${JSON.stringify(rest.slice(0, 20))} in ${source}`);
    }
  }
  return tokens;
}

function parse(source) {
  const tokens = tokenize(source);
  let pos = 0;
  const peek = () => tokens[pos]?.kind;
  const expect = kind => {
    if (peek() !== kind) throw new Error(`expected ${kind} at token ${pos} in ${source}`);
    return tokens[pos++];
  };
  const binary = (op, next) => () => {
    let left = next();
    while (peek() === op) {
      pos++;
      left = { op, left, right: next() };
    }
    return left;
  };
  const primary = () => {
    const token = tokens[pos];
    if (!token) throw new Error(`unexpected end of ${source}`);
    if (token.kind === '(') {
      pos++;
      const inner = or();
      expect(')');
      return inner;
    }
    if (token.kind === 'string' || token.kind === 'path' || token.kind === 'call') {
      pos++;
      return token;
    }
    throw new Error(`unexpected ${token.kind} at token ${pos} in ${source}`);
  };
  const comparison = () => {
    const left = primary();
    if (peek() !== '==') return left;
    pos++;
    return { op: '==', left, right: primary() };
  };
  const unary = () => {
    if (peek() !== '!') return comparison();
    pos++;
    return { op: '!', operand: unary() };
  };
  const and = binary('&&', unary);
  const or = binary('||', and);
  const ast = or();
  if (pos !== tokens.length) throw new Error(`trailing ${peek()} in ${source}`);
  return ast;
}

function evaluate(node, context, status = {}) {
  const bool = value => {
    if (typeof value !== 'boolean') throw new Error(`non-boolean operand ${JSON.stringify(value)}`);
    return value;
  };
  switch (node.op ?? node.kind) {
    case '||':
      return bool(evaluate(node.left, context, status)) || bool(evaluate(node.right, context, status));
    case '&&':
      return bool(evaluate(node.left, context, status)) && bool(evaluate(node.right, context, status));
    case '!':
      return !bool(evaluate(node.operand, context, status));
    case '==': {
      const left = evaluate(node.left, context, status);
      const right = evaluate(node.right, context, status);
      if (typeof left !== 'string' || typeof right !== 'string') {
        throw new Error(`== compares strings only: ${JSON.stringify(left)} == ${JSON.stringify(right)}`);
      }
      // GitHub compares strings case-insensitively.
      return left.toLowerCase() === right.toLowerCase();
    }
    case 'string':
      return node.value;
    case 'call':
      if (!(node.name in status)) throw new Error(`${node.name}() is not available here`);
      return status[node.name];
    case 'path':
      return node.segments.reduce((value, segment, index) => {
        if (value === null || typeof value !== 'object' || !Object.hasOwn(value, segment)) {
          throw new Error(`unknown context path ${node.segments.slice(0, index + 1).join('.')}`);
        }
        return value[segment];
      }, context);
    default:
      throw new Error(`unknown node ${JSON.stringify(node)}`);
  }
}

const usesStatusFunction = node =>
  node.kind === 'call' || [node.left, node.right, node.operand].some(child => child && usesStatusFunction(child));

const needsOf = name => [jobs[name].needs ?? []].flat();

// The gate's run step, executed as GitHub runs it on Linux, with the runner
// variables it reads and the event payload written to GITHUB_EVENT_PATH.
const gateStepPasses = (env, event) => {
  const [step, ...others] = jobs[GATE].steps;
  assert.equal(others.length, 0, 'trusted-source has a single step');
  assert.equal(step.uses, undefined, 'trusted-source loads no action');
  const dir = mkdtempSync(join(tmpdir(), 'edge-gate-'));
  const eventPath = join(dir, 'event.json');
  writeFileSync(eventPath, typeof event === 'string' ? event : JSON.stringify(event));
  const result = spawnSync('bash', ['--noprofile', '--norc', '-eo', 'pipefail', '-c', step.run], {
    env: { PATH: process.env.PATH, GITHUB_EVENT_PATH: eventPath, HEAD_SHA: 'sha', ...env },
    encoding: 'utf8',
  });
  rmSync(dir, { recursive: true });
  assert.equal(result.error, undefined);
  return result.status === 0;
};

const gateAllows = github =>
  evaluate(parse(jobs[GATE].if), { github }) &&
  gateStepPasses({ GITHUB_EVENT_NAME: github.event_name, GITHUB_REF: github.ref }, github.event);

const ciRun = overrides => ({
  event_name: 'workflow_run',
  ref: 'refs/heads/main',
  repository: REPO,
  event: {
    workflow_run: {
      conclusion: 'success',
      event: 'push',
      head_branch: 'main',
      head_sha: '0123456789abcdef0123456789abcdef01234567',
      head_repository: { full_name: REPO },
      ...overrides,
    },
  },
});
const dispatch = ref => ({ event_name: 'workflow_dispatch', ref, repository: REPO, event: {} });

test('the trust gate permits a successful same-repository main push and a manual dispatch on main', () => {
  assert.equal(gateAllows(ciRun({})), true);
  assert.equal(gateAllows(dispatch('refs/heads/main')), true);
});

test('the trust gate rejects pull requests, non-push events, failed runs and other branches', () => {
  const rejected = {
    'fork main PR': ciRun({ event: 'pull_request', head_repository: { full_name: 'someone/git-agent-harness' } }),
    'same-repository main PR': ciRun({ event: 'pull_request' }),
    'fork run reported as push': ciRun({ head_repository: { full_name: 'someone/git-agent-harness' } }),
    'pull_request_target run': ciRun({ event: 'pull_request_target' }),
    'manually dispatched CI run': ciRun({ event: 'workflow_dispatch' }),
    'scheduled CI run': ciRun({ event: 'schedule' }),
    'merge queue run': ciRun({ event: 'merge_group' }),
    'failed CI run': ciRun({ conclusion: 'failure' }),
    'cancelled CI run': ciRun({ conclusion: 'cancelled' }),
    'timed out CI run': ciRun({ conclusion: 'timed_out' }),
    'push to another branch': ciRun({ head_branch: 'feature' }),
    'manual dispatch on a branch': dispatch('refs/heads/feature'),
    'manual dispatch on a main-prefixed branch': dispatch('refs/heads/main-next'),
    'manual dispatch on a tag': dispatch('refs/tags/v0.3.0'),
    'push event': { ...dispatch('refs/heads/main'), event_name: 'push' },
    // Expression == ignores case; these are distinct branches.
    'manual dispatch on refs/heads/Main': dispatch('refs/heads/Main'),
    'manual dispatch on refs/heads/MAIN': dispatch('refs/heads/MAIN'),
    'manual dispatch on refs/heads/main with a trailing space': dispatch('refs/heads/main '),
    'manual dispatch on refs/heads/main with a trailing tab': dispatch('refs/heads/main\t'),
    'push to Main': ciRun({ head_branch: 'Main' }),
    'push to MAIN': ciRun({ head_branch: 'MAIN' }),
    'push to main with a trailing space': ciRun({ head_branch: 'main ' }),
    'push to main with a trailing tab': ciRun({ head_branch: 'main\t' }),
  };
  for (const [label, github] of Object.entries(rejected)) {
    assert.equal(gateAllows(github), false, label);
  }
});

test('the gate step fails closed on an unknown event or a malformed payload', () => {
  const main = { GITHUB_REF: 'refs/heads/main' };
  assert.equal(gateStepPasses({ ...main, GITHUB_EVENT_NAME: 'push' }, {}), false, 'unknown event');
  assert.equal(gateStepPasses({ ...main }, {}), false, 'missing event name');
  assert.equal(gateStepPasses({ GITHUB_EVENT_NAME: 'workflow_dispatch' }, {}), false, 'missing ref');
  const run = { ...main, GITHUB_EVENT_NAME: 'workflow_run' };
  assert.equal(gateStepPasses(run, {}), false, 'missing workflow_run');
  assert.equal(gateStepPasses(run, { workflow_run: {} }), false, 'missing head_branch');
  assert.equal(gateStepPasses(run, { workflow_run: { head_branch: ['main'] } }), false, 'non-string head_branch');
  assert.equal(gateStepPasses(run, '{"workflow_run":'), false, 'malformed payload');
});

test('the evaluator rejects expression syntax it does not model', () => {
  for (const source of [
    "${{ github.ref != 'refs/heads/main' }}",
    "${{ contains(github.ref, 'main') }}",
    "${{ startsWith(github.ref, 'refs/heads/') }}",
    "${{ github.event['workflow_run'] }}",
    '${{ github.run_number == 1 }}',
    "${{ github.ref == \"refs/heads/main\" }}",
  ]) {
    assert.throws(() => parse(source), /unsupported token/, source);
  }
  assert.throws(() => evaluate(parse("github.missing == 'x'"), { github: dispatch('refs/heads/main') }), /unknown context path/);
});

test('every other job needs the trust gate, directly or transitively', () => {
  assert.ok(jobs[GATE], 'trusted-source job exists');
  assert.deepEqual(needsOf(GATE), []);
  const closure = (name, seen = new Set()) => {
    for (const need of needsOf(name)) {
      assert.ok(jobs[need], `${name} needs unknown job ${need}`);
      if (!seen.has(need)) closure(need, seen.add(need));
    }
    return seen;
  };
  for (const name of Object.keys(jobs).filter(name => name !== GATE)) {
    assert.ok(closure(name).has(GATE), `${name} does not depend on ${GATE}`);
  }
});

test('when the trust gate is skipped, every other job is skipped', () => {
  // A status function such as always() or !cancelled() lets a job run after
  // a skipped need, so `needs` alone does not prove the gate holds. Simulate
  // GitHub's job selection with the gate skipped and with it passing.
  const simulate = gateResult => {
    const results = { [GATE]: gateResult };
    const outputs = { 'updater-key-gate': { has_key: 'true' } };
    const pending = Object.keys(jobs).filter(name => name !== GATE);
    while (pending.length) {
      const index = pending.findIndex(name => needsOf(name).every(need => need in results));
      assert.notEqual(index, -1, `dependency cycle among ${pending.join(', ')}`);
      const [name] = pending.splice(index, 1);
      const needs = Object.fromEntries(needsOf(name).map(need => [need, { result: results[need], outputs: outputs[need] ?? {} }]));
      const allSucceeded = needsOf(name).every(need => results[need] === 'success');
      const status = {
        success: allSucceeded,
        always: true,
        cancelled: false,
        failure: needsOf(name).some(need => results[need] === 'failure'),
      };
      const condition = jobs[name].if === undefined ? null : parse(String(jobs[name].if));
      const runs = condition && usesStatusFunction(condition)
        ? evaluate(condition, { needs }, status)
        : allSucceeded && (!condition || evaluate(condition, { needs }, status));
      results[name] = runs ? 'success' : 'skipped';
    }
    return results;
  };
  for (const [name, result] of Object.entries(simulate('skipped'))) {
    if (name !== GATE) assert.equal(result, 'skipped', `${name} runs without the trust gate`);
  }
  assert.equal(simulate('success').publish, 'success', 'publish runs once the gate passes');
});

test('only the publish job can write repository contents', () => {
  assert.deepEqual(workflow.permissions, { contents: 'read' });
  for (const [name, job] of Object.entries(jobs)) {
    if (name === 'publish') {
      assert.deepEqual(job.permissions, { contents: 'write' });
      continue;
    }
    if (job.permissions === undefined) continue;
    assert.equal(typeof job.permissions, 'object', `${name} permissions must be a scope map`);
    for (const [scope, level] of Object.entries(job.permissions)) {
      assert.ok(level === 'read' || level === 'none', `${name} grants ${scope}: ${level}`);
    }
  }
});
