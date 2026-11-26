const assert = require('node:assert/strict');
const ed = require('../src/web/editing.js');
const md = require('../src/web/markdown.js');

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

test('a slash command becomes Markdown in place of what was typed', () => {
  const mark = (text) => ed.SLASH.find((c) => c.text === text).mark;
  assert.deepEqual(ed.slash('/check', [], 0, 6, mark('Checkbox')), { lines: ['- [ ] '], line: 0, caret: 6 });
  assert.deepEqual(ed.slash('- milk /check', [], 7, 13, mark('Checkbox')), { lines: ['- [ ] milk '], line: 0, caret: 11 });
  assert.deepEqual(ed.slash('  - [ ] eggs/', [], 12, 13, mark('Bullet')), { lines: ['  - eggs'], line: 0, caret: 8 });
  assert.deepEqual(ed.slash('## Old /h', [], 7, 9, mark('Heading')), { lines: ['# Old '], line: 0, caret: 6 });
  assert.deepEqual(ed.slash('> [!note] Title /w', [], 16, 18, mark('Warning box')), { lines: ['> [!warning] Title '], line: 0, caret: 19 });
  assert.deepEqual(ed.slash('/code', [], 0, 5, mark('Code block')), { lines: ['```', '', '```'], line: 1, caret: 0 });
  assert.deepEqual(ed.slash('/div', ['Text above'], 0, 4, mark('Divider')), { lines: ['', '---', ''], line: 2, caret: 0 });
  assert.deepEqual(ed.slash('/div', [''], 0, 4, mark('Divider')), { lines: ['---', ''], line: 1, caret: 0 });
  assert.deepEqual(ed.slash('Milk /star', [], 5, 10, ':star:'), { lines: ['Milk :star: '], line: 0, caret: 12 });
  assert.deepEqual(ed.slash('a /st b', [], 2, 5, ':star:'), { lines: ['a :star: b'], line: 0, caret: 8 });
  assert.equal(md.render('std::fire::new and Note:info:x'), '<p>std::fire::new and Note:info:x</p>');
  assert.match(md.render('Done :check: and `:check:` :nope:'), /^<p>Done <svg class="note-icon" data-tone="green"[^]*<code>:check:<\/code> :nope:<\/p>/);
});

test('the outline finds headings at every level but excludes code, maths, quotes and tables', () => {
  const lines = ['# Intro', '## **Sounds**', '```md', '# Not a heading', '```', '> ## Quoted', '$$', '# Formula', '$$', '| Name |', '|---|', '# Table row |', '### Phrase [structure](https://example.com)', '###### Deep 😀', '## Again', '## Again', '####### Too deep', 'no # heading'];
  const found = ed.headings(lines);
  assert.deepEqual(found.map(({ block, ...row }) => row), [
    { line: 0, level: 1, text: 'Intro' }, { line: 1, level: 2, text: '**Sounds**' },
    { line: 12, level: 3, text: 'Phrase [structure](https://example.com)' },
    { line: 13, level: 6, text: 'Deep 😀' }, { line: 14, level: 2, text: 'Again' }, { line: 15, level: 2, text: 'Again' },
  ]);
  assert.deepEqual(ed.headings(['##', '###    ', '  ## Indented ###']).map(({ block, ...row }) => row), [{ line: 2, level: 2, text: 'Indented' }]);
  assert.equal(ed.headings(Array(3000).fill('# Section')).length, 3000);
});

test('outline groups follow heading levels, including skipped levels and repeated titles', () => {
  const rows = ed.headings(['# First', '### Nested', '##### Deep', '## Sibling', '# Next', '## Nested', '# Last']);
  const tree = ed.headingTree(rows);
  assert.deepEqual(tree.map(row => row.text), ['First', 'Next', 'Last']);
  assert.deepEqual(tree[0].children.map(row => row.text), ['Nested', 'Sibling']);
  assert.equal(tree[0].children[0].children[0].text, 'Deep');
  assert.equal(tree[1].children[0].line, 5);
  assert.deepEqual(tree[2].children, []);
  assert.deepEqual(ed.headingTree([]), []);
});

test('the heading tree keeps every row and each heading row names its block', () => {
  const lines = [];
  for (let i = 0; i < 3000; i++) lines.push(i % 3 === 0 ? `# Part ${i}` : `## Part ${i}`, 'text');
  const rows = ed.headings(lines);
  assert.equal(rows.length, 3000);
  const tree = ed.headingTree(rows);
  assert.equal(tree.length, 1000);
  assert.equal(tree.reduce((count, node) => count + 1 + node.children.length, 0), 3000);
  assert.deepEqual(tree[999].children.map((row) => row.text), ['Part 2998', 'Part 2999']);
  const blocks = ed.splitBlocks(lines);
  for (const row of rows) assert.equal(blocks[row.block].start, row.line);
  assert.deepEqual(ed.headings(['text', '```', '# no', '```', '# yes']).map((row) => row.block), [2]);
});

test('resizing counts only the pictures the page draws, so outside images and code never take the size', () => {
  const line = '`![code](a.png)` ![Logo](https://example.com/logo.png) ![[https://x.org/y.png]] ![Mine](attachments/me.png) ![Sketch](data:image/svg+xml,%3Csvg%2F%3E)';
  const drawn = md.render(line).match(/class="note-img/g).length;
  assert.equal(drawn, 2);
  assert.equal(ed.setPictureWidth(line, 0, 300), line.replace('![Mine](', '![Mine|300]('));
  assert.equal(ed.setPictureWidth(line, 1, 200), line.replace('![Sketch](', '![Sketch|200]('));
  assert.equal(ed.setPictureWidth(line, 2, 200), line, 'there is no third drawn picture');
});

test('a line names where each drawn picture sits, the same ones resizing and removing count', () => {
  const line = '`![code](a.png)` ![Logo](https://example.com/logo.png) Before ![Tree|400](attachments/tree.png) and ![[graph.png|250]] after';
  const spans = ed.pictureSpans(line);
  assert.deepEqual(spans, [{ at: line.indexOf('![Tree'), length: '![Tree|400](attachments/tree.png)'.length }, { at: line.indexOf('![[graph'), length: '![[graph.png|250]]'.length }]);
  spans.forEach(({ at }, i) => assert.equal(ed.removePicture(line, i).at, at));
  assert.deepEqual(ed.pictureSpans('no pictures'), []);
});

test('removing a picture takes out only the chosen drawn one, never code or outside images', () => {
  const line = '`![code](a.png)` ![Logo](https://example.com/logo.png) Before ![Tree|400](attachments/tree.png) and ![[graph.png|250]] after';
  assert.deepEqual(ed.removePicture(line, 0), { text: line.replace('![Tree|400](attachments/tree.png)', ''), at: line.indexOf('![Tree') });
  assert.deepEqual(ed.removePicture(line, 1), { text: line.replace('![[graph.png|250]]', ''), at: line.indexOf('![[graph') });
  assert.deepEqual(ed.removePicture(line, 2), { text: line, at: -1 }, 'there is no third drawn picture');
  assert.deepEqual(ed.removePicture('![Box](a.png "Title")', 0), { text: '', at: 0 });
  assert.deepEqual(ed.removePicture('x ![a](u)![a](u)', 1), { text: 'x ![a](u)', at: 9 }, 'the place is the removed picture’s, not where the text first differs');
});

test('a picture keeps a width in the Obsidian style and only the chosen one changes', () => {
  const line = 'Before ![Tree](attachments/tree.png) and ![[graph.png]] and ![Old|120](a.png "Title")';
  assert.equal(ed.setPictureWidth(line, 0, 400), 'Before ![Tree|400](attachments/tree.png) and ![[graph.png]] and ![Old|120](a.png "Title")');
  assert.equal(ed.setPictureWidth(line, 1, 250.4), 'Before ![Tree](attachments/tree.png) and ![[graph.png|250]] and ![Old|120](a.png "Title")');
  assert.equal(ed.setPictureWidth(line, 2, 300), 'Before ![Tree](attachments/tree.png) and ![[graph.png]] and ![Old|300](a.png "Title")');
  assert.equal(ed.setPictureWidth(line, 2, 0), 'Before ![Tree](attachments/tree.png) and ![[graph.png]] and ![Old](a.png "Title")', 'no width resets it');
  assert.equal(ed.setPictureWidth(line, 5, 300), line, 'a missing picture changes nothing');
  assert.equal(ed.setPictureWidth('![[a.png|Caption]]', 0, 200), '![[a.png|200]]');
  const shown = md.render('![Tree|400](attachments/tree.png)\n\n![[graph.png|250]]\n\n![Plain](x.png)');
  assert.match(shown, /alt="Tree" loading="lazy" width="400"/);
  assert.match(shown, /alt="graph.png" loading="lazy" width="250"/);
  assert.match(shown, /alt="Plain" loading="lazy">/);
  assert.doesNotMatch(md.render('![Huge|99999](x.png)'), /width=/);
});

test('a selection becomes its exact Markdown and 1-based lines, either direction', () => {
  const lines = ['# Heaps', '', 'A **min** heap', 'keeps 😀 the root', '$$x^2$$'];
  assert.deepEqual(ed.snapshotOf(lines, 9, 23), { from: 3, to: 3, text: 'A **min** heap' });
  assert.deepEqual(ed.snapshotOf(lines, 23, 9), { from: 3, to: 3, text: 'A **min** heap' });
  const whole = lines.join('\n');
  assert.deepEqual(ed.snapshotOf(lines, whole.indexOf('keeps'), whole.length), { from: 4, to: 5, text: 'keeps 😀 the root\n$$x^2$$' });
  assert.equal(ed.snapshotOf(lines, 0, 9).to, 2, 'ending at the start of a line does not take that line');
  assert.equal(ed.snapshotOf(lines, 8, 9), null, 'blank selections ask nothing');
  assert.equal(ed.snapshotOf(['x'.repeat(9000)], 0, 9000).text.length, ed.MOST_PASSAGE);
});

test('every ordinary line is its own block, blank lines included', () => {
  const lines = ['# Title', '', 'para one', 'para two', '- item'];
  assert.deepEqual(
    ed.splitBlocks(lines).map((b) => [b.start, b.end, b.kind]),
    [[0, 1, 'line'], [1, 2, 'line'], [2, 3, 'line'], [3, 4, 'line'], [4, 5, 'line']]
  );
});

test('code fences, tables and quotes stay whole', () => {
  const lines = ['```', 'a', '- [ ] b', '```', '| x | y |', '|---|---|', '| 1 | 2 |', '> one', '> two', 'after'];
  assert.deepEqual(
    ed.splitBlocks(lines).map((b) => [b.start, b.end, b.kind]),
    [[0, 4, 'code'], [4, 7, 'table'], [7, 9, 'quote'], [9, 10, 'line']]
  );
});

test('only the math the page itself draws as a block opens a math block', () => {
  const kinds = (lines) => ed.splitBlocks(lines).map((b) => b.kind);
  assert.deepEqual(kinds(['\\begin{document} is how LaTeX files start', '# Heading', 'text']), ['line', 'line', 'line']);
  assert.deepEqual(kinds(['\\[ not a block', '# Heading']), ['line', 'line']);
  assert.deepEqual(kinds(['\\begin{align}', 'x', '\\end{align}', 'after']), ['math', 'line']);
  assert.deepEqual(kinds(['\\[', 'x', '\\]', 'after']), ['math', 'line']);
  assert.deepEqual(kinds(['$$a=b$$.', 'after']), ['line', 'line']);
  assert.deepEqual(kinds(['$$', 'a=b', '$$.', 'after']), ['math', 'line']);
});

test('an unclosed fence runs to the end', () => {
  assert.deepEqual(ed.splitBlocks(['```', 'x']).map((b) => [b.start, b.end, b.kind]), [[0, 2, 'code']]);
});

test('star, plus, numbered and empty boxes count and toggle like leo', () => {
  const lines = ['* [ ] star', '+ [x] plus', '1. [ ] one', '2) [X] two', '- [ ]', '-[ ] not', '- [y] not'];
  assert.deepEqual(lines.map(ed.isBox), [true, true, true, true, true, false, false]);
  assert.equal(ed.toggleBox('* [ ] star'), '* [x] star');
  assert.equal(ed.toggleBox('3. [x] three'), '3. [ ] three');
  assert.equal(ed.toggleBox('- [ ]'), '- [x]');
  assert.deepEqual(['-   [ ] wide', '*  [x] wide', '1.    [ ] wide', '-     [ ] code'].map(ed.isBox), [true, true, true, false]);
  assert.equal(ed.toggleBox('-   [ ] wide'), '-   [x] wide');
  assert.equal(ed.toggleBox('  *    [x] wide'), '  *    [ ] wide');
  const html = md.render(lines.slice(0, 5).join('\n'));
  assert.equal((html.match(/data-box="/g) || []).length, 5, html);
  assert.ok(!html.includes('disabled'), html);
});

test('an empty note is one empty line', () => {
  assert.deepEqual(ed.splitBlocks(['']).map((b) => [b.start, b.end]), [[0, 1]]);
});

test('rendering a line block numbers its box globally', () => {
  const html = md.render('- [x] second', { boxOffset: 1 });
  assert.ok(html.includes('data-box="2" checked'), html);
});

test('a numbered item keeps its number when rendered alone', () => {
  assert.ok(md.render('3. third').includes('<ol start="3">'), md.render('3. third'));
});

test('list prefixes keep every rendered marker out of the text field', () => {
  for (const prefix of ['- ', '* ', '+ ', '12. ', '3) ', '- [ ] ', '* [x] ', '2. [X] ', '  - ', '    - [ ] ', '-   [ ] ']) {
    assert.equal(ed.listPrefix(prefix + '文字 😀'), prefix);
  }
  assert.equal(ed.listPrefix('- [ ]'), '- [ ]');
  for (const plain of ['Plain text', '# Heading', '> Quote', '![alt](a.png)', '-word', '1.word', 'A. Smith', 'ok. fine', 'Hi. there']) assert.equal(ed.listPrefix(plain), '');
});

test('Enter continues every kind of list with the next marker', () => {
  const next = (line) => ed.continueLine(line).prefix;
  assert.deepEqual(ed.continueLine('- milk'), { prefix: '- ', exit: false });
  assert.equal(next('  * nested'), '  * ');
  assert.equal(next('+ plus'), '+ ');
  assert.equal(next('- [x] done'), '- [ ] ');
  assert.equal(next('2. [x] done'), '3. [ ] ');
  assert.equal(next('3) [ ] pending'), '4) [ ] ');
  assert.equal(next('9. nine'), '10. ');
  assert.equal(next('a. one'), 'b. ');
  assert.equal(next('b) two'), 'c) ');
  assert.equal(next('C.  three'), 'D.  ');
  assert.equal(next('  a) nested'), '  b) ');
  assert.equal(next('z) last'), 'aa) ');
  assert.equal(next('aa) doubled'), 'bb) ');
  assert.equal(next('Z.  last'), 'AA.  ');
  assert.equal(next('Z) last'), 'AA) ');
  assert.equal(next('a. [ ] box'), 'b. [ ] ');
  assert.equal(next('> quoted'), '> ');
  assert.deepEqual(ed.continueLine('plain'), { prefix: '', exit: false });
});

test('words that only look like markers stay prose', () => {
  for (const line of ['Hello. world', 'ok. sure', 'hi) there', 'a)b', 'e.g. this', '-word', '1.word', 'a dog']) {
    assert.deepEqual(ed.continueLine(line), { prefix: '', exit: false }, line);
    assert.equal(ed.itemHead(line), '', line);
  }
});

test('Enter on an empty item steps out one level, then leaves the list', () => {
  const back = (line) => ed.continueLine(line).back;
  for (const line of ['- ', '2. [ ] ', '- [ ] ', 'a) ', '* ', '+ ', '-   [ ] ']) {
    assert.equal(ed.continueLine(line).exit, true, line);
    assert.equal(back(line), '', line);
  }
  assert.equal(back('  - '), '- ');
  assert.equal(back('    - [ ] '), '  - [ ] ');
  assert.equal(back('  2. '), '2. ');
  assert.equal(back('\ta) '), 'a) ');
});

test('Backspace at the start of an item outdents, then keeps only the text', () => {
  assert.equal(ed.stepBack('    - deep'), '  - deep');
  assert.equal(ed.stepBack('  - sub'), '- sub');
  assert.equal(ed.stepBack('- sub'), 'sub');
  assert.equal(ed.stepBack('- [x] done 😀'), 'done 😀');
  assert.equal(ed.stepBack('12. twelve'), 'twelve');
  assert.equal(ed.stepBack('c) three'), 'three');
  assert.equal(ed.stepBack('- '), '');
});

test('numbers and letters follow their place after a change', () => {
  const after = (lines, at) => {
    const out = [...lines];
    ed.renumber(out, at);
    return out;
  };
  const nested = (lines, at) => {
    const out = [...lines];
    ed.renumber(out, at, at);
    return out;
  };
  assert.deepEqual(after(['1. a', '2. ', '2. b', '3. c'], 1), ['1. a', '2. ', '3. b', '4. c']);
  assert.deepEqual(after(['a) a', 'b) ', 'b) b'], 1), ['a) a', 'b) ', 'c) b']);
  assert.deepEqual(nested(['1. a', '  2. b', '3. c'], 1), ['1. a', '  1. b', '2. c'], 'a new sub-list starts over and the outer one closes up');
  assert.deepEqual(nested(['1. a', '2. b', '  2. c'], 2), ['1. a', '2. b', '  1. c']);
  assert.deepEqual(nested(['1. a', '  c) b', '  a) c'], 1), ['1. a', '  a) b', '  b) c']);
  assert.deepEqual(after(['5. a', '6. b', '  - x', '6. c'], 1), ['5. a', '6. b', '  - x', '7. c'], 'the first number stays and sub-items are skipped');
  assert.deepEqual(after(['9. [ ] a', '9. [x] b'], 1), ['9. [ ] a', '10. [x] b']);
  assert.deepEqual(after(['- a', '1. b', 'plain', '1. c'], 1), ['- a', '1. b', 'plain', '1. c'], 'a plain line ends the list');
  assert.deepEqual(after(['```', '3. in code', '```', '1. a', '1. b'], 3), ['```', '3. in code', '```', '1. a', '2. b']);
  assert.deepEqual(after(['- a', 'plain'], 1), ['- a', 'plain'], 'a plain line changes nothing');
});

test('a change leaves numbers and letters it did not cause alone', () => {
  const after = (lines, at, fresh) => {
    const out = [...lines];
    ed.renumber(out, at, fresh);
    return out;
  };
  const stays = (lines, at) => assert.deepEqual(after(lines, at), lines);
  stays(['1999. born', '2005. moved', '2007. left', 'after'], 0);
  stays(['1. a', '2. b', '2005. moved', '2007. left'], 1);
  stays(['b. 1900', 'c. 1905', 'c. 1906'], 1);
  stays(['p. 14', 'p. 15', 'r. 16'], 0);
  stays(['i. one', 'ii. two'], 0);
  stays(['1. a', '   7. sub', '   8. more', '2. b'], 3);
  stays(['1. a', '   c) sub', '   e) more'], 0);
  stays(['- a', '* b', '+ c'], 0);
  assert.deepEqual(after(['a. one', 'b. two', 'ii. three', 'd. four'], 0), ['a. one', 'b. two', 'ii. three', 'd. four'], 'a run stops at the first marker far from its place');
  assert.deepEqual(after(['1. a', '   1999. x', '   2005. y', '2. b', '2. c'], 3), ['1. a', '   1999. x', '   2005. y', '2. b', '3. c'], 'a stop holds only on its own level');
  assert.deepEqual(after(['1. a', '1. b', '1. c'], 0), ['1. a', '2. b', '3. c']);
  stays(['2019. a', '2020. b', '2020. c', '2021. d'], 0);
  stays(['2019. a', '2020. b', '2020. c', '2021. d'], 3);
  stays(['a. x', 'c. 1900'], 0);
  assert.deepEqual(after(['2019. a', '2020. b', '2020. c', '2021. d'], 1), ['2019. a', '2020. b', '2021. c', '2022. d'], 'an edit beside a repeat does close it up');
  assert.deepEqual(after(['1. a', '3. b', '4. c'], 1), ['1. a', '2. b', '3. c'], 'a deleted item closes up');
  assert.deepEqual(after(['5. a', '9. b', '9. c'], 0), ['5. a', '9. b', '9. c'], 'items before the edit and far from their place stay');
  assert.deepEqual(after(['a) a', 'b) b', 'b) c', 'c) d'], 1), ['a) a', 'b) b', 'c) c', 'd) d']);
  assert.deepEqual(after(['z) a', 'z) b'], 0), ['z) a', 'z) b']);
  assert.deepEqual(after(['a. x', 'b. y'].concat(Array.from({ length: 24 }, (_, i) => `${String.fromCharCode(99 + i)}. k`), ['z. m']), 25)[26], 'aa. m', 'letters run on past z');
  assert.deepEqual(after(['1. a', '   7. g'], 1, 1), ['1. a', '   1. g'], 'Tab starts the new sub-list over, wherever its number was');
  assert.deepEqual(after(['1. a', '   2005. g'], 1, 1), ['1. a', '   2005. g'], 'a year stays');
  assert.deepEqual(after(['1. a', '   1. x', '5. e'].map((l, i) => (i === 2 ? '   5. e' : l)), 2, 2), ['1. a', '   1. x', '   2. e']);
});

test('a line leaving a list closes the numbers around it', () => {
  const left = (lines, at) => {
    const out = [...lines];
    ed.closeList(out, at);
    return out;
  };
  assert.deepEqual(left(['1. a', 'b', '3. c', '4. d'], 1), ['1. a', 'b', '2. c', '3. d']);
  assert.deepEqual(left(['1. a', '', '3. c'], 1), ['1. a', '', '2. c']);
  assert.deepEqual(left(['b', '3. c'], 0), ['b', '3. c'], 'nothing above, so the number stays');
  assert.deepEqual(left(['1. a', '  1. b', '  3. c'], 1), ['1. a', '  1. b', '  2. c'], 'an item is renumbered in place');
  assert.deepEqual(left(['1. a', 'b'], 1), ['1. a', 'b']);
});

test('a list item knows its level', () => {
  assert.deepEqual(ed.listLevels(['- a', '  - b', '    1. c', '  a) d', '\t- e', 'plain', '  plain']), [0, 1, 2, 1, 2, -1, -1]);
  assert.deepEqual(ed.listLevels(['1. a', '   - b', '     1) c', '   - d', '10. e', '    - f']), [0, 1, 2, 1, 0, 1], 'markers of any width nest by their depth');
  assert.equal(ed.listLevels(['text', '  - lone'])[1], 1, 'an old indent with nothing above keeps its level');
  assert.deepEqual(ed.listLevels(['p. 12', 'q. 13', 'a) one', 'b) two', 'text', 'b) prose']), [-1, -1, 0, 0, -1, -1]);
});

test('list levels take one pass over a long note', () => {
  const lines = Array.from({ length: 2000 }, (_, i) => `${i + 1}. item`);
  let reads = 0;
  const watched = new Proxy(lines, { get: (target, key, receiver) => { if (typeof key === 'string' && /^\d+$/.test(key)) reads++; return Reflect.get(target, key, receiver); } });
  ed.listLevels(watched);
  assert.ok(reads <= lines.length * 2, `a draw would wait on ${reads} reads`);
  assert.deepEqual([...new Set(ed.listLevels(lines))], [0]);
});

test('a line that only looks like a lettered marker is edited as prose', () => {
  for (const line of ['p. 14 says so', 'c. 1900', 'e. coli', 'pp. 12-13', 'ii. second']) {
    assert.deepEqual(ed.continueLine(line, []), { prefix: '', exit: false }, line);
    assert.equal(ed.itemHead(line, ['plain']), '', line);
    assert.equal(ed.listPrefix(line, []), '', line);
    assert.equal(ed.stepBack(line, []), line, line);
    assert.equal(ed.rawOffset(line, line.slice(0, 2), false), 2, line);
  }
  assert.equal(ed.itemHead('b) two', ['a) one']), 'b) ', 'it follows an item');
  assert.equal(ed.continueLine('b) two', ['a) one']).prefix, 'c) ');
  assert.equal(ed.itemHead('q. 13', ['p. 12']), '', 'a look-alike above is no item');
  assert.equal(ed.itemHead('c) [ ] task', []), 'c) [ ] ', 'a box always makes an item');
  assert.equal(ed.listed(['p. 1', 'q. 2'], 1), false);
  assert.equal(ed.listed(['a. 1', 'b. 2'], 1), true);
});

test('ticking a box flips only that box', () => {
  assert.equal(ed.toggleBox('- [ ] buy milk'), '- [x] buy milk');
  assert.equal(ed.toggleBox('  - [x] done'), '  - [ ] done');
  assert.equal(ed.toggleBox('  - [X] done'), '  - [ ] done');
  assert.equal(ed.toggleBox('not a box [ ]'), 'not a box [ ]');
});

test('Tab indents a list item and Shift-Tab brings it back', () => {
  assert.equal(ed.indent('- item'), '  - item');
  assert.equal(ed.outdent('  - item'), '- item');
  assert.equal(ed.outdent('- item'), '- item');
  assert.equal(ed.outdent(' - item'), '- item');
  assert.equal(ed.outdent('\t- item'), '- item');
  assert.equal(ed.outdent('   - item'), ' - item');
  for (const line of ['1. a', '2005. second', 'a) b']) assert.equal(ed.outdent(ed.indent(line)), line, 'a lone item comes back to its own bytes');
});

test('Tab nests under the marker above and Shift-Tab returns to the parent', () => {
  assert.equal(ed.indent('- b', ['- a']), '  - b');
  assert.equal(ed.indent('2. b', ['1. a']), '   2. b');
  assert.equal(ed.indent('b) b', ['a) a']), '   b) b');
  assert.equal(ed.indent('11. b', ['10. a']), '    11. b');
  assert.equal(ed.indent('B.  b', ['A.  a']), '    B.  b');
  assert.equal(ed.indent('   - b', ['1. a', '   - sibling']), '     - b', 'under the sibling at its own depth');
  assert.equal(ed.indent('  - old', ['1. a']), '    - old', 'an old two-space child moves a full level');
  assert.equal(ed.indent('plain'), '  plain');
  assert.equal(ed.outdent('   - b', ['1. a']), '- b');
  assert.equal(ed.outdent('      x) c', ['1. a', '   - b']), '   x) c');
  assert.equal(ed.stepBack('   - ', ['1. a']), '- ');
  assert.equal(ed.continueLine('   - ', ['1. a']).back, '- ');
});

test('a heading marker comes off in one Backspace and an empty heading ends on Enter', () => {
  assert.equal(ed.itemHead('## Title'), '## ');
  assert.equal(ed.stepBack('## Title'), 'Title');
  assert.equal(ed.stepBack('# '), '');
  assert.deepEqual(ed.continueLine('# '), { prefix: '', exit: true, back: '' });
  assert.deepEqual(ed.continueLine('# Title'), { prefix: '', exit: false });
  assert.equal(ed.itemHead('#tag'), '');
  assert.equal(ed.itemHead('####### seven'), '');
});

test('lettered tasks tick like the others', () => {
  assert.equal(ed.toggleBox('a) [ ] one'), 'a) [x] one');
  assert.equal(ed.toggleBox('  bb. [x] two'), '  bb. [ ] two');
  assert.deepEqual(['a) [ ] one', 'A.  [ ] two', 'A. [ ] prose', ''].map(ed.isBox), [true, true, false, false]);
});

test('a style wraps a selection and the same key takes it off again', () => {
  const on = (text, picked, style) => {
    const from = text.indexOf(picked);
    return ed.restyle(text, from, from + picked.length, style);
  };
  const back = (r, style) => ed.restyle(r.text, r.from, r.to, style).text;
  for (const [text, picked, style, styled] of [
    ['a word here', 'word', 'bold', 'a **word** here'],
    ['a word here', 'word', 'italic', 'a *word* here'],
    ['a word here', 'word', 'underline', 'a <u>word</u> here'],
    ['**word**', 'word', 'italic', '***word***'],
    ['*word*', 'word', 'bold', '***word***'],
    ['**word**', 'word', 'underline', '**<u>word</u>**'],
    ['- item', 'item', 'bold', '- **item**'],
    ['a) item', 'item', 'underline', 'a) <u>item</u>'],
  ]) {
    const made = on(text, picked, style);
    assert.equal(made.text, styled);
    assert.equal(made.text.slice(made.from, made.to), picked);
    assert.equal(back(made, style), text, styled);
  }
  assert.equal(on('***word***', 'word', 'bold').text, '*word*');
  assert.equal(on('***word***', 'word', 'italic').text, '**word**');
  assert.equal(on('_word_', 'word', 'italic').text, 'word');
  assert.equal(on('__word__', 'word', 'bold').text, 'word');
  assert.equal(on('**word**', '**word**', 'bold').text, 'word', 'a selection that took the marks in');
  assert.equal(on('<u>word</u>', '<u>word</u>', 'underline').text, 'word');
  assert.equal(on('<u>**word**</u>', 'word', 'underline').text, '**word**');
  assert.equal(on('**<u>word</u>**', '**<u>word</u>**', 'underline').text, '**word**');
  assert.equal(on('<u>word</u>', 'word</u>', 'underline').text, 'word', 'the page can hand back only one side of the marks');
  assert.equal(on('**word**', 'word**', 'bold').text, 'word');
  assert.equal(on('snake_case', 'snake_case', 'bold').text, '**snake_case**');
});

test('a style across lines skips markers and blank lines', () => {
  const text = '- one\n\n1. two\n# three';
  const made = ed.restyle(text, 0, text.length, 'bold');
  assert.equal(made.text, '- **one**\n\n1. **two**\n# **three**');
  assert.equal(ed.restyle(made.text, made.from, made.to, 'bold').text, text);
  assert.equal(ed.restyle('- one', 0, 2, 'bold').text, '- one', 'only the marker was picked');
  const mixed = 'one\n---\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```\ncode\n```\ntwo';
  assert.equal(ed.restyle(mixed, 0, mixed.length, 'bold').text, mixed.replace('one', '**one**').replace('two', '**two**'), 'select-all leaves tables, code and rules alone');
  const inCell = '| a | word |\n|---|---|';
  assert.equal(ed.restyle(inCell, 6, 10, 'bold').text, '| a | **word** |\n|---|---|', 'one cell of a table still can');
  const under = ed.restyle('- one\n- two', 0, 11, 'underline');
  assert.equal(under.text, '- <u>one</u>\n- <u>two</u>');
  assert.equal(ed.restyle(under.text, 5, under.text.length, 'underline').text, '- one\n- two', 'starting inside the first tag');
});

test('a style stays on the lines the selection reaches', () => {
  const text = 'hello world\nsecond line here\nthird\n- fourth line\n';
  const made = ed.restyle(text, 0, 5, 'bold');
  assert.equal(made.text, '**hello** world\nsecond line here\nthird\n- fourth line\n');
  assert.equal(made.text.slice(made.from, made.to), 'hello');
  const middle = 'first line\nsecond line\nthird line\nfourth';
  assert.equal(ed.restyle(middle, 11, 17, 'italic').text, 'first line\n*second* line\nthird line\nfourth');
  const across = middle.indexOf('line\nsecond');
  const both = ed.restyle(middle, across, middle.indexOf(' line\nthird'), 'bold');
  assert.equal(both.text, 'first **line**\n**second** line\nthird line\nfourth');
  assert.equal(ed.restyle(both.text, both.from, both.to, 'bold').text, middle);
  assert.equal(ed.restyle('one\ntwo', 4, 7, 'underline').text, 'one\n<u>two</u>');
});

test('taking a style off removes only real marks', () => {
  const on = (text, picked, style) => {
    const from = text.indexOf(picked);
    return ed.restyle(text, from, from + picked.length, style);
  };
  assert.equal(on('foo_bar_baz', 'bar', 'italic').text, 'foo_*bar*_baz');
  assert.equal(on('foo_*bar*_baz', 'bar', 'italic').text, 'foo_bar_baz');
  assert.equal(on('2*3*4', '3', 'italic').text, '2*3*4');
  assert.equal(on('2*3*4', '3', 'bold').text, '2*3*4');
  assert.equal(on('snake__case__x', 'case', 'bold').text, 'snake__**case**__x');
  assert.equal(on('2**3**4', '3', 'bold').text, '234', 'two stars still pair after a word');
  assert.equal(on('a *b* c', 'b', 'italic').text, 'a b c');
  assert.equal(on('a _b_ c', 'b', 'italic').text, 'a b c');
});

test('a style key with no selection opens a pair, and the next press closes it', () => {
  const press = (marked, style) => {
    const at = marked.indexOf('|');
    const made = ed.stylePair(marked.replace('|', ''), at, style);
    return made.text.slice(0, made.at) + '|' + made.text.slice(made.at);
  };
  for (const [from, style, to] of [
    ['a |', 'bold', 'a **|**'],
    ['a |', 'italic', 'a *|*'],
    ['a |', 'underline', 'a <u>|</u>'],
    ['a **|**', 'bold', 'a |'],
    ['a *|*', 'italic', 'a |'],
    ['a <u>|</u>', 'underline', 'a |'],
    ['**|**', 'italic', '***|***'],
    ['***|***', 'italic', '**|**'],
    ['*|*', 'bold', '***|***'],
    ['***|***', 'bold', '*|*'],
    ['<u>|</u>', 'bold', '<u>**|**</u>'],
    ['<u>**|**</u>', 'underline', '**|**'],
    ['**|**', 'underline', '**<u>|</u>**'],
    ['**<u>|</u>**', 'underline', '**|**'],
    ['a **word|** b', 'bold', 'a **word**| b'],
    ['a *word|* b', 'italic', 'a *word*| b'],
    ['**a**|', 'bold', '**a**|****'.replace('|****', '**|**')],
    ['**a****|**', 'bold', '**a**|'],
    ['*a*|', 'italic', '*a**|*'],
    ['*a**|*', 'italic', '*a*|'],
    ['a <u>word|</u> b', 'underline', 'a <u>word</u>| b'],
    ['a **word|** b', 'italic', 'a **word*|*** b'],
    ['one\n|', 'bold', 'one\n**|**'],
    ['**x**\n|**', 'bold', '**x**\n**|****'],
  ]) assert.equal(press(from, style), to, `${from} ${style}`);
});

test('a style leaves a callout marker alone', () => {
  const text = '> [!warning] Title\n> body line';
  assert.equal(ed.restyle(text, 0, text.length, 'bold').text, '> [!warning] **Title**\n> **body line**', 'a whole callout line styles its title, not the marker');
  const from = text.indexOf('Title');
  assert.equal(ed.restyle(text, from, from + 5, 'bold').text, '> [!warning] **Title**\n> body line', 'the title alone can still be styled');
  assert.equal(ed.restyle('> [!tip]- Fold\n', 0, 14, 'italic').text, '> [!tip]- *Fold*\n');
  assert.equal(ed.restyle('> [!tip]-', 0, 9, 'italic').text, '> [!tip]-', 'a marker alone has nothing to style');
  assert.equal(ed.restyle('p. 12 here', 0, 10, 'bold').text, '**p. 12 here**', 'prose has no marker to skip');
});

test('Tab walks table cells and a new row stays a table', () => {
  const text = '| A | B |\n|---|:-:|\n| 1 |  |';
  const here = (at, back) => {
    const cell = ed.nextCell(text, at, back);
    return cell && text.slice(cell.from, cell.to);
  };
  assert.equal(here(2), 'B');
  assert.equal(here(6), '1', 'the rule row is skipped');
  assert.equal(here(22, true), 'B');
  assert.equal(ed.nextCell(text, text.length - 1), null);
  assert.equal(ed.cellEnd(text, 0, 1), 7);
  assert.equal(ed.cellEnd(text, 1, 0), 23);
  assert.equal(ed.cellEnd(text, 4, 4), 'end');
  const row = ed.addRow(text);
  assert.equal(row.text, `${text}\n|  |  |`);
  assert.equal(row.at, text.length + 3);
  assert.equal(ed.splitBlocks(row.text.split('\n')).length, 1);
  assert.equal(ed.nextCell('a | b \\| c | d\n--|--|--', 0).from, 4);
});

test('a formula is one place in the shown text, drawn or not', () => {
  const F = '￼';
  const line = String.raw`area $\int_0^1 x^2$ and the sum`;
  assert.equal(ed.rawOffset(line, 'area '), 5);
  assert.equal(ed.rawOffset(line, `area ${F}`), 19);
  assert.equal(ed.rawOffset(line, `area ${F} and`), 23);
  assert.equal(ed.rawOffset(line, 'area ' + F + ' and the sum'), line.length);
  assert.equal(ed.rawOffset(String.raw`$x_{i}$ then`, ''), 0);
  assert.equal(ed.rawOffset(String.raw`$x_{i}$ then`, F), 7);
  assert.equal(ed.rawOffset(String.raw`$a_1$ and $b^2$ then`, `${F} and ${F} th`), 18);
  assert.equal(ed.rawOffset(String.raw`cost \$5 and \$6 then $y$ end`, String.raw`cost \$5 and \$6 then ${F} e`), 27);
  assert.equal(ed.rawOffset('- item $E$ then', `item ${F} th`, true), 13);
  assert.equal(ed.rawOffset('## head $\\alpha_i$ then', `head ${F} th`, false), 21);
  assert.equal(ed.rawOffset('see [a $x_i$ b](https://x.y) after', `see a ${F} b af`), 31);
  assert.equal(ed.rawOffset('a `$x$` b $y$', 'a $x$ b '), 10);
  assert.equal(ed.rawOffset('a **$x$** b', `a ${F} b`), 11);
});

test('a tap lands on the same spot in the raw text', () => {
  assert.equal(ed.rawOffset('see [link](https://x.y)', 'see link'), 'see [link](https://x.y)'.length, 'the end of a link is past its address');
  assert.equal(ed.rawOffset('**bold**', 'bold'), 8);
  assert.equal(ed.rawOffset('<u>under</u> word', 'under w'), 14);
  assert.equal(ed.rawOffset('_it_ word', 'it w'), 6);
  assert.equal(ed.rawOffset('snake_case word', 'snake_c'), 7);
  assert.equal(ed.rawOffset('a) item', 'it'), 5);
  assert.equal(ed.rawOffset('**bold** word', 'bold w'), 10);
  assert.equal(ed.rawOffset('- [ ] buy milk', 'buy'), 9);
  assert.equal(ed.rawOffset('## Heading', 'Head'), 7);
  assert.equal(ed.rawOffset('plain text', ''), 0);
  assert.equal(ed.rawOffset('[link](https://x.y) after', 'link af'), 22);
  assert.equal(ed.rawOffset('short', 'much longer than the line'), 5);
  assert.equal(ed.rawOffset('😀 after', '😀'), 2);
  assert.equal(ed.rawOffset('## 😀 **bold** after', '😀 bold af'), '## 😀 **bold** af'.length);
  assert.equal(ed.rawOffset('[😀](https://x.y) after', '😀 af'), '[😀](https://x.y) af'.length);
});

test('a picture is one place for the caret, so both of its sides can be reached', () => {
  const shown = '\ufffc';
  const task = '- [ ] ![Box](a.png)';
  assert.equal(ed.rawOffset(task, ''), 6);
  assert.equal(ed.rawOffset(task, shown), task.length);
  assert.equal(ed.rawOffset('a ![x|400](u.png) b', 'a '), 2);
  assert.equal(ed.rawOffset('a ![x|400](u.png) b', `a ${shown}`), 17);
  assert.equal(ed.rawOffset('a ![x|400](u.png) b', `a ${shown} `), 18);
  assert.equal(ed.rawOffset('![[a.png|200]] t', ''), 0);
  assert.equal(ed.rawOffset('![[a.png|200]] t', shown), 14);
  assert.equal(ed.rawOffset('![d](data:image/svg+xml;utf8,%3Csvg%3E) z', `${shown} `), 40);
  assert.equal(ed.rawOffset('![x](https://e.com/a.png) t', 'x t'), 27, 'a picture leo does not draw stays its text');
  const outside = '![[https://x/a.png]] t';
  assert.equal(ed.rawOffset(outside, '![[https://x/a.png]] t'), outside.length, 'an outside embed is shown as its text');
  assert.equal(ed.rawOffset(outside, '!'), 1);
});

test('Enter after a picture in a checkbox item starts an empty checkbox below it', () => {
  const task = '- [ ] ![Box](a.png)';
  const at = ed.rawOffset(task, '\ufffc');
  const next = ed.continueLine(task.slice(0, at), []);
  assert.deepEqual([task.slice(0, at), next.prefix + task.slice(at)], [task, '- [ ] ']);
  assert.equal(next.exit, false);
});

let failed = 0;
test('a \\[ block and a \\begin{aligned} block are each one block to edit', () => {
  const lines = ['Intro', '\\[', 'x^2', '\\]', '\\begin{aligned}', 'a &= b', '\\end{aligned}', 'After'];
  assert.deepEqual(ed.splitBlocks(lines).map((b) => [b.start, b.end, b.kind]), [[0, 1, 'line'], [1, 4, 'math'], [4, 7, 'math'], [7, 8, 'line']]);
});

test('a $$ math block is one block to edit, and a one-line $$x$$ is a line', () => {
  const lines = ['intro', '$$', 'a^2 + b^2', '= c^2$$', 'after', '$$x$$'];
  assert.deepEqual(ed.splitBlocks(lines).map((b) => [b.start, b.end, b.kind]), [[0, 1, 'line'], [1, 4, 'math'], [4, 5, 'line'], [5, 6, 'line']]);
});

setImmediate(() => {
  for (const [name, fn] of cases) {
    try {
      fn();
      console.log(`ok   ${name}`);
    } catch (e) {
      failed++;
      console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`);
    }
  }
  console.log(`${cases.length - failed} passed, ${failed} failed`);
  process.exit(failed ? 1 : 0);
});

test('Tab and Shift+Tab leave prose and chosen numbers alone', () => {
  const press = (lines, at, back) => {
    const out = [...lines];
    const above = out.slice(0, at);
    const next = back ? ed.outdent(out[at], above) : ed.indent(out[at], above);
    if (next === out[at]) return out;
    out.splice(at, 1, next);
    ed.renumber(out, at, at);
    return out;
  };
  for (const [lines, at] of [[['p. 12', 'p. 14'], 1], [['i. one', 'ii. two'], 1], [['b. x', 'c. 1905'], 1], [['c. 1905'], 0]]) {
    assert.equal(press(lines, at, true)[at], lines[at], `${lines[at]} stays on Shift+Tab`);
    assert.match(press(lines, at, false)[at], new RegExp(`^\\s*${lines[at].replace(/[.]/g, '\\.')}$`), `${lines[at]} keeps its marker on Tab`);
  }
  assert.deepEqual(press(['1999. first', '2005. second'], 1, true), ['1999. first', '2005. second']);
  assert.deepEqual(press(['1. a', '2005. second'], 1, false), ['1. a', '   2005. second']);
  assert.deepEqual(press(['1. a', '   1. b'], 1, true), ['1. a', '2. b'], 'a sub-list item joins the outer numbering');
  assert.deepEqual(press(['1. a', '   2005. b'], 1, true), ['1. a', '2005. b'], 'a chosen number survives the move out');
  assert.deepEqual(press(['1. a', '2. b'], 1, false), ['1. a', '   1. b']);
});

test('the list marker above a line is the same when only the run of list lines is passed', () => {
  const docs = [
    ['- one', '  - two', '    - three', '  - four', ''],
    ['A paragraph', 'that runs on', '', '1. first', '2. second', '   - inner', ''],
    ['Plain text', 'more plain text', 'and more', ''],
    ['- [ ] a box', '- [x] done', 'not a list', '- again', '  1. nested'],
    [''],
  ];
  for (const lines of docs) {
    for (let at = 0; at < lines.length; at++) {
      for (const line of [lines[at], '- typed', '  - typed', '3. typed', 'typed']) {
        assert.equal(ed.listPrefixAt(line, lines, at), ed.listPrefix(line, lines.slice(0, at)), `${JSON.stringify(lines)} at ${at}: ${line}`);
      }
    }
  }
});
