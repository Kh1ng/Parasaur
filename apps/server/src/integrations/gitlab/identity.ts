import type { IncomingHttpHeaders } from 'node:http';
import type { DeviceAccess } from '../../deviceAccess.js';

export const GITLAB_IDENTITY_HEADER = 'x-parasaur-identity-assertion';

/** Untrusted request evidence only. The provider must verify the assertion,
 * issuer, audience, request binding and replay policy before resolving a user.
 * Cookies and coordinator credentials are never passed to the provider. */
export interface GitLabIdentityRequest {
  readonly assertion: string;
  readonly method: string;
  readonly path: string;
  readonly host: string | undefined;
  readonly origin: string | undefined;
  readonly forwardedPrefix: string | undefined;
  readonly remoteAddress: string | undefined;
  readonly secure: boolean;
}

/** The id must name an existing, active paired controller. The provider owns
 * the GitLab-to-Parasaur mapping; public core never provisions or elevates it. */
export interface GitLabPrincipal {
  id: string;
  displayName: string;
  role: 'controller';
  /** Absolute ISO-8601 expiry, capped by the local controller's expiry. */
  expiresAt: string;
}

/** Register through createServer({ gitLabIdentityProvider }). Return null for
 * unknown or unauthorized users. Errors also deny access without fallback. */
export interface GitLabIdentityProvider {
  authenticate(request: GitLabIdentityRequest): GitLabPrincipal | null | Promise<GitLabPrincipal | null>;
}

export const anonymousGitLabIdentityProvider: GitLabIdentityProvider = {
  authenticate: () => null,
};

export type GitLabSession = GitLabPrincipal & { kind: 'device' };

type IdentityRequest = {
  headers: IncomingHttpHeaders;
  socket: { remoteAddress?: string };
  method?: string;
  url?: string;
  originalUrl?: string;
};

/** One boundary for HTTP and WebSocket controller sessions. No cookie or
 * bearer token is minted: every request needs fresh provider verification. */
export class GitLabIdentity {
  constructor(
    private provider: GitLabIdentityProvider = anonymousGitLabIdentityProvider,
    private access?: DeviceAccess,
    private now = Date.now,
  ) {}

  /** Without a registered provider, identity headers must not affect authentication. */
  get enabled(): boolean { return this.provider !== anonymousGitLabIdentityProvider; }

  async authenticate(req: IdentityRequest, secure: boolean): Promise<GitLabSession | null> {
    try {
      const assertion = req.headers[GITLAB_IDENTITY_HEADER];
      if (typeof assertion !== 'string' || !/^[A-Za-z0-9_.-]{1,8192}$/.test(assertion)) return null;
      const single = (name: string) => typeof req.headers[name] === 'string' ? req.headers[name] : undefined;
      const principal = await this.provider.authenticate({
        assertion, method: req.method ?? 'GET', path: req.originalUrl ?? req.url ?? '/',
        host: single('host'), origin: single('origin'), forwardedPrefix: single('x-forwarded-prefix'),
        remoteAddress: req.socket.remoteAddress, secure,
      });
      if (!principal || typeof principal.id !== 'string' || typeof principal.displayName !== 'string'
        || !principal.displayName.trim() || principal.displayName.length > 80 || /[\x00-\x1f\x7f]/.test(principal.displayName)
        || typeof principal.expiresAt !== 'string') return null;
      switch (principal.role) {
        case 'controller': break;
        default: {
          principal.role satisfies never;
          return null;
        }
      }
      const device = this.access?.list().find(device => device.id === principal.id);
      const expires = Math.min(Date.parse(principal.expiresAt), Date.parse(device?.expires_at ?? ''));
      if (!device || device.revoked_at !== null || !Number.isFinite(expires) || expires <= this.now()) return null;
      return { kind: 'device', id: device.id, displayName: principal.displayName, role: principal.role,
        expiresAt: new Date(expires).toISOString() };
    } catch {
      // Provider output and assertions can contain credentials; never log them.
      return null;
    }
  }
}
