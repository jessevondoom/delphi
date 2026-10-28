import test from 'node:test';
import { emptyArch } from '../src/archives.ts';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import stringWidth from 'string-width';
import * as C from '../src/composer.ts';
import { addBlock, fold, initial, type State } from '../src/fold.ts';
import { frame, type Model } from '../src/screen.ts';
import { newInbox } from '../src/inbox.ts';
import * as P from '../src/panel.ts';
import { emptyState, observeFrame, VIEWS, type OModel, type OState } from '../src/observe-screen.ts';
import type { Info } from '../src/theme.ts';

const here = dirname(fileURLToPath(import.meta.url));
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '/ws', mission: null, version: '0.5', cwd: '/proj',
  roles: {
    orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•', crown: '  ▄▄▄  ', crown_rgb: '#facc15' },
    architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•', crown: '▄▄▄▄▄▄▄', crown_rgb: '#facc15' },
    builder: { name: 'hephaestus', chassis: '#f97316', face: '•─•', crown: '▄▄▄▄▄▄▄', crown_rgb: '#facc15' },
    'red-team': { name: 'ares', chassis: '#ef4444', face: '-_-', crown: '▄     ▄', crown_rgb: '#facc15' },
  },
};

/** Golden screens: what a person would see, as plain text. UPDATE_SNAPSHOTS=1 rewrites them after a deliberate change. */
function snap(name: string, lines: string[]) {
  const file = join(here, 'snapshots-screens', name + '.txt');
  const got = lines.map((l) => plain(l).replace(/\s+$/, '')).join('\n') + '\n';
  if (process.env.UPDATE_SNAPSHOTS || !existsSync(file)) { mkdirSync(dirname(file), { recursive: true }); writeFileSync(file, got); return; }
  assert.equal(got, readFileSync(file, 'utf8'), `${name} changed (UPDATE_SNAPSHOTS=1 to accept)`);
}

const T0 = Date.parse('2026-10-24T09:00:00Z');
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8);
void clock;
function convo(): State {
  let s = initial(T0);
  s = addBlock(s, { kind: 'user', text: 'Compare a table and a file for the retry queue.', at: T0 });
  s = addBlock(s, { kind: 'reply', role: 'architect', at: T0 + 5000, text: '# Retry queue\n\nA **table** gives indexes and transactions.\n\n- atomic claims\n- easy audit\n\n```rust\nfn claim(id: u64) -> Result<(), Error> {\n    // lock the row\n    Ok(())\n}\n```\n\nSee specs/003-collaboration-and-council.md#ladder.' });
  s = addBlock(s, { kind: 'tool', role: 'architect', at: T0 + 6000, text: 'read_file(specs/003.md)', status: 'ok', detail: 'a' });
  s = addBlock(s, { kind: 'note', text: '■ stopped', at: T0 + 7000 });
  return s;
}
const model = (over: Partial<Model> = {}): Model => ({ info, pal: { dark: true, bg: [15, 23, 42] }, state: convo(), now: T0 + 9000, deck: 1, composer: C.empty(), back: 0, help: false, menu: null, expandTools: false, search: null, queue: [], inbox: newInbox(0), mode: 'default', panel: null, still: true, ...over });
const req = (id: string, tier: string, summary: string, at: number) => ({ method: 'event', params: { type: 'approval.requested', task_id: 't_1', payload: { approval_id: id, role: tier === 'red' ? 'architect' : 'builder', tier, tool: 'write_file', summary, payload_blake3: 'h' + id, payload: { preview: '--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new' }, timeout_secs: 600 } } }) as const;

test('golden screens: the chat, the menu, the inbox in both modes, the stream and a panel', () => {
  snap('chat-conversation', frame(model(), 100, 30));
  snap('chat-empty-start', frame(model({ state: initial(T0) }), 100, 30));
  snap('chat-narrow', frame(model(), 50, 20));
  let withCards = fold(initial(T0), req('a1', 'amber', 'write crates/delphi/src/deck.rs (+184)', T0), T0);
  withCards = fold(withCards, req('r1', 'red', 'Ratify spec 005b into the canonical repository', T0 + 1000), T0 + 1000);
  snap('inbox-list', frame(model({ state: withCards, deck: 2, now: T0 + 5000 }), 100, 30));
  snap('inbox-focus', frame(model({ state: withCards, deck: 2, now: T0 + 5000, inbox: { ...newInbox(0), mode: 'focus' } }), 100, 30));
  let act = fold(initial(T0), { method: 'event', params: { type: 'dag.node.added', payload: { node_id: 'n_1', role: 'architect', depth: 0 } } }, T0);
  act = fold(act, { method: 'delta.frame', params: { spans: { n_1: { thinking: 'Weighing a table against a file.', text: 'Use a table.' } } } }, T0 + 1);
  snap('panel-search', frame(model({ panel: { ...(P.openSearch('retry') as any), editing: false, hits: [{ pointer: 'specs/003-collaboration-and-council.md#ladder', heading: 'Retry ladder', preview: 'three attempts, then escalate to the operator' }], lines: ['# Collaboration', '', 'A council has three rounds.'] } }), 100, 30));
  snap('slash-menu', frame(model({ composer: C.insert(C.empty(), '/b'), menu: { items: [{ label: '/build', desc: 'A Builder task', insert: '/build' }, { label: '/budget', desc: 'Cost ceiling', insert: '/budget' }], sel: 0 } }), 100, 30));
});

function ostate(): OState {
  const s = emptyState();
  s.mission_id = '2026-10-24-ui'; s.spent_usd = 0.42; s.budget = { cap_usd: 5 }; s.cache_hit_ratio = 0.5; s.series = [0.01, 0.05, 0.02, 0.2, 0.14];
  s.tokens = { input: 1000, cache_read: 2000, cache_write: 0, output: 500, thinking: 0 };
  s.agents['orchestrator'] = { role: 'orchestrator', state: 'running', task_id: 't_1', endpoint: 'direct', tier: 'frontier', model: 'claude-x', upscaled: false, tokens: { input: 100, cache_read: 200, cache_write: 0, output: 50, thinking: 0 }, cost_usd: 0.07, calls: 2 };
  s.agents['red-team'] = { ...s.agents['orchestrator']!, role: 'red-team', state: 'waiting on you' };
  s.tasks['t_1'] = { id: 't_1', flow: 'build', prompt: 'Implement the deck router', status: 'running' };
  s.nodes['n_1'] = { id: 'n_1', role: 'orchestrator', parent: null, depth: 0, task_id: 't_1', state: 'running', tokens: 1200, cost_usd: 0.1, started_ms: 0, ended_ms: null, after: [] };
  s.narrative = ['t_1 started'];
  return s;
}

test('golden screens: the Observer views', () => {
  for (const view of VIEWS) {
    const m: OModel = { info, pal: { dark: true }, st: ostate(), view, pick: 0, scroll: 0, selNode: 0, live: {}, connected: true };
    snap('observer-' + view, observeFrame(m, 100, 30));
  }
});

// A seeded generator so a failure can be replayed.
function rng(seed: number) { let x = seed >>> 0; return () => ((x = (Math.imul(x, 1664525) + 1013904223) >>> 0) / 2 ** 32); }
const PIECES = ['plain words', '日本語のテキスト', 'emoji 🙂🎉', 'é combining', '\x1b[31mred\x1b[0m', '\x1b]0;title\x07', '```rust\nfn x() {}\n```', '| a | b |\n|---|---|\n| 1 | 2 |', '- item\n- item', '# Heading', 'a'.repeat(300), 'tabs\tand\r\nreturns', '> quote', 'https://example.com/' + 'x'.repeat(80), '​‍'];

test('fuzz: any window size and any text gives a screen of exactly h lines, none wider than w, and never throws', () => {
  const r = rng(42);
  for (let i = 0; i < 250; i++) {
    const w = 1 + Math.floor(r() * 200), h = 1 + Math.floor(r() * 70);
    let s = initial(T0);
    for (let k = 0; k < 1 + Math.floor(r() * 6); k++) {
      const text = Array.from({ length: 1 + Math.floor(r() * 4) }, () => PIECES[Math.floor(r() * PIECES.length)]!).join(r() < 0.5 ? ' ' : '\n');
      s = addBlock(s, { kind: (['user', 'reply', 'note', 'tool'] as const)[Math.floor(r() * 4)]!, role: 'architect', text, at: T0, status: 'ok', detail: text });
    }
    const typed = Array.from({ length: Math.floor(r() * 4) }, () => PIECES[Math.floor(r() * PIECES.length)]!).join(' ');
    const m = model({ state: s, deck: Math.floor(r() * 3) as 0 | 1 | 2, composer: C.insert(C.empty(), typed), back: Math.floor(r() * 40), help: r() < 0.2, expandTools: r() < 0.5 });
    const lines = frame(m, w, h);
    assert.equal(lines.length, h, `seed 42 #${i} ${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= Math.max(w, 1) , `#${i} ${w}x${h}: ${JSON.stringify(plain(l)).slice(0, 80)}`);
    for (const view of VIEWS) assert.equal(observeFrame({ info, pal: { dark: true }, st: ostate(), view, pick: 0, scroll: Math.floor(r() * 30), selNode: 0, live: {}, connected: true }, w, h).length, h);
  }
});

test('long sessions stay cheap: five thousand messages redraw in a few milliseconds, also while streaming', () => {
  let s = initial(T0);
  for (let i = 0; i < 5000; i++) {
    s = addBlock(s, { kind: i % 3 === 0 ? 'user' : 'reply', role: 'architect', text: `Message ${i}\n\n- one\n- two\n\n\`\`\`rust\nfn f${i}() {}\n\`\`\``, at: T0 + i });
  }
  const m = model({ state: s });
  frame(m, 100, 40); // fills the per-block cache once
  const t0 = performance.now();
  for (let i = 0; i < 20; i++) frame(model({ state: { ...s, live: 'streaming '.repeat(i + 1) } }), 100, 40);
  const each = (performance.now() - t0) / 20;
  assert.ok(each < 30, `a redraw took ${each.toFixed(1)} ms on average with 5000 messages`);
  const lines = frame(model({ state: s, back: 100000 }), 100, 40);
  assert.equal(lines.length, 40);
});

test('a window that is too small says so instead of drawing garbage', () => {
  const l = frame(model(), 27, 5).map(plain);
  assert.equal(l.length, 5);
  assert.match(l[0]!, /needs at least 28x9/);
  assert.equal(frame(model(), 0, 0).length, 0);
});
