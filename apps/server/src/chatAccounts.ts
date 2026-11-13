import type { ChatAccountInfo } from '@git-agent-harness/contracts';
import { chatRoute } from './chatRouting.js';
import { runConfigShowProfile } from './gahCli.js';

/** Read on the execution node. Do not launch a login or an agent to list accounts. */
export async function localChatAccounts(profile: string, backend: string, read = runConfigShowProfile): Promise<ChatAccountInfo[]> {
  const summary = await read(profile);
  return summary.backend_instances.filter(instance => instance.logical_backend === backend).map(instance => ({
    instance: instance.backend_instance,
    backend,
    label: instance.account_label ?? instance.backend_instance,
    enabled: instance.enabled,
    executableReady: instance.executable_resolved ?? instance.executable_configured ?? null,
    authReady: instance.auth_ready ?? null
  }));
}

export async function chatAccounts(profile: string, backend: string, nodeId?: string): Promise<ChatAccountInfo[]> {
  const route = await chatRoute(profile, nodeId, backend);
  if (route.remote) {
    const result = await route.remote.request<{ accounts: ChatAccountInfo[] }>({ action: 'accounts', backend });
    return result.accounts;
  }
  return localChatAccounts(route.profileName, backend);
}
