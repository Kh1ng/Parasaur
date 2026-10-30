import { withBasePath } from '../lib/basePath.js';

/** Chat and activity share this connection and its deployment prefix. */
export function webSocketUrl(
  location: Pick<Location, 'protocol' | 'host'>,
  override?: string,
  profile?: string | null
): string {
  const url = new URL(override ||
    `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}${withBasePath('/ws')}`);
  if (profile) url.searchParams.set('profile', profile);
  return url.toString();
}
