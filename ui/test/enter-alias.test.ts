// Enter with the completion menu open runs a whole command or alias that was typed, instead of rewriting it to the
// canonical name: `/clear` for `/new` used to take two Enters, and so did `/exit` and `/q`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { completions, enterAccepts } from '../src/complete.ts';
import { info } from './epic-fixture.ts';

const items = (typed: string) => completions(typed, typed.length, info, () => [])!.items;

test('a typed alias runs on the first Enter; a partial name completes; a moved highlight completes', () => {
  for (const [typed, canonical] of [['/clear', '/new'], ['/exit', '/quit'], ['/q', '/quit'], ['/new', '/new']] as const) {
    const first = items(typed)[0]!;
    assert.equal(first.insert, canonical, typed);
    assert.equal(enterAccepts(typed, first, 0), false, `${typed} runs`);
  }
  assert.equal(enterAccepts('/ne', items('/ne')[0]!, 0), true, 'a partial name is completed, not run');
  assert.equal(enterAccepts('/clear', items('/clear')[0]!, 1), true, 'the operator picked another item: take it');
  assert.equal(enterAccepts('@arch', { label: '@architect', desc: '', insert: '@architect ' }, 0), true, 'a role mention is completed');
});
