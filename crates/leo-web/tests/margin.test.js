const assert = require('node:assert/strict');
const M = require('../src/web/margin.js');

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

const source = 'Save the password in the users table,\nthen login just checks if it matches, I think?\n\nSessions last a day.';
const quote = 'just checks if it matches';

test('pauseFor: a finished last block keeps the pace and an unfinished one waits half again', () => {
  for (const [pace, ms] of [['fast', 2000], ['regular', 4000], ['slow', 8000]]) {
    assert.equal(M.pauseFor('It ends here.', pace), ms);
    assert.equal(M.pauseFor('It is still going', pace), ms * 1.5);
  }
  assert.equal(M.pauseFor('Really?', 'regular'), 4000);
  assert.equal(M.pauseFor('Stop!', 'regular'), 4000);
  assert.equal(M.pauseFor('他说完了。', 'regular'), 4000);
  assert.equal(M.pauseFor('真的吗？', 'regular'), 4000);
  assert.equal(M.pauseFor('她还在写', 'regular'), 6000);
  assert.equal(M.pauseFor('She said "done."', 'regular'), 4000);
  assert.equal(M.pauseFor('# A heading', 'regular'), 4000);
  assert.equal(M.pauseFor('- one\n- a list item', 'regular'), 4000);
  assert.equal(M.pauseFor('1. a numbered item', 'regular'), 4000);
  assert.equal(M.pauseFor('```js\nlet x = 1', 'regular'), 4000);
  assert.equal(M.pauseFor('', 'regular'), 4000);
  assert.equal(M.pauseFor(undefined, 'regular'), 4000);
  assert.equal(M.pauseFor('Unfinished', 'unknown'), 6000);
});

test('anchor: one exact match', () => {
  const at = M.find(source, { quote });
  assert.equal(source.slice(at.from, at.to), quote);
  assert.equal(at.how, 'exact');
});

test('anchor: several matches are told apart by prefix and suffix', () => {
  const text = 'one same words two\n\nthree same words four';
  const second = M.find(text, { quote: 'same words', prefix: 'three ', suffix: ' four' });
  assert.equal(second.from, text.lastIndexOf('same words'));
  const first = M.find(text, { quote: 'same words', prefix: 'one ', suffix: ' two' });
  assert.equal(first.from, text.indexOf('same words'));
  assert.equal(M.find(text, { quote: 'same words' }).from, text.indexOf('same words'));
});

test('anchor: an edited quote is found between its prefix and suffix', () => {
  const edited = source.replace('just checks if it matches', 'only compares the two');
  const at = M.find(edited, { quote, prefix: 'table,\nthen login ', suffix: ', I think?\n\nSessions last' });
  assert.equal(edited.slice(at.from, at.to), 'only compares the two');
  assert.equal(at.how, 'context');
});

test('anchor: at the very start or very end of a note', () => {
  const edited = 'Rewritten opening\n\nrest of the note';
  const start = M.find(edited, { quote: 'Old opening', prefix: '', suffix: '\n\nrest of the note' });
  assert.equal(edited.slice(start.from, start.to), 'Rewritten opening');
  const end = M.find(edited, { quote: 'Old tail', prefix: 'Rewritten opening\n\n', suffix: '' });
  assert.equal(edited.slice(end.from, end.to), 'rest of the note');
});

test('anchor: lost when the quote and its surroundings are gone', () => {
  assert.equal(M.find('Nothing like it.', { quote, prefix: 'table,\nthen login ', suffix: ', I think?' }), null);
  assert.equal(M.find(source, { quote: 'gone words', prefix: 'then login ', suffix: 'no such tail here' }), null);
  assert.equal(M.find(source, { quote: 'gone words' }), null);
  assert.equal(M.find(source, { quote: '' }), null);
});

test('anchor: lost when the passage between prefix and suffix was emptied or is too far apart', () => {
  assert.equal(M.find('table,\nthen login , I think?', { quote, prefix: 'then login ', suffix: ', I think?' }), null);
  assert.equal(M.find('then login   , I think?', { quote, prefix: 'then login ', suffix: '  , I think?' }), null);
  const far = `then login ${'word '.repeat(100)}, I think?`;
  assert.equal(M.find(far, { quote: 'short', prefix: 'then login ', suffix: ', I think?' }), null);
});

test('anchor: a short or missing side never attaches to unrelated text', () => {
  const text = 'ab some new words and a long enough tail';
  assert.equal(M.find(text, { quote: 'old words', prefix: 'ab ', suffix: ' and a long enough tail' }), null);
  assert.equal(M.find(text, { quote: 'old words', prefix: 'ab some ', suffix: ' t' }), null);
  assert.equal(M.find('anything at all here', { quote: 'old words', prefix: '', suffix: '' }), null);
});

test('edited: an insert, a delete, a replace and no change', () => {
  assert.deepEqual(M.edited('abcdef', 'abcXdef'), { from: 3, to: 3, by: 1 });
  assert.deepEqual(M.edited('abcdef', 'abef'), { from: 2, to: 4, by: -2 });
  assert.deepEqual(M.edited('abcdef', 'abXYef'), { from: 2, to: 4, by: 0 });
  assert.deepEqual(M.edited('abcdef', 'abcdef'), { from: 6, to: 6, by: 0 });
  assert.deepEqual(M.edited('aaa', 'aaaa'), { from: 3, to: 3, by: 1 });
});

test('carried: a place before the edit stays, one after it moves by the edit, one touching its context is found again', () => {
  const t = { quote: 'quote', prefix: 'say ', suffix: ' now' };
  const before = 'Here we say quote now and then more text follows here.';
  const at = M.find(before, t);
  const text = before.slice(at.from, at.to);
  const late = `${before}!!`;
  assert.deepEqual(M.carried(late, at, M.edited(before, late), text, t), at);
  const early = `XX${before}`;
  assert.deepEqual(M.carried(early, at, M.edited(before, early), text, t), { ...at, from: at.from + 2, to: at.to + 2 });
  const touching = before.replace('say ', 'said ');
  assert.equal(M.carried(touching, at, M.edited(before, touching), text, t), null);
  const inside = before.replace('quote', 'quota');
  assert.equal(M.carried(inside, at, M.edited(before, inside), text, t), null);
  assert.deepEqual(M.carried(before, at, null, text, t), at);
});

test('carried: a surrogate pair typed before a place shifts it by two units', () => {
  const t = { quote: 'quote', prefix: 'say ', suffix: ' now' };
  const before = 'Start. Here we say quote now.';
  const at = M.find(before, t);
  const after = `\u{1F600}${before}`;
  const moved = M.carried(after, at, M.edited(before, after), before.slice(at.from, at.to), t);
  assert.deepEqual(moved, { ...at, from: at.from + 2, to: at.to + 2 });
  assert.equal(after.slice(moved.from, moved.to), 'quote');
});

test("carried: a context-found place keeps how 'context' when shifted", () => {
  const t = { quote, prefix: 'table,\nthen login ', suffix: ', I think?\n\nSessions last' };
  const edited = source.replace('just checks if it matches', 'only compares the two');
  const at = M.find(edited, t);
  assert.equal(at.how, 'context');
  const after = `New first line.\n${edited}`;
  const moved = M.carried(after, at, M.edited(edited, after), edited.slice(at.from, at.to), t);
  assert.equal(moved.how, 'context');
  assert.equal(after.slice(moved.from, moved.to), 'only compares the two');
});

test('around gives the words beside a selection on its own line', () => {
  const text = 'same words\n\nsame words';
  assert.deepEqual(M.around(text, 'same words', 3), { prefix: 'same words\n\n', suffix: '' });
  assert.deepEqual(M.around(text, 'same words', 1), { prefix: '', suffix: '\n\nsame words' });
  assert.equal(M.around(text, 'missing', 1), null);
  assert.deepEqual(M.around('only here', 'only', 9), { prefix: '', suffix: ' here' });
});

test('around never splits a surrogate pair', () => {
  for (const pad of ['', 'x']) {
    const text = `${'😀'.repeat(40)}${pad}words${pad}${'😀'.repeat(40)}`;
    const beside = M.around(text, 'words', 1);
    assert.ok(beside.prefix.isWellFormed() && beside.suffix.isWellFormed());
    assert.ok(JSON.stringify(beside).length > 0);
    assert.ok(text.includes(`${beside.prefix}words${beside.suffix}`));
  }
});

test('lineAfter counts the lines up to an offset', () => {
  assert.equal(M.lineAfter('one\ntwo\nthree', 5), 2);
  assert.equal(M.lineAfter('one', 0), 1);
});

test('stacking: notes stay where they want when there is room', () => {
  assert.deepEqual(M.stack([{ id: 'a', want: 10, height: 40 }, { id: 'b', want: 200, height: 40 }]), { a: 10, b: 200 });
});

test('stacking: a collision pushes the later note down, in order', () => {
  const tops = M.stack([{ id: 'b', want: 20, height: 40 }, { id: 'a', want: 10, height: 40 }, { id: 'c', want: 30, height: 40 }], { gap: 10 });
  assert.deepEqual(tops, { a: 10, b: 60, c: 110 });
});

test('stacking: an open note holds its place and others make way above and below', () => {
  const tops = M.stack([{ id: 'a', want: 100, height: 40 }, { id: 'b', want: 110, height: 120 }, { id: 'c', want: 120, height: 40 }], { gap: 10, pinned: 'b' });
  assert.equal(tops.b, 110);
  assert.equal(tops.a, 60);
  assert.equal(tops.c, 240);
});

test('stacking: nothing goes above the floor, even with an open note', () => {
  const tops = M.stack([{ id: 'a', want: 5, height: 40 }, { id: 'b', want: 10, height: 40 }], { gap: 10, pinned: 'b', floor: 0 });
  assert.equal(tops.a, 0);
  assert.equal(tops.b, 50);
  assert.deepEqual(M.stack([], {}), {});
  assert.deepEqual(M.stack([{ id: 'a', want: -30, height: 10 }], { floor: 0 }), { a: 0 });
});

test('both margins: a crowded right side spills left', () => {
  const on = M.sides([{ id: 'a', want: 0, height: 100 }, { id: 'b', want: 10, height: 100 }, { id: 'c', want: 20, height: 100 }, { id: 'd', want: 400, height: 10 }]);
  assert.deepEqual(on, { a: 'right', b: 'left', c: 'right', d: 'right' });
});

test('both margins: spaced notes all sit on the right while it has room', () => {
  const on = M.sides([{ id: 'a', want: 0, height: 80 }, { id: 'b', want: 300, height: 80 }, { id: 'c', want: 600, height: 80 }, { id: 'd', want: 900, height: 80 }]);
  assert.deepEqual(on, { a: 'right', b: 'right', c: 'right', d: 'right' });
});

test('both margins: a note goes left only where the right is taken at its height', () => {
  const on = M.sides([{ id: 'a', want: 100, height: 120 }, { id: 'b', want: 150, height: 60 }, { id: 'c', want: 400, height: 60 }]);
  assert.deepEqual(on, { a: 'right', b: 'left', c: 'right' });
});

test('both margins: a new note never moves a placed note whose side is still free', () => {
  const items = [{ id: 'a', want: 0, height: 80 }, { id: 'b', want: 300, height: 80 }, { id: 'c', want: 600, height: 80 }];
  const before = M.sides(items);
  const after = M.sides([...items, { id: 'new', want: 150, height: 80 }], 16, before);
  for (const id of ['a', 'b', 'c']) assert.equal(after[id], before[id], id);
  assert.equal(M.sides([{ id: 'a', want: 0, height: 80 }], 16, { a: 'left', gone: 'right' }).a, 'left');
});

test('reading dots never move notes: free spot, then the other side, then below what blocks them', () => {
  const note = { top: 100, height: 80, side: 'right' };
  assert.deepEqual(M.clear({ want: 400, height: 16 }, [note]), { side: 'right', top: 400 });
  assert.deepEqual(M.clear({ want: 120, height: 16 }, [note], true), { side: 'left', top: 120 });
  assert.deepEqual(M.clear({ want: 120, height: 16 }, [note, { top: 196, height: 40, side: 'right' }]), { side: 'right', top: 252 });
});

test('layout: the margin is reserved up front, with or without threads', () => {
  assert.equal(M.arrangement({ width: 960, setting: 'right' }), 'right');
  assert.equal(M.arrangement({ width: 1076 }), 'both');
  assert.equal(M.arrangement({ width: M.ROOM_RIGHT, setting: 'right' }), 'right');
  assert.equal(M.arrangement({ width: M.ROOM_RIGHT - 1, setting: 'right' }), 'marks');
  assert.equal(M.arrangement.length, 1);
  assert.equal(M.arrangement({ width: 1076, threads: 0, along: false }), M.arrangement({ width: 1076, threads: 3, along: true }));
});

test('layout: a chosen layout never falls back to one side, only to marks', () => {
  assert.equal(M.arrangement({ width: M.ROOM_BOTH, setting: 'both' }), 'both');
  assert.equal(M.arrangement({ width: M.ROOM_BOTH - 1, setting: 'both' }), 'marks');
  assert.equal(M.arrangement({ width: M.ROOM_BOTH - 1, setting: 'right' }), 'right');
});

test('layout: marks only by choice or full width, with Felix open the room left decides', () => {
  assert.equal(M.arrangement({ width: 1600, setting: 'marks' }), 'marks');
  assert.equal(M.arrangement({ width: 1600, noteWidth: 'full' }), 'marks');
  assert.equal(M.arrangement({ width: 1600 - 520 }), 'both');
  assert.equal(M.arrangement({ width: 1100 - 520 }), 'marks');
});

test('width: notes fill the room beside the column, 240 to 360 px on one side and 180 to 360 on both', () => {
  assert.equal(M.marginWidth({ width: 1600, column: 980, layout: 'right' }), M.MOST_MARGIN);
  assert.equal(M.marginWidth({ width: 1200, column: 980, layout: 'right' }), M.MARGIN_WIDTH);
  assert.equal(M.marginWidth({ width: 1400, column: 980, layout: 'right' }), 1400 - 980 - 64 - M.MARGIN_GAP);
  assert.equal(M.marginWidth({ width: 1800, column: 980, layout: 'both' }), Math.floor((1800 - 980 - 64) / 2) - M.MARGIN_GAP);
  assert.equal(M.marginWidth({ width: 2400, column: 980, layout: 'both' }), M.MOST_MARGIN);
  assert.equal(M.marginWidth({ width: 1440, column: 980, layout: 'both' }), M.LEAST_SHARED);
  assert.equal(M.marginWidth({ width: 1500, column: 760, layout: 'both' }), Math.floor((1500 - 760 - 64) / 2) - M.MARGIN_GAP);
  assert.equal(M.marginWidth({ width: 1600, layout: 'right' }), M.MARGIN_WIDTH);
});

test('width: Both keeps notes on both sides whenever both fit, too narrow stays marks', () => {
  assert.equal(M.ROOM_BOTH, 48 + M.SHARED_COLUMN + 2 * (M.MARGIN_GAP + M.LEAST_SHARED) + 16);
  assert.ok(M.ROOM_BOTH > M.ROOM_RIGHT);
  for (const width of [1100, 1400, 1600, 1900, M.ROOM_BOTH]) assert.equal(M.arrangement({ width }), 'both');
  assert.equal(M.arrangement({ width: M.ROOM_BOTH - 1 }), 'marks');
  assert.equal(M.arrangement({ width: 1600, setting: 'right' }), 'right');
  assert.equal(M.arrangement({ width: M.ROOM_RIGHT, setting: 'right' }), 'right');
  assert.equal(M.arrangement({ width: M.ROOM_RIGHT - 1, setting: 'right' }), 'marks');
  assert.equal(M.arrangement({ width: 1600, noteWidth: 'full' }), 'marks');
});

test('width: notes never squeeze the column below its least width', () => {
  for (const column of [760, 980]) {
    for (const setting of ['both', 'right']) for (let width = setting === 'both' ? M.ROOM_BOTH : M.ROOM_RIGHT; width <= 2600; width += 7) {
      const layout = M.arrangement({ width, setting });
      assert.equal(layout, setting);
      const size = M.marginWidth({ width, column, layout });
      const sides = layout === 'both' ? 2 : 1;
      const left = width - 64 - sides * (size + M.MARGIN_GAP);
      const least = layout === 'both' ? M.LEAST_SHARED : M.MARGIN_WIDTH;
      assert.ok(size >= least && size <= M.MOST_MARGIN, `${width} ${column}`);
      assert.ok(left >= (layout === 'both' ? M.SHARED_COLUMN : M.LEAST_COLUMN), `${width} ${column} ${layout}`);
      if (size > least) assert.ok(left >= column, `${width} ${column} ${layout}`);
    }
  }
});

test('words: arrive at the usual pace and never take longer than the cap', () => {
  assert.equal(M.wordDelay(0, 1), 0);
  assert.equal(M.wordDelay(3, 5), 186);
  for (const count of [21, 60, 500]) {
    assert.equal(M.wordDelay(0, count), 0);
    assert.ok(Math.abs(M.wordDelay(count - 1, count) - 650) < 1, `${count} words`);
  }
});

test('changed blocks: only new or edited blocks, each counted once', () => {
  const before = M.blocksOf('First paragraph long enough.\n\nSame paragraph long enough.');
  assert.deepEqual(M.changedBlocks(before, 'First paragraph long enough.\n\nSame paragraph long enough.'), []);
  assert.deepEqual(M.changedBlocks(before, 'First paragraph long enough.\n\nSame paragraph long enough!'), ['Same paragraph long enough!']);
  assert.deepEqual(M.changedBlocks(['Repeated block long enough.'], 'Repeated block long enough.\n\nRepeated block long enough.'), ['Repeated block long enough.']);
});

test('changed blocks: short scraps wait and the amount sent is bounded', () => {
  assert.deepEqual(M.changedBlocks([], 'ok\n\nfine'), []);
  const many = Array.from({ length: 9 }, (_, i) => `Block number ${i} long enough to read.`).join('\n\n');
  const picked = M.changedBlocks([], many);
  assert.equal(picked.length, 6);
  assert.ok(picked[5].includes('number 8'));
  const huge = M.changedBlocks([], `${'é'.repeat(9000)}\n\n${'b'.repeat(3000)}`);
  assert.ok([...huge.join('')].length <= 8000);
  assert.ok([...huge[0]].length <= 4000);
  assert.ok(`${'é'.repeat(9000)}`.endsWith(huge[0]));
});

test('drawing: a wave spans its width', () => {
  assert.ok(M.wave(10, 100, 20).startsWith('M10.0 20.0 q'));
});


 test('both margins are the default, saved choices and narrow fallback survive', () => {
  assert.equal(M.arrangement({ width: 1800 }), 'both');
  assert.equal(M.arrangement({ width: 1800, setting: 'right' }), 'right');
  assert.equal(M.arrangement({ width: 1000 }), 'both');
  assert.equal(M.arrangement({ width: 960 }), 'marks');
  assert.equal(M.arrangement({ width: 960, setting: 'right' }), 'right');
  assert.equal(M.arrangement({ width: 800 }), 'marks');
});

const para = (n) => `Paragraph number ${n} has enough words.`;

test('passes: empty and short notes give nothing', () => {
  assert.deepEqual(M.passes(''), { parts: [], spread: false });
  assert.deepEqual(M.passes('too short\n\nalso short').parts, []);
});

test('passes: blocks with a taken quote are skipped', () => {
  const text = `${para(1)}\n\n${para(2)}`;
  assert.deepEqual(M.passes(text, ['number 2']).parts[0].blocks, [para(1)]);
});

test('passes: a 9,000 character block is cut into substrings', () => {
  const big = 'abcdefghi '.repeat(900).trim();
  const { parts } = M.passes(`${big}\n\n${para(1)}`);
  const blocks = parts.flatMap((p) => p.blocks);
  assert.ok(blocks.length >= 3);
  for (const b of blocks) {
    assert.ok([...b].length <= 4000);
    assert.ok(`${big}\n\n${para(1)}`.includes(b));
  }
});

test('passes: parts hold at most 6 blocks and 8,000 characters', () => {
  const six = Array.from({ length: 7 }, (_, i) => para(i)).join('\n\n');
  assert.deepEqual(M.passes(six).parts.map((p) => p.blocks.length), [6, 1]);
  const wide = [1, 2, 3].map((n) => `${n}${'x'.repeat(3499)}`).join('\n\n');
  assert.deepEqual(M.passes(wide).parts.map((p) => p.blocks.length), [2, 1]);
});

test('passes: from and to are 1-based lines', () => {
  const text = `${para(1)}\nsecond line of the first block\n\n\n${para(2)}`;
  assert.deepEqual(M.passes(text).parts.map(({ from, to }) => [from, to]), [[1, 5]]);
  const split = `${para(1)}\n\n${'y'.repeat(3990)}\n\n${'z'.repeat(3990)}\n\n${para(3)}`;
  assert.deepEqual(M.passes(split).parts.map(({ from, to }) => [from, to]), [[1, 3], [5, 7]]);
});

test('passes: more than 8 parts keep 8 spread across the note', () => {
  const text = Array.from({ length: 120 }, (_, i) => para(i)).join('\n\n');
  const r = M.passes(text);
  assert.equal(r.spread, true);
  assert.deepEqual(r.parts.map((p) => p.blocks[0]), [0, 3, 5, 8, 11, 14, 16, 19].map((i) => para(i * 6)));
  assert.equal(M.passes(Array.from({ length: 48 }, (_, i) => para(i)).join('\n\n')).spread, false);
});

test('passes: surrogate pairs are kept whole', () => {
  const emoji = '\u{1F600}'.repeat(4001);
  const { parts } = M.passes(emoji);
  assert.deepEqual(parts[0].blocks.map((b) => [...b].length), [4000]);
  assert.equal(parts[0].blocks.length, 1);
  assert.ok(parts.every((p) => p.blocks.every((b) => !/[\ud800-\udbff]$|^[\udc00-\udfff]/.test(b))));
});


test('a dragged page width stops where the chosen margins still fit', () => {
  const room = (width, column, sides) => Math.floor((width - column - 64) / sides) - M.MARGIN_GAP;
  const width = 1156;
  assert.equal(M.mostColumn({ width, setting: 'both' }), 684);
  assert.equal(M.mostColumn({ width, setting: 'right' }), 828);
  for (const setting of ['both', 'right']) {
    const most = M.mostColumn({ width, setting });
    const sides = setting === 'both' ? 2 : 1;
    const least = setting === 'both' ? M.LEAST_SHARED : M.MARGIN_WIDTH;
    assert.equal(M.arrangement({ width, setting }), setting);
    assert.equal(room(width, most, sides), least);
    assert.ok(room(width, most + 1, sides) < least);
  }
  assert.equal(M.mostColumn({ width: M.ROOM_BOTH, setting: 'both' }), M.SHARED_COLUMN);
  assert.equal(M.mostColumn({ width: M.ROOM_RIGHT, setting: 'right' }), M.LEAST_COLUMN);
  assert.equal(M.mostColumn({ width: M.ROOM_BOTH - 1, setting: 'both' }), Infinity);
  assert.equal(M.mostColumn({ width: M.ROOM_RIGHT - 1, setting: 'right' }), Infinity);
  assert.equal(M.mostColumn({ width, setting: 'marks' }), Infinity);
  assert.equal(M.mostColumn({ width, setting: 'both', noteWidth: 'full' }), Infinity);
});

const copies = 'alpha one. beta gamma. delta two. beta gamma. epsilon three.';

test('anchor: two copies whose context matches neither are a guess on the first', () => {
  const got = M.find(copies, { quote: 'beta gamma', prefix: 'zzzzzzzz yyyyyy ', suffix: ' qqqqqq wwwww' });
  assert.equal(got.from, copies.indexOf('beta gamma'));
  assert.equal(got.how, 'guess');
});

test('anchor: two copies, the context picks the second', () => {
  const got = M.find(copies, { quote: 'beta gamma', prefix: 'delta two. ', suffix: '. epsilon three.' });
  assert.equal(got.from, copies.lastIndexOf('beta gamma'));
  assert.equal(got.how, 'exact');
});

test('anchor: one copy is exact whatever its context', () => {
  const got = M.find('alpha one. beta gamma. delta two.', { quote: 'beta gamma', prefix: 'zzzzzzzz', suffix: 'qqqqqqqq' });
  assert.deepEqual(got, { from: 11, to: 21, how: 'exact' });
});

test('anchor: two copies with the same context are a guess', () => {
  const got = M.find('x beta gamma y. x beta gamma y.', { quote: 'beta gamma', prefix: 'x ', suffix: ' y.' });
  assert.equal(got.from, 2);
  assert.equal(got.how, 'guess');
});

test('anchor: an empty prefix confirms only a copy at the very start', () => {
  const side = { quote: 'beta gamma', prefix: '', suffix: ' qqqqqq' };
  assert.equal(M.find('beta gamma one. alpha beta gamma.', side).how, 'exact');
  assert.equal(M.find('a beta gamma one. alpha beta gamma.', side).how, 'guess');
});

test('a kept note re-anchored from its found place survives a second edit beside it', () => {
  const lead = 'The first paragraph is here and it is fairly long indeed.\n\n';
  const trail = '\n\nThe closing paragraph follows and it is also fairly long.';
  const passage = 'The quick brown fox jumps over the lazy dog near the river.';
  const v0 = lead + passage + trail;
  const anchor = { quote: passage, ...M.around(v0, passage) };
  const edited = passage.replace('quick brown', 'slow grey');
  const v1 = lead + edited + trail;
  const first = M.find(v1, anchor);
  assert.equal(first.how, 'context');
  assert.equal(v1.slice(first.from, first.to), edited);
  const found = v1.slice(first.from, first.to);
  const v2 = v1.slice(0, first.from - 24) + 'X'.repeat(24) + found + 'Y'.repeat(24) + v1.slice(first.to + 24);
  const resaved = { quote: found, ...M.contextAt(v1, first.from, first.to) };
  const again = M.find(v2, resaved);
  assert.equal(again.how, 'exact');
  assert.equal(v2.slice(again.from, again.to), found);
});

test('contextAt takes the context of the copy it is given', () => {
  const text = 'one before. beta gamma. two between. beta gamma. three after.';
  const second = text.lastIndexOf('beta gamma');
  const got = M.contextAt(text, second, second + 'beta gamma'.length);
  assert.ok(got.prefix.endsWith('two between. '));
  assert.ok(got.suffix.startsWith('. three after.'));
  assert.notDeepEqual(got, M.around(text, 'beta gamma'));
});

for (const [name, fn] of cases) {
  try {
    fn();
  } catch (e) {
    console.error(`FAILED: ${name}`);
    throw e;
  }
}
console.log(`ok ${cases.length}`);
