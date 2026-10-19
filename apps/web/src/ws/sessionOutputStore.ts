/**
 * Session output lives outside the WebSocket context so a streamed line only
 * re-renders components that show that session's output, not every
 * `useWebSocket()` consumer. The bounded buffer is mutated in place; the
 * per-session revision is what subscribers react to.
 */
import { useMemo } from 'react';
import { create } from 'zustand';
import type { Session, SessionId } from '@git-agent-harness/contracts';
import { SessionOutputBuffer, type OutputStreamName, type SessionOutputText } from './sessionOutputBuffer.js';

interface SessionOutputState {
  buffer: SessionOutputBuffer;
  revisions: Record<SessionId, number>;
  append: (sessionId: SessionId, stream: OutputStreamName, line: string) => void;
  /** Applies a session.started/stopped/status update to the retention policy. */
  sessionChanged: (session: Session) => void;
  /** Applies a fresh server session list (server.welcome). */
  sessionsReplaced: (sessions: Session[]) => void;
  clear: () => void;
}

const FINISHED_STATUSES: ReadonlySet<Session['status']> = new Set(['stopped', 'error']);

function syncRevisions(buffer: SessionOutputBuffer, revisions: Record<SessionId, number>, bumped?: SessionId) {
  const next: Record<SessionId, number> = {};
  for (const id of buffer.sessionIds()) next[id] = revisions[id] ?? 0;
  if (bumped !== undefined && buffer.has(bumped)) next[bumped] = (revisions[bumped] ?? 0) + 1;
  return next;
}

export const useSessionOutputStore = create<SessionOutputState>((set, get) => ({
  buffer: new SessionOutputBuffer(),
  revisions: {},
  append: (sessionId, stream, line) => {
    const { buffer, revisions } = get();
    const isNew = !buffer.has(sessionId);
    buffer.append(sessionId, stream, line);
    set({
      revisions: isNew
        ? syncRevisions(buffer, revisions, sessionId)
        : { ...revisions, [sessionId]: (revisions[sessionId] ?? 0) + 1 }
    });
  },
  sessionChanged: (session) => {
    const { buffer, revisions } = get();
    if (FINISHED_STATUSES.has(session.status)) buffer.markFinished(session.id);
    else buffer.markActive(session.id);
    set({ revisions: syncRevisions(buffer, revisions) });
  },
  sessionsReplaced: (sessions) => {
    const { buffer, revisions } = get();
    for (const session of sessions) {
      if (FINISHED_STATUSES.has(session.status)) buffer.markFinished(session.id);
      else buffer.markActive(session.id);
    }
    buffer.retainLive(sessions.map((session) => session.id));
    set({ revisions: syncRevisions(buffer, revisions) });
  },
  clear: () => {
    get().buffer.clear();
    set({ revisions: {} });
  }
}));

/** Retained output for one session, recomputed only when that session's
 * output changes. */
export function useSessionOutput(sessionId: SessionId): SessionOutputText | undefined {
  const revision = useSessionOutputStore((state) => state.revisions[sessionId]);
  const buffer = useSessionOutputStore((state) => state.buffer);
  return useMemo(
    () => (revision === undefined ? undefined : buffer.text(sessionId)),
    [buffer, revision, sessionId]
  );
}
