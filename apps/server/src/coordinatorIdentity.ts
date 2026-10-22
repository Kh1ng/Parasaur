import { readFileSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';
import crypto from 'node:crypto';
import { COORDINATOR_SCHEMA_SEED, COORDINATOR_VERSION } from '@git-agent-harness/contracts';
import { preserveCorruptFile, writeFileAtomic } from './atomicFile.js';

export interface CoordinatorIdentity {
  node_id: string;
  display_name: string;
  advertised_url: string;
  version: string;
  schema_digest: string;
}

export const COORDINATOR_SCHEMA_DIGEST = crypto
  .createHash('sha256')
  .update(COORDINATOR_SCHEMA_SEED)
  .digest('hex');

let cachedIdentity: { cacheKey: string; identity: CoordinatorIdentity } | null = null;

export function getCoordinatorIdentity(
  configPath?: string,
  port: number = 3773
): CoordinatorIdentity {
  const identityPath = configPath || process.env.GAH_COORDINATOR_IDENTITY_PATH || resolve(process.cwd(), 'config/coordinator-identity.json');
  const cacheKey = `${identityPath}::${port}`;
  if (cachedIdentity?.cacheKey === cacheKey) return cachedIdentity.identity;
  
  let node_id: string | undefined;
  let display_name = 'GAH Coordinator';
  let advertised_url = `http://localhost:${port}`;
  const version = COORDINATOR_VERSION;
  const schema_digest = COORDINATOR_SCHEMA_DIGEST;
  let stored: Record<string, unknown> = {};
  let persist = true;

  if (existsSync(identityPath)) {
    try {
      const data = JSON.parse(readFileSync(identityPath, 'utf8'));
      if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('not an identity object');
      stored = data;
      if (typeof data.node_id === 'string' && data.node_id) node_id = data.node_id;
      if (data.display_name) display_name = data.display_name;
      if (data.advertised_url) advertised_url = data.advertised_url;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code) {
        // The read itself failed; the file may still hold the real id, so it is never replaced.
        persist = false;
        console.warn(`warning: cannot read coordinator identity ${identityPath}; using a temporary node id.`);
      } else {
        const preserved = preserveCorruptFile(identityPath);
        console.warn(`warning: coordinator identity ${identityPath} is corrupt; ${preserved ? `moved it to ${preserved} and ` : ''}writing a new node id.`);
      }
    }
  }
  // Mint an id once and persist it, so a missing or corrupt file does not
  // give this coordinator a different identity on every start.
  if (!node_id) {
    node_id = crypto.randomUUID();
    if (persist) {
      try {
        writeFileAtomic(identityPath, JSON.stringify({ ...stored, node_id, display_name, advertised_url }, null, 2));
      } catch {
        // An unwritable config directory must not block identity resolution.
      }
    }
  }

  const identity = {
    node_id,
    display_name,
    advertised_url,
    version,
    schema_digest
  };
  cachedIdentity = { cacheKey, identity };
  return identity;
}

export function resetCachedCoordinatorIdentity() {
  cachedIdentity = null;
}
