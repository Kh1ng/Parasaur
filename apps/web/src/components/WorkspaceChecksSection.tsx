import { useEffect, useId, useRef, useState } from 'react';
import type { ProfileChecks } from '@git-agent-harness/contracts';
import { gahApi, type ProfileUpdateData } from '../api/client.js';

const LISTS = [
  ['validation_commands', 'Validation', 'validation'],
  ['auto_fix_commands', 'Automatic fixes', 'auto-fix'],
] as const;
type List = typeof LISTS[number][0];
const same = (left: string[], right: string[]) => JSON.stringify(left) === JSON.stringify(right);

function checked(value: ProfileChecks, profile: string): ProfileChecks {
  if (value.profile !== profile || !LISTS.every(([key]) => Array.isArray(value[key]) && value[key].every(command => typeof command === 'string'))
    || !Number.isFinite(value.validation_timeout_seconds)) throw new Error('Invalid checks response');
  return value;
}

/** Private command text is requested only when the owner opens the editor.
 * Each textarea is one complete command, including any embedded newlines. */
export function WorkspaceChecksSection({ profile }: { profile: string }) {
  const [checks, setChecks] = useState<ProfileChecks | null>(null);
  const [baseline, setBaseline] = useState<ProfileChecks | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);
  const formId = useId();
  const request = useRef(0);
  useEffect(() => () => { request.current++; }, []);
  const dirty = !!checks && !!baseline && LISTS.some(([key]) => !same(checks[key], baseline[key]));
  const invalid = !!checks && LISTS.some(([key]) => checks[key].length > 128
    || checks[key].some(command => !command.trim() || command.length > 16_384 || command.includes('\0')));
  const close = () => { request.current++; setChecks(null); setBaseline(null); setPending(false); setError(''); setSaved(false); };
  const load = async () => {
    const id = ++request.current;
    setPending(true); setError(''); setSaved(false);
    try {
      const result = checked(await gahApi.getProfileChecks(profile), profile);
      if (request.current !== id) return;
      setChecks(result); setBaseline(result);
    } catch {
      if (request.current === id) setError('Cannot load checks. Owner access and an up-to-date Parasaur CLI on this node are required.');
    } finally { if (request.current === id) setPending(false); }
  };
  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!checks || !baseline || !dirty || invalid || pending) return;
    const id = ++request.current;
    const data: ProfileUpdateData = {};
    for (const [key] of LISTS) if (!same(checks[key], baseline[key])) data[key] = checks[key];
    setPending(true); setError(''); setSaved(false);
    let written = false;
    try {
      await gahApi.updateProfile(profile, data);
      written = true;
      if (request.current !== id) return;
      setBaseline(checks);
      const confirmed = checked(await gahApi.getProfileChecks(profile), profile);
      if (request.current !== id) return;
      if (LISTS.some(([key]) => data[key] && !same(confirmed[key], data[key]!))) {
        setBaseline(confirmed);
        setError('The node returned different checks. Your edits are kept; update Parasaur or resolve concurrent configuration changes before saving again.');
        return;
      }
      setChecks(confirmed); setBaseline(confirmed); setSaved(true);
    } catch {
      if (request.current === id) setError(written
        ? 'The checks were saved, but could not be reloaded. Reopen the editor to check the saved state.'
        : 'Checks could not be saved. Your edits are kept; check owner access and the node connection, then retry.');
    } finally { if (request.current === id) setPending(false); }
  };
  const change = (key: List, commands: string[]) => {
    setChecks(current => current && { ...current, [key]: commands }); setSaved(false);
  };

  return <section className="card-padded max-w-4xl" aria-label="Factory checks">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <h3 className="text-sm font-semibold text-primary">Factory checks</h3>
      {(checks || pending) && <button type="button" className="btn-secondary text-xs" onClick={close}>{dirty ? 'Cancel edits' : 'Close editor'}</button>}
    </div>
    <p className="mt-1 max-w-prose text-xs leading-relaxed text-muted">Review validation and automatic-fix commands for this project on the connected node. Owner access is required to view or change command text. Saving does not run the commands. Other nodes keep their own profile checks.</p>
    {!checks && <button type="button" className="btn-secondary mt-3" disabled={pending} onClick={() => void load()}>{pending ? 'Loading checks…' : 'Edit factory checks'}</button>}
    {error && <p role="alert" className="mt-3 text-sm text-critical">{error}</p>}
    {saved && <p role="status" className="mt-3 text-sm text-good">Checks saved and reloaded.</p>}
    {checks && <form className="mt-4 space-y-4" onSubmit={event => void save(event)}>
      <p className="text-xs text-muted">Validation timeout: {checks.validation_timeout_seconds}s per command. Change it in Dispatch settings.</p>
      {LISTS.map(([key, title, singular]) => <fieldset key={key} disabled={pending} className="min-w-0 space-y-3">
        <legend className="mb-2 text-sm font-medium text-primary">{title} ({checks[key].length})</legend>
        {key === 'auto_fix_commands' && <p className="max-w-prose text-xs text-muted">Automatic fixes may change checkout files when the factory runs them.</p>}
        {checks[key].length === 0 && <p className="text-xs text-muted">No {singular} commands configured.</p>}
        {checks[key].map((command, index) => <div key={index} className="flex items-start gap-2">
          <div className="min-w-0 flex-1 space-y-1 text-xs text-secondary">
            <label htmlFor={`${formId}-${key}-${index}`} className="block">{title} command {index + 1}</label>
            <textarea id={`${formId}-${key}-${index}`} value={command} rows={2} spellCheck={false} autoComplete="off" required maxLength={16_384}
              onChange={event => change(key, checks[key].map((value, position) => position === index ? event.target.value : value))}
              className="input w-full resize-y font-mono text-xs leading-relaxed" />
          </div>
          <button type="button" aria-label={`Remove ${singular} command ${index + 1}`} className="btn-secondary mt-5 shrink-0 text-xs" onClick={() => change(key, checks[key].filter((_, position) => position !== index))}>Remove</button>
        </div>)}
        <button type="button" disabled={checks[key].length >= 128} className="btn-secondary text-xs" onClick={() => change(key, [...checks[key], ''])}>Add {singular} command</button>
      </fieldset>)}
      <p className="max-w-prose text-xs text-muted">Each box is one command. Commas, quotes, and line breaks are preserved. Commands run on the owning node in its shell environment. Use environment files for credentials.</p>
      {invalid && <p className="text-xs text-warning">Each command needs text and must be at most 16384 characters without NUL. Remove unused empty boxes before saving.</p>}
      <button type="submit" className="btn-primary" disabled={!dirty || invalid || pending}>{pending ? 'Saving…' : 'Save factory checks'}</button>
    </form>}
  </section>;
}
