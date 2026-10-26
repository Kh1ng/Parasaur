import { Router } from 'express';
import rateLimit from 'express-rate-limit';
import { randomUUID } from 'node:crypto';
import type { AgentVersionsSnapshot, NodeAgentVersions, RegisteredNode } from '@git-agent-harness/contracts';
import { requireOwner } from './authMiddleware.js';
import { nodeHeaders } from './registryService.js';
import { AgentCliError, AgentCliVersions, agentUpdatesEnabled } from './agentCliVersions.js';
import type { mutationSafety } from './mutationSafety.js';

interface Registry { getNodes(): RegisteredNode[]; getNode(id: string): RegisteredNode | undefined }

/** Fixed worker routes and registry-owned destinations, never request-supplied URLs. */
export class AgentCliFleet {
  constructor(readonly local: AgentCliVersions, private readonly registry: Registry,
    private readonly doFetch: typeof fetch = fetch) {}

  private async remote(node: RegisteredNode, action: 'local' | 'update' | 'scheduled-update', body?: object): Promise<NodeAgentVersions | void> {
    const url = new URL(`/api/agents/${action}`, node.advertised_url);
    if (url.protocol !== 'https:' && (url.protocol !== 'http:' || (process.env.GAH_ALLOW_INSECURE_HTTP !== '1'
      && !['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)))) {
      throw new AgentCliError(502, 'worker_transport_denied', 'This worker requires HTTPS or trusted LAN HTTP.');
    }
    try {
      const response = await this.doFetch(url, { method: body ? 'POST' : 'GET', redirect: 'error',
        signal: AbortSignal.timeout(30_000), headers: { ...nodeHeaders(node), 'Content-Type': 'application/json', 'Idempotency-Key': randomUUID() },
        ...(body ? { body: JSON.stringify(body) } : {}) });
      if (!response.ok) throw new AgentCliError(response.status === 409 ? 409 : 502, 'worker_agent_request_failed', 'The worker could not complete the agent request.');
      if (action !== 'local') return;
      const result = await response.json() as NodeAgentVersions;
      if (!Array.isArray(result?.versions) || !Array.isArray(result?.updates)) throw new Error('Invalid inventory');
      return { ...result, node_id: node.node_id, display_name: node.display_name };
    } catch (error) {
      if (error instanceof AgentCliError) throw error;
      throw new AgentCliError(502, 'worker_unavailable', 'The worker agent inventory is unavailable.');
    }
  }

  async snapshot(): Promise<NodeAgentVersions[]> {
    const nodes = this.registry.getNodes().filter(n => n.node_id !== this.local.node.node_id);
    const unavailable = (node: { node_id: string; display_name: string }): NodeAgentVersions => ({ ...node,
      versions: [], updates: [], auto_update: false, updates_enabled: false, error: 'Agent inventory unavailable. Check the node connection and GAH version.' });
    return Promise.all([
      this.local.snapshot().catch(() => unavailable(this.local.node)),
      ...nodes.map(async node => {
        try { return (await this.remote(node, 'local'))!; }
        catch { return unavailable({ node_id: node.node_id, display_name: node.display_name }); }
      }),
    ]);
  }

  async update(nodeId: string, backend: string): Promise<void> {
    if (nodeId === this.local.node.node_id) return this.local.start(backend);
    const node = this.registry.getNode(nodeId);
    if (!node) throw new AgentCliError(404, 'unknown_node', 'That node is no longer registered.');
    await this.remote(node, 'update', { backend });
  }

  async scheduledUpdate(): Promise<void> {
    // Each node enforces its own opt-in and admin policy, including current GAH nodes.
    await Promise.allSettled([
      this.local.scheduledUpdate(),
      ...this.registry.getNodes().filter(n => n.node_id !== this.local.node.node_id)
        .map(node => this.remote(node, 'scheduled-update', {})),
    ]);
  }
}

export function agentCliRouter(local: AgentCliVersions, fleet: AgentCliFleet | undefined, mutation: ReturnType<typeof mutationSafety>) {
  const router = Router();
  router.use(rateLimit({ windowMs: 60_000, limit: 30, standardHeaders: true, legacyHeaders: false }));
  router.use((_req, res, next) => { res.setHeader('Cache-Control', 'no-store'); next(); });
  router.get('/local', async (_req, res) => {
    try { res.json(await local.snapshot()); }
    catch { res.status(503).json({ error: 'agent_inventory_unavailable', message: 'Agent inventory is unavailable.' }); }
  });
  router.get('/versions', async (_req, res) => {
    try {
      const result: AgentVersionsSnapshot = { nodes: fleet ? await fleet.snapshot() : [await local.snapshot()],
        can_update: res.locals.authPrincipal?.kind === 'owner' && agentUpdatesEnabled() };
      res.json(result);
    } catch { res.status(503).json({ error: 'agent_inventory_unavailable', message: 'Agent inventory is unavailable.' }); }
  });
  router.use(requireOwner);
  router.use((_req, res, next) => {
    if (!agentUpdatesEnabled()) return res.status(403).json({ error: 'agent_updates_disabled', message: 'Set GAH_ENABLE_ADMIN_UPDATE=1 on this node to enable agent updates.' });
    next();
  });
  router.post('/update', mutation('agents.update'), async (req, res) => {
    try {
      const { backend, node_id: nodeId } = req.body ?? {};
      if (typeof backend !== 'string' || !['claude', 'codex', 'agy', 'vibe', 'opencode'].includes(backend)
        || (nodeId !== undefined && typeof nodeId !== 'string')) {
        throw new AgentCliError(400, 'invalid_agent_update', 'Choose a node and a supported agent CLI.');
      }
      if (nodeId && nodeId !== local.node.node_id) {
        if (!fleet) throw new AgentCliError(400, 'invalid_node', 'Workers only update their own CLIs.');
        await fleet.update(nodeId, backend);
      } else await local.start(backend);
      res.status(202).json({ accepted: true });
    } catch (error) {
      res.status(error instanceof AgentCliError ? error.status : 503).json({
        error: error instanceof AgentCliError ? error.code : 'agent_update_failed',
        message: error instanceof AgentCliError ? error.message : 'Could not start the agent update. Check node readiness.' });
    }
  });
  router.post('/scheduled-update', mutation('agents.update'), async (_req, res) => {
    try { await local.scheduledUpdate(); res.json({ accepted: true }); }
    catch { res.status(409).json({ error: 'agent_update_deferred', message: 'Node maintenance is deferred until the node is ready.' }); }
  });
  return router;
}
