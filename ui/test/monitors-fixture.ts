// A mission in full swing for the monitoring panes: a builder deep in a long run (live tokens, a loop forming, quiet for a
// moment), an architect and strategist done, a red-team waiting, changes recorded along the way, and two models that
// report their work to different degrees. Times are Unix ms; the window's clock is `NOW` (twenty minutes in).
import type { OModel, OState } from '../src/observe-screen.ts';
import { emptyArch } from '../src/archives.ts';
import { epicWorld, info, NOW, T0 } from './epic-fixture.ts';

const min = (n: number) => T0 + n * 60_000;

export function monitorsWorld(): OState {
  const s = epicWorld();
  s.token_cap = 500_000;
  s.agents['builder']!.model = 'claude-sonnet-5-5';
  s.agents['builder']!.tokens = { input: 90_000, cache_read: 210_000, cache_write: 0, output: 30_000, thinking: 0 };
  s.agents['architect']!.tokens = { input: 12_000, cache_read: 0, cache_write: 0, output: 4_000, thinking: 0 };
  s.agents['red-team']!.state = 'waiting on you';
  s.changes = [
    { session: 's_0001', at_ms: min(8), role: 'architect', task: 't_1', summary: 'wrote the retry queue design', files: ['specs/040-retry.md'] },
    { session: 's_0001', at_ms: min(16), role: 'builder', task: 't_1', summary: 'added a visibility timeout to the claim loop', files: [] },
    { session: 's_0001', at_ms: min(18), role: 'builder', task: 't_1', summary: 'wired the timeout into the config', files: [] },
  ];
  s.tools = {
    architect: { calls: 9, errors: 0, last: 'write_file', streak: { tool: 'write_file', n: 1, failed: 0 }, by_tool: { read_file: { calls: 7, errors: 0 }, write_file: { calls: 1, errors: 0 }, record_change: { calls: 1, errors: 0 } } },
    builder: { calls: 41, errors: 6, last: 'run_command', streak: { tool: 'run_command', n: 7, failed: 4 }, by_tool: { read_file: { calls: 18, errors: 0 }, edit_file: { calls: 9, errors: 1 }, run_command: { calls: 12, errors: 5 }, record_change: { calls: 2, errors: 0 } } },
  };
  s.adoption = {
    by_model: {
      'claude-sonnet-5-5': { spans: 6, calls: 38, errors: 4, worked: 5, recorded: 5 },
      'gemini-flash': { spans: 4, calls: 22, errors: 3, worked: 4, recorded: 1 },
      'gpt-oss': { spans: 2, calls: 9, errors: 0, worked: 2, recorded: 1 },
    },
    total: { spans: 12, calls: 69, errors: 7, worked: 11, recorded: 7 },
  };
  return s;
}

/** Tokens per sample over the last three minutes: the builder ramping up, the architect long quiet. */
export const pulseSamples = (): Record<string, number[]> => ({
  builder: [200, 400, 900, 1200, 800, 1500, 2600, 3100, 2200, 1800, 2900, 4200, 3800, 1200, 600, 2500, 3300, 4800, 5200, 4100, 3000, 3600, 4400, 5100, 2100, 900, 1400, 2600, 3900, 4600, 5000, 4300, 3500, 2800, 3300, 3900],
  architect: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
});

export function monitorsModel(view: OModel['view'], over: Partial<OModel> = {}): OModel {
  return {
    info, pal: { dark: true }, st: monitorsWorld(), view, pick: 0, scroll: 0, selNode: 0, arch: emptyArch(), live: {}, connected: true, still: true, now: NOW,
    liveTokens: { n3: 410_000 }, lastSeen: { n3: NOW - 190_000 }, since: NOW - 600_000, pulse: pulseSamples(), ...over,
  } as OModel;
}
