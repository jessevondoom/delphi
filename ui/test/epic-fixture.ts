// The roles and the clock the console's tests share. Times are Unix ms from `T0`; the window's clock is `NOW`.
import type { Info } from '../src/theme.ts';

export const T0 = 1_000_000_000_000;
export const NOW = T0 + 20 * 60_000;

const crown: Record<string, string> = { orchestrator: '  ▄▄▄  ', architect: '▄▄▄▄▄▄▄', builder: '▄▄▄▄▄▄▄', 'red-team': '▄     ▄', strategist: '▄▄ ▄ ▄▄', scout: '   ▄   ' };
const mk = (name: string, chassis: string, face: string, role: string) => ({ name, chassis, face, crown: crown[role], crown_rgb: '#facc15' });
export const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: { orchestrator: mk('athena', '#a855f7', '•─•', 'orchestrator'), architect: mk('daedalus', '#38bdf8', '•_•', 'architect'), builder: mk('hephaestus', '#f97316', '•─•', 'builder'), 'red-team': mk('ares', '#ef4444', '-_-', 'red-team'), strategist: mk('odysseus', '#22c55e', '•_•', 'strategist'), scout: mk('hermes', '#eab308', '•••', 'scout') },
};
