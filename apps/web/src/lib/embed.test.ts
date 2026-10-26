import {
  embedContextParams,
  embedMessageOriginAllowed,
  embedNavigatePath,
  handleEmbedMessage,
  matchEmbedProfile,
  parseEmbedContext,
  readEmbedContext,
  type EmbedContext
} from './embed.js';

const context: EmbedContext = {
  gitlab: 'https://gitlab.example.com',
  url: 'https://gitlab.example.com/team/project/-/merge_requests/42',
  project: 'team/project',
  kind: 'merge_request',
  iid: 42,
  ref: null,
  path: null
};

test('the panel query string parses into a page context', () => {
  expect(parseEmbedContext('?gitlab=https%3A%2F%2Fgitlab.example.com&url=https%3A%2F%2Fgitlab.example.com%2Fteam%2Fproject%2F-%2Fmerge_requests%2F42&project=team%2Fproject&kind=merge_request&iid=42'))
    .toEqual(context);
  expect(parseEmbedContext('?gitlab=https://gitlab.example.com&url=https://gitlab.example.com/blob/main/src/a.ts&project=team/project&kind=file&ref=main&path=src/a.ts'))
    .toEqual({ ...context, kind: 'file', iid: null, url: 'https://gitlab.example.com/blob/main/src/a.ts', ref: 'main', path: 'src/a.ts' });
  // A bare page (no project context) still identifies the GitLab origin.
  expect(parseEmbedContext('?gitlab=https://gitlab.example.com&url=https://gitlab.example.com/dashboard'))
    .toEqual({ gitlab: 'https://gitlab.example.com', url: 'https://gitlab.example.com/dashboard', project: null, kind: null, iid: null, ref: null, path: null });
});

test('contexts without a usable GitLab origin and page URL are rejected', () => {
  for (const invalid of [
    '',
    '?url=https://gitlab.example.com',
    '?gitlab=https://gitlab.example.com',
    '?gitlab=notaurl&url=https://gitlab.example.com',
    '?gitlab=javascript:alert(1)&url=https://gitlab.example.com',
    '?gitlab=https://gitlab.example.com/rejected&url=https://gitlab.example.com',
    '?gitlab=https://gitlab.example.com&url=not-a-url',
    '?gitlab=https://gitlab.example.com&url=ftp://gitlab.example.com',
    '?gitlab=%20&url=https://gitlab.example.com'
  ]) {
    expect(parseEmbedContext(invalid)).toBeNull();
  }
});

test('individual context fields are bounded and normalized', () => {
  expect(parseEmbedContext('?gitlab=https://gitlab.example.com&url=https://gitlab.example.com&kind=pipeline&iid=07'))
    .toMatchObject({ kind: 'pipeline', iid: null });
  expect(parseEmbedContext('?gitlab=https://gitlab.example.com&url=https://gitlab.example.com&kind=unknown')).toMatchObject({ kind: null });
  expect(parseEmbedContext(`?gitlab=https://gitlab.example.com&url=https://gitlab.example.com&project=${'a'.repeat(513)}`)).toMatchObject({ project: null });
  expect(parseEmbedContext('?gitlab=https://gitlab.example.com&url=https://gitlab.example.com&project=/abs')).toMatchObject({ project: null });
  // iids arrive as numbers from postMessage and as strings from the query string.
  expect(readEmbedContext({ gitlab: 'https://gitlab.example.com', url: 'https://gitlab.example.com', iid: 42 })?.iid).toBe(42);
  expect(readEmbedContext({ gitlab: 'https://gitlab.example.com', url: 'https://gitlab.example.com', iid: '99999999999' })?.iid).toBeNull();
});

test('the context round-trips through the standalone query string', () => {
  const restored = parseEmbedContext(`?${embedContextParams(context).toString()}`);
  expect(restored).toEqual(context);
  expect(restored?.kind).toBe('merge_request');
});

test('only configured origins pass the postMessage origin check', () => {
  const allowed = ['https://gitlab.example.com'];
  expect(embedMessageOriginAllowed('https://gitlab.example.com', allowed)).toBe(true);
  for (const origin of ['https://evil.example', 'https://gitlab.example.com.evil.example', 'null', '', 'https://gitlab.example.com/']) {
    expect(embedMessageOriginAllowed(origin, allowed)).toBe(false);
  }
});

test('inbound context messages are accepted from allowed origins only, and everything else is ignored', () => {
  const allowed = ['https://gitlab.example.com'];
  const message = { type: 'parasaur:context', context: { gitlab: 'https://gitlab.example.com', url: 'https://gitlab.example.com/team/project/-/issues/7', project: 'team/project', kind: 'issue', iid: 7 } };
  expect(handleEmbedMessage({ origin: 'https://gitlab.example.com', data: message }, allowed))
    .toEqual({ type: 'parasaur:context', context: { gitlab: 'https://gitlab.example.com', url: 'https://gitlab.example.com/team/project/-/issues/7', project: 'team/project', kind: 'issue', iid: 7, ref: null, path: null } });
  // Wrong origin, malformed payloads, and unknown types never produce a context.
  for (const event of [
    { origin: 'https://evil.example', data: message },
    { origin: 'https://gitlab.example.com', data: null },
    { origin: 'https://gitlab.example.com', data: 'parasaur:context' },
    { origin: 'https://gitlab.example.com', data: { type: 'parasaur:context' } },
    { origin: 'https://gitlab.example.com', data: { type: 'parasaur:context', context: { gitlab: 'javascript:x', url: 'no' } } },
    { origin: 'https://gitlab.example.com', data: { type: 'unknown: messageType' } },
    // The contract's outbound types are not inbound instructions.
    { origin: 'https://gitlab.example.com', data: { type: 'parasaur:ready' } },
    { origin: 'https://gitlab.example.com', data: { type: 'parasaur:close' } },
    { origin: 'https://gitlab.example.com', data: { type: 'parasaur:navigate', path: '/-/security/dashboard' } }
  ]) {
    expect(handleEmbedMessage(event, allowed)).toBeNull();
  }
});

test('navigate paths must be bounded same-origin absolute paths', () => {
  expect(embedNavigatePath('/group/project/-/merge_requests/42')).toBe('/group/project/-/merge_requests/42');
  for (const rejected of ['', 'relative/path', '//evil.example', '/\\evil.example', `/${'a'.repeat(2048)}`, '/a\u0000b', 42, null]) {
    expect(embedNavigatePath(rejected)).toBeNull();
  }
});

test('profiles match the page context by provider link, then by repository path, and stay unambiguous', () => {
  const profiles = [
    { name: 'exact', provider: 'gitlab', repo: 'team/project', web_url: 'https://gitlab.example.com/team/project' },
    { name: 'trailing', provider: 'gitlab', repo: 'other', web_url: 'https://gitlab.example.com/team/other/' },
    { name: 'github', provider: 'github', repo: 'team/project', web_url: 'https://github.com/team/project' }
  ];
  expect(matchEmbedProfile(profiles, context)?.name).toBe('exact');
  expect(matchEmbedProfile(profiles, { ...context, project: 'team/other' })?.name).toBe('trailing');
  // A gitlab profile without a provider link can still match by repository path.
  expect(matchEmbedProfile([{ name: 'repo', provider: 'gitlab', repo: 'team/project', web_url: null }], context)?.name).toBe('repo');
  // Ambiguity and absence fail closed.
  expect(matchEmbedProfile([...profiles, { name: 'dup', provider: 'gitlab', repo: 'x', web_url: 'https://gitlab.example.com/team/project' }], context)).toBeNull();
  expect(matchEmbedProfile(profiles, { ...context, project: 'unknown/project' })).toBeNull();
  expect(matchEmbedProfile(profiles, { ...context, project: null })).toBeNull();
  // A github profile never matches a gitlab page.
  expect(matchEmbedProfile(profiles.slice(2), { ...context, project: 'team/project' })).toBeNull();
});
