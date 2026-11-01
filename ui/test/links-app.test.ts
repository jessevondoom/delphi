// The key flow of links in the console (spec 029 slice 4) and the narrator's feed (slice 3), driven through the real
// components with a fake terminal and a fake daemon: ctrl+o, tab, enter, the reader, esc, sources and pins.
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import React from 'react';
import { EventEmitter } from 'node:events';
import { mkdtempSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { testRender } from '@opentui/react/test-utils';
import { App } from '../src/app.tsx';
import { Observer } from '../src/observer.tsx';
import { Term } from '../src/terminal.ts';
import { emptyState } from '../src/observe-screen.ts';
import { info as baseInfo, stateWorld } from './helpers.ts';

type Calls = { method: string; params: any }[];

/** A daemon that serves one document and records what it was asked. */
function fakeDaemon(extra: Record<string, (p: any) => any> = {}) {
  const calls: Calls = [];
  const rpc = Object.assign(new EventEmitter(), {
    whenReady: async () => {},
    call: async (method: string, params: any) => {
      calls.push({ method, params });
      if (extra[method]) return extra[method]!(params);
      if (method === 'kb.get') {
        const target = String(params.target);
        if (target.startsWith('/v1/doc?path=')) {
          const path = decodeURIComponent(target.slice('/v1/doc?path='.length).split('&')[0]!);
          if (path === 'specs/002-adapter-and-routing.md') return { status: 200, body: { path, collection: 'specs', trust: 'internal_generated', stale: false, text: '# Spec 002\n\nthe retry ladder is here\n\nsecond paragraph\n' } };
          return { status: 404, body: { error: 'not found' } };
        }
        return { status: 200, body: [] };
      }
      return {};
    },
  });
  return { rpc: rpc as any, calls };
}

const tick = (ms = 80) => new Promise((r) => setTimeout(r, ms));
const key = (term: Term, k: string, extra: object = {}) => term.emit('ev', { t: 'key', k, alt: false, shift: false, ctrl: false, ...extra });
const ctrl = (term: Term, c: string) => term.emit('ev', { t: 'ctrl', c });
const text = (term: Term, s: string) => term.emit('ev', { t: 'text', s });

async function consoleApp(daemon = fakeDaemon()) {
  const workspace = mkdtempSync(join(tmpdir(), 'delphi-links-'));
  const term = new Term({ mouse: false });
  const info = { ...baseInfo, workspace };
  const setup = await testRender(React.createElement(App, { cmd: daemon.rpc, ev: daemon.rpc, term, info, mission: 'm1', dark: true, themeForced: true, history: stateWorld() }), { width: 100, height: 40 });
  await setup.flush();
  const settle = async (ms = 80) => { await tick(ms); await setup.flush(); };
  const app = { unmount: () => setup.renderer.destroy() };
  return { term, app, daemon, workspace, settle, show: () => setup.captureCharFrame() };
}

test('console: ctrl+o enters link mode, tab focuses a link, enter reads it here, esc closes it and then leaves link mode', async () => {
  const c = await consoleApp();
  assert.doesNotMatch(c.show(), /link mode/);
  ctrl(c.term, 'o'); await c.settle();
  assert.match(c.show(), /link mode: tab next/);
  text(c.term, 'x'); await c.settle(); // a stray letter types nothing
  assert.doesNotMatch(c.show(), /❯ x/);
  key(c.term, 'tab'); await c.settle();
  assert.match(c.show(), /link specs\/002-adapter-and-routing\.md § 4 · enter reads it here/);
  key(c.term, 'enter'); await c.settle(250);
  assert.ok(c.daemon.calls.some((x) => x.method === 'kb.get' && String(x.params.target).startsWith('/v1/doc?path=specs%2F002-adapter-and-routing.md')), 'read through kb.get');
  assert.match(c.show(), /Spec 002/);
  assert.match(c.show(), /the retry ladder is here/);
  key(c.term, 'esc'); await c.settle();
  assert.doesNotMatch(c.show(), /the retry ladder is here/, 'the reader is closed');
  assert.match(c.show(), /link mode: tab next/, 'link mode is still on, where it was');
  key(c.term, 'esc'); await c.settle();
  assert.doesNotMatch(c.show(), /link mode/);
  c.app.unmount();
});

test('console: a document read from the chat is recorded as a source', async () => {
  const c = await consoleApp();
  ctrl(c.term, 'o'); await c.settle(); key(c.term, 'tab'); await c.settle(); key(c.term, 'enter'); await c.settle(250);
  key(c.term, 'esc'); key(c.term, 'esc'); await c.settle();
  ctrl(c.term, 's'); await c.settle();
  assert.match(c.show(), /specs\/002-adapter-and-routing\.md\s+read 2× · architect, operator/, 'one entry: the architect read it, and so did the operator');
  c.app.unmount();
});

test('console: p in the reader pins the passage to the mission notebook with its pointer and hash', async () => {
  const c = await consoleApp();
  ctrl(c.term, 'o'); await c.settle(); key(c.term, 'tab'); await c.settle(); key(c.term, 'enter'); await c.settle(250);
  text(c.term, 'p'); await c.settle(250);
  const notes = join(c.workspace, 'local/sessions/m1/notes.md');
  assert.ok(existsSync(notes), 'the notebook was written');
  const body = readFileSync(notes, 'utf8');
  assert.match(body, /· read specs\/002-adapter-and-routing\.md\n/, 'an anchor is a section, not a line range, so the whole document is pointed at');
  assert.match(body, /sha256:[0-9a-f]{12}/);
  c.app.unmount();
});

test('console: a link to a document that is not there says so and nothing opens', async () => {
  const d = fakeDaemon({ 'kb.get': () => ({ status: 404, body: { error: 'not found' } }) });
  const c = await consoleApp(d);
  ctrl(c.term, 'o'); await c.settle(); key(c.term, 'tab'); await c.settle(); key(c.term, 'enter'); await c.settle(250);
  assert.match(c.show(), /is not in the knowledge base/);
  assert.doesNotMatch(c.show(), /Spec 002/);
  c.app.unmount();
});

test('console: tool details moved to ctrl+t', async () => {
  const c = await consoleApp();
  const before = c.show();
  ctrl(c.term, 't'); await c.settle();
  assert.notEqual(c.show(), before, 'ctrl+t expands the tool calls');
  c.app.unmount();
});

// ── the narrator ────────────────────────────────────────────────────────────

test('narrator: in the change feed tab focuses a link, enter reads it over the feed, esc returns, and nothing can be pinned', async () => {
  const T0 = Date.UTC(2026, 9, 25, 10, 0);
  const st = { ...emptyState(), changes: [{ session: 's_1', at_ms: T0, role: 'architect', task: 't_1', summary: 'wrote the spec', files: ['specs/002-adapter-and-routing.md'] }], sessions: [{ id: 's_1', reason: 'new', started_ms: T0 - 1000, turns: 1 }] };
  const daemon = fakeDaemon({ 'observe.state': () => st });
  const term = new Term({ mouse: false });
  const setup = await testRender(React.createElement(Observer, { cmd: daemon.rpc, ev: daemon.rpc, term, info: baseInfo, dark: true, themeForced: true, view: 'changes' }), { width: 100, height: 40 });
  await setup.flush();
  const settle = async (ms = 80) => { await tick(ms); await setup.flush(); };
  const app = { unmount: () => setup.renderer.destroy() };
  const show = () => setup.captureCharFrame();
  assert.match(show(), /1 link · tab to follow/);
  key(term, 'tab'); await settle();
  assert.match(show(), /specs\/002-adapter-and-routing\.md {2}\(enter reads it here\)/);
  key(term, 'enter'); await settle(250);
  assert.match(show(), /the retry ladder is here/);
  key(term, 'right'); await settle(); // the view does not change under an open document
  assert.match(show(), /the retry ladder is here/);
  text(term, 'p'); await settle();
  assert.doesNotMatch(daemon.calls.map((x) => x.method).join(','), /pin/);
  key(term, 'esc'); await settle();
  assert.match(show(), /wrote the spec/);
  app.unmount();
});
