export function deployment(): { prefix: string; gitlabOrigins: string[] } {
  const config = typeof document === 'undefined' ? null : document.getElementById('parasaur-config');
  return config?.textContent ? JSON.parse(config.textContent) : { prefix: '', gitlabOrigins: [] };
}
export function appUrl(path: string): string {
  return `${deployment().prefix}${path}`;
}

/** Leading slashes in API paths must not discard an installation prefix. */
export function endpointUrl(path: string, base: string): URL {
  return new URL(path.replace(/^\//, ''), `${base.replace(/\/$/, '')}/`);
}
