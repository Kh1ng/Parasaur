import { webSocketUrl } from './webSocketUrl.js';

const global = globalThis as Record<string, unknown>;
const originalWindow = global.window;
afterEach(() => {
  if (originalWindow === undefined) delete global.window;
  else global.window = originalWindow;
});

test.each(['', '/parasaur'])('chat and activity socket follows bootstrap prefix %s', prefix => {
  global.window = { __GAH_BOOTSTRAP__: { basePath: prefix } };
  expect(webSocketUrl({ protocol: 'https:', host: 'example.com:8443' }, undefined, 'a/b'))
    .toBe(`wss://example.com:8443${prefix}/ws?profile=a%2Fb`);
  expect(webSocketUrl({ protocol: 'http:', host: 'localhost:3000' }))
    .toBe(`ws://localhost:3000${prefix}/ws`);
});

test('an explicit socket URL preserves its prefix and existing query', () => {
  global.window = { __GAH_BOOTSTRAP__: { basePath: '/parasaur' } };
  expect(webSocketUrl({ protocol: 'https:', host: 'example.com' }, 'wss://remote.example/custom/ws?key=value', 'main'))
    .toBe('wss://remote.example/custom/ws?key=value&profile=main');
});
