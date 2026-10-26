/** Output of `gah agents versions --json`; unsupported backends remain visible. */
export interface AgentCliVersion {
  backend: string;
  installed: string | null;
  latest: string | null;
  outdated: boolean;
  source: string | null;
  update_command: string | null;
  error: string | null;
}
export interface AgentCliUpdate {
  backend: string;
  status: 'running' | 'success' | 'failed';
  started_at: string;
  finished_at: string | null;
  before: string | null;
  after: string | null;
  error: string | null;
}
export interface NodeAgentVersions {
  node_id: string;
  display_name: string;
  versions: AgentCliVersion[];
  updates: AgentCliUpdate[];
  auto_update: boolean;
  updates_enabled: boolean;
  error: string | null;
}
export interface AgentVersionsSnapshot {
  nodes: NodeAgentVersions[];
  can_update: boolean;
}
