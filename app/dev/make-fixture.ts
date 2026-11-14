// Writes src/fixture/sim.json: the console simulator's `observe.state` and a minute of its events, at a fixed time.
// The simulator is the console's own (`ui/dev/preview-sim.ts`), so the app's screens are tested against the shape the
// console is tested against. Run from `app/`: `node --import tsx dev/make-fixture.ts`.
import { writeFileSync } from 'node:fs';
import { Preview } from '../../ui/dev/preview-sim.ts';

const now = Date.UTC(2026, 10, 14, 14, 30, 0);
const sim = new Preview(now, { seed: 7 });
const events: unknown[] = [];
let t = now;
for (let i = 0; i < 60; i++) {
  t += 1000;
  events.push(...sim.tick(t).events);
}
writeFileSync(new URL('../src/fixture/sim.json', import.meta.url), JSON.stringify({ now: t, state: sim.state(), events }, null, 1) + '\n');
console.log(`wrote src/fixture/sim.json: ${events.length} events`);
