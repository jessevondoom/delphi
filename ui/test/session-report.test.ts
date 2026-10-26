// `/context`, `/export all` and the session count in the footer (`specs/030` §6).
import test from 'node:test';
import assert from 'node:assert/strict';
import { contextReport, exportAll, sessionName } from '../src/session.ts';
import { fold, initial } from '../src/fold.ts';
import { footer } from '../src/screen.ts';
import { info } from './epic-fixture.ts';

const report = { session: 's_0003', budget_tokens: 6000, turns: 6, digested: 2, omitted: 1, earlier: 2, tokens: 4200, trimmed: true, ceiling: 'L5', text: 'Operator: hi\nYou: hello' };

test('/context says how much of the conversation the next task is told, how it was trimmed and at what trust', () => {
  const s = contextReport(report, false);
  assert.match(s, /^session 3 · the next task is told 6 messages · 3 whole, 2 as lines, 1 left out · about 4.2k of 6.0k tokens · 2 earlier sessions in a line each/);
  assert.match(s, /\(\/context full shows the text\)$/);
  assert.match(contextReport(report, true), /\n\nOperator: hi\nYou: hello$/);
  assert.match(contextReport({ ...report, ceiling: 'L0' }, false), /starts below full trust \(L0\).*\/new clears it/);
  assert.equal(contextReport({ ...report, turns: 0, earlier: 0, text: '' }, false), 'session 3: nothing is carried, the next task starts clean');
  assert.match(contextReport({ ...report, budget_tokens: 0 }, false), /told nothing.*conversation_tokens is 0/);
  assert.equal(sessionName('s_0012'), 'session 12');
  assert.equal(sessionName('s_0000'), 'this session', 'the log before any /new is not numbered');
});

test('/export all writes the whole mission with a section per session and the right speaker for each reply', () => {
  const at = (n: number) => `2026-10-25T10:00:0${n}Z`;
  const events = [
    { type: 'task.created', task_id: 't_1', timestamp: at(1), payload: { prompt: 'before sessions' } },
    { type: 'session.started', timestamp: at(2), payload: { session_id: 's_0001', reason: 'restart' } },
    { type: 'dag.node.added', task_id: 't_2', timestamp: at(3), payload: { node_id: 'n1', role: 'architect', depth: 0 } },
    { type: 'dag.node.added', task_id: 't_2', timestamp: at(3), payload: { node_id: 'n2', role: 'scout', depth: 1 } },
    { type: 'task.created', task_id: 't_2', timestamp: at(3), payload: { prompt: 'design the queue' } },
    { type: 'task.created', task_id: 't_3', timestamp: at(4), payload: { prompt: 'implement it', flow: 'build' } },
    { type: 'task.output', task_id: 't_2', timestamp: at(5), payload: { status: 'done', text: 'Use a visibility timeout.\x1b[2J' } },
    { type: 'session.started', timestamp: at(6), payload: { session_id: 's_0002', reason: 'new' } },
    { type: 'task.output', task_id: 't_9', timestamp: at(7), payload: { status: 'escalated', text: 'stopped' } },
  ];
  const md = exportAll(events, info, 'm1', new Date('2026-10-25T12:00:00Z'));
  assert.match(md, /^---\nid: export-m1\ntitle: "Mission export \(all sessions\)"/);
  const heads = [...md.matchAll(/^## .*$/gm)].map((m) => m[0]);
  assert.deepEqual(heads, ['## Before sessions were recorded', '## session 1 · began 2026-10-25T10:00:02Z (restart)', '## session 2 · began 2026-10-25T10:00:06Z (new)']);
  assert.match(md, /\*\*You\*\* · .*\n\ndesign the queue/);
  assert.match(md, /\(architect\) · .*\n\nUse a visibility timeout\./, 'the reply is the top agent of its task, not a sub-agent');
  assert.ok(!md.includes('implement it'), 'a build is not conversation');
  assert.ok(!md.includes('\x1b'), 'nothing a model wrote can carry a control sequence into the file');
  assert.match(md, /\(orchestrator\) · .* · stopped/);
});

test('the footer names the session and counts what was asked in it, and says nothing before any session began', () => {
  const m = (state: ReturnType<typeof initial>) => ({ state, mode: 'default', deck: 1, pal: { dark: true }, info } as never);
  const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
  assert.doesNotMatch(plain(footer(m(initial(0)), 100)), /session/);
  let s = fold(initial(0), { method: 'event', params: { type: 'session.started', payload: { session_id: 's_0004' } } }, 1);
  assert.match(plain(footer(m(s), 100)), /session 4 · 0 asks/);
  s = fold(s, { method: 'event', params: { type: 'task.created', task_id: 't_1', payload: { prompt: 'hi' } } }, 2);
  assert.match(plain(footer(m(s), 100)), /session 4 · 1 ask\b/);
});
