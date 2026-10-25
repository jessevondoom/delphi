import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { observeFrame, type OModel, type OState } from '../src/observe-screen.ts';
import { plan } from '../src/epic-rail.ts';
import { emptyState } from '../src/observe-screen.ts';
import { epicWorld, info, NOW, T0 } from './epic-fixture.ts';
import { snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const model = (st: OState, over: Partial<OModel> = {}): OModel => ({ info, pal: { dark: true }, st, view: 'epic', pick: 0, scroll: 0, selNode: 0, search: { editing: false, query: '', hits: [], note: '' }, diff: null, live: {}, connected: true, still: true, now: NOW, ...over });
const frame = (st: OState, w: number, h: number, over: Partial<OModel> = {}) => observeFrame(model(st, over), w, h);
const min = (n: number) => T0 + n * 60_000;
const ROW = /^ [✔▶○✖] (daedalus|odysseus|hephaestus|ares|athena|hermes)/;

test('golden: the epic rail at 80x24, 100x34 and as a list at 60x20', () => {
  for (const [w, h] of [[80, 24], [100, 34], [60, 20]] as const) {
    const r = snapshot(`epic-rail-${w}x${h}`, frame(epicWorld(), w, h));
    assert.ok(r.ok, r.message);
  }
  const r = snapshot('epic-rail-empty-80x24', frame(emptyState(), 80, 24));
  assert.ok(r.ok, r.message);
});

test('the waterline is a fixed column: round(0.52 * rail width), on every row', () => {
  for (const w of [72, 80, 100, 120]) {
    const lines = frame(epicWorld(), w, 30).map(plain);
    const at = 22 + Math.round(0.52 * (w - 22));
    const rows = lines.filter((l) => ROW.test(l));
    assert.equal(rows.length, 6, `${w}: one row per node`);
    for (const row of rows) assert.ok(['┆', '●', '╮', '╰', '│', '━', '═', '╌'].includes([...row][at]!), `${w}: column ${at} of ${row}`);
    assert.equal([...lines.find((l) => l.includes('●'))!].indexOf('●'), at, `${w}: the running bar ends in a dot on the waterline`);
  }
});

test('done work is a dim bar, running work ends in a dot, queued work is dashed, the last queued ends in an arrow', () => {
  const rows = frame(epicWorld(), 80, 24).map(plain).filter((l) => ROW.test(l));
  assert.match(rows[0]!, /^ ✔ daedalus\s+━+╮/);
  assert.match(rows[1]!, /^ ✔ odysseus\s+╰━+╮/);
  assert.match(rows[2]!, /^ ▶ hephaestus\s+╰━+●/);
  assert.match(rows[3]!, /^ ○ ares\s+┆╰═+╮/, 'a queued task on the critical path is double');
  assert.match(rows[4]!, /═+▸$/, 'the last queued task ends in an arrow');
  assert.match(rows[5]!, /╰╌+\s*$/, 'queued work off the critical path is dashed');
});

test('dependencies are elbows in the same column: ╮ where the predecessor ends, ╰ on the dependent row', () => {
  const rows = frame(epicWorld(), 80, 24).map(plain).filter((l) => ROW.test(l)).map((l) => [...l]);
  for (const [a, b] of [[0, 1], [1, 2]] as const) {
    const col = rows[a]!.indexOf('╮');
    assert.ok(col > 22, `row ${a} has an elbow`);
    assert.equal(rows[b]![col], '╰', `row ${b} turns at the same column`);
  }
});

test('narrative: now, next and the critical path with its length', () => {
  const t = frame(epicWorld(), 80, 24).map(plain);
  assert.match(t.find((l) => l.startsWith(' now'))!, /^ now {3}⟦•─•⟧ hephaestus is working: Add a retry queue to the claim loop/);
  assert.match(t.find((l) => l.startsWith(' next'))!, /^ next {2}⟦-_-⟧ ares starts when hephaestus ends/);
  assert.match(t.find((l) => l.startsWith(' path'))!, /^ path {2}hephaestus → ares → athena {2}about \d+ min/);
  const idle = epicWorld();
  for (const n of Object.values(idle.nodes)) { n.state = 'done'; n.ended_ms = (n.started_ms ?? 0) + 60_000; }
  const l = frame(idle, 80, 24).map(plain);
  assert.match(l.find((x) => x.startsWith(' now'))!, /nothing is running/);
  assert.match(l.find((x) => x.startsWith(' next'))!, /nothing queued/);
  assert.match(l.find((x) => x.startsWith(' path'))!, /everything has finished/);
});

test('pace is the last body row, right-aligned with one trailing space; the last twelve minutes, one cell each', () => {
  const lines = frame(epicWorld(), 80, 24).map(plain);
  const body = lines.slice(0, -2); // the hairline and the footer follow
  const last = body.at(-1)!;
  assert.match(last, /pace {2}[▁▂▃▄▅▆▇█]{12} $/);
  assert.equal(stringWidth(last), 80);
  const none = epicWorld(); none.pace = [];
  assert.ok(!frame(none, 80, 24).map(plain).join('\n').includes('pace'), 'no pace data, no pace row');
});

test('estimates: the median duration of finished work of the same role, else three minutes', () => {
  const st = epicWorld();
  const node = (id: string, role: string, state: string, a: number, b: number | null, after: string[]) => ({ id, role, parent: null, depth: 0, task_id: 't_1', state, tokens: 0, cost_usd: 0, started_ms: min(a), ended_ms: b === null ? null : min(b), after });
  st.nodes = {
    a: node('a', 'strategist', 'done', 0, 6, []),
    b: node('b', 'strategist', 'done', 6, 10, ['a']),
    c: node('c', 'strategist', 'pending', 10, null, ['b']),
    d: node('d', 'scout', 'pending', 10, null, ['b']),
  };
  const items = Object.fromEntries(plan(st, min(10)).map((i) => [i.node.id, i]));
  assert.equal(items.c!.end - items.c!.start, 5 * 60_000, 'median of 6 and 4 minutes');
  assert.equal(items.d!.end - items.d!.start, 3 * 60_000, 'no finished scout: three minutes');
  assert.equal(items.c!.start, min(10), 'a queued task starts when its predecessor ends, never in the past');
});

test('the critical path is the longest chain to the end, from running or queued work only', () => {
  const items = plan(epicWorld(), NOW);
  const crit = items.filter((i) => i.critical).map((i) => i.node.id);
  assert.deepEqual(crit.sort(), ['n3', 'n4', 'n5']);
  assert.ok(!items.find((i) => i.node.id === 'n1')!.critical, 'finished work is not on it');
});

test('below 72 columns the rail gives way to one row per task: glyph, name, state, elapsed', () => {
  const t = frame(epicWorld(), 60, 20).map(plain).join('\n');
  assert.match(t, /✔ daedalus\s+done\s+7:00/);
  assert.match(t, /▶ hephaestus\s+running\s+5:00/);
  assert.match(t, /○ ares\s+pending\s+queued/);
  assert.ok(!/[━═╌┆]/.test(t), 'no rail');
  assert.match(t.split('\n').filter((l) => l.trim()).at(-3)!, /pace/);
});

test('up and down scroll the rows; the narrative and the pace row stay', () => {
  const st = epicWorld();
  for (let i = 0; i < 12; i++) st.nodes[`x${i}`] = { id: `x${i}`, role: 'scout', parent: null, depth: 0, task_id: 't_1', state: 'done', tokens: 0, cost_usd: 0, started_ms: min(3 + i * 0.1), ended_ms: min(4 + i * 0.1), after: [] };
  const top = frame(st, 80, 20).map(plain);
  const down = frame(st, 80, 20, { scroll: 4 }).map(plain);
  assert.notDeepEqual(top, down, 'the rows moved');
  for (const l of [top, down]) {
    assert.ok(l.some((x) => x.startsWith(' now')) && l.some((x) => x.startsWith(' path')), 'narrative stays');
    assert.match(l.slice(0, -2).at(-1)!, /pace/, 'pace stays on the last body row');
    assert.equal(l.length, 20);
  }
});

test('model text cannot widen or break the rail, at any size', () => {
  const st = epicWorld();
  st.tasks['t_1']!.prompt = 'evil \x1b[2J\x1b]0;x\x07 ' + 'wide '.repeat(60) + '日本語';
  const r = { ...info, roles: { ...info.roles, builder: { ...info.roles.builder!, name: 'hephaestus-with-a-very-long-name-that-must-clip' } } };
  for (const [w, h] of [[28, 9], [40, 12], [60, 20], [72, 16], [80, 24], [140, 50], [200, 9]] as const) {
    const lines = observeFrame(model(st, { info: r }), w, h);
    assert.equal(lines.length, h, `${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${w}x${h}: ${plain(l)}`);
    assert.ok(!lines.join('\n').includes('\x1b[2J') && !lines.join('\n').includes('\x1b]0;'), 'no cursor movement from model text');
  }
});
