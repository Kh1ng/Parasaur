import { summarizeChatUsage } from './chatUsage.js';

const usage = { input_tokens: 10, output_tokens: 5, total_tokens: 100,
  cache_read_tokens: 80, cache_write_tokens: 0, reasoning_tokens: 3,
  estimated_cost_usd: 0, duration_seconds: 1 };

test('reported categories survive independently without recomputing the provider total', () => {
  expect(summarizeChatUsage([usage, usage])).toEqual({
    turns: 2, inputTokens: 20, outputTokens: 10, totalTokens: 200,
    cacheReadTokens: 160, cacheWriteTokens: 0, reasoningTokens: 6,
    estimatedCostUsd: 0, tokenIncomplete: false, costIncomplete: false
  });
});

test('missing replies and missing categories cannot become complete session totals', () => {
  const incomplete = summarizeChatUsage([usage, null]);
  expect(incomplete.totalTokens).toBeNull();
  expect(incomplete.inputTokens).toBeNull();
  expect(incomplete.estimatedCostUsd).toBeNull();
  expect(incomplete.tokenIncomplete).toBe(true);
  expect(incomplete.costIncomplete).toBe(true);
  const legacy = summarizeChatUsage([usage, { ...usage, cache_read_tokens: undefined, reasoning_tokens: null }]);
  expect(legacy.totalTokens).toBe(200);
  expect(legacy.cacheReadTokens).toBeNull();
  expect(legacy.reasoningTokens).toBeNull();
  expect(summarizeChatUsage([]).totalTokens).toBeNull();
  expect(summarizeChatUsage([{ ...usage, total_tokens: NaN }]).totalTokens).toBeNull();
});
