// The chat as tinted blocks (`specs/005e` §7, slice 4 of 005f). The conversation of the reference frames is built
// from events so the geometry, the label, the rail fallback, streaming and anchoring are tested on the real fold.
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import chalk from 'chalk';
import stringWidth from 'string-width';
import { anchorBack, bodyHeight, frame, replyAnchor, transcriptCount, type Model } from '../src/screen.ts';
import { blockWidths, hugWidth, tintBlock } from '../src/blocks.ts';
import * as C from '../src/composer.ts';
import { fold, initial, type State } from '../src/fold.ts';
import { newInbox } from '../src/inbox.ts';
import * as T from '../src/theme.ts';
import { info, plain, snapshot } from './helpers.ts';

const at = (hms: string) => `2026-10-25T${hms}Z`;
const ev = (type: string, payload: any, hms: string, task = 't_1', extra: any = {}) => ({ method: 'event', params: { type, task_id: task, payload, timestamp: at(hms), ...extra } });
const BG: T.Rgb = [15, 23, 42];

const model = (state: State, over: Partial<Model> = {}): Model => ({
  info, pal: { dark: true, bg: BG }, state, now: Date.parse('2026-10-25T09:10:00Z'), deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'mission-auto', panel: null, help: false, still: true, ...over,
});

/** The conversation of the 005e §7.2 frames: a question, an answer with a tool call, then "Now write it." and a builder's reply. */
function conversation(): State {
  let s = initial(0);
  const f = (e: any) => { s = fold(s, e, Date.parse(e.params.timestamp)); };
  f(ev('task.created', { prompt: 'Should the retry queue be a table or a file?' }, '09:00:01'));
  f(ev('dag.node.added', { node_id: 'sp_architect_1', role: 'architect', depth: 0 }, '09:00:02'));
  f(ev('agent.block.complete', { block: { type: 'tool_call', id: 'c1', name: 'read_file', arguments: { path: 'specs/003-collaboration.md' } } }, '09:00:03', 't_1', { agent: 'architect' }));
  f(ev('tool.result', { tool_call_id: 'c1', content: '# Collaboration', is_error: false }, '09:00:04'));
  f(ev('task.output', { status: 'done', text: 'A table gives indexes and transactions, so a claim is atomic and the audit trail is one query away. A file needs a lock file and careful fsync ordering.\n\n- atomic claims\n- easy audit' }, '09:00:05'));
  f(ev('task.created', { prompt: 'Now write it.' }, '09:03:38', 't_2'));
  f(ev('dag.node.added', { node_id: 'sp_builder_2', role: 'builder', depth: 0 }, '09:03:39', 't_2'));
  f(ev('agent.block.complete', { block: { type: 'tool_call', id: 'c2', name: 'write_file', arguments: { path: 'crates/queue.rs' } } }, '09:03:39', 't_2', { agent: 'builder' }));
  f(ev('tool.result', { tool_call_id: 'c2', content: 'ok', is_error: false }, '09:03:40', 't_2'));
  f(ev('task.output', { status: 'done', text: 'Starting in a worktree. Tests first.\nSchema and claim loop are in. The suite passes: 14 of 14.' }, '09:03:41', 't_2'));
  return { ...s, tokens: 12_400 };
}

/** The builder is writing: its text so far, and a write in progress. */
function streaming(): State {
  let s = initial(0);
  const f = (e: any) => { s = fold(s, e, Date.parse(e.params.timestamp)); };
  f(ev('task.created', { prompt: 'Now write it.' }, '09:03:38', 't_2'));
  f(ev('dag.node.added', { node_id: 'sp_builder_2', role: 'builder', depth: 0 }, '09:03:39', 't_2'));
  f(ev('agent.block.complete', { block: { type: 'tool_call', id: 'c2', name: 'write_file', arguments: { path: 'crates/queue.rs' } } }, '09:03:40', 't_2', { agent: 'builder' }));
  s = fold(s, { method: 'delta.frame', params: { spans: { sp_builder_2: { text: 'Starting in a worktree. Tests first.\nSchema and claim loop are in, running the su' } } } }, Date.parse(at('09:03:41')));
  return { ...s, tokens: 12_400 };
}

const lines = (m: Model, w: number, h: number) => frame(m, w, h).map(plain);
const check = (name: string, m: Model, w: number, h: number) => { const r = snapshot(name, frame(m, w, h)); assert.ok(r.ok, r.message); };

test('golden: the chat at 80x24 and 100x34, as the reference frames (005e 7.2)', () => {
  check('chat-blocks-80x24', model(conversation()), 80, 24);
  check('chat-blocks-100x34', model(conversation()), 100, 34);
});

test('golden: a reply streaming (005e 7.2), and the same without a tint (the rail)', () => {
  check('chat-streaming-80x24', model(streaming()), 80, 24);
  check('chat-streaming-rail-80x24', model(streaming(), { pal: { dark: true, bg: null } }), 80, 24);
  check('chat-blocks-rail-80x24', model(conversation(), { pal: { dark: true, bg: null } }), 80, 24);
});

/** A block is the label row, a cap row of `▄`, text rows, a cap row of `▀`; read back from a rendered frame. */
function blocksIn(rows: string[]) {
  const out: { label: number; top: number; bottom: number; col: number; width: number }[] = [];
  rows.forEach((r, i) => {
    const t = r.trim();
    if (t.length && /^▄+$/.test(t)) {
      const bottom = rows.findIndex((x, j) => j > i && /^\s*▀+\s*$/.test(x));
      out.push({ label: i - 1, top: i, bottom, col: r.length - r.trimStart().length, width: t.length });
    }
  });
  return out;
}

test('geometry at 120, 100, 80, 48, 40 and 28 columns: caps are full width, labels start at the block, none is wider than 80 %', () => {
  for (const w of [120, 100, 80, 48, 40, 28]) {
    const rows = lines(model(conversation()), w, 90);
    const found = blocksIn(rows);
    assert.ok(found.length >= 2, `${w}: blocks found`);
    const { max } = blockWidths(w);
    for (const b of found) {
      const bottom = rows[b.bottom]!.trim();
      assert.equal(bottom.length, b.width, `${w}: the two caps are the same width`);
      assert.equal(rows[b.bottom]!.length - rows[b.bottom]!.trimStart().length, b.col, `${w}: and start in the same column`);
      assert.ok(b.width <= max, `${w}: ${b.width} <= ${max}`);
      if (w >= 48) assert.ok(b.width <= Math.floor((w * 4) / 5), `${w}: at most 80 %`);
      const label = rows[b.label]!;
      assert.equal(label.length - label.trimStart().length, b.col, `${w}: the label starts in the block's first column`);
      for (let r = b.top + 1; r < b.bottom; r++) assert.ok(stringWidth(rows[r]!) <= b.col + b.width, `${w}: row ${r} stays inside the block`);
    }
    for (const l of rows) assert.ok(stringWidth(l) <= w, `${w}: ${l}`);
  }
});

test('the operator is flush right and dim, an agent flush left; the label carries the speaker and the time', () => {
  const rows = lines(model(conversation()), 100, 40);
  const you = rows.findIndex((l) => l.includes('you · 09:03:38'));
  assert.ok(you > 0);
  const u = blocksIn(rows).find((b) => b.label === you)!;
  assert.equal(u.col + u.width, 100, 'the operator block ends at the right edge');
  assert.ok(rows.some((l) => /^⟦.{3}⟧ daedalus · 09:00:05$/.test(l)), 'an agent label: badge, theme name, time, starting at column 0');
  const agent = blocksIn(rows).find((b) => rows[b.label]!.includes('hephaestus'))!;
  assert.equal(agent.col, 0, 'an agent block is flush left');
  const prev = T.currentCapabilities(), lvl = chalk.level;
  T.useCapabilities(T.capabilities({ TERM: 'xterm' }));
  try {
    const raw = frame(model(conversation()), 100, 40);
    assert.match(raw.find((l) => plain(l).includes('Now write it.'))!, /\x1b\[2m[^\n]*Now write it\./, 'the operator\'s words are dim');
    assert.doesNotMatch(raw.find((l) => plain(l).includes('Schema and claim loop'))!, /\x1b\[2m[^\n]*Schema/, 'an agent\'s are not');
  } finally { T.useCapabilities(prev); chalk.level = lvl; }
});

test('a block hugs its text: at least twelve text columns, never padded out, a longer label sets the width', () => {
  assert.equal(hugWidth([' hi'], 'you · 09:00:01', 80), 'you · 09:00:01'.length, 'the label is longer than 12 text columns plus padding');
  assert.equal(hugWidth([' hi'], 'x', 80), 14, 'twelve text columns and two of padding');
  assert.equal(hugWidth([' ' + 'a'.repeat(30)], 'x', 80), 32);
  assert.equal(hugWidth([' ' + 'a'.repeat(200)], 'x', 40), 40, 'never past the maximum');
  assert.deepEqual([blockWidths(100), blockWidths(80), blockWidths(120)].map((b) => [b.max, b.text]), [[80, 78], [64, 62], [96, 94]]);
  assert.equal(blockWidths(44).max, 38, '40-47 columns: w - 6');
  assert.equal(blockWidths(30).max, 28, 'under 40: w - 2');
});

test('the tint: the block is filled with it, the caps are its colour, and the end cells are part of the caps', () => {
  const prev = T.currentCapabilities(), level = chalk.level;
  T.useCapabilities(T.capabilities({ COLORTERM: 'truecolor' }));
  try {
    const raw = frame(model(conversation(), { pal: { dark: true, bg: BG } }), 80, 24);
    const cap = raw.find((l) => plain(l).includes('▄▄▄▄'))!;
    assert.ok(cap.includes('38;2;34;42;59'), 'the cap is the tint colour (8 % toward white from the background)');
    const row = raw.find((l) => plain(l).includes('Now write it.'))!;
    assert.ok(row.includes('48;2;34;42;59'), 'the text row has the tint as its background');
    assert.equal(stringWidth(row), 80, 'a text row is as wide as the screen line');
  } finally { T.useCapabilities(prev); chalk.level = level; }
});

test('without a tint the block is a rail: no caps, no fill, each text row starts with a dim bar', () => {
  const rows = lines(model(conversation(), { pal: { dark: true, bg: null } }), 80, 30);
  assert.ok(!rows.some((l) => /[▄▀]/.test(l)), 'no caps');
  const i = rows.findIndex((l) => l.includes('Now write it.'));
  assert.match(rows[i]!, /│ Now write it\./);
  assert.ok(rows[i - 1]!.includes('you · 09:03:38'), 'the label stays above');
  const prev = T.currentCapabilities();
  T.useCapabilities(T.capabilities({ NO_COLOR: '1' }));
  try {
    const none = frame(model(conversation()), 80, 30).join('\n');
    assert.ok(!/[▄▀]/.test(none) && /│ Now write it\./.test(plain(none)), 'NO_COLOR: the same rail, and no caps even with a known background');
    assert.ok(!/\x1b\[(?:[0-9;]*;)?(?:3[0-8]|4[0-8]|9[0-7]|10[0-7])[;m]/.test(none), 'and no colour code');
  } finally { T.useCapabilities(prev); }
});

test('tool calls are drawn inside the agent block that follows them, one dim line with a green check', () => {
  const rows = lines(model(conversation()), 80, 40);
  const b = blocksIn(rows).find((x) => rows[x.label]!.includes('hephaestus'))!;
  const inside = rows.slice(b.top + 1, b.bottom).join('\n');
  assert.match(inside, /→ write_file\(crates\/queue\.rs\) ✔/);
  assert.ok(!rows.slice(0, b.label).some((l) => l.includes('write_file')) , 'not floating above the block');
  const t = T.currentCapabilities();
  T.useCapabilities(T.capabilities({ TERM: 'xterm' }));
  try { assert.match(frame(model(conversation()), 80, 40).join('\n'), /\x1b\[32m✔/, 'the check is green'); } finally { T.useCapabilities(t); }
});

test('streaming: the same block grows with a cursor, the label says writing, the status row says who and what', () => {
  const rows = lines(model(streaming()), 80, 24);
  const i = rows.findIndex((l) => l.includes('writing') && l.includes('hephaestus'));
  assert.ok(i > 0, rows.join('\n'));
  assert.ok(rows.some((l) => l.includes('running the su▊')), 'the cursor follows the text');
  const status = rows.find((l) => l.includes('esc to stop'))!;
  assert.match(status, /hephaestus · writing write_file\(crates\/queue\.rs\)/);
  assert.ok(!rows.some((l) => l.includes('earlier messages')));
});

test('notes are marked: information, errors and successes', () => {
  let s = initial(0);
  const add = (text: string, tone?: 'info' | 'error' | 'ok') => { s = { ...s, blocks: [...s.blocks, { id: s.blocks.length + 1, kind: 'note', text, at: 0, tone }], nextId: s.nextId + 1 }; };
  add('nothing was running'); add('could not send: refused', 'error'); add('exported to local/missions/x.md', 'ok'); add('· approval mode: default');
  const out = lines(model(s), 80, 24).join('\n');
  assert.match(out, /· nothing was running/);
  assert.match(out, /✖ could not send: refused/);
  assert.match(out, /✔ exported to local\/missions\/x\.md/);
  assert.ok(!out.includes('· · approval'), 'a note that brings its own mark is not marked twice');
});

test('a short window drops the blank rows between messages; the caps stay', () => {
  const tall = lines(model(conversation()), 80, 20), short = lines(model(conversation()), 80, 15);
  assert.equal(short.length, 15);
  assert.ok(blocksIn(short).length >= 1, 'caps stay');
  // Between the first block's bottom cap and the last block's top cap there is a message boundary at every cap pair.
  const gaps = (rows: string[]) => {
    const from = rows.findIndex((l) => /^\s*▀/.test(l)), to = rows.length - 1 - [...rows].reverse().findIndex((l) => /^\s*▄/.test(l));
    return rows.slice(from, to).filter((l) => l === '').length;
  };
  assert.equal(gaps(short), 0, 'no blank row between messages');
  assert.ok(gaps(tall) >= 1, 'a tall window has them');
});

test('anchoring: a long reply starts at its label row; the marker says how many lines wait; paging and jumping are arithmetic', () => {
  let s = initial(0);
  const f = (e: any) => { s = fold(s, e, Date.parse(e.params.timestamp)); };
  f(ev('task.created', { prompt: 'Explain everything.' }, '10:00:00'));
  f(ev('dag.node.added', { node_id: 'sp_architect_1', role: 'architect', depth: 0 }, '10:00:01'));
  const paragraphs = Array.from({ length: 40 }, (_, i) => `Paragraph ${i + 1} of the long answer.`).join('\n\n');
  f(ev('task.output', { status: 'done', text: paragraphs }, '10:00:05'));
  const m = model(s);
  const a = replyAnchor(m, 80, false)!;
  assert.equal(typeof a.id, 'number');
  const h = 24, bodyH = bodyHeight(m, 80, h), total = transcriptCount(m, 80, false);
  const back = anchorBack(total, a.row, bodyH);
  assert.ok(back > 0, 'the reply outgrows the body');
  const pinned = lines(model(s, { back }), 80, h);
  const body = pinned.slice(3, 3 + bodyH);
  assert.match(body[0]!, /daedalus · 10:00:05/, 'the first body row is the reply\'s label row');
  assert.ok(body.some((l) => l.includes('Paragraph 1 of')), 'the beginning is on screen');
  const marker = body[bodyH - 1]!;
  assert.match(marker, new RegExp(`↓ ${back} more lines · pgdn or space · enter jumps to the end`));
  assert.equal(anchorBack(total, a.row, total), 0, 'a reply that fits needs no anchor');
  assert.equal(anchorBack(total, a.row, bodyH + 5), back - 5);
  assert.ok(!lines(model(s), 80, h).join('\n').includes('more lines'), 'following the end shows no marker');
});

test('a reply that streams and then lands keeps one anchor: the label row does not move', () => {
  const live = streaming();
  const m1 = model(live);
  const a1 = replyAnchor(m1, 80, false)!;
  assert.equal(a1.id, 'live');
  let s = live;
  s = fold(s, ev('task.output', { status: 'done', text: 'Starting in a worktree. Tests first.\nSchema and claim loop are in.' }, '09:03:50', 't_2'), Date.parse(at('09:03:50')));
  const a2 = replyAnchor(model(s), 80, false)!;
  assert.equal(typeof a2.id, 'number');
  assert.equal(a2.row, a1.row, 'the same row');
});

test('the chat and the activity deck keep their own scroll offsets', () => {
  const s = conversation();
  const chat = lines(model(s, { back: 5, activityBack: 0 }), 100, 24).join('\n');
  assert.ok(chat.includes('more lines'), 'the chat is scrolled');
  const deck = lines(model(s, { deck: 0, back: 5, activityBack: 0 }), 100, 24).join('\n');
  assert.ok(!deck.includes('lines below'), 'the activity deck is not scrolled by the chat\'s offset');
});

test('hostile text cannot widen a block or move the cursor', () => {
  let s = initial(0);
  s = fold(s, ev('task.created', { prompt: 'x\x1b[2J' + 'y'.repeat(300) }, '09:00:00'), 0);
  s = fold(s, ev('task.output', { status: 'done', text: '\x1b]0;evil\x07' + 'z'.repeat(400) + '\n\n```\n' + 'c'.repeat(300) + '\n```' }, '09:00:01'), 1);
  for (const w of [28, 40, 80, 120]) for (const l of frame(model(s), w, 30)) { assert.ok(stringWidth(l) <= w, `${w}: ${plain(l)}`); assert.ok(!l.includes('\x1b[2J') && !l.includes('\x1b]0'), 'no terminal control'); }
});

test('tintBlock: with a tint every text row is the block width; the rail ignores width', () => {
  const t = T.tint(BG, true, '16') ?? undefined;
  const prev = T.currentCapabilities(), lvl = chalk.level;
  T.useCapabilities(T.capabilities({ COLORTERM: 'truecolor' }));
  try {
    const tint = T.tint(BG, true)!;
    const rows = tintBlock({ label: 'L', rows: [' ab', ' a longer row'], width: 20, col: 3, tint }).map(plain);
    assert.deepEqual(rows.map((r) => r.length), [4, 23, 23, 23, 23]);
    assert.equal(rows[1], '   ' + '▄'.repeat(20));
    const rail = tintBlock({ label: 'L', rows: [' ab'], width: 20, col: 3, tint: null }).map(plain);
    assert.deepEqual(rail, ['   L', '   │ ab']);
  } finally { T.useCapabilities(prev); chalk.level = lvl; void t; }
});
