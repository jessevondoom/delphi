// Print the epic pane for the threads fixture: `node --import tsx dev/show-epic.ts <w> <h> [scroll]`. Colour is stripped.
import { observeFrame } from '../src/observe-screen.ts';
import { threadsWorld, info, NOW } from '../test/threads-fixture.ts';

const [w, h, scroll] = process.argv.slice(2).map(Number);
const m = { info, pal: { dark: true }, st: threadsWorld(), view: 'epic', pick: 0, scroll: scroll ?? 0, selNode: 0, search: { editing: false, query: '', hits: [], note: '' }, diff: null, live: {}, connected: true, still: true, now: NOW } as Parameters<typeof observeFrame>[0];
console.log(observeFrame(m, w || 80, h || 24).map((l) => l.replace(/\x1b\[[0-9;]*m/g, '')).join('\n'));
