/**
 * Session Manager - Manages agent sessions
 * Inspired by t3code's orchestration but adapted for GAH
 * 
 * After TICKET-113: Sessions now run actual `gah dispatch` commands via gahCli.ts
 * and stream the real output as session.stdout messages.
 */

import { generateSessionId, GAHError } from '@git-agent-harness/shared';
import { getProviderRegistry } from '../provider/ProviderRegistry.js';
import { getServerPushBus } from '../serverPushBus.js';
import { diagnosticTail } from '../diagnosticTail.js';
import { dispatchRequestFingerprint, assertDispatchRequestIdentity } from '../dispatchRequestIdentity.js';
import { isSessionTerminal } from '@git-agent-harness/contracts';
import { runDispatchCancellable, type DispatchOptions, type DispatchResult, type CancellableDispatch, type CancellationResult } from '../gahCli.js';
import type {
  Session,
  DispatchExecutionOptions,
  SessionId,
  ProviderKind,
  ProviderInstanceId,
  SessionStatus,
  GitLabDispatchContext,
  SessionExecutionResult
} from '@git-agent-harness/contracts';

export type SessionOptions = DispatchExecutionOptions & {
  // GAH profile id (config.toml's [profiles.<id>]) -- NOT a backend name.
  // Required: there's no sane default to guess from providerKind/repo.
  profile: string;
  providerKind: ProviderKind;
  instanceId: ProviderInstanceId;
  repo: string;
  branch?: string;
  target?: string;
  mr?: string;
  mode: string;
  backend?: string;
  backendInstance?: string;
  model?: string;
  budget?: number;
  reasoningEffort?: string;
  requestId?: string;
  /** GitLab page context (#1595): recorded on the session so the runs feed
   * can filter without parsing target text. Only the GitLab event
   * dispatcher (#1592) sets it. */
  gitlab?: GitLabDispatchContext;
};

type ProviderRegistryLike = {
  isProviderAvailable(kind: ProviderKind): boolean;
};

type PushBusLike = {
  publish(message: Parameters<ReturnType<typeof getServerPushBus>['publish']>[0]): void;
};

type DispatchRunner = typeof runDispatchCancellable;

type SessionManagerDeps = {
  providerRegistry?: ProviderRegistryLike;
  pushBus?: PushBusLike;
  dispatchRunner?: DispatchRunner;
  disableCleanupTimer?: boolean;
};

function executionResult(reason: SessionExecutionResult['reason'], result?: DispatchResult): SessionExecutionResult {
  return { reason,
    exitCode: result && Number.isInteger(result.exitCode) && result.exitCode >= 0 ? result.exitCode : null,
    signal: result?.signal ?? null };
}

class SessionManagerImpl {
  private sessions: Map<SessionId, Session> = new Map();
  private requestToSessionId: Map<string, SessionId> = new Map();
  private requestFingerprints = new Map<string, string>();
  private pendingSessions: Map<string, Promise<Session>> = new Map();
  private activeDispatches: Map<SessionId, CancellableDispatch> = new Map();
  private profileDispatchTails: Map<string, Promise<void>> = new Map();
  private providerRegistry: ProviderRegistryLike;
  private pushBus: PushBusLike;
  private dispatchRunner: DispatchRunner;
  
  constructor(deps: SessionManagerDeps = {}) {
    this.providerRegistry = deps.providerRegistry ?? getProviderRegistry();
    this.pushBus = deps.pushBus ?? getServerPushBus();
    this.dispatchRunner = deps.dispatchRunner ?? runDispatchCancellable;

    // Set up periodic session cleanup. unref() so this housekeeping timer
    // never keeps the process (or a test importing this singleton) alive.
    if (!deps.disableCleanupTimer) {
      const timer = setInterval(() => this.cleanupFinishedSessions(), 60000);
      timer.unref?.();
    }
  }
  async startSession(options: SessionOptions): Promise<Session> {
    options = { ...options, gitlab: options.gitlab && { ...options.gitlab } };
    const { requestId } = options;
    const fingerprint = dispatchRequestFingerprint(options);
    if (requestId) {
      const previous = this.requestFingerprints.get(requestId);
      if (previous) assertDispatchRequestIdentity(previous, fingerprint);
      const pending = this.pendingSessions.get(requestId);
      if (pending) {
        return pending;
      }

      const existingSessionId = this.requestToSessionId.get(requestId);
      if (existingSessionId) {
        const existingSession = this.sessions.get(existingSessionId);
        if (existingSession) {
          return existingSession;
        }
        this.requestToSessionId.delete(requestId);
      }
    }

    // Publish only after this request owns its pending slot: subscribers can
    // synchronously replay a start when they receive session.started.
    const startPromise = Promise.resolve().then(() => this.startSessionFresh(options));
    if (requestId) {
      this.requestFingerprints.set(requestId, fingerprint);
      this.pendingSessions.set(requestId, startPromise);
    }
    try {
      const session = await startPromise;
      if (requestId) {
        this.requestToSessionId.set(requestId, session.id);
      }
      return session;
    } finally {
      if (requestId) {
        this.pendingSessions.delete(requestId);
        if (!this.requestToSessionId.has(requestId)) this.requestFingerprints.delete(requestId);
      }
    }
  }

  private async startSessionFresh(options: SessionOptions): Promise<Session> {
    const sessionId = generateSessionId();

    // Check if provider is available
    if (!this.providerRegistry.isProviderAvailable(options.providerKind)) {
      throw new GAHError(
        `Provider ${options.providerKind} is not available`,
        'PROVIDER_NOT_AVAILABLE'
      );
    }
    
    // Create initial session
    const session: Session = {
      id: sessionId,
      requestId: options.requestId,
      providerKind: options.providerKind,
      instanceId: options.instanceId,
      status: 'starting',
      startedAt: new Date().toISOString(),
      repo: options.repo,
      branch: options.branch,
      target: options.target,
      mr: options.mr,
      mode: options.mode,
      backend: options.backend,
      backendInstance: options.backendInstance,
      model: options.model,
      budget: options.budget,
      gitlab: options.gitlab
    };
    
    this.sessions.set(sessionId, session);

    // Notify about session start immediately
    this.pushBus.publish({
      type: 'session.started',
      session
    });
    
    // Prepare dispatch options
    const dispatchOptions: DispatchOptions = {
      profile: options.profile,
      mode: options.mode,
      backend: options.backend,
      backendInstance: options.backendInstance,
      target: options.target,
      branch: options.branch,
      mr: options.mr,
      model: options.model,
      budget: options.budget,
      dryRun: options.dryRun,
      retries: options.retries,
      allowDraftFail: options.allowDraftFail,
      prod: options.prod,
      allowUnknownRedBaseline: options.allowUnknownRedBaseline,
      escalate: options.escalate,
      manualWorker: options.manualWorker,
      reasoningEffort: options.reasoningEffort
    };
    
    // Start the actual gah dispatch process
    const cancellableDispatch = this.startDispatchProcess(sessionId, dispatchOptions);
    
    // Store the active dispatch so we can potentially cancel it
    this.activeDispatches.set(sessionId, cancellableDispatch);
    
    // Wait for dispatch to complete (but don't block the session start)
    cancellableDispatch.promise.then((result) => {
      this.handleDispatchComplete(sessionId, result);
    }).catch((error) => {
      this.handleDispatchError(sessionId, error);
    });
    
    // The profile queue promotes to running only when this session owns its slot.
    
    return session;
  }
  
  /**
   * Run the actual gah dispatch process and stream output
   */
  private startDispatchProcess(
    sessionId: SessionId,
    options: DispatchOptions
  ): CancellableDispatch {
    if (options.manualWorker) {
      // Explicit extra workers launch immediately; their CLI keeps atomic work claims.
      return this.dispatchRunner(options, (line) => this.addSessionOutput(sessionId, line, false));
    }
    const previous = this.profileDispatchTails.get(options.profile) ?? Promise.resolve();
    let release: () => void;
    const slot = new Promise<void>((resolve) => { release = resolve; });
    // Serializes same-profile dispatches: the next queued dispatch on this
    // profile awaits `tail`, which only resolves once `release()` runs --
    // and `release()` is not called until this dispatch's own termination
    // (natural completion or cancel()) has finished. So the profile stays
    // serialized for as long as this dispatch's process tree is alive.
    const tail = previous.catch(() => undefined).then(() => slot);
    this.profileDispatchTails.set(options.profile, tail);

    // Deferred: the real dispatch process is only started once it's this
    // session's turn in the profile queue (see startAfterPrevious below).
    let actualDispatch: CancellableDispatch | null = null;

    const startAfterPrevious = async (): Promise<CancellableDispatch> => {
      await previous.catch(() => undefined);

      // A session cancelled while still queued must never start -- and
      // this check is scoped to exactly this sessionId, so stopping one
      // queued session can never skip an unrelated sibling on the same
      // profile.
      const session = this.sessions.get(sessionId);
      if (!session || session.status === 'stopping' || session.status === 'stopped' || session.status === 'error') {
        return {
          promise: Promise.resolve({ exitCode: -1, stderr: 'Session was cancelled before starting' }),
          cancel: async () => ({ cancelled: true })
        };
      }

      // Promote status now that this session owns the profile slot.
      if (session?.status === 'starting') {
        session.status = 'running';
        this.sessions.set(sessionId, session);
        this.pushBus.publish({ type: 'session.status', session });
      }

      actualDispatch = this.dispatchRunner(options, (line: string) => {
        // Forward each line as session.stdout message
        this.addSessionOutput(sessionId, line, false);
      });
      return actualDispatch;
    };

    // Start the process after previous completes
    const dispatchPromise = startAfterPrevious().then(dispatch => dispatch.promise);

    const promise = new Promise<DispatchResult>((resolve, reject) => {
      dispatchPromise
        .then(resolve, reject)
        .finally(() => {
          release();
          if (this.profileDispatchTails.get(options.profile) === tail) {
            this.profileDispatchTails.delete(options.profile);
          }
        });
    });

    let cancelPromise: Promise<CancellationResult> | null = null;

    // Memoized so every caller observes the same settlement and the
    // underlying process-tree termination only runs once per dispatch.
    const cancel = (): Promise<CancellationResult> => {
      if (cancelPromise) return cancelPromise;

      cancelPromise = (async (): Promise<CancellationResult> => {
        const result = actualDispatch ? await actualDispatch.cancel() : { cancelled: true };
        // A failed cancellation is an observation, not termination proof.
        if (result.cancelled) {
          release();
          // A cancelled queued slot still follows its live predecessor. Do
          // not erase that dependency and let a new sibling jump the queue.
          void tail.then(() => {
            if (this.profileDispatchTails.get(options.profile) === tail) this.profileDispatchTails.delete(options.profile);
          });
        }
        return { ...result, notStarted: actualDispatch === null };
      })();

      return cancelPromise;
    };

    return { promise, cancel };
  }
  
  /**
   * Handle completion of a dispatch process
   */
  private handleDispatchComplete(sessionId: SessionId, result: DispatchResult): void {
    const session = this.sessions.get(sessionId);
    if (!session) return;
    
    this.activeDispatches.delete(sessionId);
    
    // A confirmed cancellation remains terminal even if its callback is late.
    if (session.status === 'stopped' || session.status === 'error') {
      // Session was already cancelled or stopped, ignore late completion
      return;
    }
    
    const cancelled = session.status === 'stopping';
    session.execution = executionResult(result.spawnFailed ? 'launch_failed' : cancelled ? 'cancelled' : 'exited', result);
    session.status = cancelled || result.exitCode === 0 ? 'stopped' : 'error';
    session.error = session.status === 'error'
      ? diagnosticTail(result.stderr || `Dispatch failed with exit code ${result.exitCode}`)
      : undefined;
    session.endedAt = new Date().toISOString();
    this.sessions.set(sessionId, session);
    this.pushBus.publish({ type: 'session.stopped', session });
  }
  
  /**
   * Handle error in dispatch process
   */
  private handleDispatchError(sessionId: SessionId, error: unknown): void {
    const session = this.sessions.get(sessionId);
    if (!session) return;
    
    this.activeDispatches.delete(sessionId);
    
    // A confirmed cancellation remains terminal even if its callback is late.
    if (session.status === 'stopped' || session.status === 'error') {
      // Session was already cancelled or stopped, ignore late error
      return;
    }
    
    session.status = 'error';
    session.execution = executionResult('launch_failed');
    session.error = diagnosticTail(error instanceof Error ? error.message : String(error));
    session.endedAt = new Date().toISOString();
    this.sessions.set(sessionId, session);
    
    this.pushBus.publish({
      type: 'session.stopped',
      session
    });
  }
  
  async stopSession(sessionId: SessionId): Promise<Session> {
    const session = this.sessions.get(sessionId);
    
    if (!session) {
      throw new GAHError(`Session ${sessionId} not found`, 'SESSION_NOT_FOUND');
    }
    
    if (isSessionTerminal(session) || session.status === 'stopping') {
      return session;
    }
    
    // Update session status to stopping
    session.status = 'stopping';
    this.sessions.set(sessionId, session);
    
    // Notify about session stop request
    this.pushBus.publish({
      type: 'session.status',
      session
    });

    const activeDispatch = this.activeDispatches.get(sessionId);
    if (activeDispatch) {
      let timeout: ReturnType<typeof setTimeout> | undefined;
      try {
        const outcome = await Promise.race([
          activeDispatch.cancel().then(result => ({ timedOut: false as const, result })),
          new Promise<{ timedOut: true }>(resolve => {
            timeout = setTimeout(() => resolve({ timedOut: true }), 30_000);
          })
        ]);
        // Completion can win the race while cancellation is being observed.
        if (session.status !== 'stopping') return session;
        if (outcome.timedOut) {
          session.error = 'Cancellation timed out; execution is still unconfirmed';
        } else if (!outcome.result.cancelled) {
          session.error = diagnosticTail(outcome.result.error ?? 'Cancellation failed; execution is still unconfirmed');
        } else {
          session.status = 'stopped';
          session.execution = executionResult(outcome.result.notStarted ? 'not_started' : 'cancelled', outcome.result.result);
          session.error = undefined;
          this.activeDispatches.delete(sessionId);
        }
      } catch (error) {
        if (session.status !== 'stopping') return session;
        session.error = diagnosticTail(`Cancellation unconfirmed: ${error instanceof Error ? error.message : String(error)}`);
      } finally {
        clearTimeout(timeout);
      }
    } else {
      // A queued session that never obtained a process is safe to retire.
      session.status = 'stopped';
      session.execution = executionResult('not_started');
    }
    if (session.status === 'stopped') session.endedAt = new Date().toISOString();
    this.sessions.set(sessionId, session);
    this.pushBus.publish({ type: session.status === 'stopped' ? 'session.stopped' : 'session.status', session });
    return session;
  }
  
  async sendCommand(sessionId: SessionId, command: string): Promise<void> {
    const session = this.sessions.get(sessionId);
    
    if (!session) {
      throw new GAHError(`Session ${sessionId} not found`, 'SESSION_NOT_FOUND');
    }
    
    if (session.status !== 'running') {
      throw new GAHError(
        `Cannot send command to session in ${session.status} state`,
        'SESSION_NOT_RUNNING'
      );
    }
    
    // For now, just log the command and add it to the output
    // In a future implementation, this could be sent to a running dispatch process
    console.log(`Session ${sessionId} command: ${command}`);
    
    this.pushBus.publish({
      type: 'session.stdout',
      sessionId,
      data: `> ${command}\n`,
      timestamp: Date.now()
    });
  }
  
  addSessionOutput(sessionId: SessionId, data: string, isStderr: boolean = false): void {
    const session = this.sessions.get(sessionId);
    if (!session || session.status === 'stopped' || session.status === 'error') return;
    // The browser has bounded replay and the CLI owns durable logs. Do not keep
    // a second, unread transcript in the server for the lifetime of every session.
    this.pushBus.publish({
      type: isStderr ? 'session.stderr' : 'session.stdout',
      sessionId,
      data,
      timestamp: Date.now()
    });
  }
  
  getSession(sessionId: SessionId): Session | undefined {
    return this.sessions.get(sessionId);
  }
  
  getAllSessions(): Session[] {
    return Array.from(this.sessions.values());
  }
  
  getSessionsByProvider(providerKind: ProviderKind): Session[] {
    return Array.from(this.sessions.values())
      .filter(session => session.providerKind === providerKind);
  }
  
  getSessionsByStatus(status: SessionStatus): Session[] {
    return Array.from(this.sessions.values())
      .filter(session => session.status === status);
  }
  
  getActiveSessions(): Session[] {
    return Array.from(this.sessions.values())
      .filter(session => ['starting', 'running', 'stopping'].includes(session.status));
  }
  
  private cleanupFinishedSessions(): void {
    const now = Date.now();
    const finishedSessions: SessionId[] = [];
    
    for (const [sessionId, session] of this.sessions) {
      if (session.status === 'stopped' || session.status === 'error') {
        if (session.endedAt) {
          const endedAt = new Date(session.endedAt).getTime();
          const age = now - endedAt;
          
          // Clean up sessions older than 1 hour
          if (age > 60 * 60 * 1000) {
            finishedSessions.push(sessionId);
          }
        }
      }
    }
    
    for (const sessionId of finishedSessions) {
      const requestIdsToRemove = Array.from(this.requestToSessionId.entries())
        .filter(([, mappedSessionId]) => mappedSessionId === sessionId)
        .map(([requestId]) => requestId);
      for (const requestId of requestIdsToRemove) {
        this.requestToSessionId.delete(requestId);
        this.requestFingerprints.delete(requestId);
        this.pendingSessions.delete(requestId);
      }
      this.sessions.delete(sessionId);
    }
    
    if (finishedSessions.length > 0) {
      console.log(`Cleaned up ${finishedSessions.length} finished sessions`);
    }
  }
}

const sessionManager = new SessionManagerImpl();

export function getSessionManager(): SessionManagerImpl {
  return sessionManager;
}

export function createSessionManager(deps?: SessionManagerDeps): SessionManagerImpl {
  return new SessionManagerImpl(deps);
}
