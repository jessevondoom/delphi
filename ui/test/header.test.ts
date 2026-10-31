// The header, footer, badges and help (`specs/005e` §10, slice 3 of 005f).
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import * as C from '../src/composer.ts';
import type { Agent } from '../src/fold.ts';
import { initial } from '../src/fold.ts';
import { helpKey } from '../src/help.ts';
import type { Ev } from '../src/input.ts';
import { newInbox } from '../src/inbox.ts';
import { cycleMode, MODES, modeNote, nextMode } from '../src/mode.ts';
import { BLINK_SLOT_MS, faceOf, SUCCESS_MS, stripFaces, STRIP_ROLES, THINKING_WINDOW_MS, tickKey, TICK_MS } from '../src/motion.ts';
import { footer, frame, header, titleRow, type Model } from '../src/screen.ts';
import * as T from '../src/theme.ts';
import { info, plain, snapshot, stateWorld } from './helpers.ts';

const model = (over: Partial<Model> = {}): Model => ({
  info, pal: { dark: true, bg: [15, 23, 42] }, state: initial(0), now: 1000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'default', panel: null, help: false, still: true, ...over,
});
const running = (extra: Partial<Agent> = {}): Agent => ({ state: 'running', doing: 'working', ...extra });
const idle = (extra: Partial<Agent> = {}): Agent => ({ state: 'idle', doing: '', ...extra });

test('the strip is badges only, in the fixed role order, and the title row is the row under it', () => {
  const strip = plain(header(model(), 100)[0]!);
  assert.match(strip, /^(⟦[^⟧]{3}⟧ ){5}⟦[^⟧]{3}⟧$/, 'six badges, single spaces, nothing else');
  assert.equal(strip.replace(/[⟦⟧•─_x?\- ^…·●]/g, ''), '', 'no letters, no names, no state words');
  const lines = frame(model(), 100, 30).map(plain);
  assert.match(lines[1]!, /^delphi v0\.5 · \/proj · chat$/);
  assert.equal(lines[2], '', 'exactly one blank row under the title');
});

test('the first message does not add a second blank row under the title', () => {
  const body = frame(model({ state: stateWorld() }), 100, 30).map(plain);
  assert.equal(body[2], '');
  assert.notEqual(body[3], '', 'the first block begins directly after the one blank row');
});

test('on a short window the blank row under the title is dropped', () => {
  const lines = frame(model({ state: stateWorld() }), 100, 14).map(plain);
  assert.equal(lines.length, 14);
  assert.notEqual(lines[2], '');
});

test('the title row keeps the pane name: the path gives way from the left', () => {
  const long = '/srv/work/delphi/.claude/worktrees/agent-a42560a2ca3e1fc4b/crates/x';
  const m = model({ info: { ...info, cwd: long.padEnd(70, 'y') } });
  const t = plain(titleRow(m, 80));
  assert.ok(stringWidth(t) <= 80, t);
  assert.match(t, / · chat$/);
  assert.match(t, /^delphi v0\.5 · …/, 'the path is cut at its start');
  assert.ok(t.endsWith(long.padEnd(70, 'y').slice(-10) + ' · chat'), 'and keeps its end');
  for (const w of [28, 40, 47]) assert.equal(plain(titleRow(m, w)), 'delphi · chat', `below 48 columns it is the short form (${w})`);
  for (const w of [48, 60, 100]) assert.match(plain(titleRow(m, w)), / · chat$/, `${w}`);
  assert.match(plain(titleRow(model({ deck: 2 }), 80)), / · decisions$/);
});

test('badge frames: idle blinks for 150 ms at random, 7-30 s apart, and each role blinks on its own schedule', () => {
  const at = (role: string, t: number) => faceOf(idle(), role, t, false).frame;
  const starts = (role: string) => {
    const out: number[] = [];
    for (let t = 0; t < 40 * BLINK_SLOT_MS; t += 50) if (at(role, t) === 'blink' && at(role, t - 50) !== 'blink') out.push(t);
    return out;
  };
  const b = starts('orchestrator');
  assert.equal(b.length, 40, 'one blink per slot');
  const gaps = b.slice(1).map((t, i) => t - b[i]!);
  assert.ok(gaps.every((g) => g >= 7000 - 50 && g <= 30_000 + 50), `gaps ${gaps}`);
  assert.ok(new Set(gaps).size > 10, 'the gaps are not regular');
  assert.equal([...Array(3).keys()].filter((i) => at('orchestrator', b[0]! + i * 50) === 'blink').length, 3, 'a blink lasts 150 ms');
  assert.notDeepEqual(starts('architect').slice(0, 5), b.slice(0, 5), 'another role is out of step');
  assert.ok(![...Array(200).keys()].some((i) => faceOf(idle(), 'scout', i * 50, true).frame === 'blink'), 'reduced motion: never blinks');
});

test('badge frames: a thinking agent pans every 600 ms, and goes back to working 1.5 s after the last delta', () => {
  const frames: string[] = [];
  for (let t = 6000; t < 7800; t += TICK_MS) frames.push(faceOf(running({ thinkingAt: t - 100 }), 'builder', t, false).frame);
  assert.deepEqual(frames, ['thinking_left', 'thinking_left', 'thinking_left', 'thinking_left', 'thinking_right', 'thinking_right', 'thinking_right', 'thinking_right', 'thinking_left', 'thinking_left', 'thinking_left', 'thinking_left']);
  const last = 10_000;
  assert.equal(faceOf(running({ thinkingAt: last }), 'builder', last + THINKING_WINDOW_MS - 1, false).frame.startsWith('thinking'), true);
  assert.equal(faceOf(running({ thinkingAt: last }), 'builder', last + THINKING_WINDOW_MS, false).frame, 'working');
  assert.equal(faceOf(running({ thinkingAt: 0 }), 'builder', 600, true).frame, 'thinking_left', 'reduced motion pins the first thinking frame');
  assert.equal(faceOf(running({ thinkingAt: 0 }), 'builder', 1200 - 1, true).frame, 'thinking_left');
});

test('badge frames: working, waiting and escalated are static; success is held 3 s; an escalation lasts 10 s', () => {
  assert.equal(faceOf(running(), 'builder', 123_456, false).frame, 'working');
  assert.deepEqual(faceOf({ state: 'waiting on you', doing: '' }, 'red-team', 1, false), { frame: 'waiting_approval', state: 'waiting on you' });
  const esc: Agent = { state: 'escalated', doing: '', since: 1000 };
  assert.equal(faceOf(esc, 'orchestrator', 1000 + 9999, false).frame, 'escalated');
  assert.notEqual(faceOf(esc, 'orchestrator', 1000 + 10_000, false).frame, 'escalated');
  assert.equal(faceOf(idle({ successAt: 5000 }), 'orchestrator', 5000 + SUCCESS_MS - 1, false).frame, 'success');
  assert.notEqual(faceOf(idle({ successAt: 5000 }), 'orchestrator', 5000 + SUCCESS_MS, false).frame, 'success');
});

test('one timer, repaint only on change: an idle window repaints once a second and a pinned one never blinks', () => {
  const roles = STRIP_ROLES;
  const idleAgents = {};
  let changes = 0, last = '';
  for (let t = 100_000; t < 120_000; t += TICK_MS) { const k = tickKey(idleAgents, roles, t, true, false); if (k !== last) { changes++; last = k; } }
  assert.equal(changes, 20, 'reduced motion and idle: one repaint per second for the clock, none for the badges');
  // with animation on, the only extra repaints are the blinks (two edges each), a handful in 20 s
  let animated = 0; last = '';
  for (let t = 100_000; t < 120_000; t += TICK_MS) { const k = tickKey(idleAgents, roles, t, false, false); if (k !== last) { animated++; last = k; } }
  assert.ok(animated > changes && animated <= changes + 2 * 6 * 4 + 2, `${animated}`);
  // while a task runs the clock moves at the spinner's pace
  let spin = 0; last = '';
  for (let t = 100_000; t < 101_000; t += TICK_MS) { const k = tickKey({ builder: running() }, roles, t, false, true); if (k !== last) { spin++; last = k; } }
  assert.ok(spin >= 4, `at least four spinner steps in a second (${spin}; a badge frame may add one)`);
  last = ''; spin = 0;
  for (let t = 100_000; t < 101_000; t += TICK_MS) { const k = tickKey({ builder: running() }, roles, t, true, true); if (k !== last) { spin++; last = k; } }
  assert.equal(spin, 1, 'reduced motion: the clock moves once a second even while a task runs');
});

test('the strip frames come from the agents: a thinking builder pans while the others stay put', () => {
  const faces = (t: number) => stripFaces({ builder: running({ thinkingAt: t - 50 }) }, STRIP_ROLES, t, true).map((f) => f.frame);
  assert.deepEqual(faces(6000).slice(0, 3), ['idle', 'idle', 'thinking_left']);
  assert.deepEqual(faces(6700 - 700).slice(2, 3), ['thinking_left']);
  const m = model({ state: { ...initial(0), agents: { builder: running({ thinkingAt: 6650 }) } }, now: 6700, still: false });
  assert.ok(plain(header(m, 100)[0]!).includes('⟦ ••⟧'), 'the right-hand pan frame, 600 ms after the left');
});

test('the footer: tokens and mode on the left, the hint and dots on the right, every separator two spaces each side', () => {
  const f = plain(footer(model({ state: { ...initial(0), tokens: 12_400 }, mode: 'mission-auto' }), 80));
  assert.match(f, /^12\.4k tokens  ·  auto {2,}← \/ \? →  ·  ○ ● ○$/);
  assert.match(plain(footer(model({ mode: 'accept-edits' }), 80)), /^0 tokens  ·  accept edits/);
  assert.match(plain(footer(model({ mode: 'default' }), 80)), /^0 tokens  ·  default /);
  assert.equal(stringWidth(footer(model(), 80)), 80);
  assert.doesNotMatch(plain(footer(model(), 60)), /← \/ \?/, 'under 64 columns the dots stand alone');
});

test('auto is the only mode drawn in the warning-red colour', () => {
  const prev = T.currentCapabilities();
  T.useCapabilities(T.capabilities({ COLORTERM: 'truecolor' }));
  try {
    const raw = (mode: string) => footer(model({ mode }), 80);
    assert.ok(raw('mission-auto').includes('38;2;249;115;22'), 'orange, bold');
    assert.ok(!raw('mission-auto').includes('[7m'), 'never reverse video');
    for (const mode of ['default', 'accept-edits']) assert.ok(!raw(mode).includes('38;2;249;115;22'), mode);
  } finally { T.useCapabilities(prev); }
});

test('Shift+Tab cycles default → accept edits → auto, never to optimistic, and each change says what it means', () => {
  assert.deepEqual(MODES, ['default', 'accept-edits', 'shell-auto', 'mission-auto']);
  assert.deepEqual(['default', 'accept-edits', 'shell-auto', 'mission-auto'].map(nextMode), ['accept-edits', 'shell-auto', 'mission-auto', 'default']);
  assert.ok(!MODES.some((m) => /optimistic/.test(m)) && !/optimistic/.test(modeNote('optimistic')), 'optimistic is chosen per build, not a mode');
  assert.deepEqual(cycleMode('shell-auto', false), { set: null, armed: true }, 'entering auto waits for a confirming press');
  assert.deepEqual(cycleMode('shell-auto', true), { set: 'mission-auto', armed: false });
  assert.deepEqual(cycleMode('accept-edits', false), { set: 'shell-auto', armed: false }, 'shell auto runs only safe commands, so it needs no confirmation');
  assert.ok(modeNote('shell-auto').includes('shell auto'));
  assert.deepEqual(cycleMode('default', false), { set: 'accept-edits', armed: false });
  assert.deepEqual(cycleMode('mission-auto', false), { set: 'default', armed: false });
  assert.equal(modeNote('mission-auto'), '· approval mode: auto — Amber actions run without asking this mission; Red actions always ask');
  assert.equal(modeNote('accept-edits'), '· approval mode: accept edits — file edits run without asking; commands and Red actions still ask');
  assert.equal(modeNote('default'), '· approval mode: default');
});

const key = (k: Extract<Ev, { t: 'key' }>['k']): Ev => ({ t: 'key', k, alt: false, shift: false, ctrl: false });

test('help keys: ? opens on an empty prompt only; ? and Esc close; paging is ignored; anything else closes and is handled', () => {
  const q: Ev = { t: 'text', s: '?' };
  assert.equal(helpKey(false, q, true, true), 'open');
  assert.equal(helpKey(false, q, false, true), 'pass', '? inside text types ?');
  assert.equal(helpKey(false, q, true, false), 'pass', 'only on the chat deck');
  assert.equal(helpKey(true, q, false, true), 'close');
  assert.equal(helpKey(true, key('esc'), true, true), 'close');
  assert.equal(helpKey(true, key('pgup'), true, true), 'ignore');
  assert.equal(helpKey(true, key('pgdn'), true, true), 'ignore');
  assert.equal(helpKey(true, { t: 'wheel', dir: 'up' }, true, true), 'ignore');
  assert.equal(helpKey(true, { t: 'text', s: 'a' }, true, true), 'close-and-pass');
  assert.equal(helpKey(true, key('enter'), true, true), 'close-and-pass');
  assert.equal(helpKey(true, { t: 'ctrl', c: 'o' }, true, true), 'close-and-pass');
  assert.equal(helpKey(true, { t: 'bg', dark: true, rgb: [0, 0, 0] }, true, true), 'pass');
});

test('the help overlay is a card of up to 76 columns, centred, over the transcript; it never scrolls it', () => {
  for (const [w, h] of [[80, 24], [120, 40], [40, 24], [28, 12]] as const) {
    const lines = frame(model({ state: stateWorld(), help: true }), w, h).map(plain);
    assert.equal(lines.length, h);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${w}x${h}: ${l}`);
    const label = lines.findIndex((l) => l.includes('keys · esc closes'));
    assert.ok(label >= (h < 16 ? 2 : 3), `${w}x${h}: the label is in the body`);
    assert.equal(lines.some((l) => l.includes('How should the retry queue')), false, 'the transcript is under the card, not beside it');
  }
  const wide = frame(model({ help: true }), 120, 30).map(plain);
  const cap = wide.find((l) => l.includes('▄'))!;
  assert.equal(cap.trim().length, 76);
  assert.equal(cap.length - cap.trimStart().length, 22, 'centred in 120 columns');
  assert.ok(wide.join('\n').includes('shift+tab  approval mode'), 'the keys are listed');
  const before = frame(model({ state: stateWorld(), back: 5 }), 100, 24).map(plain).join('\n');
  const during = frame(model({ state: stateWorld(), back: 5, help: true }), 100, 24).map(plain);
  assert.ok(!during.join('\n').includes('↓ 5 lines below') || before.includes('↓ 5 lines below'), 'the scroll offset is untouched by the overlay');
});

test('golden: the help overlay (005e 10.1) and the strip with a thinking agent', () => {
  const h = snapshot('help-overlay-80x24', frame(model({ help: true, state: { ...initial(0), tokens: 12_400 } }), 80, 24));
  assert.ok(h.ok, h.message);
  const t = snapshot('strip-thinking-80x24', frame(model({ state: { ...stateWorld(), agents: { builder: running({ thinkingAt: 6650 }) } }, now: 6700, still: false }), 80, 24));
  assert.ok(t.ok, t.message);
});

test('the scope and library counter sit on the footer, quiet, and give way to the tokens and mode when narrow', () => {
  const noted = (ids: string[]) => ({ ...initial(0), noted: ids });
  const scoped = { ...info, workspace_name: 'acme', mission: 'm-7' };
  const f = plain(footer(model({ info: scoped, state: noted(['a', 'b', 'c']), review: 1, bundles: 2 }), 120));
  assert.match(f, /^0 tokens {2}· {2}default {2}· {2}acme\/m-7 {2}· {2}2 bundles {2}· {2}\+3 noted · 1 to review {2,}← \/ \? →/);
  assert.doesNotMatch(plain(footer(model({ info: scoped }), 120)), /noted|to review/, 'nothing added: nothing said');
  assert.match(plain(footer(model({ info: scoped, state: noted(['a']), bundles: 2 }), 80)), /\+1 noted/);
  const narrow = plain(footer(model({ info: scoped, state: noted(['a']), bundles: 2 }), 64));
  assert.match(narrow, /^0 tokens {2}· {2}default/);
  assert.equal(stringWidth(narrow), 64);
});

test('a staged note is counted once, however often it is announced, and a new session starts the count over', async () => {
  const { fold } = await import('../src/fold.ts');
  const ev = (type: string, payload: object) => ({ method: 'event', params: { type, payload } });
  let s = initial(0);
  for (const id of ['n1', 'n1', 'n2']) s = fold(s, ev('learn.staged', { id }), 1);
  assert.equal(s.noted.length, 2);
  s = fold(s, ev('session.started', { session_id: 's_0002' }), 2);
  assert.equal(s.noted.length, 0);
});
