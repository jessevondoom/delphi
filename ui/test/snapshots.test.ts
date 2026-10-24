// Golden screens: the console at fixed sizes, as plain text. A visual change shows up as a diff in review.
// Accept an intended change with `UPDATE_SNAPSHOTS=1 npm test` and read the changed files before committing.
process.env.TZ = 'UTC'; // reply headers show a clock time
import test from 'node:test';
import assert from 'node:assert/strict';
import { frame, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import { cardsOf, inboxInit } from '../src/inbox.ts';
import { panelInit, VIEWS, type PanelUi } from '../src/panels.ts';
import { collectSources } from '../src/sources.ts';
import { completions } from '../src/complete.ts';
import { info, observerWorld, snapshot, stateWorld } from './helpers.ts';

const state = stateWorld();
const obs = observerWorld();
const base: Model = {
  info, pal: { dark: true }, state, now: 5000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], help: false, inbox: inboxInit(), mode: 'default', obs, panel: null,
};
const check = (name: string, m: Model, w: number, h: number) => {
  const r = snapshot(name, frame(m, w, h));
  assert.ok(r.ok, r.message);
};
const panel = (ui: PanelUi): Model['panel'] => ({
  ui, cap: 5, pending: 1, mission: 'm1',
  sources: collectSources(state.blocks, [{ pointer: 'context/glossary.md#term' }]),
  notebook: [{ at: '2026-10-24T10:00:00.000Z', source: 'daedalus (architect)', text: 'Use a table: transactions.' }],
  preview: (rel) => [`# ${rel}`, '', 'The first lines of the document.'],
});

test('chat: a conversation with a tool call, a markdown answer, a card waiting and a task running', () => {
  const m: Model = { ...base, composer: C.insert(C.empty(), 'and then?'), queue: ['one more thing'] };
  check('chat-wide', m, 100, 34);
  check('chat-narrow', m, 48, 18);
});

test('chat: tool details expanded, the slash menu, help and history search', () => {
  check('chat-tools-open', { ...base, expandTools: true }, 100, 24);
  const typed = C.insert(C.empty(), '/pa');
  const menu = completions(typed.chars, typed.cur, info, () => []);
  check('chat-menu', { ...base, composer: typed, menu: { items: menu!.items, sel: 0 } }, 100, 24);
  check('chat-help', { ...base, help: true }, 100, 40);
  check('chat-search', { ...base, search: { q: 'retry', match: 'how does the retry ladder work?' } }, 100, 20);
});

test('chat: scrolled back shows the marker', () => {
  check('chat-scrolled', { ...base, back: 6 }, 100, 20);
});

test('inbox: list and focus', () => {
  check('inbox-list', { ...base, deck: 2 }, 100, 24);
  check('inbox-focus', { ...base, deck: 2, inbox: { ...inboxInit(), mode: 'focus' } }, 100, 24);
  check('inbox-payload', { ...base, deck: 2, inbox: { ...inboxInit(), mode: 'focus', view: true } }, 100, 24);
  assert.equal(cardsOf(state).length, 2);
});

test('thinking deck', () => {
  check('deck-thinking', { ...base, deck: 0 }, 100, 24);
});

test('every panel view at 100x30', () => {
  for (const view of VIEWS) check(`panel-${view}`, { ...base, panel: panel(panelInit(view)) }, 100, 30);
});

test('panels at a narrow width', () => {
  check('panel-council-narrow', { ...base, panel: panel(panelInit('council')) }, 56, 24);
  check('panel-epic-narrow', { ...base, panel: panel(panelInit('epic')) }, 56, 24);
});
