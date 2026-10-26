// Print the monitoring panes for a fixture mission: `node --import tsx dev/sketch-panes.ts [pulse|timeline|tools|agents] [w] [h] [--color]`.
// With no view it prints all of them. Colour is stripped unless `--color`. A sketch: edit the fixture or the panes and run it again.
import { observeFrame, VIEWS } from '../src/observe-screen.ts';
import { monitorsModel } from '../test/monitors-fixture.ts';

const args = process.argv.slice(2).filter((a) => !a.startsWith('--'));
const color = process.argv.includes('--color');
const views = args[0] && (VIEWS as readonly string[]).includes(args[0]) ? [args[0]] : ['agents', 'pulse', 'timeline', 'tools'];
const nums = args.filter((a) => /^\d+$/.test(a)).map(Number);
const [w, h] = [nums[0] ?? 100, nums[1] ?? 30];
for (const v of views) {
  const lines = observeFrame(monitorsModel(v as never), w, h);
  console.log(`\n=== ${v} (${w}x${h}) ===`);
  console.log(lines.map((l) => (color ? l : l.replace(/\x1b\[[0-9;]*m/g, ''))).join('\n'));
}
