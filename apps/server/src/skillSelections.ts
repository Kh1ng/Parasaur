import type { SkillBinding, SkillSelection } from '@git-agent-harness/contracts';

export function isBinding(value: unknown): value is SkillBinding {
  if (typeof value === 'string') return true;
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
  const binding = value as Record<string, unknown>;
  return typeof binding.label === 'string'
    && (binding.version === undefined || (typeof binding.version === 'string' && binding.version.trim().length > 0));
}

export function bindingLabel(binding: SkillBinding): string {
  return typeof binding === 'string' ? binding : binding.label;
}

export function bindingVersion(binding: SkillBinding): string | undefined {
  return typeof binding === 'string' ? undefined : binding.version;
}

/** Validate the entire request before replacing a binding scope. */
export function parseSkillSelections(value: unknown): SkillSelection[] {
  if (!Array.isArray(value)) throw new Error('bindings or skillIds must be an array');
  const selected = new Map<string, SkillSelection>();
  for (const raw of value) {
    const item = typeof raw === 'string' ? { id: raw } : raw;
    if (typeof item !== 'object' || item === null || Array.isArray(item)
      || typeof item.id !== 'string' || item.id.trim().length === 0
      || (item.version !== undefined && (typeof item.version !== 'string' || item.version.trim().length === 0))) {
      throw new Error('Each binding requires an id and optional nonempty version');
    }
    const previous = selected.get(item.id);
    if (previous && previous.version !== item.version) throw new Error(`Conflicting bindings for '${item.id}'`);
    selected.set(item.id, { id: item.id, ...(item.version === undefined ? {} : { version: item.version }) });
  }
  return [...selected.values()];
}
