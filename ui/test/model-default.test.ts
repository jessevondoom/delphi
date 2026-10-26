// The panes show the model an agent is really on, marked against the tier its role starts on (`specs/035` D-14..D-16).
import test from 'node:test';
import assert from 'node:assert/strict';
import { emptyArch } from '../src/archives.ts';
import { observeFrame, type OModel, type OState } from '../src/observe-screen.ts';
import { mark, versus, words } from '../src/tiers.ts';
import { epicWorld, info, NOW } from './epic-fixture.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const model = (st: OState, view: 'agents' | 'dashboard'): OModel => ({ info, pal: { dark: true }, st, view, pick: 0, scroll: 0, selNode: 0, arch: emptyArch(), live: {}, connected: true, still: true, now: NOW } as OModel);

test('a model is above, below or at its roles default tier, and unknowns say nothing', () => {
  assert.deepEqual([versus('frontier', 'workhorse'), versus('fast', 'workhorse'), versus('workhorse', 'workhorse'), versus('', 'workhorse'), versus('frontier', undefined), versus('huge', 'fast')], ['above', 'below', 'at', '', '', '']);
  assert.equal(plain(mark('frontier', 'workhorse')), '▲');
  assert.equal(plain(mark('fast', 'workhorse')), '▼');
  assert.equal(mark('workhorse', 'workhorse'), '');
  assert.equal(words('frontier', 'workhorse'), 'above its default (workhorse)');
  assert.equal(words('fast', 'workhorse'), 'below its default (workhorse)');
});

function world(tier: string, modelName: string, reason = ''): OState {
  const s = epicWorld();
  const b = s.agents['builder']!;
  b.tier = tier; b.model = modelName; b.default_tier = 'workhorse';
  s.nodes['n3']!.tier = tier; s.nodes['n3']!.model = modelName; s.nodes['n3']!.default_tier = 'workhorse'; s.nodes['n3']!.tier_reason = reason;
  return s;
}

test('the agents pane shows the model it ran on and marks it above the default, with the orchestrator reason', () => {
  const text = observeFrame(model(world('frontier', 'claude-opus-5-5', 'it touches the claim loop'), 'agents'), 110, 34).map(plain).join('\n');
  assert.match(text, /frontier ▲ · claude-opus-5-5[^\n]*above its default \(workhorse\) · it touches the claim loop/);
});

test('a lesser stand-in is marked below the default, and a model at its default carries no mark', () => {
  const below = observeFrame(model(world('fast', 'claude-haiku-5-5'), 'agents'), 110, 34).map(plain).join('\n');
  assert.match(below, /fast ▼ · claude-haiku-5-5[^\n]*below its default \(workhorse\)/);
  const at = observeFrame(model(world('workhorse', 'claude-sonnet-5-5'), 'agents'), 110, 34).map(plain).join('\n');
  assert.match(at, /workhorse · claude-sonnet-5-5/);
  assert.doesNotMatch(at.split('\n').find((l) => l.includes('claude-sonnet-5-5')) ?? '', /[▲▼]/);
});

test('a sub-agent with no agent row of its own shows the model it ran on, and the dashboard row carries a model chip', () => {
  const s = world('workhorse', 'claude-sonnet-5-5');
  delete s.agents['scout'];
  s.nodes['n9'] = { ...s.nodes['n3']!, id: 'n9', role: 'scout', state: 'running', model: 'gemini-flash', tier: 'fast', default_tier: 'fast', endpoint: 'gem', task_id: 't_1', parent: 'n3', depth: 1 };
  const roster = observeFrame(model(s, 'agents'), 110, 40).map(plain).join('\n');
  assert.match(roster, /fast · gemini-flash/, 'the sub-agent row names its model');
  s.tasks['t_1']!.status = 'running';
  const dash = observeFrame(model(world('frontier', 'claude-opus-5-5'), 'dashboard'), 120, 34).map(plain).join('\n');
  assert.match(dash, /claude-opus-5-5 ▲/, 'the dashboard row says which model, above the default');
});
