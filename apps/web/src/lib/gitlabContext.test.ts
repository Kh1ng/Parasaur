import { contextText, matchesContextRun, navigationPath, parseGitlabContext } from './gitlabContext.js';
const origins = ['https://gitlab.example'];
const context = { gitlab: origins[0], project: 'team/repo', url: `${origins[0]}/team/repo/-/merge_requests/12`, kind: 'mr', iid: '12', ref: 'main', path: 'src/index.ts' };
test('context retains all page fields and rejects untrusted hosts and project mismatches', () => {
  expect(parseGitlabContext(context, origins)).toEqual(context);
  expect(parseGitlabContext(context, [])).toBeNull();
  expect(parseGitlabContext({ ...context, url: 'https://evil.example/team/repo' }, origins)).toBeNull();
  expect(parseGitlabContext({ ...context, project: 'team/other' }, origins)).toBeNull();
  expect(parseGitlabContext({ ...context, iid: '12x' }, origins)).toBeNull();
  expect(contextText(context)).toContain(JSON.stringify(context));
});
test('navigation permits only same-origin paths', () => {
  expect(navigationPath('/team/repo?x=1#diff', origins[0])).toBe('/team/repo?x=1#diff');
  for (const value of ['//evil.example', 'https://evil.example', '/\\evil.example', '/\n/evil']) expect(navigationPath(value, origins[0])).toBeNull();
});

test('run filtering distinguishes GitLab issue and MR identities within a project', () => {
  expect(matchesContextRun({ repo: context.project, mr: '12' }, context)).toBe(true);
  expect(matchesContextRun({ repo: context.project, target: '!12' }, context)).toBe(true);
  expect(matchesContextRun({ repo: context.project, target: '#12' }, context)).toBe(false);
  expect(matchesContextRun({ repo: 'other/repo', mr: '12' }, context)).toBe(false);
  const issue = { ...context, kind: 'issue', url: `${origins[0]}/team/repo/-/issues/12` };
  expect(matchesContextRun({ repo: context.project, target: '#12' }, issue)).toBe(true);
  expect(matchesContextRun({ repo: context.project, mr: '12' }, issue)).toBe(false);
});
