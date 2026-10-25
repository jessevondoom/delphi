import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { observeFrame, emptyState, type OModel, type OState } from '../src/observe-screen.ts';
import { threads } from '../src/epic-threads.ts';
import { info, NOW, threadsWorld } from './threads-fixture.ts';
import { snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const model = (st: OState, over: Partial<OModel> = {}): OModel => ({ info, pal: { dark: true }, st, view: 'epic', pick: 0, scroll: 0, selNode: 0, search: { editing: false, query: '', hits: [], note: '' }, diff: null, live: {}, connected: true, still: true, now: NOW, ...over });
const frame = (st: OState, w: number, h: number, over: Partial<OModel> = {}) => observeFrame(model(st, over), w, h);

test('golden: the threads pane at 80x24 and 100x34, a thread opened, narrow, and empty', () => {
  const cases: [string, OState, number, number, Partial<OModel>][] = [
    ['epic-threads-80x24', threadsWorld(), 80, 24, {}],
    ['epic-threads-100x34', threadsWorld(), 100, 34, {}],
    ['epic-threads-open-80x24', threadsWorld(), 80, 24, { scroll: 1 }],
    ['epic-threads-open-last-100x34', threadsWorld(), 100, 34, { scroll: 3 }],
    ['epic-threads-40x14', threadsWorld(), 40, 14, {}],
    ['epic-threads-empty-80x24', emptyState(), 80, 24, {}],
  ];
  for (const [name, st, w, h, over] of cases) {
    const r = snapshot(name, frame(st, w, h, over));
    assert.ok(r.ok, r.message);
  }
});

test('work is grouped by what it is about, not by agent: the queue build and its docs are one thread, the rest are their own', () => {
  const list = threads(model(threadsWorld()), NOW);
  assert.equal(list.length, 4);
  const retry = list.find((t) => t.tasks.includes('t_3'))!;
  assert.deepEqual([...retry.tasks].sort(), ['t_3', 't_4']);
  assert.ok(!list.some((t) => t.tasks.includes('t_1') && t.tasks.includes('t_2')), 'a vendor comparison and a pricing model share only two words of a different kind');
});

test('threads are ordered by what needs you: stuck, then in flight, then ahead, then settled', () => {
  const st = threadsWorld();
  assert.deepEqual(threads(model(st), NOW).map((t) => t.state), ['active', 'active', 'ahead', 'settled']);
  st.councils.push({ ...st.councils[0]!, task_id: 't_2', proposition: 'Free tier or trial?', phase: 'verdict', escalation: 'seats deadlocked', options: [['free', ['architect']], ['trial', ['scout']]] });
  const list = threads(model(st), NOW);
  assert.equal(list[0]!.state, 'needs-you');
  assert.ok(list[0]!.tasks.includes('t_2'));
  const rows = frame(st, 80, 24).map(plain);
  assert.match(rows.find((l) => l.includes("council is split"))!, /council is split: free or trial · needs you/);
});

test('each thread says what happened, what is happening and what comes next; a verdict is the first thing said about the past', () => {
  const l = frame(threadsWorld(), 80, 24).map(plain);
  const at = l.findIndex((x) => x.includes('Add a retry queue to the claim loop'));
  assert.match(l[at + 1]!, /^   ✔ done chose table 2 of 3 · design · plan$/);
  assert.match(l[at + 2]!, /^   ▶ now {2}hephaestus building 5:00 · tests 12 of 14 passed$/);
  assert.match(l[at + 3]!, /^   ○ next audit → research → wrap up$/);
});

test('a thread that waits on another says so, and the other says it feeds it', () => {
  const t = frame(threadsWorld(), 100, 34).map(plain).join('\n');
  assert.match(t, /Compare managed vector databases.*⇢ feeds Draft the knowl/);
  assert.match(t, /Draft the knowledge base pri.*⇠ waits on Compare managed/);
  const list = threads(model(threadsWorld()), NOW);
  const pricing = list.find((x) => x.tasks.includes('t_2'))!;
  assert.deepEqual(pricing.ties.map((x) => [x.dir, x.open, x.step]), [['waits', true, 'audit']]);
});

test('down opens a thread into its checklist, with a build\'s gates under the step that made them', () => {
  const l = frame(threadsWorld(), 100, 34, { scroll: 1 }).map(plain);
  const at = l.findIndex((x) => x.includes('Add a retry queue to the claim loop'));
  assert.match(l[at + 1]!, /^   ✔ design$/);
  assert.match(l[at + 2]!, /^   ✔ council: Should the retry queue be a table or a\s*…?\s+chose table 2 of 3$/);
  assert.match(l[at + 4]!, /^   ▶ build {2}hephaestus building 5:00$/);
  assert.match(l[at + 5]!, /^      ▶ tests 12 of 14 passed$/);
  assert.match(l[at + 7]!, /^      ○ merge waits for you$/);
  assert.match(l.join('\n'), /▶ research · Document the retry queue|○ research · Document the retry queue/, 'a step of the docs task says which task it is');
  const pricing = frame(threadsWorld(), 100, 34, { scroll: 3 }).map(plain).join('\n');
  assert.match(pricing, /⇠ waits on the audit in Compare managed vector databases/);
});

test('a finished thread is one line and its verdicts; nothing running reads as settled', () => {
  const st = threadsWorld();
  for (const n of Object.values(st.nodes)) if (n.task_id === 't_5') n.state = 'done';
  const survey = threads(model(st), NOW).find((t) => t.tasks.includes('t_5'))!;
  assert.equal(survey.state, 'settled');
  assert.equal(survey.done, survey.total);
});

test('an explicit grouping from the daemon wins over the words', () => {
  const st = threadsWorld();
  st.threads = [{ id: 'launch', label: 'Launch plan', tasks: ['t_1', 't_2', 't_5'] }];
  const list = threads(model(st), NOW);
  const launch = list.find((t) => t.id === 'launch')!;
  assert.equal(launch.label, 'Launch plan');
  assert.deepEqual([...launch.tasks].sort(), ['t_1', 't_2', 't_5']);
  assert.equal(launch.ties.length, 0, 'a dependency inside a thread is not a tie');
});

test('model text cannot widen or break the pane, at any size', () => {
  const st = threadsWorld();
  st.tasks['t_3']!.prompt = 'evil \x1b[2J\x1b]0;x\x07 ' + 'wide '.repeat(60) + '日本語';
  st.councils[0]!.proposition = '\x1b[31m' + 'long '.repeat(40);
  const r = { ...info, roles: { ...info.roles, builder: { ...info.roles.builder!, name: 'hephaestus-with-a-very-long-name-that-must-clip' } } };
  for (const scroll of [0, 1, 2, 3, 9]) for (const [w, h] of [[28, 9], [40, 12], [60, 20], [72, 16], [80, 24], [140, 50], [200, 9]] as const) {
    const lines = observeFrame(model(st, { info: r, scroll }), w, h);
    assert.equal(lines.length, h, `${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${w}x${h}: ${plain(l)}`);
    assert.ok(!lines.join('\n').includes('\x1b[2J') && !lines.join('\n').includes('\x1b]0;'), 'no cursor movement from model text');
  }
});

test('pace stays on the last body row, right-aligned with one trailing space', () => {
  const lines = frame(threadsWorld(), 80, 24).map(plain);
  const last = lines.slice(0, -2).at(-1)!;
  assert.match(last, /pace {2}[▁▂▃▄▅▆▇█]{12} $/);
  assert.equal(stringWidth(last), 80);
});
