import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { citations, noteSources, notesPath, physical, pin, previewOf, readNotes } from '../src/research.ts';
import * as P from '../src/panel.ts';
import { fold, initial, replay } from '../src/fold.ts';
import { frame, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import { newInbox } from '../src/inbox.ts';
import type { Info } from '../src/theme.ts';

const ws = mkdtempSync(join(tmpdir(), 'dl-ws-'));
mkdirSync(join(ws, 'team/specs'), { recursive: true });
writeFileSync(join(ws, 'team/specs/003-collab.md'), '# Collaboration\n\nA council has three rounds.\n');
const info: Info = { console_socket: '', narrator_socket: '', workspace: ws, mission: null, version: '0.5', cwd: '/p', roles: { orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•' }, architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•' } } };
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const model = (over: Partial<Model> = {}): Model => ({ info, pal: { dark: true, bg: [15, 23, 42] }, state: initial(0), now: 1000, deck: 1, composer: C.empty(), back: 0, help: false, menu: null, expandTools: false, search: null, queue: [], inbox: newInbox(0), mode: 'default', panel: null, ...over });
const text = (s: string): any => ({ t: 'text', s });
const key = (k: string): any => ({ t: 'key', k, alt: false, shift: false, ctrl: false });

test('citations find workspace paths in prose and ignore the rest', () => {
  assert.deepEqual(citations('See specs/003-collab.md#rounds, and (missions/m1/notes.md). Also @project/app/src/lib.rs and https://x.org/specs/a.'), ['specs/003-collab.md#rounds', 'missions/m1/notes.md', '@project/app/src/lib.rs']);
  assert.deepEqual(citations('The rubric is REVIEW.md, not my-REVIEW.md'), ['REVIEW.md']);
  assert.deepEqual(citations('nothing here, just /usr/bin and a.b'), []);
});

test('sources count reads and citations per path, and reading wins over citing', () => {
  let l = noteSources({}, ['specs/a.md'], 'cited', 'architect', 1);
  l = noteSources(l, ['specs/a.md', 'specs/b.md'], 'read', 'builder', 2);
  assert.deepEqual([l['specs/a.md']!.how, l['specs/a.md']!.count, l['specs/a.md']!.roles], ['read', 2, ['architect', 'builder']]);
  assert.equal(l['specs/b.md']!.how, 'read');
});

test('only workspace documents resolve to a place on disk; traversal, absolute paths and project files do not', () => {
  assert.equal(physical(info, 'specs/003-collab.md#x'), join(ws, 'team/specs/003-collab.md'));
  for (const bad of ['../etc/passwd', 'specs/../../x', '/etc/passwd', '@project/app/a.rs', '@payload/abc', 'specs/missing.md']) assert.equal(physical(info, bad), null, bad);
  assert.deepEqual(previewOf(info, 'specs/003-collab.md').slice(0, 1), ['# Collaboration']);
  assert.deepEqual(previewOf(info, '../x'), []);
});

test('the notebook is created with frontmatter once, then entries are appended with where they came from', () => {
  pin(info, 'm1', 'athena (orchestrator)', 'First finding.', new Date('2026-10-24T10:00:00Z'));
  pin(info, 'm1', 'typed by the operator', 'Second \x1b[2Jone.', new Date('2026-10-24T10:05:00Z'));
  const raw = readFileSync(notesPath(info, 'm1'), 'utf8');
  assert.equal(raw.match(/^---$/gm)!.length, 2, 'one frontmatter block');
  assert.match(raw, /source_type: internal_generated/);
  assert.match(raw, /## 2026-10-24T10:00:00.000Z · athena \(orchestrator\)\n\nFirst finding\./);
  assert.ok(!raw.includes('\x1b'));
  assert.match(readNotes(info, 'm1'), /^\s*# Notebook[\s\S]*Second \[2Jone\./);
});

test('search panel: typing, searching, moving loads the preview, enter cites, p pins, esc closes', () => {
  let p = P.openSearch();
  let r = P.key(p, text('ret'), {}); p = r.panel;
  r = P.key(p, text('ry'), {}); p = r.panel;
  r = P.key(p, key('enter'), {});
  assert.deepEqual(r.action, { t: 'search', q: 'retry' });
  p = { ...(r.panel as any), editing: false, hits: [{ pointer: 'specs/003-collab.md#retry', heading: 'Retry', preview: 'three attempts' }, { pointer: 'specs/004-x.md', heading: 'X', preview: 'y' }], sel: 0 };
  r = P.key(p, key('down'), {});
  assert.deepEqual(r.action, { t: 'load', path: 'specs/004-x.md' });
  assert.deepEqual(P.key(p, key('enter'), {}).action, { t: 'insert', text: '@specs/003-collab.md#retry ' });
  assert.deepEqual(P.key(p, text('p'), {}).action, { t: 'pin', from: 'search “retry” → specs/003-collab.md#retry', text: 'specs/003-collab.md#retry — Retry\n\nthree attempts' });
  assert.deepEqual(P.key(p, key('esc'), {}).action, { t: 'close' });
  assert.deepEqual(P.key(P.openSearch(), key('esc'), {}).action, { t: 'close' });
});

test('the panels are drawn in the chat area with a key hint in place of the prompt', () => {
  const hits = [{ pointer: 'specs/003-collab.md#retry', heading: 'Retry ladder', preview: 'three attempts then escalate' }];
  const s = frame(model({ panel: { ...(P.openSearch('retry') as any), editing: false, hits, lines: ['# Collaboration', '', 'A council has three rounds.'] } }), 100, 30).map(plain).join('\n');
  assert.match(s, /Search the knowledge base/);
  assert.match(s, /❯ specs\/003-collab\.md\s+Retry ladder/);
  assert.match(s, /three attempts then escalate/);
  assert.match(s, /A council has three rounds\./);
  assert.match(s, /↑↓ choose · enter put @path in the prompt · p pin to notes/);
  const src = noteSources({}, ['specs/003-collab.md'], 'cited', 'architect', 5);
  const st = { ...initial(0), sources: src };
  const so = frame(model({ state: st, panel: P.openSources() }), 100, 30).map(plain).join('\n');
  assert.match(so, /Sources \(1\)/);
  assert.match(so, /specs\/003-collab\.md\s+cited 1× · architect/);
  assert.match(frame(model({ panel: P.openNotes('') }), 100, 30).map(plain).join('\n'), /The notebook is empty/);
});

test('replies that cite documents feed the sources list, also when a mission is reopened; reads show live', () => {
  const reopened = replay(initial(0), [
    { type: 'dag.node.added', payload: { node_id: 'n_1', role: 'architect', depth: 0 } },
    { type: 'task.output', timestamp: '2026-10-24T09:00:00Z', payload: { text: 'Per specs/003-collab.md#rounds the council has three rounds.', status: 'done' } },
  ]);
  assert.deepEqual(Object.keys(reopened.sources), ['specs/003-collab.md#rounds']);
  const live = fold(initial(0), { method: 'event', params: { type: 'agent.block.complete', agent: 'builder', payload: { block: { type: 'tool_call', id: 'c', name: 'read_file', arguments: { path: 'specs/005b.md' } } } } }, 1);
  assert.equal(live.sources['specs/005b.md']!.how, 'read');
});

test('a session lives under local/sessions unless an older one is under local/missions, and both are listed', async () => {
  const { listMissions, sessionDir } = await import('../src/missions.ts');
  const ws = mkdtempSync(join(tmpdir(), 'delphi-sess-'));
  mkdirSync(join(ws, 'local/missions/old/events'), { recursive: true });
  mkdirSync(join(ws, 'local/sessions/fresh/events'), { recursive: true });
  assert.equal(sessionDir(ws, 'old'), join(ws, 'local/missions/old'));
  assert.equal(sessionDir(ws, 'fresh'), join(ws, 'local/sessions/fresh'));
  assert.equal(sessionDir(ws, 'brand-new'), join(ws, 'local/sessions/brand-new'));
  assert.deepEqual(listMissions(ws).map((r) => r.id).sort(), ['fresh', 'old']);
  writeFileSync(join(ws, 'local/sessions/fresh/notes.md'), 'x');
  assert.ok(physical({ workspace: ws } as Info, 'missions/fresh/notes.md')?.endsWith('local/sessions/fresh/notes.md'));
});
