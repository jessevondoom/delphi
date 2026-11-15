import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import type { Envelope, OState } from '../src/types.ts';
import { STAGES, clock, councilEntries, defaultCouncil, entryOf, familyOf, isBlind, isDecided, phaseNote, roster, seatsOf, stageOf, stream, taskRoles, tilde, waiting } from '../src/view.ts';
import type { Council } from '../src/types.ts';

const sim = JSON.parse(readFileSync(new URL('../src/fixture/sim.json', import.meta.url), 'utf8')) as { state: OState; events: Envelope[] };

test('the roster lists the six roles of the mark first and says what each is doing', () => {
  const r = roster(sim.state);
  assert.deepEqual(r.map((a) => a.role), ['orchestrator', 'strategist', 'architect', 'scout', 'red-team', 'builder']);
  const by = Object.fromEntries(r.map((a) => [a.role, a]));
  assert.equal(by['red-team']!.activity, 'waiting_approval');
  assert.equal(by['red-team']!.doing, 'Waiting on you');
  assert.equal(by.builder!.doing, 'Add a retry queue to the claim loop');
  assert.equal(by.builder!.family, 'claude');
  assert.equal(by.orchestrator!.doing, 'Idle');
  assert.equal(by.orchestrator!.family, '', 'an unknown model names no family');
});

test('a family comes from the provider the daemon names before the model id', () => {
  const s = { ...sim.state, providers: [{ endpoint: 'e1', family: 'gemini', models: 1, cooling: 0, out: 0, back_ms: null }] };
  assert.equal(familyOf(s, 'e1', 'claude-x'), 'gemini');
  assert.equal(familyOf(s, 'other', 'gpt-5.6'), 'gpt');
});

test('what waits on the operator is listed oldest first', () => {
  const w = waiting({ ...sim.state, approvals: { b: { role: 'builder', summary: 'two', since_ms: 20 }, a: { role: 'scout', summary: 'one', since_ms: 10 } } });
  assert.deepEqual(w.map((x) => x.summary), ['one', 'two']);
  assert.deepEqual(waiting({ ...sim.state, approvals: undefined }), []);
});

test('the stream is newest first, keeps what is worth reading and adds changes the events no longer hold', () => {
  const s = stream(sim.events, sim.state);
  assert.ok(s.length > 0);
  for (let i = 1; i < s.length; i++) assert.ok(s[i - 1]!.at_ms >= s[i]!.at_ms);
  assert.ok(!s.some((e) => e.text.includes('undefined')));
  assert.ok(s.some((e) => e.kind === 'recorded' && e.code?.includes('specs/040-retry.md')));
  assert.ok(s.some((e) => e.kind === 'waiting'));
  // A change both in the events and in the state is shown once.
  const texts = s.filter((e) => e.kind === 'recorded').map((e) => `${e.role}|${e.text}`);
  assert.equal(new Set(texts).size, texts.length);
  // Usage and bookkeeping events are left out.
  assert.equal(entryOf({ type: 'agent.usage', timestamp: '2026-11-14T14:30:00Z', payload: {} }, 0), null);
  assert.equal(stream([], null).length, 0);
});

test('a tool call is one clipped line, and a block that is not a tool call is left out', () => {
  const call = entryOf({ type: 'agent.block.complete', timestamp: '2026-11-14T14:30:00Z', agent: 'scout', payload: { block: { type: 'tool_call', name: 'read_file', arguments: { path: 'x'.repeat(400) } } } }, 0)!;
  assert.equal(call.kind, 'read');
  assert.equal(call.role, 'scout');
  assert.ok(call.code!.startsWith('read_file {"path"') && call.code!.length <= 240 && call.code!.endsWith('…'));
  assert.equal(entryOf({ type: 'agent.block.complete', timestamp: '2026-11-14T14:30:00Z', payload: { block: { type: 'text' } } }, 0), null);
});

test('a time is shown as hours and minutes', () => {
  assert.equal(clock(Date.UTC(2026, 10, 14, 14, 2, 30), 'UTC'), '14:02');
  assert.equal(clock(0), '');
});

test("a reply is the role's the task was given to, and what the operator asked is the operator's", () => {
  // The shapes a real daemon sends: `task.created` names a source and a target; `task.output` names nobody.
  const events: Envelope[] = [
    { type: 'task.created', timestamp: '2026-11-13T22:33:21.477Z', seq: 1, task_id: 't_0001', payload: { prompt: 'what do you see?', source: 'operator', target: 'strategist' } },
    { type: 'task.output', timestamp: '2026-11-13T22:33:36.854Z', seq: 14, task_id: 't_0001', payload: { status: 'done', text: 'One path.' } },
    { type: 'task.output', timestamp: '2026-11-13T22:34:00.000Z', seq: 20, task_id: 't_0002', payload: { text: 'Built.' } },
  ];
  const state = { ...sim.state, tasks: { t_0002: { id: 't_0002', flow: 'build', prompt: 'p', status: 'done' } }, changes: [] };
  assert.deepEqual([...taskRoles(events, state)], [['t_0002', 'build'], ['t_0001', 'strategist']]);
  const s = stream(events, state, 200, (r) => ['strategist', 'orchestrator', 'operator'].includes(r));
  const asked = s.find((e) => e.kind === 'asked')!;
  assert.deepEqual([asked.role, asked.to, asked.text], ['operator', 'strategist', 'what do you see?']);
  assert.equal(s.find((e) => e.text === 'One path.')!.role, 'strategist');
  // A task given to a flow that is not a role is the orchestrator's to speak for.
  assert.equal(s.find((e) => e.text === 'Built.')!.role, 'orchestrator');
});

test('a home folder is written as a tilde', () => {
  assert.equal(tilde('/' + 'Users' + '/someone/delphi/project'), '~/delphi/project');
  assert.equal(tilde('/' + 'home' + '/someone'), '~');
  assert.equal(tilde('/opt/place'), '/opt/place');
});

const running = sim.state.councils[1]!;
const blindOf = (c: Council): Council => ({ ...c, phase: 'round 1', critiques: [], stances: c.stances.slice(0, 2) });

test('a council names its stage, and the one shown first is one still running', () => {
  assert.equal(STAGES.length, 5);
  assert.deepEqual([stageOf(blindOf(running)), stageOf(running), stageOf(sim.state.councils[0]!), stageOf({ ...running, phase: 'escalated' })], [1, 3, 4, 4]);
  assert.deepEqual([isBlind(blindOf(running)), isBlind(running), isDecided(running), isDecided(sim.state.councils[0]!), isDecided({ ...running, phase: 'escalated' })], [true, false, false, true, true]);
  assert.equal(defaultCouncil(sim.state), 1);
  assert.equal(defaultCouncil({ ...sim.state, councils: [sim.state.councils[0]!] }), 0);
  assert.equal(phaseNote({ ...running, phase: 'escalated' }), 'needs your decision');
});

test('after the barrier a seat shows its position, lens and critique', () => {
  const seats = seatsOf(running);
  assert.deepEqual(seats.map((x) => x.role), ['strategist', 'red-team', 'architect']);
  assert.ok(seats.every((x) => x.position.state === 'shown' && x.position.text.length > 0));
  assert.equal(seats[0]!.lens, 'what finishes soonest');
  assert.equal(seats[1]!.critique!.recommendation, 'hand on');
  assert.deepEqual(seats[1]!.critique!.findings, [['major', 'a retry in place can loop']]);
  assert.equal(seats[0]!.critique, null);
});

test('in the blind round no seat\'s words reach the screen, from the state or from the events', () => {
  const blind = blindOf(running);
  const seats = seatsOf(blind);
  assert.deepEqual(seats.map((x) => x.position.state), ['sealed', 'sealed', 'writing']);
  const words = [...running.stances.map((x) => x.text), ...running.critiques.map((x) => x.text)];
  const said = JSON.stringify(seats) + JSON.stringify(councilEntries(sim.events, blind));
  for (const w of words) assert.ok(!said.includes(w), `a sealed sentence leaked: ${w}`);
  // What a seat read is still shown, and that it answered.
  const entries = councilEntries(sim.events, blind);
  assert.ok(entries.some((e) => e.kind === 'read' && e.code!.includes('read_file')));
  assert.ok(entries.some((e) => e.sealed && e.text === ''));
});

test('a council\'s activity is its own events, newest first', () => {
  const entries = councilEntries(sim.events, running);
  assert.equal(entries.length, 9);
  for (let i = 1; i < entries.length; i++) assert.ok(entries[i - 1]!.at_ms >= entries[i]!.at_ms);
  assert.ok(entries[0]!.text.startsWith('Critique, recommending hand on:'));
  assert.ok(entries.some((e) => e.text.startsWith('Position: Hand it to another worker')));
  assert.equal(councilEntries(sim.events, sim.state.councils[0]!).length, 0, 'another council\'s events are not this one\'s');
});
