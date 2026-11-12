/**
 * The profile a request means when it names none.
 *
 * This was the literal `gah`: the name of this project's own profile, which
 * no other installation has. A dashboard opened anywhere else therefore
 * started on a project that does not exist. The default now follows the
 * configured profiles: it stays where it is while that profile exists,
 * prefers `gah` for installations that have it, and otherwise takes the
 * first profile by name. With no profiles at all it remains `gah`, so an
 * unconfigured server answers as it always did.
 */
const LEGACY_DEFAULT = 'gah';

let current = LEGACY_DEFAULT;

export function defaultProfile(): string {
  return current;
}

/** Re-chooses the default from the configured profile names and returns it. */
export function chooseDefaultProfile(configured: readonly string[]): string {
  if (configured.length > 0 && !configured.includes(current)) {
    current = configured.includes(LEGACY_DEFAULT) ? LEGACY_DEFAULT : [...configured].sort()[0];
  }
  return current;
}
