/**
 * Headless (Tier B) manager-chat adapter (slice 4).
 *
 * For backends with no ACP or equivalent structured protocol (vibe, agy):
 * each turn is ONE non-interactive CLI invocation (`--print`-
 * style). Conversation context is reconstructed textually per turn — the
 * same replay approach the ACP path uses when a backend forgets its
 * history (see resumePrompt in acpAdapter.ts): full transcript replay for
 * a fresh process, since a one-shot process has no memory at all.
 *
 * The replayed prompt (full history + new message) is delivered to the
 * child process over stdin, never argv (issue #1009): a long-running
 * session's replayed transcript easily exceeds the OS ARG_MAX when passed
 * as a command-line argument, and spawn(2) fails with E2BIG before the
 * backend even starts. Argv stays a small, fixed set of flags regardless
 * of conversation length.
 *
 * What this buys: every backend in the unified chat surface, session
 * worktree binding (cwd per conversation), backend interchange, quota
 * handoff, and the event-sourced log — everything except streaming, native
 * slash commands, and the permission round-trip, which need a structured
 * protocol. AGY's CLI-native model/effort options are exposed, Vibe's
 * config-native model selection is exposed through the gah CLI (#1510), and
 * OpenHands lists its profile provider's models with the configured one
 * marked (#1512).
 */

import { spawn, execFile as execFileCallback, execFileSync } from 'node:child_process';
import { readFileSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import type { ChatTranscriptTurn, ChatUsage } from '@git-agent-harness/contracts';
import { runConfigShowProfile, runVibeModelSummary, runVibeSetModel } from '../gahCli.js';
import { boundedUpstreamFetch } from '../cliRouter.js';
import type { ManagerConversation, ManagerAdapter, ManagerCommandInfo, ManagerModelInfo, ManagerReasoningEffortInfo } from './registry.js';
import { isInsideWorkingDirectory } from './permissionPolicy.js';
import { createAgyModelOptions } from './agyModels.js';
import { agentChildEnv } from './childEnv.js';

export type { ManagerCommandInfo, ManagerModelInfo };

const execFile = promisify(execFileCallback);

export interface HeadlessSpawnSpec {
  command: string;
  args: string[];
  /** Env for the child process. */
  env?: Record<string, string>;
}

/** A backend tool request decoded out of reply text (#1041). */
export interface HeadlessToolRequest {
  name: string;
  args: Record<string, unknown>;
  /** The exact reply the request was decoded from, replayed back to the
   * backend so its own context stays intact across the continuation. */
  raw: string;
}

interface HeadlessProcessResult {
  stdout: string;
  stderr: string;
  exitCode: number | null;
}

/** Builds the CLI argv for one turn: cwd is passed by the engine; the spec
 * adds backend-specific flags. Bounded and content-free — never derived
 * from the prompt or history, so argv size cannot grow with conversation
 * length (issue #1009). */
export interface HeadlessBackendSpec {
  id: string;
  displayName: string;
  /** Release metadata refresh timers when an installation is replaced. */
  dispose?: () => void;
  modelCatalogVersion?: ManagerAdapter['modelCatalogVersion'];
  /** Argv for a print-mode turn. Must not embed prompt/history -- only the
   * session's pinned model/reasoning-effort (#1032 reopened), which are
   * bounded ids, not conversation content. A spec that has no native flag
   * for one or both (e.g. vibe) may ignore the argument entirely. */
  turnArgs: (opts?: { model?: string | null; reasoningEffort?: string | null }) => string[];
  /** Encode this turn's full prompt (history already replayed in) for the
   * backend's stdin channel. */
  encodeStdin: (prompt: string) => string;
  /** Extra environment resolved for this GAH profile and this turn's pinned
   * model, for backends whose model selection is an env the child reads
   * (OpenHands' LLM_MODEL) rather than a CLI flag. Secret values stay in
   * the server process and child environment; they never enter argv or
   * API responses. */
  spawnEnv?: (conversation: ManagerConversation, opts?: { model?: string | null }) => Promise<Record<string, string>>;
  /** Extract the reply text from a finished process, or throw a
   * descriptive error. Default: trimmed stdout on exit 0, else an error
   * built from stderr. */
  parseReply?: (result: HeadlessProcessResult) => string;
  /** Selectable options resolved using the caller's real GAH profile;
   * the opaque key identifies conversation state only. */
  modelOptions?: (conversation: ManagerConversation) => Promise<{
    models: ManagerModelInfo[];
    reasoningEfforts: ManagerReasoningEffortInfo[];
    modelReasoningEfforts?: Record<string, ManagerReasoningEffortInfo[]>;
    /** The backend's own current model, for backends whose selection lives
     * outside this process (vibe's config file, OpenHands' configured
     * profile model); an in-memory selection still wins when one was made
     * here. */
    currentModelId?: string | null;
  }>;
  /** Persist a model selection for backends with no CLI flag to carry it
   * (vibe writes `active_model` into its own config). Called by setModel
   * after validation, and before a turn whose session pinned a model.
   * Uses the caller's real GAH profile for the config write. */
  applyModel?: (conversation: ManagerConversation, modelId: string) => Promise<void>;
  /** Identifies the installation-global resource `applyModel` persists to
   * (vibe's config home — the one config file every conversation using
   * that Vibe installation, ambient or instance, shares). When present,
   * the adapter serializes pinned turns and model writes sharing a scope
   * on a per-scope mutex; see withModelSelectionLock. */
  modelSelectionScope?: () => string;
  /** Detect a tool request the backend leaked into the reply text instead
   * of executing (#1041). Returns null for an ordinary reply; throws when
   * tool-request-shaped text cannot be decoded, so the turn ends with an
   * actionable error instead of a false-success reply. */
  decodeToolRequest?: (reply: string) => HeadlessToolRequest | null;
}

interface ConversationState {
  knownHistory: ChatTranscriptTurn[];
  /** Cancellation belongs to a turn, including its wait for a model lock. */
  activeTurn: { controller: AbortController; child: ReturnType<typeof spawn> | null } | null;
  currentModelId: string | null;
  currentReasoningEffortId: string | null;
}

/** Mirrors acpAdapter's resumePrompt: fresh process => replay the whole
 * conversation as text before the new prompt. */
function replayPrompt(message: string, history: ChatTranscriptTurn[]): string {
  if (history.length === 0) return message;
  const lines = history.map((turn) => `${turn.role}: ${turn.text}`);
  return `${lines.join('\n')}\n\nuser: ${message}`;
}

/** The continuation prompt after servicing a decoded tool request (#1041):
 * the same roleful replay format with the backend's own request and the
 * tool result appended, so the model continues from the result. */
function continuationPrompt(message: string, history: ChatTranscriptTurn[], exchange: ChatTranscriptTurn[]): string {
  const lines = history.map((turn) => `${turn.role}: ${turn.text}`);
  const prefix = lines.length > 0 ? `${lines.join('\n')}\n\n` : '';
  return `${prefix}user: ${message}\n${exchange.map((turn) => `${turn.role}: ${turn.text}`).join('\n')}`;
}

function defaultParseReply(displayName: string): (result: HeadlessProcessResult) => string {
  return ({ stdout, stderr, exitCode }) => {
    if (exitCode !== 0) {
      const detail = stderr.trim().slice(0, 400) || `exit code ${exitCode}`;
      throw new Error(`${displayName} turn failed: ${detail}`);
    }
    return stdout.trim();
  };
}

const TURN_TIMEOUT_MS = 10 * 60_000;

/** Per-scope async mutexes (#1510 round 2). Vibe's `active_model` is
 * installation-global — one config file per Vibe home shared by every
 * conversation using that installation — so two concurrent pinned turns
 * used to race: turn B's pin overwrote turn A's before A's child read
 * the config, and both ran B's model. A pinned turn now holds its
 * installation's mutex from applyModel through turn completion (tool
 * continuations included: they spawn under the same lock), and setModel
 * takes the mutex for its write, so no other writer can move the
 * selection under a turn that already pinned one. Unpinned turns stay
 * lock-free: they read whatever the installation's active model is at
 * spawn time, which is Vibe's own semantics. Keyed by the config home
 * (ambient or an instance's isolated state root), so distinct
 * installations never serialize against each other. */
const modelSelectionLocks = new Map<string, Promise<unknown>>();

function withModelSelectionLock<T>(scope: string, action: () => Promise<T>): Promise<T> {
  const prior = modelSelectionLocks.get(scope) ?? Promise.resolve();
  const run = prior.then(action);
  // The stored tail never rejects, so one failed turn cannot poison the
  // chain for the next waiter.
  modelSelectionLocks.set(scope, run.then(() => undefined, () => undefined));
  return run;
}

export function createHeadlessBackend(spec: HeadlessBackendSpec): ManagerAdapter {
  const states = new Map<string, ConversationState>();
  const parseReply = spec.parseReply ?? defaultParseReply(spec.displayName);

  function stateFor(key: string): ConversationState {
    let state = states.get(key);
    if (!state) {
      state = { knownHistory: [], activeTurn: null, currentModelId: null, currentReasoningEffortId: null };
      states.set(key, state);
    }
    return state;
  }

  return {
    id: spec.id,
    displayName: spec.displayName,
    implemented: true,

    dispose(): void {
      // Retire the adapter without interrupting a turn already using its key.
      states.clear();
      spec.dispose?.();
    },

    async runTurn(conversation, input) {
      const state = stateFor(conversation.key);
      const cwd = input.cwd ?? process.cwd();
      const activeTurn: NonNullable<ConversationState['activeTurn']> = { controller: new AbortController(), child: null };
      state.activeTurn = activeTurn;
      const { signal } = activeTurn.controller;
      const cancelled = new Promise<never>((_resolve, reject) => {
        signal.addEventListener('abort', () => reject(signal.reason), { once: true });
      });

      // The turn itself. When the session pinned a model and the backend
      // persists pins installation-globally (vibe), the whole turn runs
      // under that installation's mutex so no other writer can move the
      // selection between this pin's applyModel and the child's config
      // read — including the extra invocations tool continuations spawn.
      const turn = async (): Promise<{ reply: string; model: string | null; usage: ChatUsage | null }> => {
        signal.throwIfAborted();
        // A backend with no CLI flag for the pin (vibe) persists the
        // selection through its own config before the turn, so the process
        // it is about to spawn picks the pinned model up (#1510). Only an
        // explicit pin writes; the in-memory fallback was already persisted
        // by setModel.
        if (input.model && spec.applyModel) {
          await spec.applyModel(conversation, input.model);
        }

        // One non-interactive invocation: fixed argv, prompt over stdin.
        const invoke = async (prompt: string): Promise<string> => {
          signal.throwIfAborted();
          // The same resolved pin feeds argv (agy's --model) and the child
          // env (OpenHands' LLM_MODEL) so every backend honors it the one
          // way it supports (#1512).
          const pinnedModel = input.model === undefined ? state.currentModelId : input.model;
          const args = spec.turnArgs({
            model: pinnedModel,
            reasoningEffort: input.reasoningEffort === undefined ? state.currentReasoningEffortId : input.reasoningEffort
          });
          const env = agentChildEnv(await spec.spawnEnv?.(conversation, { model: pinnedModel }));
          signal.throwIfAborted();
          const child = spawn(args[0], args.slice(1), {
            cwd,
            env,
            stdio: ['pipe', 'pipe', 'pipe']
          });
          activeTurn.child = child;

          let stdout = '';
          let stderr = '';
          child.stdout.on('data', (chunk) => { stdout += chunk; });
          child.stderr.on('data', (chunk) => { stderr += chunk; });
          // The child may exit (e.g. a fast validation failure) before we
          // finish writing; a write past that point would otherwise raise an
          // unhandled EPIPE and crash the server.
          child.stdin.on('error', () => {});
          child.stdin.write(spec.encodeStdin(prompt));
          child.stdin.end();

          const killTimer = setTimeout(() => {
            child.kill('SIGKILL');
          }, TURN_TIMEOUT_MS);
          killTimer.unref?.();

          try {
            const code = await new Promise<number | null>((resolve, reject) => {
              child.on('error', reject);
              child.on('close', (exitCode) => resolve(exitCode));
            });
            signal.throwIfAborted();
            const reply = parseReply({ stdout, stderr, exitCode: code });
            if (reply.trim().length === 0) {
              throw new Error(`${spec.displayName} turn produced no output.`);
            }
            return reply;
          } finally {
            clearTimeout(killTimer);
            activeTurn.child = null;
          }
        };

        // A headless process has no memory: every turn replays the full
        // conversation. (historyDelta-style catch-up is meaningless here, but
        // keeping knownHistory lets future stream-json modes upgrade in place.)
        let prompt = replayPrompt(input.prompt, input.history);
        let reply = await invoke(prompt);

        // #1041: a print-mode backend whose model emits a tool call the CLI
        // didn't execute leaks the raw request syntax as reply text, which
        // used to surface verbatim as a successful assistant reply. Decode
        // it, gate it through the same permission round-trip the ACP backends
        // use, execute the servable read-only subset GAH-side, then continue
        // the turn with the tool result replayed into the next invocation.
        // Undecodable or unknown requests fail the turn with an actionable
        // error instead.
        const toolExchange: ChatTranscriptTurn[] = [];
        if (spec.decodeToolRequest) {
          for (let round = 1; ; round += 1) {
            const request = spec.decodeToolRequest(reply);
            if (!request) break;
            if (round > MAX_TOOL_ROUNDS_PER_TURN) {
              throw new Error(`${spec.displayName} requested more than ${MAX_TOOL_ROUNDS_PER_TURN} tool calls in a single turn; stopping so raw tool syntax can never surface as a reply.`);
            }
            const plan = planHeadlessTool(spec.displayName, request.name, request.args, cwd);
            const toolCallId = `${spec.id}-tool-${Date.now().toString(36)}-${round}`;
            const emitToolCall = (status: 'pending' | 'completed' | 'failed', summary: string | null) =>
              input.onToolCall?.({
                toolCallId,
                name: request.name,
                title: plan.title,
                kind: plan.kind,
                status,
                locations: plan.locations,
                summary
              });
            emitToolCall('pending', null);
            let decision: string;
            if (input.requestPermission) {
              decision = await Promise.race([input.requestPermission({
                title: plan.title,
                options: [
                  { optionId: 'allow-once', name: 'Allow', kind: 'allow_once' },
                  { optionId: 'reject-once', name: 'Reject', kind: 'reject_once' }
                ],
                locations: plan.locations,
                kind: plan.kind,
                rawInput: request.args
              }), cancelled]);
            } else {
              console.warn(`[managerChat] ${spec.displayName} requested permission for "${plan.title}" -- declining (no permission UI attached)`);
              decision = 'cancelled';
            }
            signal.throwIfAborted();
            if (decision !== 'allow-once') {
              const outcome = decision === 'cancelled' ? 'cancelled' : `declined (${decision})`;
              const message = `${spec.displayName} requested "${plan.title}" but the permission request was ${outcome}; the turn cannot continue without it.`;
              emitToolCall('failed', message);
              throw new Error(message);
            }
            let output: string;
            try {
              output = await plan.run();
            } catch (error) {
              const message = error instanceof Error ? error.message : String(error);
              emitToolCall('failed', summarizeOutput(message));
              throw error;
            }
            emitToolCall('completed', summarizeOutput(output));
            input.onToolResult(request.name, output);
            toolExchange.push(
              { role: 'assistant', text: request.raw, timestamp: Date.now() },
              { role: 'tool', text: output, timestamp: Date.now() }
            );
            prompt = continuationPrompt(input.prompt, input.history, toolExchange);
            reply = await invoke(prompt);
          }
        }

        signal.throwIfAborted();
        state.knownHistory = [
          ...input.history,
          { role: 'user', text: input.prompt, timestamp: Date.now() },
          ...toolExchange,
          { role: 'assistant', text: reply, timestamp: Date.now() }
        ];
        return { reply, model: null, usage: null };
      };

      // Vibe's `active_model` is installation-global: one config file is
      // shared by every conversation using that Vibe installation
      // (ambient home or one instance's isolated state root). Two
      // concurrent pinned turns would otherwise race — each turn's
      // applyModel overwrites the other's before the children read the
      // config, and both run the second pin's model — so a pinned turn
      // holds its installation's mutex (see withModelSelectionLock)
      // from applyModel through turn completion, and setModel's write
      // waits behind it too. This lock is the direct consequence of
      // Vibe's config-driven, per-installation model selection.
      const scope = input.model && spec.applyModel ? spec.modelSelectionScope?.() : undefined;
      try {
        // Settle the caller immediately, but keep the lock until the action
        // unwinds so an in-progress config write cannot race the next turn.
        return await Promise.race([scope ? withModelSelectionLock(scope, turn) : turn(), cancelled]);
      } finally {
        if (state.activeTurn === activeTurn) state.activeTurn = null;
      }
    },

    async listCommands(): Promise<ManagerCommandInfo[]> {
      return []; // no native slash commands over a one-shot pipe
    },

    modelCatalogVersion: spec.modelCatalogVersion,
    async listModels(conversation) {
      const options = await spec.modelOptions?.(conversation);
      const state = stateFor(conversation.key);
      if (options?.modelReasoningEfforts && state.currentModelId && !options.models.some(model => model.id === state.currentModelId)) {
        state.currentModelId = null;
      }
      const efforts = options?.modelReasoningEfforts
        ? options.modelReasoningEfforts[state.currentModelId ?? options.currentModelId ?? ''] ?? []
        : options?.reasoningEfforts ?? [];
      if (options?.modelReasoningEfforts && !efforts.some(effort => effort.id === state.currentReasoningEffortId)) {
        state.currentReasoningEffortId = null;
      }
      return {
        models: options?.models ?? [],
        currentModelId: state.currentModelId ?? options?.currentModelId ?? null,
        reasoningEfforts: efforts,
        currentReasoningEffortId: state.currentReasoningEffortId,
        contextUsage: null
      };
    },

    async setModel(conversation, modelId): Promise<void> {
      const options = await spec.modelOptions?.(conversation);
      if (!options) throw new Error(`${spec.displayName} doesn't support model selection in headless mode.`);
      if (!options.models.some((model) => model.id === modelId)) throw new Error(`Unknown model "${modelId}" for ${spec.displayName}`);
      // The write takes the same per-installation mutex as a pinned turn:
      // landing between another turn's applyModel and its child's config
      // read (or a tool continuation's spawn) would make that turn run
      // this write's model instead of its own pin.
      const write = async () => { await spec.applyModel?.(conversation, modelId); };
      const scope = spec.applyModel ? spec.modelSelectionScope?.() : undefined;
      if (scope) {
        await withModelSelectionLock(scope, write);
      } else {
        await write();
      }
      const state = stateFor(conversation.key);
      state.currentModelId = modelId;
      if (options.modelReasoningEfforts && !options.modelReasoningEfforts[modelId]?.some(effort => effort.id === state.currentReasoningEffortId)) {
        state.currentReasoningEffortId = null;
      }
    },

    async setReasoningEffort(conversation, effortId): Promise<void> {
      const options = await spec.modelOptions?.(conversation);
      if (!options) throw new Error(`${spec.displayName} doesn't support reasoning-effort selection in headless mode.`);
      const efforts = options.modelReasoningEfforts
        ? options.modelReasoningEfforts[stateFor(conversation.key).currentModelId ?? options.currentModelId ?? ''] ?? []
        : options.reasoningEfforts;
      if (!efforts.some((effort) => effort.id === effortId)) throw new Error(`Unknown reasoning effort "${effortId}" for ${spec.displayName}`);
      stateFor(conversation.key).currentReasoningEffortId = effortId;
    },

    async steerTurn(): Promise<{ outcome: 'injected' }> {
      throw new Error(`${spec.displayName} doesn't support mid-turn steering in headless mode.`);
    },

    async cancelTurn(gahProfile): Promise<void> {
      const activeTurn = states.get(gahProfile)?.activeTurn;
      if (!activeTurn) return;
      activeTurn.controller.abort(new Error('Turn cancelled'));
      const child = activeTurn.child;
      if (child) {
        child.kill('SIGTERM');
        setTimeout(() => child.kill('SIGKILL'), 3000).unref?.();
      }
    }
  };
}

/** Fixed `-c` bootstrap for vibe's own Python interpreter: reads the prompt
 * from stdin, sets it into *in-process* `sys.argv`, then calls the same
 * `vibe.cli.entrypoint.main` the real launcher calls. Argv is assigned after
 * the interpreter is already running, so it never goes through execve() and
 * stays outside ARG_MAX (issue #1009).
 *
 * Also sidesteps a bug in vibe's own `get_prompt_from_stdin()`
 * (vibe/cli/cli.py): it reads a piped prompt, then reopens `/dev/tty` to
 * restore interactive stdin, which raises OSError with no controlling
 * terminal (as under this server) and discards the prompt it just read. This
 * bridge drains stdin first, so that call sees EOF and returns None — our
 * sys.argv prompt wins instead. Confirmed against the installed CLI. */
const VIBE_STDIN_BRIDGE = [
  'import sys',
  'prompt = sys.stdin.buffer.read().decode("utf-8", "replace")',
  'sys.argv = ["vibe", "-p", prompt, "--output", "text"]',
  'from vibe.cli.entrypoint import main',
  'sys.exit(main())'
].join('\n');

/** Resolve the Python interpreter backing the installed `vibe` launcher
 * from its own shebang, rather than hard-coding a host-specific path. Not
 * cached: it's one cheap `command -v` + file read per turn, and caching
 * would make the resolved interpreter outlive a test's fake PATH. */
function resolveVibeInterpreter(executable?: string): string {
  const launcherPath = executable ?? execFileSync('/bin/sh', ['-c', 'command -v vibe'], { encoding: 'utf8' }).trim();
  if (!launcherPath) {
    throw new Error('vibe executable not found on PATH.');
  }
  const realLauncher = realpathSync(launcherPath);
  const shebang = readFileSync(realLauncher, 'utf8').split('\n', 1)[0];
  const match = /^#!(\S+)/.exec(shebang);
  if (!match) {
    throw new Error(`Could not determine vibe's Python interpreter from ${realLauncher}.`);
  }
  if (!path.isAbsolute(match[1]) || !/^python/.test(path.basename(match[1]))) {
    throw new Error('Vibe requires an absolute Python interpreter in its launcher shebang.');
  }
  return match[1];
}

/** vibe: Mistral's CLI, print mode, driven through the stdin bridge above
 * so the replayed transcript never reaches process argv.
 * `resolveInterpreter` is overridable so tests can prove the argv/stdin
 * split against a fake interpreter instead of requiring a real vibe
 * install.
 *
 * Vibe's CLI has no model flag either: its config file holds the offered
 * models and the active one. The picker reads both through the gah CLI
 * (`config show-vibe-models`, no network), and choosing a model persists
 * `active_model` there (`config set-vibe-model`) — scoped to `instance`'s
 * isolated state when a backend instance is selected (#1510). */
export function vibeBackendSpec(overrides: {
  resolveInterpreter?: () => string;
  executable?: string;
  instance?: string | null;
  /** The instance's isolated state root (runtime.state_root); the Vibe
   * config this spec reads and writes lives at `stateRoot/.vibe`. */
  stateRoot?: string | null;
} = {}): HeadlessBackendSpec {
  const resolveInterpreter = overrides.resolveInterpreter ?? (() => resolveVibeInterpreter(overrides.executable));
  return {
    id: 'vibe',
    displayName: 'Vibe',
    // A pinned model never reaches argv; it reaches vibe through its own
    // config via applyModel instead.
    turnArgs: () => [resolveInterpreter(), '-c', VIBE_STDIN_BRIDGE],
    // The scope is the config home itself, matching what a spawned Vibe
    // child resolves: an instance's isolated `state_root/.vibe`, else the
    // ambient `VIBE_HOME`/`HOME/.vibe`. Keying by the config path (not the
    // instance name) means two instances sharing a state root also share
    // the mutex, and the ambient installation shares one mutex across all
    // GAH profiles that use it.
    modelSelectionScope: () =>
      overrides.stateRoot
        ? path.join(overrides.stateRoot, '.vibe')
        : process.env.VIBE_HOME ?? path.join(process.env.HOME ?? '', '.vibe'),
    modelOptions: async ({ profile }) => {
      const summary = await runVibeModelSummary(profile, overrides.instance);
      return {
        models: summary.models,
        reasoningEfforts: [],
        currentModelId: summary.active_model
      };
    },
    applyModel: ({ profile }, modelId) => runVibeSetModel(profile, overrides.instance, modelId),
    encodeStdin: (prompt) => prompt,
    decodeToolRequest: decodeVibeToolRequest
  };
}

/** The parsed contents of ~/.openhands/profiles/<name>.json. Fields stay
 * null when absent so env resolution can require them while the model
 * listing (#1512) can proceed with only the configured model. */
interface OpenhandsProfile {
  name: string;
  baseUrl: string | null;
  apiKey: string | null;
  model: string | null;
}

/** Read the OpenHands profile this GAH profile is bound to (oh_profile).
 * Returns null when the GAH profile binds none; throws on a malformed
 * name or profile file, exactly as a spawned turn would. */
async function readOpenhandsProfile(gahProfile: string): Promise<OpenhandsProfile | null> {
  const { oh_profile: profile } = await runConfigShowProfile(gahProfile);
  if (!profile) return null;
  if (!/^[A-Za-z0-9_-][A-Za-z0-9._-]*$/.test(profile)) {
    throw new Error(`Invalid OpenHands profile name "${profile}".`);
  }
  const profilePath = path.join(process.env.HOME ?? '/root', '.openhands', 'profiles', `${profile}.json`);
  let parsed: unknown;
  try {
    parsed = JSON.parse(readFileSync(profilePath, 'utf8'));
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`Could not load OpenHands profile "${profile}": ${detail}`);
  }
  if (!parsed || typeof parsed !== 'object') {
    throw new Error(`OpenHands profile "${profile}" is not a JSON object.`);
  }
  const value = parsed as Record<string, unknown>;
  const field = (key: 'base_url' | 'api_key' | 'model'): string | null => {
    const raw = value[key];
    return typeof raw === 'string' && raw.length > 0 ? raw : null;
  };
  return { name: profile, baseUrl: field('base_url'), apiKey: field('api_key'), model: field('model') };
}

async function resolveOpenhandsEnv(
  gahProfile: string,
  includeApiKey = true,
  /** The session's pinned model (#1512): OpenHands has no model flag, so
   * the pin travels as LLM_MODEL ahead of the ambient env and the profile. */
  pinnedModel?: string | null
): Promise<Record<string, string>> {
  const config = await readOpenhandsProfile(gahProfile);
  if (!config) return {};
  const required = (key: 'base_url' | 'api_key' | 'model'): string => {
    const field = key === 'base_url' ? config.baseUrl : key === 'api_key' ? config.apiKey : config.model;
    if (!field) {
      throw new Error(`OpenHands profile "${config.name}" is missing ${key}.`);
    }
    return field;
  };
  return {
    LLM_BASE_URL: process.env.LLM_BASE_URL ?? required('base_url'),
    ...(includeApiKey ? { LLM_API_KEY: process.env.LLM_API_KEY ?? required('api_key') } : {}),
    LLM_MODEL: pinnedModel ?? process.env.LLM_MODEL ?? required('model'),
    OPENHANDS_SUPPRESS_BANNER: '1'
  };
}

/** Picker deadline for a provider model listing (#1512): the listing is
 * optional, so it must never hold the picker hostage. */
const OPENHANDS_MODELS_TIMEOUT_MS = 5_000;

/** Last listing diagnostic logged per OpenHands profile, the same
 * once-per-transition shape wsServer uses for its events refresh
 * (#1598/#1616): the picker can poll listModels repeatedly, so the
 * SAME failure repeating stays quiet, a different failure logs and
 * replaces it, and a success clears the entry so a failure after
 * recovery logs again. */
const openhandsListingFailures = new Map<string, string>();

/** One sanitized diagnostic per listing failure (#1512): the OpenHands
 * profile and a status or error class only — never the URL, the API
 * key, the response body, or exception text or stack. */
function warnOpenhandsListingUnavailable(profile: string, detail: string): void {
  if (openhandsListingFailures.get(profile) === detail) return;
  openhandsListingFailures.set(profile, detail);
  console.warn(`[managerChat] openhands model listing unavailable for ${profile}: ${detail}`);
}

/** Ask the profile's LLM provider for its OpenAI-compatible /v1/models
 * inventory (#1512) — the same listing source the gah-router refresher
 * uses (cliRouter's bounded fetch: no redirects, size cap, short
 * deadline). OpenHands itself offers no inventory: its CLI has no models
 * command and its profile names one model. Best-effort by design: any
 * failure logs the single sanitized diagnostic above and returns an
 * empty list, and the picker falls back to the configured model. */
async function listOpenhandsModels(
  profile: string,
  baseUrl: string,
  apiKey: string,
  fetchFn?: typeof globalThis.fetch
): Promise<string[]> {
  const trimmed = baseUrl.replace(/\/+$/, '');
  // LiteLLM base URLs normally end in /v1 (the API root); profiles with a
  // bare origin get the conventional /v1 prefix appended.
  const url = trimmed.endsWith('/v1') ? `${trimmed}/models` : `${trimmed}/v1/models`;
  try {
    const result = await boundedUpstreamFetch({
      url,
      headers: { Authorization: `Bearer ${apiKey}` },
      timeoutMs: OPENHANDS_MODELS_TIMEOUT_MS,
      fetchFn
    });
    if (result.status !== 200) {
      warnOpenhandsListingUnavailable(profile, `http ${result.status}`);
      return [];
    }
    const body = result.body as { data?: Array<{ id?: unknown }> };
    if (!body?.data || !Array.isArray(body.data)) {
      warnOpenhandsListingUnavailable(profile, 'malformed response');
      return [];
    }
    const ids: string[] = [];
    for (const entry of body.data) {
      const id = entry?.id;
      if (typeof id === 'string' && id.length > 0 && id.length <= 200 && !/[\x00-\x1F\x7F]/.test(id)) ids.push(id);
    }
    openhandsListingFailures.delete(profile);
    return ids;
  } catch (error) {
    // The error CLASS only: an exception's message or stack can carry the
    // URL or upstream text, none of which may reach the log.
    const name = (error instanceof Error ? error.name : 'error').replace(/[\x00-\x1F\x7F]/g, '').slice(0, 60);
    warnOpenhandsListingUnavailable(profile, name || 'error');
    return [];
  }
}

/** OpenHands accepts a task file but not a raw stdin task. `/dev/stdin`
 * keeps the replayed transcript off argv while satisfying its headless CLI.
 * Model selection is env-driven (`--override-with-envs` + LLM_MODEL): the
 * picker lists the profile provider's models with the configured one
 * marked (#1512), and a chosen model reaches the child as LLM_MODEL. */
export function openhandsBackendSpec(
  overrides: {
    resolveEnv?: (conversation: ManagerConversation, opts?: { model?: string | null }) => Promise<Record<string, string>>;
    instanceCredential?: boolean;
    /** Test seam for the provider /v1/models listing; no real network. */
    fetchFn?: typeof globalThis.fetch;
  } = {}
): HeadlessBackendSpec {
  return {
    id: 'openhands',
    displayName: 'OpenHands',
    turnArgs: () => [
      'openhands',
      '--headless',
      '--json',
      '--file',
      '/dev/stdin',
      '--exit-without-confirmation',
      '--always-approve',
      '--override-with-envs'
    ],
    encodeStdin: (prompt) => prompt,
    spawnEnv: overrides.resolveEnv ?? (({ profile }, opts) => resolveOpenhandsEnv(profile, !overrides.instanceCredential, opts?.model)),
    modelOptions: async ({ profile }) => {
      // The listing is best-effort end to end: a GAH profile that binds no
      // OpenHands profile lists no models instead of failing the picker.
      // The caller supplies the real profile for the exact-key lookup.
      // Turns validate it strictly (resolveOpenhandsEnv throws on the same files).
      let config: OpenhandsProfile | null;
      try {
        config = await readOpenhandsProfile(profile);
      } catch {
        config = null;
      }
      if (!config) return { models: [], reasoningEfforts: [], currentModelId: null };
      const current = process.env.LLM_MODEL ?? config.model;
      // An instance's credential (not this profile) owns its provider
      // endpoint and key, so there is nothing here to list against; the
      // picker shows the configured model only. Otherwise the listing
      // credentials are a profile-only PAIR: the ambient LLM_API_KEY and
      // LLM_BASE_URL never substitute for either half, so the profile key
      // cannot be sent to an unrelated ambient endpoint and an ambient key
      // cannot be sent to the profile endpoint. A profile missing either
      // half has nothing to list against either — the picker falls back
      // to the configured model.
      const apiKey = overrides.instanceCredential ? null : config.apiKey;
      const baseUrl = config.baseUrl;
      const ids = baseUrl && apiKey ? await listOpenhandsModels(config.name, baseUrl, apiKey, overrides.fetchFn) : [];
      const models: ManagerModelInfo[] = [];
      for (const id of current ? [current, ...ids] : ids) {
        if (!models.some((model) => model.id === id)) models.push({ id, name: id });
      }
      return { models, reasoningEfforts: [], currentModelId: current };
    },
    parseReply: parseOpenhandsReply
  };
}

function textParts(value: unknown): string | null {
  if (typeof value === 'string') return value;
  if (!Array.isArray(value)) return null;
  const parts = value
    .filter((part): part is { type: string; text: string } =>
      Boolean(part) && typeof part === 'object' && (part as { type?: unknown }).type === 'text' && typeof (part as { text?: unknown }).text === 'string')
    .map((part) => part.text);
  return parts.length > 0 ? parts.join('\n') : null;
}

/** Extract only the final agent message from OpenHands JSONL. */
export function parseOpenhandsReply(result: HeadlessProcessResult): string {
  if (result.exitCode !== 0) return defaultParseReply('OpenHands')(result);
  let reply: string | null = null;
  for (const line of result.stdout.split('\n')) {
    let event: Record<string, unknown>;
    try {
      event = JSON.parse(line) as Record<string, unknown>;
    } catch {
      continue;
    }
    if (event.source === 'agent' && event.action && typeof event.action === 'object') {
      const message = (event.action as { message?: unknown }).message;
      if (event.tool_name === 'finish' && typeof message === 'string') reply = message;
    }
    if (event.source === 'agent' && event.llm_message && typeof event.llm_message === 'object') {
      reply = textParts((event.llm_message as { content?: unknown }).content) ?? reply;
    }
    const role = event.role ?? (event.message as { role?: unknown } | undefined)?.role;
    if (role === 'assistant') {
      reply = textParts(event.content ?? (event.message as { content?: unknown } | undefined)?.content) ?? reply;
    }
  }
  if (!reply?.trim()) throw new Error('OpenHands turn produced no assistant reply.');
  return reply.trim();
}

/** Bounded servicing budget per turn (#1041): every decoded request costs
 * one more backend invocation, and an unbounded loop would let a backend
 * that only ever emits tool requests spin the turn forever. */
const MAX_TOOL_ROUNDS_PER_TURN = 5;
const MAX_TOOL_OUTPUT_BYTES = 256 * 1024;

/** The wire shape vibe's print mode leaks when its model emits a tool call
 * the CLI didn't execute (#1041): `<name>\u{C8F0}<json args>` -- one Unicode
 * separator (U+C8F0) between the tool name and the JSON argument object,
 * observed live as
 * `read_file\u{C8F0}{"file_path": ".../memoryGatewayClient.ts"}`. Returns
 * null for an ordinary reply; throws when the shape IS present but cannot
 * be decoded, so the turn ends with an actionable error rather than
 * surfacing raw tool syntax as a successful assistant reply. */
export function decodeVibeToolRequest(reply: string): HeadlessToolRequest | null {
  const raw = reply.trim();
  const match = /^(\S+?)\u{C8F0}(\{[\s\S]*\})$/u.exec(raw);
  if (!match) return null;
  const name = match[1];
  let args: unknown;
  try {
    args = JSON.parse(match[2]);
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`Vibe emitted a tool request for "${name}" whose arguments are not valid JSON (${detail}); refusing to surface raw tool syntax as a reply.`);
  }
  return { name, args: args as Record<string, unknown>, raw };
}

interface HeadlessToolPlan {
  title: string;
  kind: string;
  locations: string[];
  run: () => Promise<string>;
}

/** GAH-side executor for decoded tool requests (#1041). The request names
 * the BACKEND's tool vocabulary, so only the read-only subset GAH can
 * safely execute itself (a bounded repository read) is servable; anything
 * else fails the turn with an actionable error. Reads are confined to the
 * conversation's working directory -- the session worktree a headless turn
 * already runs in. */
function planHeadlessTool(displayName: string, name: string, args: Record<string, unknown>, cwd: string): HeadlessToolPlan {
  if (name !== 'read_file') {
    throw new Error(`${displayName} requested tool "${name}", which Parasaur cannot execute on the backend's behalf. Executable tools: read_file.`);
  }
  const requested = args.file_path;
  if (typeof requested !== 'string' || requested.trim().length === 0) {
    throw new Error(`${displayName} requested read_file without a valid "file_path" argument.`);
  }
  const resolved = path.resolve(cwd, requested);
  if (!isInsideWorkingDirectory(resolved, cwd)) {
    throw new Error(`${displayName} requested to read "${requested}", which resolves outside this conversation's working directory (${cwd}).`);
  }
  return {
    title: `Read ${resolved}`,
    kind: 'read',
    locations: [resolved],
    run: async () => {
      let content: Buffer;
      try {
        content = readFileSync(resolved);
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error);
        throw new Error(`${displayName} requested to read "${resolved}" but the read failed: ${detail}`);
      }
      return content.length > MAX_TOOL_OUTPUT_BYTES
        ? `${content.subarray(0, MAX_TOOL_OUTPUT_BYTES).toString('utf8')}\n[GAH truncated the read at ${MAX_TOOL_OUTPUT_BYTES} bytes]`
        : content.toString('utf8');
    }
  };
}

/** Mirrors acpAdapter's card digest: first 5 lines, 400 chars. */
function summarizeOutput(text: string): string {
  return text.split('\n').slice(0, 5).join('\n').slice(0, 400);
}

interface AgyStreamResult {
  status: 'SUCCESS' | 'ERROR' | string;
  response?: string;
  error?: string;
}

/** agy: print mode with NDJSON stdin/stdout so the prompt never touches
 * argv (issue #1009). `--input-format stream-json` reads one `{"event":
 * "user", "message": {"content": [{"type": "text", "text": "..."}]}}`
 * line per turn; `--output-format stream-json` emits one JSON object per
 * line, terminated by an `{"event": "result", "result": {...}}` line
 * carrying the final text and status. This exact shape was confirmed
 * against the installed CLI (its own errors named the required "event",
 * "message", and "content" fields one at a time) — none of it is
 * documented in `agy --help`.
 *
 * Headless has no permission round-trip (stdin is the prompt channel, not
 * a TTY), so without --dangerously-skip-permissions agy auto-denies every
 * tool call and the turn silently degrades to a no-tool reply. --sandbox
 * is kept alongside it so the auto-approved tools still run confined to
 * the session cwd.
 *
 * #1032 reopened: a session pinned to a model/effort was displayed as such
 * but silently launched agy with its own default (model="", whichever pool
 * `agy` itself falls back to) because turnArgs never carried the pin. The
 * installed CLI takes both natively as `--model`/`--effort`; append them
 * only when the session actually pinned one, so an unpinned session's argv
 * is unchanged. */
export function agyBackendSpec(runtime?: { executable: string; state_root: string | null }): HeadlessBackendSpec {
  const modelOptions = createAgyModelOptions(runtime);
  return {
    dispose: modelOptions.dispose,
    id: 'agy',
    displayName: 'Agy',
    ...(runtime?.state_root ? { spawnEnv: async () => ({ HOME: runtime.state_root! }) } : {}),
    modelOptions,
    modelCatalogVersion: modelOptions.version,
    turnArgs: (opts = {}) => {
      const args = [
        runtime?.executable ?? 'agy',
        '--input-format',
        'stream-json',
        '--output-format',
        'stream-json',
        '--dangerously-skip-permissions',
        '--sandbox'
      ];
      if (opts.model) args.push('--model', opts.model);
      if (opts.reasoningEffort) args.push('--effort', opts.reasoningEffort);
      return args;
    },
    encodeStdin: (prompt) =>
      `${JSON.stringify({
        event: 'user',
        message: { content: [{ type: 'text', text: prompt }] }
      })}\n`,
    parseReply: ({ stdout, stderr, exitCode }) => {
      const lines = stdout.split('\n');
      for (let i = lines.length - 1; i >= 0; i--) {
        const line = lines[i].trim();
        if (!line) continue;
        let event: { event?: string; result?: AgyStreamResult };
        try {
          event = JSON.parse(line);
        } catch {
          continue;
        }
        if (event.event !== 'result' || !event.result) continue;
        const result = event.result;
        if (result.status === 'SUCCESS') return result.response ?? '';
        const detail = result.error || stderr.trim().slice(0, 400) || `exit code ${exitCode}`;
        throw new Error(`Agy turn failed: ${detail}`);
      }
      // No result line at all: a fatal failure before agy could emit one
      // (missing binary, killed by signal, crash).
      const detail = stderr.trim().slice(0, 400) || `exit code ${exitCode}`;
      throw new Error(`Agy turn failed: ${detail}`);
    }
  };
}
