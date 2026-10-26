import { agentModelOptions, taskReasoningEfforts } from './agentModelOptions.js';

const levels = (ids: string[]) => ids.map((id) => ({ id, name: id }));

test('drops advertised levels the native CLI flag cannot carry', () => {
  expect(taskReasoningEfforts('codex', levels(['default', 'minimal', 'low', 'high', 'ultra', 'turbo'])).map((effort) => effort.id))
    .toEqual(['default', 'low', 'high', 'ultra']);
  expect(taskReasoningEfforts('claude', levels(['low', 'max', 'ultra'])).map((effort) => effort.id)).toEqual(['low', 'max']);
});

test('falls back to the Claude CLI levels when the catalog advertises none', () => {
  expect(taskReasoningEfforts('claude').map((effort) => effort.id)).toEqual(['low', 'medium', 'high', 'xhigh', 'max']);
  expect(taskReasoningEfforts('codex')).toEqual([]);
});


test('AGY task pickers group only provider-advertised effort variants, including new levels', () => {
  const reasoningEfforts = levels(['low', 'turbo']);
  const options = agentModelOptions('agy', [
    { id: 'nova-turbo', name: 'Nova (Turbo)', reasoningEfforts },
    { id: 'nova-low', name: 'Nova (Low)', reasoningEfforts },
    { id: 'unknown-high', name: 'Unknown (High)' },
    { id: 'plain-thinking', name: 'Plain (Thinking)', reasoningEfforts: [] },
  ], []);
  expect(options[0].label).toBe('Nova');
  expect(options[0].variants?.map(variant => variant.effort)).toEqual(['low', 'turbo']);
  expect(options[1].variants).toBeUndefined();
  expect(options[2].variants).toBeUndefined();
});
