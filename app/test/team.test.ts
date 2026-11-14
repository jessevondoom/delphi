import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { DocRow } from '../src/kb.ts';
import { daemonFacts, skillRows, teamRows } from '../src/team.ts';

const row = (rel_path: string, source_type = 'user_ratified'): DocRow => ({ rel_path, zone: rel_path.split('/')[0]!, kind: 'other', status: 'active', source_type, updated_at: '2026-11-01T00:00:00Z' });

test('each role with a manifest is one row, the six of the mark first, flagged from what the daemon says', () => {
  const docs = [row('agents/writer/AGENT.md'), row('agents/builder/notes.md'), row('agents/builder/AGENT.md'), row('agents/orchestrator/AGENT.md'), row('agents/scout/checklist.md'), row('skills/x/SKILL.md'), row('agents/deep/er/AGENT.md')];
  const rows = teamRows(docs, { interactive_agents: ['orchestrator'], forbidden_agents: ['writer'], enabled_agents: ['orchestrator', 'writer', 'scout'] });
  assert.deepEqual(rows.map((r) => [r.role, r.path, r.flag]), [
    ['orchestrator', 'agents/orchestrator/AGENT.md', 'you can address it'],
    ['scout', 'agents/scout/checklist.md', ''],
    ['builder', 'agents/builder/AGENT.md', 'switched off'],
    ['writer', 'agents/writer/AGENT.md', 'not allowed'],
  ]);
  // A daemon that says nothing about agents flags nothing.
  assert.ok(teamRows(docs, {}).every((r) => r.flag === ''));
});

test('skills are listed by name with where each came from', () => {
  const rows = skillRows([row('skills/zeta/SKILL.md'), row('skills/alpha/SKILL.md', 'internal_generated'), row('skills/alpha/extra.md'), row('agents/x/AGENT.md')]);
  assert.deepEqual(rows, [{ name: 'alpha', path: 'skills/alpha/SKILL.md', source: 'written by an agent' }, { name: 'zeta', path: 'skills/zeta/SKILL.md', source: 'ratified by the operator' }]);
});

test('the daemon\'s settings are shown as it sent them, and a field it did not send is left out', () => {
  const facts = daemonFacts({ mission_id: 'm1', mode: 'default', budget: { cap_usd: 25, spent_usd: 3.4, available_usd: 21.1 }, solo_mode: false, running_tasks: [{}, {}], pending_approvals: [] });
  assert.deepEqual(facts, [
    { label: 'Mission', value: 'm1' }, { label: 'Approvals', value: 'default' }, { label: 'Budget cap', value: '$25.00' }, { label: 'Spent', value: '$3.40' },
    { label: 'Still available', value: '$21.10' }, { label: 'Solo mode', value: 'off' }, { label: 'Tasks running', value: '2' }, { label: 'Waiting on you', value: '0' },
  ]);
  assert.deepEqual(daemonFacts({}), []);
  assert.deepEqual(daemonFacts({ budget: { cap_usd: 'lots' }, mode: 7 }), []);
});
