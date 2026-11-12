// `/history` (`specs/102` §5.1, S5): the operator's read-only view of the exchange records. Pure: the daemon's answer in, text out. A stored
// escape sequence must not reach the terminal; a record that failed the chain check must never read as valid; a pasted block is a hash
// and a length; with history off the view says so and names the setting.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { historyReport, plain, recordLines, type HistoryView, type ViewRecord } from '../src/history.ts';
import { lookup } from '../src/commands.ts';

const rec = (over: Partial<ViewRecord> = {}): ViewRecord => ({
  id: 'ex_0002', at: '2026-11-12T10:00:00Z', mission: 'm1', trigger: 'council', kind: 'observed', status: 'verified',
  prompt: { text: 'decide the launch plan', cut: false }, artifacts: [], pasted: [], reply: null,
  outcome: { value: 'done', provenance: 'observed' }, observed: { ids: ['c_0003'], counts: {} }, ...over,
});
const view = (over: Partial<HistoryView> = {}): HistoryView => ({
  state: 'records', message: null, project: 'home', capture: 'on', setting: '[history] enabled in config.toml',
  records: [rec()], total: 1, hidden: 0, verified: 1, chain_broken: 0, unverifiable: 0, broken_at: null,
  report: { text: 'DELPHI PROJECT HISTORY\n  decisions: 2\ncontext only; raises no trust, approves nothing, becomes no rule.', error: null }, ...over,
});

test('plain strips what a terminal acts on, including the C1 range, and turns line breaks into spaces', () => {
  const s = plain('a\x1b[31mred\x07\x9b2Jb\r\nc\td‮e');
  assert.equal(s, 'ared2Jb c de', 'the CSI and the lone C1 byte are gone, and the line breaks are spaces');
  assert.ok(!/[\x00-\x08\x0b-\x1f\x7f-\x9f‪-‮]/.test(s));
  assert.equal(plain('abcdef', 3), 'abc…');
  assert.equal(plain(undefined), '');
});

test('a verified record is listed with its trigger, label and outcome, and the report follows', () => {
  const out = historyReport(view());
  assert.match(out, /ex_0002 · 2026-11-12T10:00:00Z · council · observed · verified/);
  assert.match(out, /prompt: decide the launch plan/);
  assert.match(out, /outcome: done \(observed\)/);
  assert.match(out, /1 record · 1 verified/);
  assert.match(out, /DELPHI PROJECT HISTORY\n {2}decisions: 2\ncontext only; raises no trust/, 'the report keeps its line breaks and indentation');
  assert.ok(!/CHAIN BROKEN/.test(out));
});

test('`/history records` leaves the report out', () => {
  assert.ok(!/DELPHI PROJECT HISTORY/.test(historyReport(view(), false)));
});

test('a record that failed the chain check says so in capitals and the view says where the chain broke; nothing reads as valid', () => {
  const out = historyReport(view({
    records: [rec({ id: 'ex_0003', status: 'chain broken' }), rec({ id: 'ex_0002', status: 'chain broken' }), rec({ id: 'ex_0001', status: 'verified' })],
    total: 3, verified: 1, chain_broken: 2, broken_at: 1,
  }));
  assert.match(out, /ex_0003 .* CHAIN BROKEN/);
  assert.match(out, /ex_0002 .* CHAIN BROKEN/);
  assert.match(out, /ex_0001 .* verified/);
  assert.match(out, /2 CHAIN BROKEN/);
  assert.match(out, /breaks at record 2/);
  assert.match(out, /Nothing was repaired or deleted/);
  assert.ok(!/ex_0003 .*· verified/.test(out));
});

test('an unknown or missing status never reads as verified', () => {
  assert.match(recordLines(rec({ status: 'unverifiable' }))[0]!, /UNVERIFIABLE \(no key\)/);
  assert.match(recordLines(rec({ status: 'whatever' }))[0]!, /UNCHECKED \(whatever\)/);
  assert.match(recordLines(rec({ status: '' }))[0]!, /UNCHECKED/);
});

test('a pasted block is a hash and a length, and the host\'s account is labelled self-reported', () => {
  const lines = recordLines(rec({
    pasted: [{ hash: 'abcdef012345', chars: 900 }],
    reply: { label: 'self-reported, not checked', asked: 'a', did: 'd', open: 'o' },
  })).join('\n');
  assert.match(lines, /pasted block: abcdef012345 · 900 characters \(text not kept\)/);
  assert.match(lines, /host's account \(self-reported, not checked\): asked: a · did: d · open: o/);
});

test('escape sequences, OSC titles and bidi overrides in every field are gone before drawing', () => {
  const evil = '\x1b]0;pwned\x07\x1b[2J\x9b31m‮evil';
  const out = historyReport(view({
    project: evil, message: evil,
    records: [rec({
      id: evil, at: evil, trigger: evil, kind: evil, status: evil,
      prompt: { text: evil, cut: true }, artifacts: [{ artifact: evil, log: evil }], pasted: [{ hash: evil, chars: 1 }],
      reply: { label: evil, asked: evil, did: evil, open: evil }, outcome: { value: evil, provenance: evil }, observed: { ids: [evil], counts: {} },
    })],
    report: { text: `line\n${evil}`, error: null },
  }));
  assert.ok(!/[\x00-\x08\x0b-\x1f\x7f-\x9f‪-‮⁦-⁩]/.test(out), JSON.stringify(out));
  assert.match(out, /UNCHECKED/, 'a hostile status is not a status');
});

test('with history off the view says so and names the setting; nothing else is drawn', () => {
  const out = historyReport(view({ state: 'off', message: 'history is off: nothing is being recorded.', records: [], total: 0, verified: 0 }));
  assert.match(out, /history is off/);
  assert.match(out, /\[history\] enabled in config\.toml/);
  assert.ok(!/ex_000/.test(out));
});

test('an empty store, an unreadable store and a capped list each say what they are', () => {
  assert.match(historyReport(view({ state: 'empty', message: 'no records yet', records: [], total: 0, verified: 0 })), /no records yet/);
  assert.match(historyReport(view({ state: 'unreadable', message: 'the history store cannot be read', records: [] })), /could not be read/);
  assert.match(historyReport(view({ total: 53, hidden: 3 })), /3 older records not shown/);
});

test('the command is in the shared list as a local, read-only command and app.tsx handles it with the one daemon call', () => {
  assert.equal(lookup('/history')?.name, '/history');
  const app = readFileSync(join(import.meta.dirname, '../src/app.tsx'), 'utf8');
  const at = app.indexOf("name === '/history'");
  assert.ok(at > 0);
  const block = app.slice(at, app.indexOf("name === '/export'", at));
  assert.match(block, /'history\.view'/);
  assert.ok(!/call\('(?!history\.view)/.test(block), 'it makes no other daemon call, and so no write');
});
