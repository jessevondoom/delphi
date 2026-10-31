import test from 'node:test';
import assert from 'node:assert/strict';
import { bundleTarget, completions, searchHistory } from '../src/complete.ts';
import type { Info } from '../src/theme.ts';

const info = { roles: { architect: { name: 'daedalus', chassis: '#0', face: 'x' }, builder: { name: 'hephaestus', chassis: '#0', face: 'x' } } } as unknown as Info;
const fs = (rel: string) => (rel === 'specs' ? [{ name: '005b-decks.md', dir: false }, { name: '005c-inline.md', dir: false }, { name: 'assets', dir: true }, { name: '.hidden', dir: false }] : []);

test('a slash at the start completes commands; elsewhere it does not', () => {
  assert.ok(completions('/obs', 4, info, fs)!.items.some((i) => i.label === '/observe'));
  assert.equal(completions('see /obs', 8, info, fs), null);
});

test('@ completes roles by id or theme name, and specs by path', () => {
  assert.deepEqual(completions('ask @dae', 8, info, fs)!.items.map((i) => i.insert), ['@architect ']);
  assert.deepEqual(completions('@b', 2, info, fs)!.items.map((i) => i.label), ['@builder']);
  assert.deepEqual(completions('read @specs/005', 15, info, fs)!.items.map((i) => i.label), ['@specs/005b-decks.md', '@specs/005c-inline.md']);
  assert.deepEqual(completions('@specs/a', 8, info, fs)!.items.map((i) => i.insert), ['@specs/assets/']);
  assert.equal(completions('@zzz', 4, info, fs), null);
});

test('the completion replaces just the token under the cursor', () => {
  const c = completions('ask @dae now', 8, info, fs)!;
  assert.deepEqual([c.from, c.to], [4, 8]);
});

test('ctrl+r finds the newest match, then older ones', () => {
  const h = ['fix the bug', 'write docs', 'fix the other bug'];
  const a = searchHistory(h, 'fix', h.length);
  assert.equal(a, 2);
  assert.equal(searchHistory(h, 'fix', a), 0);
  assert.equal(searchHistory(h, 'fix', 0), -1);
  assert.equal(searchHistory(h, '', 3), -1);
});

test("@bundle:agent names a bundle agent; a line reference is not one", () => {
  assert.equal(bundleTarget("acme-plat:reviewer"), "bundle:acme-plat:reviewer");
  assert.equal(bundleTarget("acme/plat:reviewer"), "bundle:acme/plat:reviewer");
  assert.equal(bundleTarget("specs/a-b:12"), null);
  assert.equal(bundleTarget("architect"), null);
  assert.equal(bundleTarget("a:b:c"), null);
});

test('/projects offers the home workspace first, then registered projects, and Enter runs the pick', () => {
  const withProjects = { ...info, projects: [{ name: 'default', root: '/ws', home: true }, { name: 'My App', root: '/code/My App', home: false }, { name: 'notes', root: '/code/notes', home: false }] } as unknown as Info;
  const all = completions('/projects ', 10, withProjects, fs)!;
  assert.deepEqual(all.items.map((i) => i.insert), ['default', 'my-app', 'notes']);
  assert.ok(all.items.every((i) => i.run));
  assert.deepEqual(completions('/projects my', 12, withProjects, fs)!.items.map((i) => i.insert), ['my-app']);
  assert.equal(completions('/projects zzz', 13, withProjects, fs), null);
  assert.equal(completions('/projects ', 10, info, fs), null, 'an older delphi sends no list');
});
