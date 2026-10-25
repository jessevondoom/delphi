// The council view, "Weave" (`specs/005e` §9.2, slice 9 of 005f): the three phases of the reference frames, the sealed
// blind round, the cards at every width, escalation and hostile text.
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import chalk from 'chalk';
import stringWidth from 'string-width';
import { emptyState, observeFrame, type OModel, type OState } from '../src/observe-screen.ts';
import { phaseOf, positionOf, weave } from '../src/weave.ts';
import * as T from '../src/theme.ts';
import { info, plain, snapshot } from './helpers.ts';

const T0 = 1_000_000_000;
const SEATS = [{ role: 'architect', family: 'a' }, { role: 'strategist', family: 'b' }, { role: 'scout', family: 'c' }];
const tk = { input: 14_200, cache_read: 0, cache_write: 0, output: 0, thinking: 0 };

/** One council of three seats in the given phase, built the way the daemon's `observe.state` reports it. */
function world(phase: 'round 1' | 'round 2' | 'verdict', over: Partial<OState['councils'][number]> = {}): OState {
  const s = emptyState();
  s.tokens = tk;
  for (const r of ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout']) s.agents[r] = { role: r, state: 'idle', task_id: '', endpoint: '', tier: '', model: '', upscaled: false, tokens: tk, cost_usd: 0, calls: 0 };
  const node = (id: string, role: string, start: number, end: number | null) => { s.nodes[id] = { id, role, parent: null, depth: 0, task_id: 't_2', state: end ? 'done' : 'running', tokens: 0, cost_usd: 0, started_ms: T0 + start, ended_ms: end ? T0 + end : null, after: [] }; };
  const stance = (role: string, option: string, reason: string) => ({ role, answered: true, text: `OPTION: ${option}\n${reason}` });
  const reasons: Record<string, [string, string]> = { architect: ['table', 'atomic claims and a one-query audit'], strategist: ['table', 'a table is already the house pattern'], scout: ['file', 'a file needs no migration'] };
  if (phase === 'round 1') {
    node('n_a', 'architect', 0, 9000); node('n_s', 'strategist', 0, 11_000); node('n_c', 'scout', 0, null);
    s.councils = [{ task_id: 't_2', proposition: 'Should the retry queue be a table or a file?', seats: [...SEATS], homogeneous: false, phase, stances: [stance('architect', ...reasons.architect!), stance('strategist', ...reasons.strategist!)], critiques: [], escalation: null, path: '', unresolved: [], options: [], ...over }];
  } else {
    const done = phase === 'verdict';
    node('n_a', 'architect', 0, 9000); node('n_s', 'strategist', 0, 11_000); node('n_c', 'scout', 0, done ? 40_000 : null);
    s.councils = [{ task_id: 't_2', proposition: 'Should the retry queue be a table or a file?', seats: [...SEATS], homogeneous: false, phase, stances: SEATS.map((x) => stance(x.role, ...reasons[x.role]!)), critiques: done ? SEATS.map((x) => ({ role: x.role, recommendation: reasons[x.role]![0], concur: x.role !== 'scout', findings: [] as [string, string][] })) : [{ role: 'architect', recommendation: 'table', concur: true, findings: [] }, { role: 'strategist', recommendation: 'table', concur: true, findings: [] }], escalation: null, path: '', unresolved: [], options: [], ...over }];
    if (done) for (const n of Object.values(s.nodes)) { n.state = 'done'; n.ended_ms = T0 + 48_000; }
  }
  return s;
}

const model = (st: OState, over: Partial<OModel> = {}): OModel => ({
  info, pal: { dark: true, bg: [15, 23, 42] }, st, view: 'council', pick: 0, scroll: 0, selNode: 0, search: { editing: false, query: '', hits: [], note: '' }, diff: null,
  live: {}, connected: true, now: T0 + 19_000, still: true, ...over,
});
const lines = (m: OModel, w = 80, h = 24) => observeFrame(m, w, h).map(plain);
const check = (name: string, m: OModel, w = 80, h = 24) => { const r = snapshot(name, observeFrame(m, w, h)); assert.ok(r.ok, r.message); };

test('golden: the three phases at 80x24, as the reference frames (005e 9.2)', () => {
  check('council-blind-80x24', model(world('round 1')));
  check('council-debate-80x24', model(world('round 2'), { now: T0 + 34_000, live: { n_c: 'migration cost is zero but the lock file needs careful fsync ordering and a recovery story' } }));
  check('council-verdict-80x24', model(world('verdict'), { now: T0 + 48_000 }));
});

test('golden: wide, narrow and stacked, and without a tint (the rail)', () => {
  check('council-blind-110x34', model(world('round 1')), 110, 34);
  check('council-debate-60x30', model(world('round 2'), { now: T0 + 34_000 }), 60, 30);
  check('council-verdict-40x24', model(world('verdict'), { now: T0 + 48_000 }), 40, 24);
  check('council-debate-rail-80x24', model(world('round 2'), { now: T0 + 34_000, pal: { dark: true, bg: null } }));
});

test('the blind round: sealed cards, the shutter, a ten-cell meter, and no stance text anywhere on screen', () => {
  const l = lines(model(world('round 1')));
  const all = l.join('\n');
  assert.match(all, /question\s+Should the retry queue be a table or a file\?/);
  assert.match(all, /round\s+● blind ╌{9} ○ debate ╌{9} ○ verdict\s+00:19 $/m);
  assert.equal((all.match(/sealed\b/g) ?? []).length, 4, 'three cards and the verdict card say sealed');
  assert.match(all, /✔ answered 00:09/);
  assert.match(all, /✔ answered 00:11/);
  assert.match(all, /● thinking 00:19/);
  assert.match(all, /▓{20}/);
  assert.match(all, /[░▒]{10}/);
  assert.match(all, /sealed until all three answer/);
  assert.match(all, /2 of 3 answered {2}▰{7}▱{3}/);
  for (const stance of ['atomic claims', 'house pattern', 'migration', 'table', 'file needs'].filter((x) => x !== 'table')) assert.ok(!all.includes(stance), `a stance leaked: ${stance}`);
  assert.ok(!/OPTION|\btable\b(?! or a file)/.test(all.replace(/Should the retry queue be a table or a file\?/, '')), 'no option word either');
});

test('the blind round never reads a stance, even one the daemon already sent: an unanswered seat shows nothing', () => {
  const st = world('round 1');
  st.councils[0]!.stances.push({ role: 'scout', answered: false, text: 'OPTION: file\nSECRET-REASON' });
  const m = model(st, { live: { n_c: 'SECRET-LIVE-TEXT' } });
  assert.ok(!lines(m).join('\n').includes('SECRET'));
  assert.ok(!lines(m, 40, 30).join('\n').includes('SECRET'));
});

test('the debate: stances revealed, who is answering, the live tail with a cursor, the positions', () => {
  const m = model(world('round 2'), { now: T0 + 34_000, live: { n_c: 'migration cost is zero but the lock file needs careful fsync ordering' } });
  const all = lines(m).join('\n');
  assert.match(all, /✔ blind ━{9} ● debate ╌{9} ○ verdict\s+00:34 $/m);
  assert.match(all, /atomic claims and a/);
  assert.match(all, /a table is already/);
  assert.match(all, /● answering \w+/, 'the speaking seat');
  assert.match(all, /▊/, 'the cursor');
  assert.match(all, /none yet · round 2 of 3/);
  assert.match(all, /positions\s+table 2 · file 1\s+\w+ is replying/);
  assert.ok(!all.includes('sealed'));
});

test('the verdict: agreed or dissent noted, the verdict card, success faces in the strip, done with the time', () => {
  const l = lines(model(world('verdict'), { now: T0 + 48_000 }));
  const all = l.join('\n');
  assert.match(all, /✔ blind ━{9} ✔ debate ━{9} ✔ verdict\s+done 00:48 $/m);
  assert.equal((all.match(/✔ agreed/g) ?? []).length, 2);
  assert.equal((all.match(/✔ dissent noted/g) ?? []).length, 1);
  assert.match(all, /verdict\s+table\s+2 of 3 · one round/);
  assert.match(all, /agreement\s+▰{7}▱{3}\s+minority: \w+, noted/);
  assert.match(l[0]!, /⟦\^─\^⟧.*⟦\^─\^⟧.*⟦\^/, 'seats show the success face');
  assert.match(l.at(-1)!, /^3 agents  ·  14\.2k tokens/);
});

test('an escalated council: the verdict card needs you, the track waits, and the attention slot is the band', () => {
  const st = world('verdict', { escalation: 'deadlock: no majority', options: [['table', ['architect']], ['file', ['strategist', 'scout']]] });
  st.councils[0]!.critiques = st.councils[0]!.seats.map((x) => ({ role: x.role, recommendation: x.role === 'architect' ? 'table' : 'file', concur: false, findings: [] }));
  const l = lines(model(st, { now: T0 + 48_000 }));
  const all = l.join('\n');
  assert.match(l[3]!, /▼ needs you/);
  assert.match(all, /verdict\s+needs you/);
  assert.match(all, /options\s+table \(.*\) · file \(/);
  assert.match(all, /● verdict|○ verdict/);
  assert.match(all, /needs you\s*$/m);
  assert.ok(!all.includes('✔ agreed'));
});

test('phases, positions and the way seats are read', () => {
  const c = world('round 1').councils[0]!;
  assert.deepEqual(['round 1', 'round 2', 'verdict', 'escalated', 'closed'].map((p) => phaseOf({ ...c, phase: p })), ['blind', 'debate', 'verdict', 'verdict', 'verdict']);
  assert.deepEqual(positionOf('OPTION: table\nbecause\nof reasons'), { option: 'table', reason: 'because of reasons' });
  assert.deepEqual(positionOf('option : File'), { option: 'File', reason: '' });
  assert.deepEqual(positionOf('just words\nmore'), { option: 'just words', reason: 'more' });
  assert.deepEqual(positionOf(''), { option: '', reason: '' });
});

test('geometry: cards are 24 cells with two between them at 80; the verdict card is 76; nothing is wider than the window', () => {
  const l = lines(model(world('round 1')));
  const caps = l.filter((x) => /^ *▄+( +▄+)*$/.test(x)).filter((_, i) => i % 2 === 0); // tops: each bottom cap draws as ▄ too
  assert.equal(caps.length, 2, 'the card row and the verdict card');
  assert.deepEqual(caps[0]!.trim().split(/ +/).map((x) => x.length), [24, 24, 24]);
  assert.equal(caps[0]!.match(/▄ +▄/)![0].length, 4, 'two cells apart');
  assert.equal(caps[1]!.trim().length, 76);
  for (const [w, h] of [[120, 40], [100, 30], [80, 24], [72, 24], [71, 30], [60, 30], [48, 20], [40, 14], [28, 12]] as const) {
    for (const [name, st] of [['blind', world('round 1')], ['debate', world('round 2')], ['verdict', world('verdict')]] as const) {
      const out = observeFrame(model(st, { now: T0 + 30_000 }), w, h);
      assert.equal(out.length, h, `${name} ${w}x${h}`);
      for (const x of out) assert.ok(stringWidth(x) <= w, `${name} ${w}x${h}: ${plain(x)}`);
    }
  }
});

test('below 72 columns the cards stack, one per row', () => {
  const l = lines(model(world('round 2'), { now: T0 + 34_000 }), 60, 40);
  const caps = l.filter((x) => /^ *▄+ *$/.test(x)).filter((_, i) => i % 2 === 0);
  assert.equal(caps.length, 4, 'three seat cards and the verdict card, one per row');
  assert.ok(l.every((x) => !/▄ +▄/.test(x)), 'never two side by side');
});

test('more than three seats wrap onto a second row of cards', () => {
  const st = world('round 1');
  st.councils[0]!.seats.push({ role: 'red-team', family: 'd' });
  const l = lines(model(st), 100, 40);
  const capRows = l.filter((x) => /^ *▄+( +▄+)*$/.test(x)).filter((_, i) => i % 2 === 0); // tops: each bottom cap draws as ▄ too
  assert.equal(capRows.length, 3, 'two rows of cards and the verdict card');
  assert.deepEqual(capRows[1]!.trim().split(/ +/).length, 1, 'the second row holds the fourth seat');
});

test('when it does not fit, the cards scroll and the question and track stay', () => {
  const st = world('round 2');
  const tall = observeFrame(model(st), 80, 14);
  const top = tall.map(plain);
  assert.match(top[3]!, /nothing needs you|needs you/);
  assert.match(top.join('\n'), /question/);
  assert.match(top.join('\n'), /round\s+✔ blind/);
  const down = observeFrame(model(st, { scroll: 99 }), 80, 14).map(plain).join('\n');
  assert.match(down, /question/);
  assert.match(down, /verdict\s+none yet/, 'scrolled to the end, the verdict card is in view');
  assert.match(top.join('\n'), /↓ \d+ more/, 'the first view says there is more below');
  assert.doesNotMatch(top.join('\n'), /↑/, 'and nothing above');
  assert.match(down, /↑ \d+ above/, 'scrolled down it says what is above');
  assert.doesNotMatch(down, /↓ \d+ more/, 'and nothing below');
});

test('no tint means a rail: no caps and no fill, the same words', () => {
  const l = lines(model(world('round 1'), { pal: { dark: true, bg: null } }));
  const all = l.join('\n');
  assert.ok(!/[▄▀]/.test(all.replace(/▓|░|▒/g, '')), 'no caps');
  assert.match(all, /│ ✔ answered 00:09|│ *✔ answered 00:09/);
  assert.match(all, /sealed until all three answer/);
});

test('the speaking seat has the brighter tint', () => {
  const prev = T.currentCapabilities(), lvl = chalk.level;
  T.useCapabilities(T.capabilities({ COLORTERM: 'truecolor' }));
  try {
    const raw = observeFrame(model(world('round 1')), 80, 24);
    const bg = (row: string) => [...row.matchAll(/48;2;(\d+);(\d+);(\d+)/g)].map((x) => x.slice(1).join(','));
    const capRow = raw.find((x) => plain(x).trim().startsWith('▄') && plain(x).includes('▄ '))!;
    const inks = [...capRow.matchAll(/38;2;(\d+);(\d+);(\d+)/g)].map((x) => x.slice(1).join(','));
    assert.equal(new Set(inks).size, 2, 'two tints on the card row: the answered seats and the one still thinking');
    assert.ok(inks[0] === inks[1] && inks[1] !== inks[2], `${inks.join(' | ')}`);
    assert.ok(bg(raw.find((x) => plain(x).includes('thinking'))!).length > 0);
  } finally { T.useCapabilities(prev); chalk.level = lvl; }
});

test('the strip pans the seats still answering', () => {
  const blind = lines(model(world('round 1'), { still: false, now: T0 + 19_000 }))[0]!;
  const quiet = lines(model(world('round 1'), { view: 'epic', still: false, now: T0 + 19_000 }))[0]!;
  assert.notEqual(blind, quiet, 'the council view shows the thinking seat in its strip');
});

test('hostile text in a proposition, a stance or a live tail cannot widen a card or move the cursor', () => {
  const evil = '\x1b[2J\x1b]0;x\x07' + 'Z'.repeat(300);
  const st = world('round 2');
  st.councils[0]!.proposition = evil;
  st.councils[0]!.stances = st.councils[0]!.stances.map((x) => ({ ...x, text: `OPTION: ${evil}\n${evil}` }));
  const m = model(st, { now: T0 + 34_000, live: { n_c: evil } });
  for (const [w, h] of [[80, 24], [40, 24], [28, 14]] as const) for (const x of observeFrame(m, w, h)) { assert.ok(stringWidth(x) <= w, plain(x)); assert.ok(!x.includes('\x1b[2J') && !x.includes('\x1b]0'), 'no terminal control'); }
});

test('with no council the view says how to convene one', () => {
  const l = lines(model(emptyState()));
  assert.match(l.join('\n'), /No Council yet\./);
  assert.equal(weave(model(emptyState()), 80, 10).length, 4);
});

test('several councils: the one picked is shown, with its place in the list', () => {
  const st = world('round 1');
  st.councils.push({ ...st.councils[0]!, task_id: 't_9', proposition: 'A second question?' });
  assert.match(lines(model(st, { pick: 1 })).join('\n'), /A second question\?\s+\(2 of 2\)/);
  assert.match(lines(model(st, { pick: 0 })).join('\n'), /\(1 of 2\)/);
});
