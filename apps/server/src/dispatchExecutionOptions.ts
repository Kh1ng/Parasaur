import type { DispatchExecutionOptions } from '@git-agent-harness/contracts';

const booleanOptions = ['dryRun', 'allowDraftFail', 'prod', 'allowUnknownRedBaseline', 'escalate', 'manualWorker'] as const;

/** Reject malformed execution controls rather than silently executing a requested preview. */
export function parseDispatchExecutionOptions(value: unknown): DispatchExecutionOptions | 'invalid' {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return 'invalid';
  const input = value as Record<string, unknown>;
  const options: DispatchExecutionOptions = {};
  for (const key of booleanOptions) {
    if (input[key] === undefined) continue;
    if (typeof input[key] !== 'boolean') return 'invalid';
    options[key] = input[key];
  }
  if (input.retries !== undefined) {
    if (typeof input.retries !== 'number' || !Number.isInteger(input.retries) || input.retries < 0 || input.retries > 0xffff_ffff) return 'invalid';
    options.retries = input.retries;
  }
  return options;
}

/** Older workers ignore these fields; never route a meaningful override to them. */
export function requiresDispatchExecutionOptions(options: DispatchExecutionOptions): boolean {
  return options.retries !== undefined || booleanOptions.some(key => key !== 'manualWorker' && options[key] === true);
}
