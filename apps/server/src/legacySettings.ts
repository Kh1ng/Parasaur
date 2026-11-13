/**
 * Settings read from the environment were named `GAH_*` before the product
 * became Parasaur. Every setting is now `PARASAUR_*`. An installed node still
 * carries the old names in its unit files, launch agents and env files, so
 * every entry point imports this module first. Nothing else in the server
 * knows the old prefix.
 *
 * Delete this module and its imports once installed nodes have been rewritten.
 */
const LEGACY = 'GAH_';
const CURRENT = 'PARASAUR_';

/** Gives every setting that is present only under its old name its current
 * name too. A current name that is already set wins. The old variable stays,
 * so a registration that names it (`env:GAH_NODE_TOKEN`) keeps resolving. */
export function adoptLegacySettings(env: NodeJS.ProcessEnv = process.env): void {
  for (const [name, value] of Object.entries(env)) {
    if (!name.startsWith(LEGACY) || value === undefined) continue;
    const current = CURRENT + name.slice(LEGACY.length);
    if (env[current] === undefined) env[current] = value;
  }
}

adoptLegacySettings();
