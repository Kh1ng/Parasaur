import type { ChatUsage } from '@git-agent-harness/contracts';

export interface SessionUsageSummary {
  turns: number;
  inputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  cacheReadTokens?: number | null;
  cacheWriteTokens?: number | null;
  reasoningTokens?: number | null;
  tokenIncomplete?: boolean;
  estimatedCostUsd: number | null;
  costIncomplete: boolean;
}

/** A session total needs a reading from every assistant reply. Category
 * counts are independent provider facts, not an additive token equation. */
export function summarizeChatUsage(replies: Array<ChatUsage | null | undefined>): SessionUsageSummary {
  const sum = (field: keyof ChatUsage): number | null => {
    const values = replies.map(usage => usage?.[field]);
    return values.length && values.every((value): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0)
      ? values.reduce((total, value) => total + value, 0) : null;
  };
  return {
    turns: replies.filter(Boolean).length,
    inputTokens: sum('input_tokens'),
    outputTokens: sum('output_tokens'),
    totalTokens: sum('total_tokens'),
    cacheReadTokens: sum('cache_read_tokens'),
    cacheWriteTokens: sum('cache_write_tokens'),
    reasoningTokens: sum('reasoning_tokens'),
    tokenIncomplete: replies.some(usage => usage?.total_tokens == null),
    estimatedCostUsd: sum('estimated_cost_usd'),
    costIncomplete: replies.some(usage => usage?.estimated_cost_usd == null)
  };
}
