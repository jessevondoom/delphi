// Golden screens: the console and the Observer at fixed sizes, as plain text. A visual change shows up as a diff in
// review. Accept an intended change with `UPDATE_SNAPSHOTS=1 npm test` and read the changed files before committing.
process.env.TZ = 'UTC'; // reply headers show a clock time
import test from 'node:test';
import { emptyArch } from '../src/archives.ts';
import assert from 'node:assert/strict';
import { frame, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import { newInbox } from '../src/inbox.ts';
import { observeFrame, VIEWS, type OModel } from '../src/observe-screen.ts';
import { openNotes, openSources, openSearch } from '../src/panel.ts';
import { completions } from '../src/complete.ts';
import { info, observerWorld, snapshot, stateWorld } from './helpers.ts';

const state = stateWorld();
const base: Model = {
  info, pal: { dark: true, bg: [15, 23, 42] }, state, now: 5000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'default', panel: null, help: false, still: true,
};
const check = (name: string, m: Model, w: number, h: number) => {
  const r = snapshot(name, frame(m, w, h));
  assert.ok(r.ok, r.message);
};

test('chat: a conversation with a tool call, a markdown answer, cards waiting and a task running', () => {
  const m: Model = { ...base, composer: C.insert(C.empty(), 'and then?'), queue: ['one more thing'] };
  check('chat-wide', m, 100, 34);
  check('chat-narrow', m, 48, 18);
});

test('chat: tool details expanded, the slash menu, help and history search, and scrolled back', () => {
  check('chat-tools-open', { ...base, expandTools: true }, 100, 24);
  const typed = C.insert(C.empty(), '/pa');
  const menu = completions(typed.chars, typed.cur, info, () => []);
  check('chat-menu', { ...base, composer: typed, menu: menu ? { items: menu.items, sel: 0 } : null }, 100, 24);
  check('chat-help', { ...base, help: true }, 100, 40);
  check('chat-search', { ...base, search: { q: 'retry', match: 'how does the retry ladder work?' } }, 100, 20);
  check('chat-scrolled', { ...base, back: 6 }, 100, 20);
});

test('decks: the inbox', () => {
  check('deck-inbox', { ...base, deck: 2 }, 100, 24);
});

test('research panels over the chat', () => {
  check('panel-sources', { ...base, panel: openSources() }, 100, 24);
  check('panel-notes', { ...base, panel: openNotes('## 2026-10-24 · daedalus\n\nUse a table: transactions.') }, 100, 24);
  check('panel-search', { ...base, panel: openSearch('retry') }, 100, 24);
});

test('the Observer: every view at 100x30, and two narrow', () => {
  const om = (view: (typeof VIEWS)[number]): OModel => ({ info, pal: { dark: true }, st: observerWorld(), view, pick: 0, scroll: 0, selNode: 1, arch: emptyArch(), live: {}, connected: true });
  for (const view of VIEWS) {
    const r = snapshot(`observe-${view}`, observeFrame(om(view), 100, 30));
    assert.ok(r.ok, r.message);
  }
  for (const view of ['council', 'epic'] as const) {
    const r = snapshot(`observe-${view}-narrow`, observeFrame(om(view), 56, 24));
    assert.ok(r.ok, r.message);
  }
});
