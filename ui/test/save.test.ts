import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathOf, plainLabel, saveCommand } from '../src/save.ts';
import type { Info } from '../src/theme.ts';

const info = { delphi_exe: '/bin/delphi', workspace_name: 'w', home: '/h' } as Info;

test('/save sends the last reply to `kb import -`, and a dragged-in path to `kb import <path>`', () => {
  const r = saveCommand(info, '', 'The answer');
  assert.deepEqual(r, { args: ['kb', 'import', '-', '--workspace', 'w', '--home', '/h'], stdin: 'The answer' });
  const dir = mkdtempSync(join(tmpdir(), 'save-'));
  const f = join(dir, 'my notes.md');
  writeFileSync(f, '# x');
  assert.deepEqual(saveCommand(info, `'${f}'`, undefined), { args: ['kb', 'import', f, '--workspace', 'w', '--home', '/h'] });
  assert.equal(pathOf(f.replace(/ /g, '\\ ')), f);
});

test('/save says what to do when there is nothing to save, no such file, or no delphi', () => {
  assert.match((saveCommand(info, '', undefined) as { error: string }).error, /nothing to save/);
  assert.match((saveCommand(info, '/no/such.md', 'x') as { error: string }).error, /no such file/);
  assert.match((saveCommand({} as Info, '', 'x') as { error: string }).error, /delphi kb import/);
});

test('the console says "not yet checked" and keeps the exact machine label beside it', () => {
  const machine = 'summary · operator-imported · external_unvalidated';
  assert.deepEqual(plainLabel(machine), { plain: 'imported summary · not yet checked', machine });
  assert.equal(plainLabel('trusted'), null);
});
