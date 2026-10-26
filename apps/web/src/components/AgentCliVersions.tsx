import { useCallback, useEffect, useState } from 'react';
import type { AgentVersionsSnapshot } from '@git-agent-harness/contracts';
import { agentCliApi } from '../api/client.js';
import { useAutoRefresh } from '../hooks/useAutoRefresh.js';

/** Node-local inventory, including unavailable workers and unsupported providers. */
export function AgentCliVersions() {
  const [snapshot, setSnapshot] = useState<AgentVersionsSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const refresh = useCallback(async () => {
    setLoading(true);
    try { setSnapshot(await agentCliApi.versions()); setError(null); }
    catch (e) { setError(e instanceof Error ? e.message : 'Could not load CLI versions.'); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  useAutoRefresh(() => { void refresh(); }, 10_000);

  const update = async (nodeId: string, backend: string) => {
    setPending(`${nodeId}:${backend}`); setError(null); setNotice(null);
    try {
      await agentCliApi.update(nodeId, backend);
      setNotice(`${backend} update requested. The result will appear below.`);
      await refresh();
    } catch (e) { setError(e instanceof Error ? e.message : 'Could not start the update.'); }
    finally { setPending(null); }
  };

  return <section className="card-padded space-y-4" aria-labelledby="agent-cli-heading">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div><h2 id="agent-cli-heading" className="text-base font-semibold text-primary">Agent CLI versions</h2>
        <p className="mt-1 text-sm text-secondary">Installed CLIs and available releases on each node. Automatic updates require that node’s opt-in.</p></div>
      <button type="button" className="btn-secondary min-h-11" disabled={loading} onClick={() => void refresh()}>{loading ? 'Checking…' : 'Refresh versions'}</button>
    </div>
    {error && <p role="alert" className="text-sm text-critical">{error}</p>}
    {notice && <p role="status" className="text-sm text-secondary">{notice}</p>}
    {!snapshot && !error && <p role="status" className="text-sm text-muted">Checking installed CLIs and release registries…</p>}
    {snapshot && !snapshot.can_update && <p className="text-sm text-muted">Updates require owner access and node admin updates to be enabled.</p>}
    {snapshot?.nodes.map(node => <div key={node.node_id} className="space-y-2">
      <h3 className="text-sm font-medium text-primary">{node.display_name} <span className="font-normal text-muted">· Automatic CLI updates {node.auto_update ? 'on' : 'off'}</span></h3>
      {node.error && <p className="text-sm text-critical">{node.error}</p>}{node.versions.length > 0 && <div className="overflow-x-auto">
        <table className="w-full text-left text-sm"><thead><tr className="border-b border-subtle text-muted">
          <th scope="col" className="py-2 pr-4">Agent</th><th scope="col" className="py-2 pr-4">CLI version / latest</th><th scope="col" className="py-2 pr-4">Status</th><th scope="col" className="py-2">Action</th>
        </tr></thead><tbody>{node.versions.map(row => {
          const result = node.updates.find(u => u.backend === row.backend);
          const busy = result?.status === 'running' || pending === `${node.node_id}:${row.backend}`;
          return <tr key={row.backend} className="border-b border-subtle align-top">
            <th scope="row" className="py-3 pr-4 font-medium text-primary">{row.backend}</th>
            <td className="py-3 pr-4 tabular-nums text-secondary">{row.installed ?? 'Unavailable'} / {row.latest ?? 'Unknown'}</td>
            <td className="py-3 pr-4 text-secondary"><span className={row.outdated ? 'text-warning' : ''}>{busy ? 'Updating…' : row.outdated ? 'Outdated' : row.installed && row.latest ? 'Current' : 'Unknown'}</span>
              {row.error && <p className="mt-1 max-w-sm text-xs text-muted">{row.error}</p>}
              {result && result.status !== 'running' && <p className={`mt-1 text-xs ${result.status === 'failed' ? 'text-critical' : 'text-muted'}`}>{result.status === 'success' ? `Last update succeeded · ${result.after}` : `Last update failed · ${result.error ?? 'Check the node update log.'}`}</p>}
            </td>
            <td className="py-2">{snapshot.can_update && node.updates_enabled && row.update_command && row.installed && (row.outdated || result?.status === 'failed') && <button type="button" className="btn-secondary min-h-11" disabled={pending !== null || node.updates.some(u => u.status === 'running')} aria-label={`Update ${row.backend} on ${node.display_name}`} onClick={() => void update(node.node_id, row.backend)}>{busy ? 'Updating…' : 'Update'}</button>}</td>
          </tr>;
        })}</tbody></table>
      </div>}
    </div>)}
  </section>;
}
