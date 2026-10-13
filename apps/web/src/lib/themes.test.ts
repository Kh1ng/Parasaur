import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { THEMES } from './themes.js';

// A token a theme block leaves out silently falls back to Dark's value (the
// `:root` block), and the contrast test would then pass on Dark's numbers.
const css = readFileSync(join(__dirname, '..', 'index.css'), 'utf8');

function themeBlock(id: string): string | undefined {
  const selector = id === 'dark' ? ":root,\n[data-theme='dark']" : `[data-theme='${id}']`;
  const start = css.indexOf(`${selector} {`);
  if (start < 0) return undefined;
  return css.slice(start, css.indexOf('}', start));
}

function tokens(block: string): Map<string, string> {
  return new Map([...block.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map(([, name, value]) => [name, value.trim()]));
}

const darkTokens = [...tokens(themeBlock('dark') ?? '').keys()];

test('the Dark block declares the token set', () => {
  expect(darkTokens).toEqual(expect.arrayContaining(['--surface-page', '--ink-primary', '--accent-fill', '--on-fill', '--overlay']));
});

test.each(THEMES.map((theme) => [theme.id, theme] as const))('%s declares every token and its chrome matches its page', (id, theme) => {
  const block = themeBlock(id);
  expect(block).toBeDefined();
  const declared = tokens(block!);
  expect(darkTokens.filter((name) => !declared.has(name))).toEqual([]);
  const page = declared.get('--surface-page')!.split(/\s+/).map(Number);
  expect(theme.chrome).toBe(`#${page.map((channel) => channel.toString(16).padStart(2, '0')).join('')}`);
});
