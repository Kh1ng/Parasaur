import { GitBranch, Cpu, Brain } from 'lucide-react';
import { isSessionTerminal, type Session, type SessionExecutionResult } from '@git-agent-harness/contracts';
import { StatusBadge, type StatusTone } from './ui/StatusBadge.js';
import { providerIcon } from '../lib/icons.js';
import { formatDispatchName } from '../lib/format.js';

type SessionCardProps = {
  session: Session;
  onClick: () => void;
};

const STATUS_TONE: Record<Session['status'], StatusTone> = {
  idle: 'unknown',
  starting: 'warning',
  running: 'good',
  stopping: 'warning',
  stopped: 'unknown',
  error: 'critical'
};

function executionLabel(result: SessionExecutionResult): string {
  if (result.reason === 'not_started') return 'No process was started';
  if (result.reason === 'launch_failed') return 'Process failed to launch';
  const detail = result.signal ? result.signal : result.exitCode === null ? 'exit code not reported' : `exit code ${result.exitCode}`;
  return `${result.reason === 'cancelled' ? 'Cancelled' : 'Process exited'} · ${detail}`;
}

export function SessionCard({ session, onClick }: SessionCardProps) {
  const Icon = providerIcon(session.providerKind);
  const name = formatDispatchName(session);

  return (
    <button
      onClick={onClick}
      className="card-padded w-full text-left transition-colors hover:border-accent/40"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-start gap-3 min-w-0">
          <Icon size={18} className="text-muted shrink-0 mt-0.5" aria-hidden="true" />
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-primary truncate" title={name}>{name}</h3>
            <p className="text-xs text-muted truncate">{session.repo || session.id}</p>

            <div className="mt-2.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-secondary">
              {session.mode && <span>{session.mode}</span>}
              {session.branch && (
                <span className="inline-flex items-center gap-1">
                  <GitBranch size={12} aria-hidden="true" />
                  {session.branch}
                </span>
              )}
              {session.backend && (
                <span className="inline-flex items-center gap-1">
                  <Cpu size={12} aria-hidden="true" />
                  {session.backend}
                  {session.backendInstance && ` / ${session.backendInstance}`}
                </span>
              )}
              {session.nodeId && <span className="break-all">{session.nodeId}</span>}
              {session.model && (
                <span className="inline-flex items-center gap-1">
                  <Brain size={12} aria-hidden="true" />
                  {session.model}
                </span>
              )}
            </div>
          </div>
        </div>

        <StatusBadge tone={STATUS_TONE[session.status]} label={session.leaseState === 'uncertain_reconciling' ? 'Execution unconfirmed' : session.status} />
      </div>

      {(session.execution || isSessionTerminal(session)) && <p className="mt-2 text-xs text-secondary">
        {session.execution ? executionLabel(session.execution) : 'Exit result not reported by this node'}
      </p>}

      {session.error && (
        <div className="mt-3 p-2 rounded-md text-xs text-critical" style={{ background: 'rgb(var(--status-critical) / 0.08)' }}>
          {session.error}
        </div>
      )}
    </button>
  );
}
