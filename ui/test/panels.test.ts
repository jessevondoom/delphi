import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Observer, totalTokens } from '../src/observer.ts';
import { addPin, readNotebook } from '../src/notebook.ts';
import { collectSources, extractPointers, previewOf, resolveSource } from '../src/sources.ts';
import type { Block } from '../src/fold.ts';
import { panelBody, panelFooter, panelHint, panelInit, panelKey, parseView, selectedBuild, sparkline, VIEWS, type PanelCtx, type PanelUi } from '../src/panels.ts';
import { thinkingBody, type Model } from '../src/screen.ts';
import { frame } from '../src/screen.ts';
import { inboxInit } from '../src/inbox.ts';
import { initial } from '../src/fold.ts';
import * as C from '../src/composer.ts';
import type { Info } from '../src/theme.ts';

const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: {
    orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•' },
    architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•' },
    builder: { name: 'hephaestus', chassis: '#f59e0b', face: '•_•' },
    'red-team': { name: 'ares', chassis: '#ef4444', face: '-_-' },
  },
};
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const key = (k: string, extra = {}) => ({ t: 'key', k, alt: false, shift: false, ctrl: false, ...extra }) as any;
const text = (s: string) => ({ t: 'text', s }) as any;
let seq = 0;
const ev = (type: string, task: string, payload: any, extra: any = {}) => ({ seq: ++seq, type, task_id: task, payload, ...extra });

function world(): Observer {
  seq = 0;
  const o = new Observer(['orchestrator', 'architect', 'builder', 'red-team']);
  const run = [
    ev('task.created', 't1', { flow: 'council', prompt: 'Table or file for the retry queue?' }),
    ev('council.started', 't1', { proposition: 'Table or file for the retry queue?', seats: [{ role: 'architect', family: 'claude' }, { role: 'red-team', family: 'gemini' }], homogeneous: false }),
    ev('dag.node.added', 't1', { node_id: 'n1', role: 'architect', depth: 0 }),
    ev('dag.node.added', 't1', { node_id: 'n2', role: 'builder', depth: 1, parent: 'n1' }),
    ev('agent.usage', 't1', { input_tokens: 1000, cache_read_tokens: 3000, output_tokens: 200, thinking_tokens: 50, cost_usd: 0.02 }, { agent: 'architect', span_id: 'n1', model_tier: 'fast', model_id: 'haiku', endpoint_id: 'direct' }),
    ev('agent.usage', 't1', { input_tokens: 500, output_tokens: 100, cost_usd: 0.08 }, { agent: 'architect', span_id: 'n1', model_tier: 'frontier', model_id: 'opus', endpoint_id: 'direct' }),
    ev('council.stance', 't1', { role: 'architect', status: 'answered', text: 'A table, because of transactions.' }),
    ev('council.stance', 't1', { role: 'red-team', status: 'abstained', text: 'timed out' }),
    ev('council.barrier.released', 't1', {}),
    ev('council.critique', 't1', { role: 'red-team', recommendation: 'file', concur: false, findings: [{ severity: 'P0', title: 'lock contention' }] }),
    ev('council.verdict', 't1', { escalated: 'seats split', options: [{ label: 'table', seats: ['architect'] }], unresolved: [['x']].map(() => ({ severity: 'P1', seat: 'red-team', title: 'contention unresolved' })), path: 'missions/m1/council.md' }),
    ev('task.created', 't2', { flow: 'build', prompt: 'build the queue' }),
    ev('build.attempt', 't2', { attempt: 1, tier: 'workhorse', family: 'claude' }),
    ev('build.attempt', 't2', { attempt: 2, tier: 'frontier', family: 'claude', homogeneous_retry: true }),
    ev('worktree.created', 't2', { branch: 'delphi/t2' }),
    ev('gates.evaluated', 't2', { sha: 'abcdef0123456789', gates: [{ name: 'tests', state: 'green', detail: '41 passed' }, { name: 'clippy', state: 'red', detail: '2 warnings' }] }),
    ev('audit.recorded', 't2', { findings: [{ severity: 'P1', title: 'unbounded retry' }] }),
    ev('gate.flaky', 't2', {}),
  ];
  for (const e of run) o.apply(e);
  return o;
}
const ctx = (o: Observer, over: Partial<PanelCtx> = {}): PanelCtx => ({ obs: o, info, pal: { dark: true }, now: 0, cap: 5, pending: 2, mission: 'm1', ...over });
const body = (ui: PanelUi, o: Observer, w = 100, h = 30) => panelBody(ui, ctx(o), w, h).map(plain).join('\n');

test('the fold keeps cost, tokens, tiers and the upscale count the way the Observer does', () => {
  const o = world();
  const a = o.agents.get('architect')!;
  assert.equal(a.calls, 2);
  assert.equal(a.upscaled, true, 'the second call was on a higher tier');
  assert.equal(o.retries, 1);
  assert.ok(Math.abs(o.spentUsd - 0.1) < 1e-9);
  assert.equal(o.tokens.cacheRead, 3000);
  assert.equal(totalTokens(o.tokens), 1000 + 3000 + 200 + 50 + 500 + 100);
  assert.ok(Math.abs(o.cacheHitRatio() - 3000 / 4500) < 1e-9);
  assert.equal(o.nodes.get('n1')!.costUsd, 0.1);
});

test('events at or below the last sequence number are ignored, so a snapshot and the stream can overlap', () => {
  const o = new Observer(['builder']);
  const e = ev('agent.usage', 't1', { cost_usd: 1 }, { agent: 'builder' });
  o.apply(e); o.apply(e); o.apply({ ...e, seq: e.seq - 1 });
  assert.equal(o.spentUsd, 1);
});

test('agent state follows its nodes: running, waiting on you, back to running, then idle or escalated', () => {
  seq = 0;
  const o = new Observer(['builder', 'red-team']);
  o.apply(ev('dag.node.added', 't', { node_id: 'a', role: 'builder' }));
  o.apply(ev('dag.node.added', 't', { node_id: 'b', role: 'builder' }));
  o.apply(ev('dag.node.state', 't', { node_id: 'b', state: 'running' }));
  assert.equal(o.agents.get('builder')!.state, 'running');
  o.apply(ev('dag.node.state', 't', { node_id: 'a', state: 'done' }));
  assert.equal(o.agents.get('builder')!.state, 'running', 'another node of the role still runs');
  o.apply(ev('approval.requested', 't', { approval_id: 'x', role: 'builder', summary: 'write' }));
  assert.equal(o.agents.get('builder')!.state, 'waiting on you');
  o.apply(ev('approval.resolved', 't', { approval_id: 'x' }));
  assert.equal(o.agents.get('builder')!.state, 'running');
  o.apply(ev('dag.node.state', 't', { node_id: 'b', state: 'escalated' }));
  assert.equal(o.agents.get('builder')!.state, 'escalated');
});

test('the default view is the council while one is in progress, the forge while a build runs, else the epic', () => {
  seq = 0;
  const o = new Observer([]);
  assert.equal(o.defaultView(), 'epic');
  o.apply(ev('council.started', 't1', { proposition: 'p', seats: [] }));
  assert.equal(o.defaultView(), 'council');
  o.apply(ev('council.verdict', 't1', {}));
  o.apply(ev('task.created', 't2', { flow: 'build', prompt: 'b' }));
  o.apply(ev('build.attempt', 't2', { attempt: 1 }));
  assert.equal(o.defaultView(), 'forge');
});

test('streamed text is kept per span, bounded, and stripped of escape sequences', () => {
  const o = new Observer([]);
  o.applyFrame({ spans: { s1: { text: 'hello \x1b[31mred\x1b[0m' } } });
  assert.equal(o.live.get('s1'), 'hello red');
  o.applyFrame({ spans: { s1: { text: 'x'.repeat(7000) } } });
  assert.equal(o.live.get('s1')!.length, 6000);
  o.apply(ev('dag.node.added', 't', { node_id: 's1', role: 'builder' }));
  o.apply(ev('dag.node.state', 't', { node_id: 's1', state: 'done' }));
  assert.equal(o.live.has('s1'), false, 'cleared when the node finishes');
});

test('every view renders exactly h lines of at most w columns, at every size', () => {
  const o = world();
  for (const view of VIEWS) for (const [w, h] of [[110, 34], [80, 20], [40, 10]] as const) {
    const lines = panelBody(panelInit(view), ctx(o), w, h);
    assert.equal(lines.length, h, `${view} ${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${view} ${w}: ${plain(l)}`);
  }
});

test('council: seats in columns, blind stance, abstention, critique, escalation with options and the minority report', () => {
  const t = body(panelInit('council'), world(), 110, 34);
  assert.match(t, /Council 1\/1 · t1 · escalated/);
  assert.match(t, /daedalus \(claude\)/);
  assert.match(t, /ares \(gemini\)/);
  assert.match(t, /A table, because of transactions\./);
  assert.match(t, /abstained: timed out/);
  assert.match(t, /Round 2 → file · does not concur/);
  assert.match(t, /P0 lock contention/);
  assert.match(t, /Escalated to you: seats split/);
  assert.match(t, /option: table — architect/);
  assert.match(t, /\[P1\] red-team: contention unresolved/);
  assert.match(t, /saved: missions\/m1\/council\.md/);
});

test('council with nothing to show says how to start one', () => {
  assert.match(body(panelInit('council'), new Observer([])), /No Council yet\.[\s\S]*\/council <proposition>/);
});

test('epic: the tree nests by parent, shows state, tokens and cost, and keeps the narrative underneath', () => {
  const t = body(panelInit('epic'), world(), 110, 30);
  assert.match(t, /t1 council · Table or file/);
  assert.match(t, /daedalus +pending +\d+ tok +\$0\.100/);
  const lines = t.split('\n');
  const parent = lines.findIndex((l) => l.includes('daedalus')), child = lines.findIndex((l) => l.includes('hephaestus'));
  assert.ok(child > parent && lines[child]!.indexOf('hephaestus') > lines[parent]!.indexOf('daedalus'), 'the child is indented under its parent');
  assert.match(t, /── Narrative/);
  assert.match(t, /t1: council needs your decision/);
});

test('forge: attempts, gates, audit, flaky note and the live diff with its stat', () => {
  const o = world();
  const ui = { ...panelInit('forge'), diff: { available: true, stat: ' a.rs | 3 +++', diff: '@@ -1 +1 @@\n-old\n+new', untracked: ['b.rs'] } };
  const t = body(ui, o, 110, 34);
  assert.match(t, /Build 1\/1 · t2 · branch delphi\/t2 · commit abcdef012345 · in progress/);
  assert.match(t, /attempt 2 · frontier · claude \(same family\)/);
  assert.match(t, /✔ tests +41 passed/);
  assert.match(t, /✖ clippy +2 warnings/);
  assert.match(t, /audit P1 unbounded retry/);
  assert.match(t, /flaky/);
  assert.match(t, /a\.rs \| 3 \+\+\+/);
  assert.match(t, /\?\? b\.rs/);
  assert.match(t, /\+new/);
  assert.match(body(panelInit('forge'), o), /no live worktree for this build/);
  assert.match(body(panelInit('forge'), new Observer([])), /No builds yet\./);
});

test('agents: a roster row per enabled role with state, model line and token line', () => {
  const t = body(panelInit('agents'), world(), 110, 34);
  assert.match(t, /athena/); assert.match(t, /hephaestus/);
  assert.match(t, /frontier · opus ⬆ · direct/);
  assert.match(t, /in 1500 · cache 3000 · out 300 · think 50 · \$0\.100 · 2 call\(s\)/);
  assert.match(t, /no call yet/);
});

test('spend: sparkline, token bars, cache hit, per-agent shares and the ceiling', () => {
  const t = body(panelInit('spend'), world(), 110, 34);
  assert.match(t, /2 call\(s\) · total \$0\.100 · 2% of the \$5\.00 ceiling/);
  assert.match(t, /cache read +3000 +▓/);
  assert.match(t, /prompt cache hit 67% of input-side tokens · retries 1/);
  assert.match(t, /daedalus +▓+░* +\$0\.100/);
  assert.equal(sparkline([0, 0], 10), '▁▁');
  assert.equal(sparkline([1, 2, 4], 2), '▅█');
});

test('the footer line shows spend against the ceiling and the proposals waiting', () => {
  const f = plain(panelFooter(ctx(world()), 120));
  assert.match(f, /spend \$0\.100 \/ \$5\.00 ▓?░+ · tokens 5\.?\d*k?|spend \$0\.100 \/ \$5\.00/);
  assert.match(f, /cache 67% · retries 1 · 2 to \/ratify/);
  assert.match(plain(panelFooter(ctx(world(), { cap: 0, pending: 0 }), 120)), /spend \$0\.100 \/ \?/);
});

test('keys: shift+arrows and tab cycle the views, digits jump, esc and q close', () => {
  const o = world();
  let r = panelKey(panelInit('council'), key('right', { shift: true }), o);
  assert.equal(r.ui.view, 'epic');
  r = panelKey(r.ui, key('left', { shift: true }), o); r = panelKey(r.ui, key('left', { shift: true }), o);
  assert.equal(r.ui.view, 'connectors', 'wraps around');
  assert.equal(panelKey(r.ui, text('3'), o).ui.view, 'forge');
  assert.equal(panelKey(panelInit('spend'), key('tab'), o).ui.view, 'archives');
  assert.deepEqual(panelKey(panelInit('spend'), key('esc'), o).act, { t: 'close' });
  assert.deepEqual(panelKey(panelInit('spend'), text('q'), o).act, { t: 'close' });
});

test('keys: left/right pick another build and ask for its diff; r refreshes; epic up/down select; others scroll', () => {
  seq = 100;
  const o = world();
  o.apply(ev('build.attempt', 't3', { attempt: 1 }));
  let r = panelKey(panelInit('forge'), key('left'), o);
  assert.equal(r.ui.pick, 0);
  assert.deepEqual(r.act, { t: 'diff', task: 't2' });
  assert.equal(selectedBuild(r.ui, o)!.taskId, 't2');
  r = panelKey(r.ui, key('right'), o);
  assert.equal(r.ui.pick, -1, 'back on the newest');
  assert.deepEqual(panelKey(panelInit('forge'), text('r'), o).act, { t: 'diff', task: 't3' });
  assert.equal(panelKey(panelInit('epic'), key('down'), o).ui.node, 1);
  assert.equal(panelKey(panelInit('epic'), key('up'), o).ui.node, 0);
  assert.equal(panelKey(panelInit('spend'), key('down'), o).ui.scroll, 1);
  assert.equal(panelKey(panelInit('spend'), key('pgdn'), o).ui.scroll, 10);
  assert.equal(panelKey(panelInit('spend'), key('up'), o).ui.scroll, 0);
});

test('archives: / starts a search, typing builds the query, enter asks, hits are selectable with a preview', () => {
  const o = world();
  let r = panelKey(panelInit('archives'), text('/'), o);
  assert.equal(r.ui.kb.editing, true);
  r = panelKey(r.ui, text('retry'), o); r = panelKey(r.ui, text(' q'), o); r = panelKey(r.ui, key('backspace'), o);
  assert.equal(r.ui.kb.query, 'retry ');
  assert.match(plain(panelHint(r.ui)), /enter search/);
  const asked = panelKey(r.ui, key('enter'), o);
  assert.deepEqual(asked.act, { t: 'search', q: 'retry' });
  assert.equal(asked.ui.kb.editing, false);
  const ui: PanelUi = { ...asked.ui, kb: { ...asked.ui.kb, note: '', hits: [{ pointer: 'specs/002.md#4', heading: 'Retry ladder', preview: 'Escalate the tier after two failures.' }, { pointer: 'REVIEW.md#1', heading: 'Rules', preview: 'second preview' }] } };
  let t = body(ui, o);
  assert.match(t, /❯ 1\. specs\/002\.md#4 +Retry ladder/);
  assert.match(t, /Escalate the tier after two failures\./);
  const next = panelKey(ui, key('down'), o).ui;
  t = body(next, o);
  assert.match(t, /❯ 2\. REVIEW\.md#1/);
  assert.match(t, /second preview/);
  assert.doesNotMatch(t, /Escalate the tier/);
  assert.equal(panelKey({ ...panelInit('archives'), kb: { ...panelInit('archives').kb, editing: true } }, key('esc'), o).ui.kb.editing, false);
  assert.match(body({ ...panelInit('archives'), kb: { ...ui.kb, hits: [], note: 'no results' } }, o), /no results/);
});

test('view names and numbers are parsed', () => {
  assert.equal(parseView('Chamber'), 'council'); assert.equal(parseView('kb'), 'archives'); assert.equal(parseView('5'), 'spend'); assert.equal(parseView('nope'), null);
});

test('the left deck shows live spans per agent and recent tool calls, from memory only', () => {
  const o = world();
  o.applyFrame({ spans: { n1: { text: 'Considering the options\nfor the queue' } } });
  const base: Model = { info, pal: { dark: true }, state: initial(0), now: 0, deck: 0, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null, queue: [], help: false, inbox: inboxInit(), mode: 'default', obs: o, panel: null };
  const t = thinkingBody(base, 80, 20).map(plain).join('\n');
  assert.match(t, /memory only, never saved/);
  assert.match(t, /daedalus · n1/);
  assert.match(t, /Considering the options/);
  assert.match(thinkingBody({ ...base, obs: new Observer([]) }, 80, 10).map(plain).join('\n'), /nothing running/);
  for (const [w, h] of [[80, 20], [40, 6]] as const) {
    const l = thinkingBody(base, w, h);
    assert.equal(l.length, h);
    for (const x of l) assert.ok(stringWidth(x) <= w, plain(x));
  }
});

test('a panel takes the body and the prompt row, and the screen stays exactly w by h', () => {
  const o = world();
  const m: Model = { info, pal: { dark: true }, state: initial(0), now: 0, deck: 1, composer: C.insert(C.empty(), 'my draft'), back: 0, menu: null, expandTools: false, search: null, queue: [], help: false, inbox: inboxInit(), mode: 'default', obs: o, panel: { ui: panelInit('spend'), cap: 5, pending: 0, mission: 'm1' } };
  for (const [w, h] of [[100, 30], [60, 14]] as const) {
    const lines = frame(m, w, h);
    assert.equal(lines.length, h);
    for (const l of lines) assert.ok(stringWidth(l) <= w);
  }
  const out = frame(m, 100, 30).map(plain).join('\n');
  assert.match(out, /SPEND TELEMETRY · mission m1/);
  assert.match(out, /Spend 5\/9 · tab or shift\+←→ view/);
  assert.doesNotMatch(out, /my draft/);
});

test('keys that arrive in one read are taken one at a time, except while typing a query', () => {
  const o = world();
  const r = panelKey(panelInit('archives'), text('/abc'), o);
  assert.equal(r.ui.kb.editing, true);
  assert.equal(r.ui.kb.query, 'abc', 'after the slash everything typed is the query');
  assert.equal(panelKey(panelInit('spend'), text('31'), o).ui.view, 'council', 'digits are keys too, applied in order');
});

// ── phase 5: sources, notebook, connectors ──────────────────────────────────

const blocks = (): Block[] => [
  { id: 1, kind: 'user', text: 'Compare @specs/002-adapter-and-routing.md with REVIEW.md please', at: 1 },
  { id: 2, kind: 'reply', role: 'architect', text: 'See specs/002-adapter-and-routing.md#4 and https://example.com/specs/zzz.md. Also memory/agents/architect.md.', at: 2 },
  { id: 3, kind: 'tool', role: 'architect', text: 'read_file(specs/002-adapter-and-routing.md)', at: 3 },
  { id: 4, kind: 'note', text: 'ignore specs/never.md in notes', at: 4 },
];

test('pointers are read out of replies, tool calls and mentions; URLs and ../ are ignored', () => {
  assert.deepEqual(extractPointers('a specs/x.md#top b REVIEW.md c ../specs/y.md d https://h/specs/z.md e context/../specs/q.md'), [{ path: 'specs/x.md', anchor: 'top' }, { path: 'REVIEW.md', anchor: '' }]);
});

test('sources are counted per document, with anchors, who referenced them and how; notes are not evidence', () => {
  const s = collectSources(blocks(), [{ pointer: 'context/glossary.md#term' }], (r) => (r === 'architect' ? 'daedalus' : r));
  const top = s[0]!;
  assert.equal(top.path, 'specs/002-adapter-and-routing.md');
  assert.equal(top.count, 3);
  assert.deepEqual(top.anchors, ['4']);
  assert.deepEqual(top.by, ['you', 'daedalus']);
  assert.deepEqual(top.how.sort(), ['asked', 'said', 'tool']);
  assert.ok(s.some((x) => x.path === 'REVIEW.md' && x.by[0] === 'you'));
  assert.ok(s.some((x) => x.path === 'memory/agents/architect.md'));
  assert.ok(s.some((x) => x.path === 'context/glossary.md' && x.how[0] === 'search'));
  assert.ok(!s.some((x) => x.path.includes('never') || x.path.includes('zzz')));
});

test('a source is previewed from the workspace without its frontmatter, and cannot escape it', () => {
  const ws = mkdtempSync(join(tmpdir(), 'ui-src-'));
  mkdirSync(join(ws, 'team/specs'), { recursive: true });
  writeFileSync(join(ws, 'team/specs/a.md'), '---\nid: a\n---\n\n# Title\n\nbody \x1b[31mred\x1b[0m\n');
  writeFileSync(join(ws, 'secret.txt'), 'nope');
  assert.deepEqual(previewOf(ws, 'specs/a.md'), ['# Title', '', 'body red', '']);
  assert.deepEqual(previewOf(ws, '../secret.txt'), ['(not found in this workspace)']);
  assert.deepEqual(previewOf(ws, 'specs/missing.md'), ['(not found in this workspace)']);
  assert.equal(resolveSource(ws, '/etc/passwd'), null);
  rmSync(ws, { recursive: true });
});

test('the sources view lists, selects and previews', () => {
  const o = new Observer([]);
  const sources = collectSources(blocks());
  const c = ctx(o, { sources, preview: (rel) => [`first line of ${rel}`, 'second line'] });
  let ui = panelInit('sources');
  let t = panelBody(ui, c, 110, 24).map(plain).join('\n');
  assert.match(t, /Sources · 3/);
  assert.match(t, /❯  1\. specs\/002-adapter-and-routing\.md +#4 +×3 · asked\/said\/tool · you, architect/);
  assert.match(t, /first line of specs\/002-adapter-and-routing\.md/);
  ui = panelKey(ui, key('down'), o).ui;
  t = panelBody(ui, c, 110, 24).map(plain).join('\n');
  assert.match(t, /first line of (REVIEW|memory)/);
  assert.equal(panelKey(panelInit('sources'), key('up'), o).ui.src, 0);
  assert.match(panelBody(panelInit('sources'), ctx(o, { sources: [] }), 100, 12).map(plain).join('\n'), /Nothing referenced yet/);
});

test('the notebook appends pins to one document, reads them back newest first, and survives headings in the text', () => {
  const ws = mkdtempSync(join(tmpdir(), 'ui-nb-'));
  assert.deepEqual(readNotebook(ws, 'm1'), []);
  const file = addPin(ws, 'm1', { source: 'daedalus (architect)', text: 'Use a table.\n\n## Why\n\nTransactions.' }, new Date('2026-10-24T10:00:00Z'));
  addPin(ws, 'm1', { source: 'note', text: 'check lock contention' }, new Date('2026-10-24T11:30:00Z'));
  const raw = readFileSync(file, 'utf8');
  assert.match(raw, /^---\nid: notebook-m1\ntitle: "Mission notebook"\nauthor: operator\n/);
  assert.equal(raw.match(/^---$/gm)!.length, 2, 'one frontmatter block, however many pins');
  const pins = readNotebook(ws, 'm1');
  assert.equal(pins.length, 2);
  assert.deepEqual(pins[0], { at: '2026-10-24T10:00:00.000Z', source: 'daedalus (architect)', text: 'Use a table.\n\n## Why\n\nTransactions.' });
  assert.equal(pins[1]!.text, 'check lock contention');
  const t = panelBody(panelInit('notebook'), ctx(new Observer([]), { notebook: pins }), 100, 20).map(plain).join('\n');
  assert.ok(t.indexOf('check lock contention') < t.indexOf('Use a table.'), 'newest first');
  assert.match(t, /2 · 2026-10-24 11:30 · note/);
  assert.match(panelBody(panelInit('notebook'), ctx(new Observer([])), 100, 12).map(plain).join('\n'), /Nothing pinned yet\./);
  rmSync(ws, { recursive: true });
});

test('connectors: configured ones show tools and pin state; events move a connector between ready, needs login and unavailable', () => {
  seq = 0;
  const o = new Observer([]);
  o.setConnectors([{ name: 'drive', url: 'https://mcp.example/drive', account: 'a@b.c', login: 'google', roles: ['scout'], tools: [{ name: 'search', class: 'read', pinned: true }, { name: 'upload', class: 'write', pinned: false }] }]);
  let t = panelBody(panelInit('connectors'), ctx(o), 110, 24).map(plain).join('\n');
  assert.match(t, /drive +○ idle/);
  assert.match(t, /account a@b\.c · login google · roles scout/);
  assert.match(t, /search +read +pinned/);
  assert.match(t, /upload +write +not ratified: delphi connectors pin drive, then \/ratify/);
  o.apply(ev('connector.call', 't', { connector: 'drive', tool: 'search' }, { timestamp: '2026-10-24T10:00:00Z' }));
  o.apply(ev('connector.result', 't', { connector: 'drive', tool: 'search', is_error: false }));
  o.apply(ev('connector.denied', 't', { connector: 'drive', tool: 'upload', reason: 'not available to the scout' }));
  t = panelBody(panelInit('connectors'), ctx(o), 110, 24).map(plain).join('\n');
  assert.match(t, /drive +● ready/);
  assert.match(t, /1 call\(s\) · 1 result\(s\) · 1 refused · last search/);
  o.apply(ev('connector.reauth_required', 't', { connector: 'drive', reason: 'token expired' }));
  assert.match(panelBody(panelInit('connectors'), ctx(o), 110, 24).map(plain).join('\n'), /✖ needs login · token expired/);
  o.apply(ev('connector.unavailable', 't', { connector: 'drive', reason: 'listing the tools timed out' }));
  assert.match(panelBody(panelInit('connectors'), ctx(o), 110, 24).map(plain).join('\n'), /✖ unavailable · listing the tools timed out/);
  assert.ok(o.narrative.some((l) => l.includes('drive needs you to sign in again')));
  assert.match(panelBody(panelInit('connectors'), ctx(new Observer([])), 100, 12).map(plain).join('\n'), /No connectors are configured/);
});

test('an event for a connector the snapshot did not list still gets a row', () => {
  seq = 0;
  const o = new Observer([]);
  o.apply(ev('connector.call', 't', { connector: 'slack', tool: 'post' }));
  assert.equal(o.connectors.get('slack')!.calls, 1);
});

test('alt+arrows move between views too, for terminals that do not send shift+arrows', () => {
  const o = world();
  assert.equal(panelKey(panelInit('council'), key('right', { alt: true }), o).ui.view, 'epic');
  assert.equal(panelKey(panelInit('council'), key('left', { alt: true }), o).ui.view, 'connectors');
});
