// Only fixed labels reach operational logs. Error messages, stacks, paths,
// URLs and arbitrary `code` properties can contain credentials or user data.
const SYSTEM_CODES = new Set([
  'EACCES', 'EPERM', 'ENOENT', 'ENOTDIR', 'EISDIR', 'EIO', 'ENOSPC', 'EROFS',
  'EMFILE', 'ENFILE', 'ENOMEM', 'ECONNRESET', 'ECONNREFUSED', 'ETIMEDOUT', 'EPIPE'
]);
const OPERATIONS = new Set(['open', 'read', 'stat', 'scandir', 'write', 'connect']);

export function requestFailure(error: unknown, path: string) {
  const detail = error != null && typeof error === 'object'
    ? error as { code?: unknown; syscall?: unknown }
    : {};
  return {
    surface: path === '/api' || path.startsWith('/api/') ? 'api'
      : path.startsWith('/assets/') ? 'dashboard_asset' : 'document_or_other',
    code: typeof detail.code === 'string' && SYSTEM_CODES.has(detail.code) ? detail.code : 'UNKNOWN',
    operation: typeof detail.syscall === 'string' && OPERATIONS.has(detail.syscall) ? detail.syscall : 'unknown'
  };
}
