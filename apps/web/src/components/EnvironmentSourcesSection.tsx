import { useEffect, useId, useRef, useState } from 'react';
import { gahApi, type ProfileUpdateData } from '../api/client.js';

const SOURCES = [['env_file', 'Development'], ['env_file_prod', 'Production']] as const;
type Source = typeof SOURCES[number][0];
type Choice = { action: 'keep' | 'replace' | 'clear'; path: string };
const unchanged = (): Record<Source, Choice> => ({ env_file: { action: 'keep', path: '' }, env_file_prod: { action: 'keep', path: '' } });

/** Environment source values stay private on the node. The editor receives
 * presence flags and sends only explicit replacements or removals. */
export function EnvironmentSourcesSection({ profile, configured, onSaved }: {
  profile: string;
  configured: Record<Source, boolean | undefined>;
  onSaved: () => Promise<void>;
}) {
  const [choices, setChoices] = useState(unchanged);
  const formId = useId();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const changed = SOURCES.some(([field]) => choices[field].action !== 'keep');
  const invalid = SOURCES.some(([field]) => choices[field].action === 'replace'
    && (!choices[field].path.trim() || /[\x00-\x1f\x7f]/.test(choices[field].path)));

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!changed || invalid || pending) return;
    setPending(true); setError(''); setSaved(false);
    const data: ProfileUpdateData = {};
    for (const [field] of SOURCES) {
      const choice = choices[field];
      if (choice.action === 'replace') data[field] = choice.path.trim();
      if (choice.action === 'clear') (data.clear ??= []).push(field);
    }
    try {
      await gahApi.updateProfile(profile, data);
      if (!alive.current) return;
      setChoices(unchanged());
      setSaved(true);
      try { await onSaved(); }
      catch { if (alive.current) setError('The sources were saved, but their status could not be refreshed. Refresh Profile to check the saved state.'); }
    } catch {
      // CLI failures can contain path/source text. Keep this error fixed;
      // the settings response likewise never exposes those saved values.
      if (alive.current) setError('Environment sources could not be saved. Your entries are kept; check the node connection and owner access, then retry.');
    } finally { if (alive.current) setPending(false); }
  };

  return <section className="card-padded max-w-4xl" aria-labelledby="environment-sources-title">
    <h3 id="environment-sources-title" className="text-sm font-semibold text-primary">Workspace environment files</h3>
    <p className="mt-1 max-w-prose text-xs leading-relaxed text-muted">Development files supply application variables to new chats and factory dispatches. Chat keeps provider account settings and work-scoped credentials separate. Production is used only with <code>gah dispatch --prod</code>. Running agents keep their environment.</p>
    <form onSubmit={event => void save(event)} className="mt-4 space-y-4">
      <div className="grid gap-4 md:grid-cols-2">
        {SOURCES.map(([field, label]) => <fieldset key={field} disabled={pending} className="min-w-0 space-y-2">
          <legend className="mb-2 text-xs font-medium text-primary">{label} <span className="font-normal text-muted">· {configured[field] === true ? 'configured' : configured[field] === false ? 'not configured' : 'status unavailable'}</span></legend>
          <label htmlFor={`${formId}-${field}`} className="block text-xs text-secondary">{label} source</label>
          <select id={`${formId}-${field}`} value={choices[field].action} onChange={event => {
              const action = event.target.value as Choice['action'];
              setChoices(current => ({ ...current, [field]: { ...current[field], action } })); setSaved(false);
            }} className="input w-full">
              <option value="keep">Keep saved setting</option>
              <option value="replace">Use a file</option>
              <option value="clear">Remove source</option>
          </select>
          {choices[field].action === 'replace' && <div className="space-y-1">
            <label htmlFor={`${formId}-${field}-path`} className="block text-xs text-secondary">{label} file path on node</label>
            <input id={`${formId}-${field}-path`} value={choices[field].path} onChange={event => {
              const path = event.target.value;
              setChoices(current => ({ ...current, [field]: { ...current[field], path } })); setSaved(false);
            }} className="input w-full" autoComplete="off" spellCheck={false} required placeholder="/srv/project/.env" />
          </div>}
          {choices[field].action === 'clear' && <p className="text-xs text-warning">New agents will stop loading this source. The file itself stays on the node.</p>}
        </fieldset>)}
      </div>
      <p className="max-w-prose text-xs text-muted">Owner access is required to save. Use absolute paths on the execution node to share a source between chat and factory. Enter file paths, never keys or environment assignments. Existing source values and file contents are not displayed.</p>
      {error && <p role="alert" className="text-sm text-critical">{error}</p>}
      {saved && <p role="status" className="text-sm text-good">Environment sources saved for new agents.</p>}
      <button type="submit" disabled={!changed || invalid || pending} className="btn-primary">{pending ? 'Saving…' : 'Save environment sources'}</button>
    </form>
  </section>;
}
