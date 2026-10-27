// Started by test/elog.test.ts: initialise the central error log, then fail in one of several ways (with and without the Term's own handlers).
import { Term } from '../../src/terminal.ts';
import { initElog, watchProcess } from '../../src/elog.ts';

initElog({ component: 'ui', version: 'test' }); // DELPHI_HOME names the temporary home
watchProcess();
const mode = process.argv[2]!;
if (mode.startsWith('term-')) new Term().start();
const secret = 'sk-ant-api03-GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG';
setTimeout(() => {
  if (mode.endsWith('throw')) throw new Error(`boom ${secret}\nsecond line \x1b[31m did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`);
  if (mode.endsWith('reject')) void Promise.reject(new Error(`rejected ${secret}`));
}, 40);
setInterval(() => {}, 1000);
