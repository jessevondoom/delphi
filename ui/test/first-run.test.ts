// First run, the no-model state and attention cards (`specs/005e` §6, slice 1 of 005f). The golden screens are the
// reference frames of §6.3-6.5 as this chrome draws them; the header and the cap tint change with slices 2-4.
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { classify, stripWrapper } from '../src/attention.ts';
import * as C from '../src/composer.ts';
import { fold, initial, type State } from '../src/fold.ts';
import { footer, frame, header, type Model } from '../src/screen.ts';
import { newInbox } from '../src/inbox.ts';
import { info, plain, snapshot } from './helpers.ts';

const model = (over: Partial<Model> = {}): Model => ({
  info, pal: { dark: true }, state: initial(0), now: 1000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'default', panel: null, help: false, ...over,
});
const ev = (type: string, payload: any, task = 't_0001') => ({ method: 'event', params: { type, task_id: task, payload } });
const NO_MODEL = '[escalated to the operator: no_model_configured: no model is configured for this workspace]';
const THROTTLED = '[escalated to the operator: every allowed endpoint is throttled or unavailable: no configured model can serve this request]';

const stopped = (reason = NO_MODEL): State => {
  let s = fold(initial(0), ev('task.created', { prompt: 'hello there' }), 1000);
  s = fold(s, ev('dag.node.added', { node_id: 'sp_orchestrator_1', role: 'orchestrator', depth: 0 }), 1100);
  return fold(s, ev('task.output', { status: 'escalated', text: reason }), 2000);
};

test('an escalated task is an attention block with its reason and cause, not a reply', () => {
  const s = stopped();
  const b = s.blocks[s.blocks.length - 1]!;
  assert.equal(b.kind, 'attention');
  assert.equal(b.cause, 'no_model_configured');
  assert.equal(b.text, 'Stopped: no model is configured for this workspace.');
  assert.equal(b.reason, 'no_model_configured: no model is configured for this workspace', 'the wrapper is gone');
  assert.ok(!s.blocks.some((x) => x.kind === 'reply'));
  assert.equal(s.agents['orchestrator']?.state, 'escalated');
});

test('a failed task is an attention block too, and a finished one is still a reply', () => {
  const failed = fold(fold(initial(0), ev('task.created', { prompt: 'x' }), 1), ev('task.failed', { error: 'disk full' }), 2);
  assert.equal(failed.blocks[1]!.kind, 'attention');
  assert.equal(failed.blocks[1]!.text, 'Stopped: disk full');
  const done = fold(fold(initial(0), ev('task.created', { prompt: 'x' }), 1), ev('task.output', { status: 'done', text: 'Hello.' }), 2);
  assert.equal(done.blocks[1]!.kind, 'reply');
});

test('the copy table: each known stop has its words, and the wrapper never survives', () => {
  assert.equal(stripWrapper('[escalated to the operator: a: b]'), 'a: b');
  assert.equal(stripWrapper('plain'), 'plain');
  assert.equal(classify('[escalated to the operator: mission budget: this call could cost up to $1 but only $0 remains (raise it with /budget)]').cause, 'budget');
  const re = classify('re-authenticate corp-gw');
  assert.deepEqual([re.cause, re.headline], ['reauth_required', 'Stopped: corp-gw needs you to sign in again.']);
  assert.equal(classify(THROTTLED).cause, 'exhausted');
  const other = classify('[escalated to the operator: the same turn was interrupted 3 times in a row]');
  assert.deepEqual([other.cause, other.headline, other.hints], ['other', 'Stopped: the same turn was interrupted 3 times in a row', ['/resume to try again · /models · /usage']]);
});

test('the face of an escalation does not linger: gone when a new task starts, and after ten seconds', () => {
  const s = stopped();
  const strip = (st: State, now: number) => plain(header(model({ state: st, now }), 100)[0]!);
  assert.match(strip(s, 3000), /⟦x_x⟧/, 'fresh: the card and the strip agree');
  assert.doesNotMatch(strip(s, 2000 + 10_000), /x_x/, 'ten seconds on, the card carries it');
  assert.doesNotMatch(strip(fold(s, ev('task.created', { prompt: 'again' }, 't_0002'), 4000), 4000), /x_x/, 'a new task starts clean');
});

test('the welcome shows until the operator says something, and never comes back', () => {
  const welcome = (m: Model) => frame(m, 80, 24).map(plain).join('\n');
  assert.match(welcome(model()), /Ask anything about \/proj, or hand out work:/);
  const said = fold(initial(0), ev('task.created', { prompt: 'hi' }), 1);
  assert.doesNotMatch(welcome(model({ state: said })), /Ask anything/);
  assert.doesNotMatch(welcome(model({ state: { ...said, blocks: [] } })), /Ask anything/, '/clear does not bring it back');
});

test('with no model: the card replaces the welcome, and the footer says so', () => {
  const text = frame(model({ noModel: true }), 80, 24).map(plain).join('\n');
  assert.match(text, /delphi · no model configured/);
  assert.doesNotMatch(text, /Ask anything/);
  assert.match(plain(footer(model({ noModel: true }), 80)), /^0 tokens · no model +/);
  assert.doesNotMatch(plain(footer(model(), 80)), /no model/);
});

test('nothing on screen says throttled or shows the bracketed wrapper for a workspace with no model', () => {
  const text = frame(model({ state: stopped(), noModel: true }), 80, 24).map(plain).join('\n');
  assert.match(text, /Stopped: no model is configured for this workspace\./);
  assert.doesNotMatch(text, /throttled|\[escalated/);
});

test('the screens fit their size and the cards never exceed their widths', () => {
  for (const [w, h] of [[80, 24], [40, 12], [28, 9], [120, 40]] as const) {
    for (const m of [model({ noModel: true }), model(), model({ state: stopped(), noModel: true }), model({ state: stopped(THROTTLED) })]) {
      const lines = frame(m, w, h);
      assert.equal(lines.length, h);
      for (const l of lines) assert.ok(stringWidth(l) <= w, `${w}x${h}: ${plain(l)}`);
    }
  }
  const card = frame(model({ noModel: true }), 120, 30).map(plain).find((l) => l.includes('▄'))!;
  assert.equal(card.trim().length, 76, 'the no-model card is 76 columns at 120, never stretched');
  assert.ok(card.startsWith('  ▄'), 'with a two-column margin');
});

const check = (name: string, m: Model, w: number, h: number) => {
  const r = snapshot(name, frame(m, w, h));
  assert.ok(r.ok, r.message);
};

test('golden: no model (005e 6.3)', () => {
  check('first-run-no-model-80x24', model({ noModel: true }), 80, 24);
  check('first-run-no-model-40x12', model({ noModel: true }), 40, 12);
  check('first-run-no-model-40x20', model({ noModel: true }), 40, 20);
});

test('golden: the welcome of a ready console (005e 6.5)', () => {
  check('first-run-welcome-80x24', model(), 80, 24);
  check('first-run-welcome-40x12', model(), 40, 12);
});

test('golden: an escalation card after a prompt (005e 6.4)', () => {
  const m = model({ state: stopped(), noModel: true });
  check('first-run-attention-80x24', m, 80, 24);
  check('first-run-attention-40x12', m, 40, 12);
  check('first-run-attention-40x20', m, 40, 20);
  check('first-run-attention-budget-80x24', model({ state: stopped('[escalated to the operator: mission budget: this call could cost up to $1.0000 but only $0.0001 remains (raise it with /budget)]') }), 80, 24);
});
