import test from 'node:test';
import { emptyArch } from '../src/archives.ts';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { observeFrame, emptyState, type OModel, type OState } from '../src/observe-screen.ts';
import { autoLabel, kilo, threads, topicOf } from '../src/epic-threads.ts';
import * as T from '../src/theme.ts';
import { info, NOW, namedWorld, threadsWorld } from './threads-fixture.ts';
import { snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const model = (st: OState, over: Partial<OModel> = {}): OModel => ({ info, pal: { dark: true }, st, view: 'epic', pick: 0, scroll: 0, selNode: 0, arch: emptyArch(), live: {}, connected: true, still: true, now: NOW, ...over });
const frame = (st: OState, w: number, h: number, over: Partial<OModel> = {}) => observeFrame(model(st, over), w, h);
const lines = (st: OState, w = 80, h = 24) => frame(st, w, h).map(plain);

test('golden: named threads at 80x24 and 100x34, derived labels, narrow, scrolled and empty', () => {
  const cases: [string, OState, number, number, Partial<OModel>][] = [
    ['epic-threads-80x24', namedWorld(), 80, 24, {}],
    ['epic-threads-100x34', namedWorld(), 100, 34, {}],
    ['epic-threads-derived-80x24', threadsWorld(), 80, 24, {}],
    ['epic-threads-40x14', namedWorld(), 40, 14, {}],
    ['epic-threads-scrolled-80x14', namedWorld(), 80, 14, { scroll: 5 }],
    ['epic-threads-empty-80x24', emptyState(), 80, 24, {}],
  ];
  for (const [name, st, w, h, over] of cases) {
    const r = snapshot(name, frame(st, w, h, over));
    assert.ok(r.ok, r.message);
  }
});

test('work is grouped by what it is about when nobody has named it: the queue build and its docs are one thread', () => {
  const { list } = threads(model(threadsWorld()), NOW);
  assert.equal(list.length, 4);
  const retry = list.find((t) => t.tasks.includes('t_3'))!;
  assert.deepEqual([...retry.tasks].sort(), ['t_3', 't_4']);
  assert.ok(!list.some((t) => t.tasks.includes('t_1') && t.tasks.includes('t_2')));
});

test('the orchestrator\'s names win: its label is the heading, and words do not move a task it placed', () => {
  const st = namedWorld();
  const { list } = threads(model(st), NOW);
  assert.deepEqual(list.map((t) => t.label).sort(), ['Competitor onboarding', 'Pricing tiers', 'Retry queue', 'Vector database choice']);
  st.threads = [{ id: 'launch', label: 'Launch plan', tasks: ['t_1', 't_2', 't_5'] }]; // t_3 and t_4 are left to the words
  const mixed = threads(model(st), NOW).list;
  assert.deepEqual([...mixed.find((t) => t.id === 'launch')!.tasks].sort(), ['t_1', 't_2', 't_5']);
  assert.equal(mixed.find((t) => t.id === 'launch')!.ties.length, 0, 'a dependency inside a thread is not a tie');
  assert.ok(mixed.some((t) => t.tasks.includes('t_3') && t.tasks.includes('t_4')));
  st.threads = [{ id: 'a', label: 'A', tasks: ['t_3'] }, { id: 'b', label: 'B', tasks: ['t_4'] }]; // placed apart, so apart
  assert.equal(threads(model(st), NOW).list.filter((t) => t.tasks.includes('t_3') || t.tasks.includes('t_4')).length, 2);
});

test('the critical path leads the pane: a summary line, a bar down the threads it passes through, and nothing on the others', () => {
  const { path, list } = threads(model(namedWorld()), NOW);
  assert.deepEqual(path.threads.map((t) => t.label), ['Vector database choice', 'Pricing tiers']);
  assert.equal(Math.ceil(path.eta / 60_000), 11);
  assert.deepEqual(list.filter((t) => t.critical).map((t) => t.label), ['Vector database choice', 'Pricing tiers']);
  const l = lines(namedWorld());
  assert.match(l[5]!, /^ ▌ critical path {2}Vector database choice → Pricing tiers\s+about 11 min $/);
  const at = (s: string) => l.findIndex((x) => /^.?[◐○▼] /.test(x) && x.includes(s));
  for (const s of ['Vector database choice', 'Pricing tiers']) assert.ok(l[at(s)]!.startsWith('▌'), `${s} carries the bar`);
  assert.ok(!l[at('Retry queue')]!.startsWith('▌') && !l[at('Retry queue') + 1]!.startsWith('▌'), 'work off the path does not');
  assert.ok(at('Vector database choice') < at('Retry queue'), 'and it is listed first');
});

test('steps on the critical path are bold, in colour and without it; the same steps off the path are not', () => {
  const prev = T.currentCapabilities();
  for (const env of [{ TERM: 'xterm-256color', COLORTERM: 'truecolor' }, { NO_COLOR: '1' }]) {
    T.useCapabilities(T.capabilities(env));
    try {
      const raw = frame(namedWorld(), 80, 24);
      const pricing = raw.find((x) => plain(x).includes('plan → wrap up'))!;
      const retry = raw.find((x) => plain(x).includes('audit → research'))!;
      assert.match(pricing, /\x1b\[1m/, `bold on the path (${Object.keys(env)})`);
      assert.ok(!/\x1b\[1m/.test(retry), `not bold off it (${Object.keys(env)})`);
      assert.match(plain(pricing), /^▌/, 'and the bar does not depend on colour');
    } finally { T.useCapabilities(prev); }
  }
});

test('each thread gives highlights only: what was decided, what is happening, and the next few steps', () => {
  const l = lines(namedWorld());
  const at = l.findIndex((x) => x.includes('Retry queue'));
  assert.match(l[at]!, /^ ◐ Retry queue\s+44k tok 61%  ▰▰▰▱▱▱ 3 of 7 $/);
  assert.match(l[at + 1]!, /^   ✔ chose table 2 of 3 · 2 steps done$/);
  assert.match(l[at + 2]!, /^   ▶ hephaestus building 5:00 · tests 12 of 14 passed$/);
  assert.match(l[at + 3]!, /^   ○ audit → research → wrap up$/);
  assert.ok(!l.join('\n').includes('council: Should'), 'no per-step detail');
});

test('a thread that waits on another says so, and the other says it feeds it', () => {
  const t = lines(namedWorld(), 100, 34).join('\n');
  const l = t.split('\n');
  // The tie rides on the last line of each block, not on the header, so it costs no row.
  const last = (heading: string) => { const at = l.findIndex((x) => /^.?[◐○▼] /.test(x) && x.includes(heading)); let i = at + 1; while (l[i] && /^.?\s{2}[^─\s]/.test(l[i]!)) i++; return l[i - 1]!; };
  assert.match(last('Vector database choice'), /○ audit\s+⇢ feeds Pricing tiers $/);
  assert.match(last('Pricing tiers'), /○ plan → wrap up\s+⇠ waits on Vector database choice $/);
});

test('finished threads are one line at the bottom', () => {
  const l = lines(namedWorld());
  const at = l.findIndex((x) => x.includes('settled'));
  assert.match(l[at]!, /^ {2}✔ settled {2}Competitor onboarding 9\.3k$/);
  assert.ok(!l.some((x) => x.includes('Competitor onboarding') && !x.includes('settled')));
});

test('what needs you comes first: a split council or a failing audit, ahead of the critical path', () => {
  const st = namedWorld();
  st.councils.push({ ...st.councils[0]!, task_id: 't_2', proposition: 'Free tier or trial?', phase: 'verdict', escalation: 'seats deadlocked', options: [['free', ['architect']], ['trial', ['scout']]] });
  let first = threads(model(st), NOW).list[0]!;
  assert.equal(first.label, 'Pricing tiers');
  assert.equal(first.state, 'needs-you');
  assert.match(lines(st).join('\n'), /✖ council is split: free or trial · needs you/);
  const st2 = namedWorld();
  st2.builds[0]!.findings = [['P1', 'unchecked unwrap in claim.rs']];
  first = threads(model(st2), NOW).list[0]!;
  assert.equal(first.label, 'Retry queue');
  assert.match(lines(st2).join('\n'), /✖ hephaestus building 5:00 · audit P1: unchecked unwrap in claim\.rs/);
});

test('with nothing waiting on anything, there is no path to show', () => {
  const st = namedWorld();
  for (const n of Object.values(st.nodes)) { n.state = 'done'; n.ended_ms = n.started_ms + 60_000; }
  const l = lines(st);
  assert.match(l[5]!, /no critical path/);
  assert.ok(!l.some((x) => x.startsWith('▌')));
});

test('model text cannot widen or break the pane, at any size', () => {
  const st = namedWorld();
  st.tasks['t_3']!.prompt = 'evil \x1b[2J\x1b]0;x\x07 ' + 'wide '.repeat(60) + '日本語';
  st.threads![0]!.label = 'evil \x1b[2J ' + 'wide '.repeat(30) + '日本語';
  st.councils[0]!.proposition = '\x1b[31m' + 'long '.repeat(40);
  const r = { ...info, roles: { ...info.roles, builder: { ...info.roles.builder!, name: 'hephaestus-with-a-very-long-name-that-must-clip' } } };
  for (const scroll of [0, 1, 4, 99]) for (const [w, h] of [[28, 9], [40, 12], [60, 20], [72, 16], [80, 24], [140, 50], [200, 9]] as const) {
    const out = observeFrame(model(st, { info: r, scroll }), w, h);
    assert.equal(out.length, h, `${w}x${h}`);
    for (const l of out) assert.ok(stringWidth(l) <= w, `${w}x${h}: ${plain(l)}`);
    assert.ok(!out.join('\n').includes('\x1b[2J') && !out.join('\n').includes('\x1b]0;'), 'no cursor movement from model text');
  }
});

test('pace stays on the last body row, right-aligned with one trailing space; a hint says when there is more', () => {
  const l = lines(namedWorld());
  const last = l.slice(0, -2).at(-1)!;
  assert.match(last, /pace {2}[▁▂▃▄▅▆▇█]{12} $/);
  assert.equal(stringWidth(last), 80);
  assert.match(lines(namedWorld(), 80, 14).slice(0, -2).at(-1)!, /↑ ↓ {2}more/);
});

test('each thread shows what it has used and its share of everything, so the one eating the mission stands out', () => {
  const { list } = threads(model(namedWorld()), NOW);
  assert.deepEqual(list.map((t) => [t.label, t.tokens]), [['Vector database choice', 18_400], ['Pricing tiers', 0], ['Retry queue', 43_800], ['Competitor onboarding', 9_300]]);
  const st = namedWorld();
  assert.equal(list.reduce((a, t) => a + t.tokens, 0), st.tokens.input + st.tokens.cache_read + st.tokens.cache_write + st.tokens.output + st.tokens.thinking, 'the threads add up to the footer\'s total');
  const l = lines(namedWorld());
  assert.match(l.find((x) => x.includes('Vector database choice') && x.includes('tok'))!, /18k tok 26%/);
  assert.match(l.find((x) => x.includes('Retry queue') && x.includes('tok'))!, /44k tok 61%/);
  assert.ok(!l.find((x) => x.includes('Pricing tiers') && /^.?[◐○▼]/.test(x))!.includes('tok'), 'a thread that used nothing shows no count');
  // Narrow windows keep the count and drop the share.
  assert.match(lines(namedWorld(), 64, 24).find((x) => x.includes('Retry queue') && x.includes('tok'))!, /44k tok {2}▰/);
});

test('a thread without tokens from the daemon falls back to what its nodes counted', () => {
  const st = namedWorld();
  for (const t of Object.values(st.tasks)) delete t.tokens;
  const { list } = threads(model(st), NOW);
  assert.ok(list.every((t) => t.tokens === t.tasks.reduce((a, id) => a + Object.values(st.nodes).filter((n) => n.task_id === id).reduce((b, n) => b + n.tokens, 0), 0)));
  assert.ok(list.find((t) => t.label === 'Retry queue')!.tokens > 0);
});

test('threads are separated by a hairline, and the settled line is separated too', () => {
  const l = lines(namedWorld());
  const rules = l.map((x, i) => [x, i] as const).filter(([x]) => /^ {2}─{20,}$/.test(x)).map(([, i]) => i);
  assert.equal(rules.length, 3, 'between three live threads and before the settled line');
  assert.ok(l[rules[2]! + 1]!.includes('settled'));
});

test('kilo keeps a count short', () => {
  assert.deepEqual([0, 812, 999, 1000, 4200, 9999, 10_000, 38_400, 999_999, 1_000_000, 1_300_000].map(kilo), ['0', '812', '999', '1.0k', '4.2k', '10.0k', '10k', '38k', '1000k', '1.0M', '1.3M']);
});

test('threads nobody named get a name made from what the requests are about', () => {
  assert.deepEqual(threads(model(threadsWorld()), NOW).list.map((t) => [t.label, t.named]).sort(), [
    ['Competitor onboarding flows', false], ['Knowledge base pricing tiers', false], ['Managed vector databases', false], ['Retry queue', false]]);
  assert.ok(threads(model(namedWorld()), NOW).list.every((t) => t.named), 'the orchestrator\'s names are marked as its own');
  // Verbs, questions, articles and everything after the first preposition go.
  assert.equal(topicOf('Add a retry queue to the claim loop'), 'Retry queue');
  assert.equal(topicOf('Compare managed vector databases for the knowledge base'), 'Managed vector databases');
  assert.equal(topicOf('Should the retry queue be a table or a file?'), 'Retry queue');
  assert.equal(topicOf('please write the Q3 board memo, then send it'), 'Q3 board memo');
  assert.equal(topicOf('Fix it'), 'It');
  assert.equal(topicOf('one two three four five six seven'), 'One two three four five');
  // A thread of several requests is named for the words they all share.
  assert.equal(autoLabel(['Add a retry queue to the claim loop', 'Document the retry queue for operators']), 'Retry queue');
  assert.equal(autoLabel(['Survey rival pricing pages', 'Interview three customers']), 'Rival pricing pages', 'nothing shared: the first request alone');
  assert.equal(autoLabel([]), '');
});

test('a made-up name cannot carry control codes or run past the heading width', () => {
  const st = threadsWorld();
  st.tasks['t_1']!.prompt = '\x1b[2J\x1b]0;x\x07 Compare ' + 'enormous '.repeat(40) + 'databases';
  const t = threads(model(st), NOW).list.find((x) => x.tasks.includes('t_1'))!;
  assert.ok(!/[\x00-\x1f]/.test(t.label) && [...t.label].length <= 44, t.label);
});
