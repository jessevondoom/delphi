// Started by test/hardening.test.ts: take over the terminal the way the console does, then fail in one of several ways.
import { Term } from '../../src/terminal.ts';

const t = new Term();
t.start();
const mode = process.argv[2];
setTimeout(() => {
  if (mode === 'throw') throw new Error('boom');
  if (mode === 'reject') void Promise.reject(new Error('boom'));
  if (mode === 'term') process.kill(process.pid, 'SIGTERM');
  if (mode === 'hup') process.kill(process.pid, 'SIGHUP');
  if (mode === 'int') process.kill(process.pid, 'SIGINT');
  if (mode === 'exit') process.exit(0);
}, 60);
setInterval(() => {}, 1000); // the real console is kept alive by its stdin and sockets; a signal alone does not keep a process running
