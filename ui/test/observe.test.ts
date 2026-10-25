import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { emptyState, mascot, observeFrame, parseView, stepView, VIEWS, type OModel, type OState } from '../src/observe-screen.ts';
import type { Info } from '../src/theme.ts';

const crown: Record<string, string> = { orchestrator: '  ▄▄▄  ', architect: '▄▄▄▄▄▄▄', builder: '▄▄▄▄▄▄▄', 'red-team': '▄     ▄', strategist: '▄▄ ▄ ▄▄', scout: '   ▄   ' };
const mk = (name: string, chassis: string, face: string, role: string) => ({ name, chassis, face, crown: crown[role], crown_rgb: '#facc15' });
const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: { orchestrator: mk('athena', '#a855f7', '•─•', 'orchestrator'), architect: mk('daedalus', '#38bdf8', '•_•', 'architect'), builder: mk('hephaestus', '#f97316', '•─•', 'builder'), 'red-team': mk('ares', '#ef4444', '-_-', 'red-team'), strategist: mk('odysseus', '#22c55e', '•_•', 'strategist'), scout: mk('hermes', '#eab308', '•••', 'scout') },
};
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const tk = (n: number) => ({ input: n, cache_read: n * 2, cache_write: 0, output: n / 2, thinking: 0 });

function sample(): OState {
  const s = emptyState();
  s.mission_id = '2026-10-24-ui'; s.spent_usd = 0.42; s.budget = { cap_usd: 5 }; s.retries = 1; s.cache_hit_ratio = 0.5; s.series = [0.01, 0.05, 0.02, 0.2, 0.14];
  s.tokens = tk(1000);
  for (const [i, r] of ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout'].entries()) {
    s.agents[r] = { role: r, state: r === 'builder' ? 'running' : r === 'red-team' ? 'waiting on you' : 'idle', task_id: 't_1', endpoint: 'direct', tier: 'frontier', model: 'claude-x', upscaled: false, tokens: tk(100 * (i + 1)), cost_usd: 0.07, calls: i + 1 };
  }
  s.tasks['t_1'] = { id: 't_1', flow: 'build', prompt: 'Implement the deck router', status: 'running' };
  s.nodes['n_1'] = { id: 'n_1', role: 'orchestrator', parent: null, depth: 0, task_id: 't_1', state: 'running', tokens: 1200, cost_usd: 0.1, started_ms: 0, ended_ms: null, after: [] };
  s.nodes['n_2'] = { id: 'n_2', role: 'builder', parent: 'n_1', depth: 1, task_id: 't_1', state: 'done', tokens: 8000, cost_usd: 0.2, started_ms: 0, ended_ms: null, after: [] };
  s.narrative = ['t_1 started', 'builder finished'];
  s.councils = [{ task_id: 't_2', proposition: 'Table or file?', seats: [{ role: 'architect', family: 'a' }, { role: 'red-team', family: 'b' }, { role: 'strategist', family: 'c' }], homogeneous: false, phase: 'verdict', stances: [{ role: 'architect', answered: true, text: 'Use a table.\nIt indexes.' }, { role: 'red-team', answered: false, text: 'no answer' }], critiques: [{ role: 'architect', recommendation: 'revise', concur: false, findings: [['P1', 'lost updates']] }], escalation: 'seats deadlocked', path: 'missions/m/councils/008.md', unresolved: [['P0', 'red-team', 'the race']], options: [['table', ['architect']], ['file', ['strategist']]] }];
  s.builds = [{ task_id: 't_1', branch: 'delphi/t_1', attempts: ['attempt 1: red'], sha: 'abc1234', gates: [{ name: 'tests', state: 'green', detail: '12 passed' }, { name: 'audit', state: 'red', detail: '1 finding' }], findings: [['P1', 'unchecked unwrap']], flaky: true, merge: '' }];
  return s;
}
const model = (view: (typeof VIEWS)[number], over: Partial<OModel> = {}): OModel => ({ info, pal: { dark: true }, st: sample(), view, pick: 0, scroll: 0, selNode: 1, search: { editing: false, query: '', hits: [], note: '' }, diff: null, live: {}, connected: true, ...over });

test('every view fits the window exactly, with no boxes and no money-free surprises', () => {
  for (const view of VIEWS) for (const [w, h] of [[110, 34], [70, 20], [40, 12]] as const) {
    const lines = observeFrame(model(view), w, h);
    assert.equal(lines.length, h, `${view} ${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${view}: line wider than ${w}: ${plain(l)}`);
    assert.ok(!/[┌┐└┘│]/.test(lines.map(plain).join('\n')), `${view}: no box characters`);
  }
});

test('the chrome: a badges-only strip, the title row right under it, a blank row, the attention slot, a hairline and the footer', () => {
  const l = observeFrame(model('agents'), 80, 24).map(plain);
  assert.match(l[0]!, /^⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦x\?x⟧ ⟦•_•⟧ ⟦•••⟧\s*$/, 'the strip: badges and single spaces, nothing else');
  assert.equal(l[1]!.trimEnd(), 'delphi v0.5 · agents', 'no path, no mission id');
  assert.equal(l[2], '', 'one blank row');
  assert.match(l[3]!, /^ ▼ needs you/, 'an approval is waiting in the sample, so the slot is the band');
  assert.match(l.at(-2)!, /^─+$/);
  assert.match(l.at(-1)!, /^1 running  ·  [\d.]+k? tokens\s+← →  ·  ○ ○ ○ ● ○ ○$/);
  const all = l.join('\n');
  assert.ok(!/\$\d/.test(all) && !/spend|\bcost\b/i.test(all), 'no money anywhere');
  assert.ok(!/ \/ |\?|mission 2026/.test(l.at(-1)!) && !all.includes('2026-10-24-ui'), 'no / or ? in the footer, no mission id');
});

test('the title row drops the version below 48 columns, the footer drops the arrows below 64', () => {
  const narrow = observeFrame(model('epic'), 40, 14).map(plain);
  assert.equal(narrow[1]!.trimEnd(), 'delphi · epic');
  assert.doesNotMatch(narrow.at(-1)!, /←/);
  assert.match(narrow.at(-1)!, /○ ● ○ ○ ○ ○$/);
  assert.match(observeFrame(model('epic'), 70, 20).map(plain).at(-1)!, /← →  ·  ○ ● ○ ○ ○ ○$/);
});

test('the attention slot is body row 1 of every view: "nothing needs you" or the band, with what and where', () => {
  const quiet = sample(); quiet.agents['red-team']!.state = 'idle'; quiet.councils[0]!.escalation = null;
  for (const view of VIEWS) {
    const l = observeFrame(model(view, { st: quiet }), 80, 24).map(plain);
    assert.equal(l[3]!.trimEnd(), ' ✔ nothing needs you', view);
  }
  const waiting = sample(); waiting.councils[0]!.escalation = null; waiting.pending_approvals = { ap_1: 'write specs/009.md', ap_2: 'run cargo test' };
  for (const w of [80, 100, 40]) {
    const row = observeFrame(model('epic', { st: waiting }), w, 24).map(plain)[3]!;
    assert.match(row, /^ ▼ needs you {3}write specs\/009/, String(w));
    if (w >= 80) assert.match(row, /write specs\/009\.md {2}\(\+1 more\) +answer in the console $/);
    assert.equal(stringWidth(row) <= w, true);
  }
  const ratify = sample(); ratify.councils[0]!.escalation = null; ratify.pending_proposals = 2;
  assert.match(observeFrame(model('forge', { st: ratify }), 80, 24).map(plain)[3]!, /2 proposal\(s\) await \/ratify/);
  assert.match(observeFrame(model('council', { st: sample() }), 80, 24).map(plain)[3]!, /council t_2 needs a decision/, 'an escalated council');
});

test('views: six, in order; spend, dag, telemetry and digits are gone; the arrows stop at the ends', () => {
  assert.deepEqual([...VIEWS], ['council', 'epic', 'forge', 'agents', 'archives', 'thinking']);
  for (const gone of ['spend', 'dag', 'telemetry', '1', '4', '7']) assert.equal(parseView(gone), null, gone);
  assert.equal(stepView('council', -1), 'council');
  assert.equal(stepView('thinking', 1), 'thinking');
  assert.equal(stepView('epic', 1), 'forge');
  assert.equal(stepView('forge', -1), 'epic');
});

test('mascots are seven columns in every state; the builder keeps its white crown', () => {
  for (const role of Object.keys(info.roles)) for (const st of ['idle', 'running', 'waiting on you', 'escalated']) {
    for (const row of mascot(role, st, info, { dark: true })) assert.equal(stringWidth(row), 7, `${role} ${st}`);
  }
  const roster = observeFrame(model('agents'), 110, 40).map(plain).join('\n');
  assert.match(roster, /athena\s+orchestrator\s+○ idle/);
  assert.match(roster, /frontier · claude-x · direct/);
  assert.match(roster, /in 100 · cache 200 · out 50 · think 0 · 1 call\(s\)/);
});

test('council: one open column per seat, blind notes, critiques, verdict and minority report', () => {
  const t = observeFrame(model('council'), 110, 34).map(plain).join('\n');
  assert.match(t, /Council 1\/1 · t_2 · verdict/);
  assert.match(t, /athena|daedalus \(a\)/);
  assert.match(t, /Round 1 \(blind\)/);
  assert.match(t, /abstained: no answer/);
  assert.match(t, /Round 2 → revise · does not concur/);
  assert.match(t, /P1 lost updates/);
  assert.match(t, /Escalated to you: seats deadlocked/);
  assert.match(t, /option: table — architect/);
  assert.match(t, /\[P0\] red-team: the race/);
  assert.match(t, /saved: missions\/m\/councils\/008\.md/);
});

test('the blind seat shows the live text of its own node only', () => {
  const s = sample();
  s.councils[0]!.phase = 'round 1'; s.councils[0]!.stances = [];
  s.nodes['n_9'] = { id: 'n_9', role: 'architect', parent: null, depth: 1, task_id: 't_2', state: 'running', tokens: 0, cost_usd: 0, started_ms: 0, ended_ms: null, after: [] };
  const t = observeFrame(model('council', { st: s, live: { n_9: 'thinking about the index' } }), 110, 34).map(plain).join('\n');
  assert.match(t, /writing… \(blind\)\s/);
  assert.match(t, /thinking about the index/);
  assert.match(t, /Round 1 is blind/);
});

test('epic: the tree with indentation, the selected row, and the narrative', () => {
  const t = observeFrame(model('epic'), 110, 34).map(plain).join('\n');
  assert.match(t, /t_1 build · Implement the deck router/);
  assert.match(t, /● athena\s+running/);
  assert.match(t, /\n\s{4}✔ hephaestus\s+done\s+8000 tok/);
  assert.match(t, /── Narrative/);
  assert.match(t, /builder finished/);
});

test('forge: gates, attempts, findings, the flaky note and a coloured live diff', () => {
  const diff = { available: true, stat: ' a.rs | 2 +-', diff: '@@ -1 +1 @@\n-old\n+new', untracked: ['b.rs'] };
  const t = observeFrame(model('forge', { diff }), 110, 34).map(plain).join('\n');
  assert.match(t, /Build 1\/1 · t_1 · branch delphi\/t_1 · commit abc1234 · in progress/);
  assert.match(t, /✔ tests\s+12 passed/);
  assert.match(t, /✖ audit\s+1 finding/);
  assert.match(t, /audit P1 unchecked unwrap/);
  assert.match(t, /flaky/);
  assert.match(t, /── Diff against the base commit/);
  assert.match(t, /\?\? b\.rs/);
  assert.match(t, /\n\+new/);
  assert.match(observeFrame(model('forge', { diff: { available: false } }), 110, 34).map(plain).join('\n'), /no live worktree for this build/);
});

test('archives: the hits of a search and where to search from', () => {
  assert.match(observeFrame(model('archives'), 110, 34).map(plain).join('\n'), /from the console: ctrl\+k/);
  const a = observeFrame(model('archives', { search: { editing: true, query: 'retry', hits: [{ pointer: 'specs/003.md#retry', heading: 'Retry ladder', preview: 'three attempts\nthen escalate' }], note: '' } }), 110, 34).map(plain).join('\n');
  assert.match(a, /last search: retry/);
  assert.match(a, /1\. specs\/003\.md#retry\s+Retry ladder/);
  assert.match(a, /three attempts/);
});

test('model text cannot move the cursor, and view names parse', () => {
  const s = sample(); s.narrative = ['evil \x1b[2J\x1b]0;x\x07done'];
  const t = observeFrame(model('epic', { st: s }), 110, 34).join('\n');
  assert.ok(!t.includes('\x1b[2J') && !t.includes('\x1b]0;'));
  assert.deepEqual([parseView('Council'), parseView('roster'), parseView('kb'), parseView('nope')], ['council', 'agents', 'archives', null]);
});

test('thinking: reasoning per running agent with its badge, tier and model; empty when nothing thinks; nothing from other spans', () => {
  const s = sample();
  s.nodes['n_7'] = { id: 'n_7', role: 'architect', parent: null, depth: 0, task_id: 't_1', state: 'running', tokens: 0, cost_usd: 0, started_ms: 0, ended_ms: null, after: [] };
  const t = observeFrame(model('thinking', { st: s, think: { n_7: 'Weighing a table against a file.\nThe table indexes.' } }), 110, 30).map(plain).join('\n');
  assert.match(t, /⟦•─•⟧ daedalus\s+frontier · claude-x · t_1/);
  assert.match(t, /Weighing a table against a file\.\n\s+The table indexes\./);
  assert.match(observeFrame(model('thinking'), 110, 30).map(plain).join('\n'), /Nothing is thinking right now[\s\S]*never saved/);
  assert.equal(parseView('reasoning'), 'thinking');
});
