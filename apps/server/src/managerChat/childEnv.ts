/**
 * Environment for Manager Chat agent CLIs (ACP and headless backends).
 *
 * Mirrors the Rust runner's `apply_child_env`: an agent child starts from an
 * allow-list of the server's environment, never the whole of it. The server
 * process holds control-plane credentials (the coordinator token, gateway,
 * messaging and pairing secrets) that agent children must not inherit. An
 * adapter that needs a value passes it explicitly as an override.
 */

/** Runtime, locale, network and backend-auth variables an agent CLI needs. */
const INHERITED_NAMES = new Set([
  // Process runtime and identity.
  'PATH', 'HOME', 'USER', 'LOGNAME', 'SHELL', 'TMPDIR', 'TMP', 'TEMP', 'TZ',
  'LANG', 'LANGUAGE', 'TERM', 'COLORTERM',
  'SSH_AUTH_SOCK',
  // Windows process runtime.
  'SYSTEMROOT', 'SYSTEMDRIVE', 'WINDIR', 'COMSPEC', 'PATHEXT', 'USERNAME', 'USERPROFILE',
  'HOMEDRIVE', 'HOMEPATH', 'APPDATA', 'LOCALAPPDATA', 'PROGRAMDATA', 'PROGRAMFILES',
  // Outbound proxy and CA bundles.
  'HTTP_PROXY', 'HTTPS_PROXY', 'NO_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'no_proxy', 'all_proxy',
  'NODE_EXTRA_CA_CERTS', 'SSL_CERT_FILE', 'SSL_CERT_DIR', 'REQUESTS_CA_BUNDLE', 'CURL_CA_BUNDLE', 'GIT_SSL_CAINFO',
  // Backend configuration locations.
  'CODEX_HOME', 'CLAUDE_CONFIG_DIR', 'HERMES_HOME', 'VIBE_HOME', 'OPENCODE_CONFIG', 'OPENCODE_CONFIG_DIR',
  // Repository-host tokens the Rust runner also passes to backends.
  'GITHUB_TOKEN', 'GH_TOKEN', 'GITLAB_PAT', 'GITLAB_PAT2',
  // Provider credentials meant for backends (see credentials::default_env).
  'OPENAI_API_KEY', 'OPENAI_BASE_URL', 'CODEX_API_KEY',
  'ANTHROPIC_API_KEY', 'ANTHROPIC_BASE_URL', 'CLAUDE_CODE_OAUTH_TOKEN',
  'GOOGLE_API_KEY', 'GEMINI_API_KEY', 'MISTRAL_API_KEY', 'NOUS_API_KEY', 'XAI_API_KEY',
  'MOONSHOT_API_KEY', 'DEEPSEEK_API_KEY', 'OPENROUTER_API_KEY', 'GROQ_API_KEY',
  'TOGETHER_API_KEY', 'CEREBRAS_API_KEY',
  // Nonsecret GAH locations: `gah` launches named instances and agents run it.
  'GAH_CONFIG', 'GAH_CONFIG_PATH', 'GAH_CANONICAL_CONFIG', 'GAH_MEMORY_HOOK_CONFIG',
  // The memory gateway endpoint (not its key) for session memory hooks.
  'TDAI_GATEWAY_URL'
]);

const INHERITED_PREFIXES = ['LC_', 'XDG_'];

/** Gateway variables a worker hands its agents' memory hooks. */
const MEMORY_GATEWAY_NAMES = ['TDAI_GATEWAY_URL', 'TDAI_GATEWAY_API_KEY'] as const;

let memoryGateway: Record<string, string> = {};

/** Register the memory gateway variables agent children receive. Only the
 * gateway's own names are kept, so a caller cannot widen the grant. */
export function grantAgentMemoryGateway(env: Record<string, string>): void {
  memoryGateway = Object.fromEntries(MEMORY_GATEWAY_NAMES.flatMap((name) => env[name] ? [[name, env[name]]] : []));
}

function inherited(name: string, platform: NodeJS.Platform): boolean {
  // Windows environment names are case-insensitive (`Path`, `SystemRoot`).
  const key = platform === 'win32' ? name.toUpperCase() : name;
  return INHERITED_NAMES.has(key) || INHERITED_PREFIXES.some((prefix) => key.startsWith(prefix));
}

/** Build an agent child's environment: the allow-listed parent variables,
 * then the registered memory gateway grant, then the adapter's own env. */
export function agentChildEnv(
  overrides: Record<string, string> = {},
  parent: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform
): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [name, value] of Object.entries(parent)) {
    if (value !== undefined && inherited(name, platform)) env[name] = value;
  }
  return { ...env, ...memoryGateway, ...overrides };
}
