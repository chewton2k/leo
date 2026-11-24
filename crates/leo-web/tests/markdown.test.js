const assert = require('node:assert/strict');
const md = require('../src/web/markdown.js');
const ed = require('../src/web/editing.js');
const { render } = md;

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

const boxes = (html) =>
  [...html.matchAll(/<input type="checkbox" data-box="(\d+)"( checked)?>/g)].map(
    (m) => [Number(m[1]), Boolean(m[2])]
  );

test('text is escaped, so a note cannot inject markup', () => {
  const html = render('<script>alert(1)</script> & "quotes"');
  assert.ok(!html.includes('<script>'), html);
  assert.ok(html.includes('&lt;script&gt;'), html);
  assert.ok(html.includes('&amp;'), html);
});

test('underline is that one tag pair and nothing else', () => {
  assert.equal(render('<u>under</u>'), '<p><u>under</u></p>');
  assert.equal(render('**<u>both</u>**'), '<p><strong><u>both</u></strong></p>');
  assert.equal(render('<u>**both**</u>'), '<p><u><strong>both</strong></u></p>');
  assert.ok(render('[<u>x</u>](https://a.b)').includes('><u>x</u></a>'));
  assert.ok(render('`<u>x</u>`').includes('<code>&lt;u&gt;x&lt;/u&gt;</code>'));
  assert.equal(md.plain('# <u>Title</u> and <u>more</u>'), 'Title and more');
  const hostile = [
    '<script>alert(1)</script>',
    '<u><script>alert(1)</script></u>',
    '</u><script>alert(1)</script><u>',
    '<u onclick=alert(1)>x</u>',
    '<u style="position:fixed">x</u>',
    '<u><img src=x onerror=alert(1)></u>',
    '<u><svg onload=alert(1)></u>',
    '<U>x</U>',
    '<u >x</u >',
    '<ul><li>x</li></ul>',
    '<b>x</b><i>y</i>',
    '<iframe src=//a.b></iframe>',
    '<a href="javascript:alert(1)">x</a>',
    '&lt;script&gt;',
    '<u>&lt;script&gt;alert(1)&lt;/script&gt;</u>',
    '<u><u>x</u></u>',
    '<<u>script>alert(1)<</u>/script>',
  ];
  for (const text of hostile) {
    for (const shaped of [text, `# ${text}`, `- ${text}`, `a) ${text}`, `> ${text}`, `| ${text} |\n|---|\n| ${text} |`, `**${text}**`, `[${text}](https://a.b)`]) {
      const html = render(shaped);
      const tags = [...html.matchAll(/<\/?([a-zA-Z0-9]+)[^>]*>/g)].map((m) => m[1]);
      for (const tag of tags) assert.ok(['p', 'h2', 'ul', 'ol', 'li', 'blockquote', 'div', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'strong', 'a', 'u'].includes(tag), `${tag} in ${html}`);
      assert.ok(!/<u\s[^>]/.test(html), html);
      assert.ok(!/<[^>]*\son\w+=/.test(html), html);
    }
  }
});

test('marks nest, and the ones that do not pair stay as written', () => {
  const line = (text) => render(text).replace(/^<p>|<\/p>$/g, '');
  assert.equal(line('***both***'), '<em><strong>both</strong></em>');
  assert.equal(line('___both___'), '<em><strong>both</strong></em>');
  assert.equal(line('**bold *italic***'), '<strong>bold <em>italic</em></strong>');
  assert.equal(line('*italic **bold***'), '<em>italic <strong>bold</strong></em>');
  assert.equal(line('***bold** italic*'), '<em><strong>bold</strong> italic</em>');
  assert.equal(line('***italic* bold**'), '<strong><em>italic</em> bold</strong>');
  assert.equal(line('__bold _italic___'), '<strong>bold <em>italic</em></strong>');
  assert.equal(line('~~**gone**~~'), '<del><strong>gone</strong></del>');
  assert.equal(line('**`code`**'), '<strong><code>code</code></strong>');
  assert.equal(line('**a** and **b**'), '<strong>a</strong> and <strong>b</strong>');
  for (const literal of ['snake_case_word', '2 * 3 * 4', '2*3*4', 'a ** b', 'a _ b', '**open', 'close**', '*open', 'x_ y_', '~~open']) {
    assert.equal(line(literal), literal);
  }
  assert.ok(!line('*a _b* c_').includes('</em> c</em>'), 'marks never cross');
});

test('headings start below the note title', () => {
  assert.ok(render('# Graphs').includes('<h2>Graphs</h2>'));
  assert.ok(render('## BFS').includes('<h3>BFS</h3>'));
  assert.ok(render('###### deep').includes('<h6>deep</h6>'));
});

test('bold, italics, strikethrough and inline code', () => {
  const html = render('**bold** *it* _also_ ~~gone~~ `x < y`');
  assert.ok(html.includes('<strong>bold</strong>'), html);
  assert.ok(html.includes('<em>it</em>'), html);
  assert.ok(html.includes('<em>also</em>'), html);
  assert.ok(html.includes('<del>gone</del>'), html);
  assert.ok(html.includes('<code>x &lt; y</code>'), html);
});

test('formatting inside inline code is left alone', () => {
  assert.ok(render('`**not bold**`').includes('<code>**not bold**</code>'));
});

test('snake_case words are not italic', () => {
  assert.ok(!render('call my_long_name here').includes('<em>'));
});

test('links open in a new tab without passing anything along', () => {
  const html = render('[docs](https://example.com/a?b=1)');
  assert.ok(
    html.includes('<a href="https://example.com/a?b=1" target="_blank" rel="noopener noreferrer">docs</a>'),
    html
  );
});

test('only web and mail links become links', () => {
  const html = render('[x](javascript:alert(1)) [y](data:text/html,hi)');
  assert.ok(!html.includes('<a '), html);
});

test('bare addresses become links', () => {
  const html = render('see https://github.com/chewton2k/leo-cli.');
  assert.ok(html.includes('href="https://github.com/chewton2k/leo-cli"'), html);
  assert.ok(html.includes('</a>.'), html);
});

test('fenced code keeps its text and says its language', () => {
  const html = render('```rust\nfn main() { let x = 1 < 2; }\n**no**\n```');
  assert.ok(html.includes('<pre><code class="language-rust">'), html);
  assert.ok(html.includes('1 &lt; 2'), html);
  assert.ok(html.includes('**no**'), html);
});

test('checkbox numbers match the ones leo ticks', () => {
  const html = render('- [ ] a\ntext\n  - [x] b\n- [X] c\n- [ ]\n');
  assert.deepEqual(boxes(html), [
    [1, false],
    [2, true],
    [3, true],
    [4, false],
  ]);
});

test('a checkbox with extra spaces after the dash is a checkbox too', () => {
  assert.deepEqual(boxes(render('-   [ ] wide\n*  [x] also\n- [ ] plain')), [[1, false], [2, true], [3, false]]);
});

test('a checkbox inside a code block still counts, but is not a checkbox', () => {
  const html = render('```\n- [ ] in code\n```\n- [ ] real');
  assert.deepEqual(boxes(html), [[2, false]]);
});

test('a checkbox in a quote does not count, as leo does not tick it', () => {
  const html = render('> - [ ] quoted\n\n- [ ] real');
  assert.deepEqual(boxes(html), [[1, false]]);
});

test('numbered and nested lists', () => {
  const html = render('1. one\n2. two\n   - inner\n3. three');
  assert.ok(html.includes('<ol>'), html);
  assert.ok(html.includes('<li>two<ul class="list-2"><li>inner</li></ul></li>'), html);
});

test('tables', () => {
  const html = render('| a | b |\n|---|:-:|\n| 1 | **2** |');
  assert.ok(html.includes('<table>'), html);
  assert.ok(html.includes('<th>a</th>'), html);
  assert.ok(html.includes('<td class="align-center"><strong>2</strong></td>'), html);
});

test('table columns keep their alignment, and a short row is filled in', () => {
  const html = render('| L | C | R | N |\n|:--|:-:|--:|---|\n| 1 | 2 |');
  assert.ok(html.includes('<th class="align-left">L</th><th class="align-center">C</th><th class="align-right">R</th><th>N</th>'), html);
  assert.ok(html.includes('<td class="align-left">1</td><td class="align-center">2</td><td class="align-right"></td><td></td>'), html);
});

test('an escaped pipe stays inside its cell', () => {
  const html = render('| a \\| b | c |\n|---|---|\n| d | e |');
  assert.ok(html.includes('<th>a | b</th><th>c</th>'), html);
});

test('quotes and rules', () => {
  const html = render('> said\n\n---\n\nafter');
  assert.ok(html.includes('<blockquote><p>said</p></blockquote>'), html);
  assert.ok(html.includes('<hr>'), html);
});

test('lines of a paragraph keep their breaks', () => {
  assert.ok(render('one\ntwo').includes('<p>one<br>two</p>'));
});

test('an answered @leo question reads as a question and answer', () => {
  const html = render('**Q:** what is BFS?\nIt explores level by level.');
  assert.ok(html.includes('<strong>Q:</strong> what is BFS?'), html);
});

test('a picture with a described alt text carries it as a caption, and a file name or empty alt carries none', () => {
  const caption = (alt) => md.render(`![${alt}](a.png)`).includes('class="note-fig"');
  assert.ok(caption('Heap diagram') && caption('A graph of the loss'));
  for (const alt of ['', 'Pasted picture', 'image', 'Drawing', 'IMG_4821', 'Screenshot 2026-11-01 at 10.23.45', 'board.png']) assert.ok(!caption(alt), alt);
  assert.ok(md.render('![Heap diagram|300](a.png)').includes('data-cap="Heap diagram"'));
});

test('pictures in a note are shown from leo, and outside ones stay links', () => {
  const html = md.render('![Heap diagram](attachments/heap.png)\n\n![](<attachments/a b.png>)\n\n![[board.png|300]]\n\n![x](https://example.com/x.png)', { dir: 'cs130' });
  assert.ok(html.includes('<img class="note-img" src="/api/image?path=attachments%2Fheap.png&amp;from=cs130" alt="Heap diagram" loading="lazy">'), html);
  assert.ok(html.includes('src="/api/image?path=attachments%2Fa%20b.png&amp;from=cs130" alt=""'), html);
  assert.ok(html.includes('src="/api/image?path=board.png&amp;from=cs130" alt="board.png"'), html);
  assert.ok(html.includes('<a href="https://example.com/x.png"'), html);
  assert.ok(!/<img[^>]*example\.com/.test(html), 'no picture is loaded from elsewhere');
  assert.ok(!md.render('![a](javascript:alert(1))').includes('<img'));
  assert.ok(!md.render('![a](data:image/png;base64,AAAA)').includes('<img'));
  assert.ok(md.render('![a"><script>](x.png)').includes('alt="a&quot;&gt;&lt;script&gt;"'));
  assert.equal(md.plain('Look ![Heap](attachments/h.png) and ![[b.png]] then [link](https://x.y)'), 'Look and then link');
  assert.equal(md.plain('Sizes:\n\n| a | b |\n|---|:--:|\n| 1 | 2 |'), 'Sizes: a b 1 2');
});


test('nested lists take their look from their depth, repeating after three', () => {
  const html = md.render(['- one', '  - two', '    - three', '      - four', '', '1. one', '  1. two', '    1. three', '      1. four'].join('\n'));
  assert.deepEqual(html.match(/<[uo]l[^>]*>/g), ['<ul>', '<ul class="list-2">', '<ul class="list-3">', '<ul class="list-1">', '<ol>', '<ol class="list-2">', '<ol class="list-3">', '<ol class="list-1">']);
  assert.ok(md.render('2. b', { depth: 2 }).startsWith('<ol class="list-1" start="2">'), 'a list that does not start at 1 keeps its numbers at any depth');
  assert.ok(md.render('1. a\n   7. g\n   8. h').includes('<ol class="list-1" start="7"><li>g</li><li>h</li></ol>'));
  assert.ok(md.render('1) a\n   7) g').includes('<ol class="list-paren" start="7">'));
  assert.ok(md.render('2. d', { depth: 3, first: 1 }).startsWith('<ol class="list-3" start="2">'), 'a line that continues a list from 1 keeps the depth look');
  assert.ok(md.render('8. h', { depth: 2, first: 7 }).startsWith('<ol class="list-1" start="8">'), 'a line in a list that began at 7 stays decimal');
  assert.ok(md.render('1. a\n   1. g').includes('<ol class="list-2"><li>g</li>'));
  assert.ok(md.render('- c', { depth: 3 }).startsWith('<ul class="list-3">'));
  assert.ok(md.render('# not a list', { depth: 2 }).startsWith('<h'));
});

test('a typed `1)` keeps its parenthesis at level one; nested lists follow the depth', () => {
  assert.ok(md.render('3) c').startsWith('<ol class="list-paren" start="3">'));
  assert.ok(md.render('1) a\n  1) b').startsWith('<ol class="list-paren"><li>a<ol class="list-2">'));
});

let failed = 0;
test('letters draw as typed at any depth', () => {
  assert.equal(render('a) one\nb) two'), '<ol class="list-lower-paren"><li value="1">one</li><li value="2">two</li></ol>');
  assert.equal(render('a. one\nb. two'), '<ol class="list-lower"><li value="1">one</li><li value="2">two</li></ol>');
  assert.equal(render('A) one\nB) two'), '<ol class="list-upper-paren"><li value="1">one</li><li value="2">two</li></ol>');
  assert.equal(render('A.  one\nB.  two'), '<ol class="list-upper"><li value="1">one</li><li value="2">two</li></ol>');
  assert.equal(render('c) alone', { after: true }), '<ol class="list-lower-paren"><li value="3">alone</li></ol>');
  assert.equal(render('z) one\naa) two\nbb) three', { after: true }), '<ol class="list-lower-paren"><li value="26">one</li><li value="27">two</li><li value="54">three</li></ol>', 'CSS counts aa as 27 and bb as 54');
  assert.ok(render('1. one\n   a) sub').includes('<ol class="list-lower-paren"><li value="1">sub</li></ol></li></ol>'));
  assert.ok(render('c) alone', { depth: 2, after: true }).startsWith('<ol class="list-lower-paren">'));
  assert.ok(render('a) [ ] task\nA.  [x] done').includes('data-box="2"'));
});

test('a letter in prose stays prose', () => {
  for (const text of ['We chose a) this and b) that', 'A. Smith wrote this', 'J. R. Tolkien', 'ok. fine', 'Hi. there', 'e.g. this', 'a)b', 'abc) no', 'aaa. no', 'ab) no', 'I. think']) assert.ok(render(text).startsWith('<p>'), text);
});

test('a lettered list begins at a or A, or continues an item', () => {
  for (const text of ['p. 12', 'c. 1900', 'e. coli', 'pp. 12-13', 'b) second', 'C) third', 'i. one', 'ii. two']) assert.ok(render(text).startsWith('<p>'), text);
  assert.equal(render('i. one\nii. two\niii. three\niv. four'), '<p>i. one<br>ii. two<br>iii. three<br>iv. four</p>');
  assert.equal(render('see\np. 12'), '<p>see<br>p. 12</p>');
  assert.equal(render('a. one\nb. two'), '<ol class="list-lower"><li value="1">one</li><li value="2">two</li></ol>');
  assert.equal(render('h. eight\ni. nine', { after: true }), '<ol class="list-lower"><li value="8">eight</li><li value="9">nine</li></ol>');
  assert.ok(render('1. x\n   c) sub').includes('<li value="3">sub</li>'), 'a sub-list continues the item above it');
  assert.ok(render('c) [ ] box').includes('data-box="1"'), 'the page and leo-core count the same boxes');
  assert.equal(render('c. 1900', { after: false }), '<p>c. 1900</p>');
  assert.ok(render('2005. year').startsWith('<ol start="2005">'), 'numbers stay Markdown');
  assert.equal(md.plain('p. 12 and a) one\n- two'), 'p. 12 and a) one two');
  assert.equal(md.plain('a) one'), 'one');
});

test('lists of different markers do not run together', () => {
  assert.equal(render('- a\n1. b\na) c'), '<ul><li>a</li></ul><ol><li>b</li></ol><ol class="list-lower-paren"><li value="1">c</li></ol>');
  assert.equal(render('1. a\n1) b'), '<ol><li>a</li></ol><ol class="list-paren"><li>b</li></ol>');
});

test('old two-space and new marker-width nesting draw the same', () => {
  const old = render('1. a\n  - b\n  - c');
  assert.equal(old, render('1. a\n   - b\n   - c'));
  assert.ok(old.includes('<ul class="list-2">'), old);
});

test('a mermaid block becomes a diagram slot that keeps its source, escaped', () => {
  const html = render('```mermaid\nflowchart LR\n  A["<b>x</b>"] --> B\n```\n- [ ] after');
  assert.match(html, /<figure class="diagram"><pre class="diagram-src"><code>flowchart LR\n  A\[&quot;&lt;b&gt;x&lt;\/b&gt;&quot;\] --&gt; B<\/code><\/pre><\/figure>/);
  assert.deepEqual(boxes(html), [[1, false]]);
  assert.doesNotMatch(render('```js\nx\n```'), /diagram/);
});

test('an svg block becomes a picture that can never run or load anything', () => {
  const svg = '<svg viewBox="0 0 100 40"><title>RC circuit</title><line x1="0" y1="20" x2="100" y2="20" stroke="#000"/><script>alert(1)</script></svg>';
  const html = render('```svg\n' + svg + '\n```\n- [ ] after');
  const src = html.match(/<figure class="drawing"><img src="data:image\/svg\+xml;charset=utf-8,([^"]*)" alt="RC circuit"><\/figure>/);
  assert.ok(src, html);
  assert.doesNotMatch(html, /<svg|<script/);
  assert.match(decodeURIComponent(src[1]), /^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg" viewBox/);
  assert.deepEqual(boxes(html), [[1, false]]);
  assert.match(render('```svg\n<svg xmlns="http://www.w3.org/2000/svg"><title>"x"</title></svg>\n```'), /alt="&quot;x&quot;"/);
  assert.match(render('```svg\n<svg viewBox="0 0 1 1"><line'), /drawing pending/);
  assert.match(render('```svg\nnot a drawing\n```'), /<pre><code class="language-svg">not a drawing<\/code><\/pre>/);
  assert.match(render('![RC](data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=)'), /<img class="note-img drawing-img" src="data:image\/svg\+xml;base64,PHN2Zz48L3N2Zz4=" alt="RC">/);
  assert.doesNotMatch(render('![x](data:text/html;base64,PHNjcmlwdD4=)'), /<img/);
});

test('a [[Title]] link to another note opens it, with an optional label, and stays escaped', () => {
  assert.equal(
    render('See [[Graph traversals]] and [[Heaps|heaps]].'),
    '<p>See <a class="wiki-link" href="#/search/Graph%20traversals" data-action="open-title" data-title="Graph traversals">Graph traversals</a> and <a class="wiki-link" href="#/search/Heaps" data-action="open-title" data-title="Heaps">heaps</a>.</p>'
  );
  assert.match(render('[[A <b> & "c"]]'), /data-title="A &lt;b&gt; &amp; &quot;c&quot;">A &lt;b&gt; &amp; &quot;c&quot;<\/a>/);
  assert.match(render('[[Lecture#Part 2]]'), /data-title="Lecture">Lecture<\/a>/);
  assert.doesNotMatch(render('![[pic.png]]'), /wiki-link/);
});

test('mathSpans finds exactly the formulas the renderer draws', () => {
  const outer = (spans) => {
    let end = 0;
    return spans.sort((a, b) => a[0] - b[0]).filter(([from, to]) => from >= end && (end = to, true));
  };
  for (const line of ['Energy $E = mc^2$ here.', 'a $x$ and $$y$$ z', String.raw`cost \$5 and \$6 $x$`, 'price $5 and $6 ok', 'code `$a$` then $b$', String.raw`\(a\) and \[b\] and $c$`, '$a$$b$ x', String.raw`nested $a \(b\) c$ end`, '- [ ] todo $x_1$ [link $y$](u) $z$', 'no formula here', '$a$ `x` $b$ `y` $c$']) {
    const spans = outer(md.mathSpans(line));
    assert.equal(spans.length, (render(line).match(/class="math/g) || []).length, line);
  }
  assert.deepEqual(md.mathSpans('a $x$ b'), [[2, 5]]);
  assert.deepEqual(md.mathSpans('a `$x$` b $y$'), [[10, 13]]);
  assert.deepEqual(md.mathSpans(String.raw`\$5 and $x$`), [[8, 11]]);
});

test('math in LaTeX becomes slots the page draws, and prices stay prices', () => {
  assert.equal(render('Energy $E = mc^2$ here.'), '<p>Energy <span class="math" data-tex="E = mc^2">E = mc^2</span> here.</p>');
  assert.equal(render('It costs $5 and $10.'), '<p>It costs $5 and $10.</p>');
  assert.doesNotMatch(render('Keep \\$x$ literal'), /class="math"/);
  assert.doesNotMatch(render('`$a$` is code'), /class="math"/);
  assert.equal(render('$$\n\\frac{a}{b} < c\n$$'), '<div class="math math-block" data-tex="\\frac{a}{b} &lt; c">\\frac{a}{b} &lt; c</div>');
  assert.equal(render('$$ x^2 $$'), '<div class="math math-block" data-tex="x^2">x^2</div>');
  assert.match(render('$$\nx\n$$\n- [ ] after'), /data-box="1"/);
});

test('Obsidian callouts become boxes, and a - or + makes them fold', () => {
  assert.equal(
    render('> [!example]- How the loop works\n> It walks the **queue**.'),
    '<details class="callout callout-example"><summary>How the loop works</summary><div class="callout-body"><p>It walks the <strong>queue</strong>.</p></div></details>'
  );
  assert.match(render('> [!tip]+ Open by default\n> x'), /<details class="callout callout-tip" open><summary>Open by default<\/summary>/);
  assert.equal(render('> [!warning]\n> Careful'), '<div class="callout callout-warning"><div class="callout-title">Warning</div><div class="callout-body"><p>Careful</p></div></div>');
  assert.match(render('> [!note"><img src=x>] t\n> x'), /^<blockquote>/);
  assert.match(render('> plain quote'), /^<blockquote>/);
});

test('a formula followed by words or a full stop never swallows the text after it', () => {
  assert.equal(render('$$a=b$$.\n\nAfter'), '<p><span class="math math-block" data-tex="a=b">a=b</span>.</p><p>After</p>');
  assert.equal(render('$$a=b$$ where a is\n\nAfter'), '<p><span class="math math-block" data-tex="a=b">a=b</span> where a is</p><p>After</p>');
  assert.equal(render('$$\na=b\n$$.\n\nAfter'), '<div class="math math-block" data-tex="a=b">a=b</div><p>After</p>');
  assert.equal(render('$$\na $$ where b\n\nAfter'), '<div class="math math-block" data-tex="a">a</div><p>where b</p><p>After</p>');
  assert.match(render('$$a = \\$5$$ where\n\nAfter'), /<p>After<\/p>$/);
  assert.equal(render('$$a\n=b$$ where x\n\n$$c$$'), '<div class="math math-block" data-tex="a\n=b">a\n=b</div><p>where x</p><div class="math math-block" data-tex="c">c</div>');
});

test('the other ways models write math are drawn too', () => {
  assert.equal(render('Energy \\(E = mc^2\\) here.'), '<p>Energy <span class="math" data-tex="E = mc^2">E = mc^2</span> here.</p>');
  assert.equal(render('So \\[a^2 + b^2 = c^2\\] holds.'), '<p>So <span class="math math-block" data-tex="a^2 + b^2 = c^2">a^2 + b^2 = c^2</span> holds.</p>');
  assert.equal(render('Then $$x_1 + x_2$$ follows.'), '<p>Then <span class="math math-block" data-tex="x_1 + x_2">x_1 + x_2</span> follows.</p>');
  assert.equal(render('\\[\n\\sum_{i=1}^n i = \\frac{n(n+1)}{2}\n\\]'), '<div class="math math-block" data-tex="\\sum_{i=1}^n i = \\frac{n(n+1)}{2}">\\sum_{i=1}^n i = \\frac{n(n+1)}{2}</div>');
  assert.equal(render('\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}\nAfter.'), '<div class="math math-block" data-tex="\\begin{aligned}\na &amp;= b \\\\\nc &amp;= d\n\\end{aligned}">\\begin{aligned}\na &amp;= b \\\\\nc &amp;= d\n\\end{aligned}</div><p>After.</p>');
  assert.doesNotMatch(render('A price of $5 and $10.'), /class="math"/);
  assert.doesNotMatch(render('`\\(x\\)` stays code'), /class="math"/);
  assert.match(render('Subscripts like \\(x_i\\) and \\(y_j\\) stay math, not italics'), /data-tex="x_i".*data-tex="y_j"/);
  const told = render('In full:\n\\[\nA = \\pi r^2\n\\]\nand also\n$$\nx\n$$\n\\begin{aligned}\na &= b\n\\end{aligned}');
  assert.equal((told.match(/class="math math-block"/g) || []).length, 3, told);
  assert.ok(told.startsWith('<p>In full:</p>'), 'a block of math right after a sentence still starts its own block');
});

test('read-only previews retain task appearance without editable note controls', () => {
  const html = render('- [ ] Due Friday\n- [x] Submitted', { interactive: false });
  assert.ok(!html.includes('data-box='));
  assert.equal((html.match(/type="checkbox" disabled/g) || []).length, 2);
  assert.ok(html.includes('disabled checked'));
  assert.deepEqual(boxes(render('- [ ] Still editable')), [[1, false]]);
});

test('a small svg and the paragraph after it become a drawing row only when rows are asked for', () => {
  const svg = (w) => `\`\`\`svg\n<svg viewBox="0 0 ${w} 80"><title>Cell</title></svg>\n\`\`\``;
  const rows = (text) => render(text, { rows: true });
  for (const gap of ['\n', '\n\n']) {
    const html = rows(`${svg(120)}${gap}**Nucleus**\nHolds the DNA\nIt keeps the genes safe.\n\nAfter.`);
    assert.equal((html.match(/class="drawing-row"/g) || []).length, 1);
    assert.ok(html.includes('<span class="drawing-title">Nucleus</span>'));
    assert.ok(html.includes('<span class="drawing-sub">Holds the DNA</span>'));
    assert.ok(html.includes('It keeps the genes safe.</p></div>'));
    assert.ok(html.includes('<p>After.</p>') && !html.includes('```'));
  }
  assert.ok(!rows(`${svg(120)}\n\n\nText`).includes('drawing-row'));
  assert.ok(!rows(`${svg(120)}\n\n- item`).includes('drawing-row'));
  assert.ok(!rows(`${svg(400)}\n\nText`).includes('drawing-row'));
  assert.ok(!render(`${svg(120)}\n\nText`).includes('drawing-row'));
  assert.equal((rows(`${svg(120)}\nOne\n\n${svg(120)}\nTwo`).match(/drawing-row"/g) || []).length, 2);
  assert.ok(rows(`${svg(120)}\n**Leaf**\nGreen`).includes('<span class="drawing-title">Leaf</span>'));
  assert.ok(rows(`${svg(120)}\n**Leaf** cell\nGreen`).includes('<span class="drawing-title"><strong>Leaf</strong> cell</span>'));
  assert.ok(!rows(`${svg(120)}\n**\nGreen`).includes('drawing-title'));
});

test('placeholders inside links, images and math resolve, and attributes only ever get plain text', () => {
  const code = render('See [`Vec::push`](https://doc.rust-lang.org/x)');
  assert.ok(code.includes('><code>Vec::push</code></a>'), code);
  const image = render('[![x](a.png)](https://x.com)');
  assert.ok(image.includes('<a href="https://x.com"') && image.includes('<img class="note-img"'), image);
  const alt = render('![$a$](foo.png)');
  assert.ok(alt.includes('alt="$a$"') && alt.includes('data-cap="$a$"') && !alt.includes('data-tex'), alt);
  const tex = render('$x \\[y\\] z$');
  assert.ok(tex.includes('data-tex="x \\[y\\] z"'), tex);
  const wiki = render('[[note|`c`]]');
  assert.ok(wiki.includes('><code>c</code></a>'), wiki);
  for (const html of [code, image, alt, tex, wiki]) assert.ok(!html.includes('\u0000'), html);
  assert.ok(!render('a\u00000\u0000b `c`').includes('\u0000'));
});

test('the note page alone right-aligns number columns, unless the Markdown aligned them', () => {
  const table = '| Item | Price | Share | Mixed | Set |\n|---|---|---|---|:-:|\n| Tea | $1,200.50 | 15% | 3 | 4 |\n| Cake | -2 |  | n/a | 5 |\n| Jam | 0.15 | 7.5% | 4 | 6 |';
  const html = render(table, { noteTools: true });
  assert.ok(html.includes('<th>Item</th><th class="align-right">Price</th><th class="align-right">Share</th><th>Mixed</th><th class="align-center">Set</th>'), html);
  assert.ok(html.includes('<td>Cake</td><td class="align-right">-2</td><td class="align-right"></td><td>n/a</td>'), html);
  assert.ok(!render('| a |\n|---|\n| 1,2 |\n| . |', { noteTools: true }).includes('align-right'));
  assert.ok(!render('| a |\n|---|\n|  |', { noteTools: true }).includes('align-right'));
});

test('the note page alone labels code with its language and offers Copy without adding text', () => {
  const html = render('```python\nprint(1 < 2)\n```\n\n```\nplain\n```', { noteTools: true });
  assert.ok(html.startsWith('<div class="code" data-lang="python"><pre><code class="language-python">print(1 &lt; 2)</code></pre><button type="button" class="code-copy" aria-label="Copy code" title="Copy code"><svg'), html);
  assert.ok(html.includes('<div class="code"><pre><code>plain</code></pre><button'), html);
  assert.equal(html.replace(/<[^>]*>/g, ''), 'print(1 &lt; 2)plain');
});

test('without the note page option every other reader gets the same markup as before', () => {
  const body = '| a | n |\n|---|---|\n| x | 1 |\n| y | 2.5 |\n\n```js\nlet a = 1;\n```\n\n> quote';
  assert.equal(render(body), '<div class="table"><table><thead><tr><th>a</th><th>n</th></tr></thead><tbody><tr><td>x</td><td>1</td></tr><tr><td>y</td><td>2.5</td></tr></tbody></table></div><pre><code class="language-js">let a = 1;</code></pre><blockquote><p>quote</p></blockquote>');
  assert.equal(render(body, { noteTools: false }), render(body));
});

test('the read time counts words outside code at 238 a minute', () => {
  assert.equal(md.readTime(''), '');
  assert.equal(md.readTime('# Hello\n\n- [ ] one task\n\n```\nskip these words\n```\n\n| a | b |\n|---|---|\n| c — d |'), '1 min read');
  assert.equal(md.readTime('one\n\n```\n' + 'skip '.repeat(300) + '\n```\n'), '1 min read');
  assert.equal(md.readTime('```\n' + 'skip '.repeat(300) + '\n```'), '');
  assert.equal(md.wordCount('# Hello\n\n```\nskip these\n```\n\none two'), 3);
  assert.equal(md.wordCount(''), 0);
  assert.equal(md.readTime('w '.repeat(238)), '1 min read');
  assert.equal(md.readTime('w '.repeat(239)), '2 min read');
  assert.equal(md.readTime('w '.repeat(1200)), '6 min read');
});

test('the read time of a note with a long run of blank lines comes back quickly', () => {
  for (const gap of ['\n', ' \n', '\r\n', '\t\n']) {
    const started = Date.now();
    assert.equal(md.readTime(`start${gap.repeat(4000)}end`), '1 min read');
    assert.ok(Date.now() - started < 1000, JSON.stringify(gap));
  }
  assert.equal(md.plain(`a${' \n'.repeat(3000)}---${' \n'.repeat(3000)}b`), 'a b');
});

test('plain on a long run of blank lines grows linearly', () => {
  for (const gap of ['\n', ' \n', '\r\n', '\t\n']) {
    const time = (n) => {
      const started = performance.now();
      md.plain(`start${gap.repeat(n)}end | a |${gap.repeat(n)}end`);
      return performance.now() - started;
    };
    const small = time(40000);
    const large = time(80000);
    assert.ok(large < 1000 && large <= Math.max(small * 4, 50), `${JSON.stringify(gap)} ${small} ${large}`);
  }
});

test('plain turns table pipes into spaces exactly as the old pattern did', () => {
  for (const text of ['', '|', '||', 'a||b', 'a | | b', '| a', 'a |', '| a |', '|a|b|', 'a\r\n|\r\nb', 'a\t|\tb', 'a | b', 'a \n | \n b', 'no pipes here', ' | ']) {
    const old = text.replace(/\s*\|\s*/g, ' ');
    const now = md.plain(text);
    assert.equal(now, old.replace(/\s+/g, ' ').trim(), JSON.stringify(text));
  }
});

test('an edit reads as kept, removed and added words, and too long an edit gives up', () => {
  const words = (s) => s.split(/\s+/).filter(Boolean).join(' ');
  const pairs = [
    ['Each row of the matrix sums to one hundred percent.', 'Each row of the matrix sums to one, because it is a probability distribution.'],
    ['BFS takes the newest vertex.', 'BFS takes the oldest vertex.'],
    ['the cat sat on a mat', 'the dog lay on a rug'],
    ['Line one\n\nLine two\n', 'Line one\n\nLine 2 here\n'],
    ['', 'Ask about mixing time next week.'],
    ['Gone entirely.', ''],
    ['café über naïve', 'café unter naïve'],
  ];
  for (const [before, after] of pairs) {
    const parts = md.wordDiff(before, after);
    assert.equal(parts.filter((p) => p.kind !== 'del').map((p) => p.text).join(''), after);
    assert.equal(words(parts.filter((p) => p.kind !== 'ins').map((p) => p.text).join('')), words(before));
    assert.ok(parts.every((p) => p.text && ['same', 'del', 'ins'].includes(p.kind)), JSON.stringify(parts));
  }
  assert.deepEqual(md.wordDiff('Each row sums to one hundred percent.', 'Each row sums to one, because it is a distribution.'), [
    { kind: 'same', text: 'Each row sums to one' },
    { kind: 'del', text: ' hundred percent' },
    { kind: 'ins', text: ', because it is a distribution' },
    { kind: 'same', text: '.' },
  ]);
  assert.deepEqual(md.wordDiff('takes the newest vertex', 'takes the oldest vertex'), [
    { kind: 'same', text: 'takes the ' },
    { kind: 'del', text: 'newest' },
    { kind: 'ins', text: 'oldest' },
    { kind: 'same', text: ' vertex' },
  ]);
  assert.deepEqual(md.wordDiff('a b', 'a b'), [{ kind: 'same', text: 'a b' }]);
  assert.equal(md.wordDiff('word '.repeat(400), 'other '.repeat(400)), null);
  assert.ok(md.wordDiff('word '.repeat(300), 'other '.repeat(300)));
});

test('an edit whose marks would hide a change of line breaks or indentation gives up, so the card shows both texts', () => {
  for (const [find, replace] of [['one\ntwo', 'one two'], ['- a\n- b', '- a\n\n- b'], ['  indented', 'indented'], ['alpha beta\ngamma', 'alpha delta gamma'], ['a, b', 'a,b']]) {
    assert.equal(md.wordDiff(find, replace), null, JSON.stringify([find, replace]));
  }
  for (const [find, replace] of [['the newest one', 'the oldest one'], ['a b c', 'a c'], ['x', 'x y'], ['x y', 'x']]) {
    assert.ok(md.wordDiff(find, replace), JSON.stringify([find, replace]));
  }
  assert.deepEqual(md.wordDiff('Intro. Next point here.', 'Intro.\n\nNew para. Next point here.'), [{ kind: 'same', text: 'Intro.' }, { kind: 'ins', text: '\n\nNew para.' }, { kind: 'same', text: ' Next point here.' }]);
  assert.deepEqual(md.wordDiff('a b', 'a b\n'), [{ kind: 'same', text: 'a b' }, { kind: 'ins', text: '\n' }]);
});

test('a long run of unmatched openers grows linearly', () => {
  const brackets = ['![[', '[', '![a|', '[a](', '![a]('].map((opener) => [opener, ['', ']', ')', '])']]);
  for (const [opener, tails, prefix = ''] of [
    ...brackets,
    ['\\[', ['', '\\]']],
    ['\\(', ['', '\\)']],
    ['\\[', [''], 'Before \\[x\\] '],
    ['\\(', [''], 'Before \\(x\\) '],
    ['[a](<', [' )']],
    ['![a](<', [' )']],
    ['![a](< ', ['', ')']],
    ['![a](< )', ['', '>', '>)']],
  ]) {
    for (const tail of tails) {
      const time = (n) => {
        const input = prefix + opener.repeat(n) + tail;
        const started = process.cpuUsage();
        render(input);
        md.plain(input);
        const { user, system } = process.cpuUsage(started);
        return (user + system) / 1000;
      };
      const small = Math.min(time(20000), time(20000), time(20000));
      const large = Math.min(time(40000), time(40000), time(40000));
      assert.ok(large < 1000 && large <= Math.max(small * 3, 50), `${prefix}${opener} ${tail} ${small} ${large}`);
    }
  }
});

test('math keeps the same delimiters and escaping beside unclosed openers', () => {
  assert.equal(render('Before \\[x < y\\] and \\[unfinished'), '<p>Before <span class="math math-block" data-tex="x &lt; y">x &lt; y</span> and \\[unfinished</p>');
  assert.equal(render('Before \\(x_i\\) and \\(unfinished'), '<p>Before <span class="math" data-tex="x_i">x_i</span> and \\(unfinished</p>');
  assert.equal(render('Before \\[\\] \\(\\) \\[unfinished'), '<p>Before \\[\\] \\(\\) \\[unfinished</p>');
  assert.equal(render('Before \\[unclosed and \\(x_i\\) and $$y$$'), '<p>Before \\[unclosed and <span class="math" data-tex="x_i">x_i</span> and <span class="math math-block" data-tex="y">y</span></p>');
});

test('links, pictures and embeds read the same beside unclosed brackets', () => {
  assert.equal(render('[a [b](https://a.b)'), '<p><a href="https://a.b" target="_blank" rel="noopener noreferrer">a [b</a></p>');
  assert.equal(render('[a] [b](https://a.b) [c'), '<p>[a] <a href="https://a.b" target="_blank" rel="noopener noreferrer">b</a> [c</p>');
  assert.equal(render('![x](javascript:1)'), '<p>![x](javascript:1)</p>');
  assert.equal(render('![a](<>) ![b](<c>)'), '<p>![a](&lt;&gt;) <span class="note-fig" data-cap="b"><img class="note-img" src="/api/image?path=c&amp;from=" alt="b" loading="lazy"></span></p>');
  assert.equal(render('![a](<first) then > broken)'), '<p><span class="note-fig" data-cap="a"><img class="note-img" src="/api/image?path=%3Cfirst&amp;from=" alt="a" loading="lazy"></span> then &gt; broken)</p>');
  assert.equal(render('![bad](< ) then ![a](<path with spaces.png> "title") ![tail](<'), '<p><span class="note-fig" data-cap="bad"><img class="note-img" src="/api/image?path=%20)%20then%20!%5Ba%5D(%3Cpath%20with%20spaces.png&amp;from=" alt="bad" loading="lazy"></span> ![tail](&lt;</p>');
  assert.equal(render('![bad](< ) then ![a](<path with spaces.png> nope) ![b](fallback.png)'), '<p>![bad](&lt; ) then ![a](&lt;path with spaces.png&gt; nope) <span class="note-fig" data-cap="b"><img class="note-img" src="/api/image?path=fallback.png&amp;from=" alt="b" loading="lazy"></span></p>');
  assert.equal(md.plain('[a] ![[b]] ![c](d) [e](f) [g'), '[a] e [g');
});

test('a copied selection keeps its pictures as data urls and nothing else that is live', () => {
  const dot = 'data:image/png;base64,iVBORw0KGgo=';
  const html = `<p>Before <strong>bold</strong> <a href="https://a.b/c?x=1&amp;y=2" class="x" onclick="evil()">link</a></p><p><img class="note-img" src="${dot}" alt="a dot" onerror="evil()"></p>`;
  assert.equal(md.copyHtml(html), `<p>Before <strong>bold</strong> <a href="https://a.b/c?x=1&amp;y=2">link</a></p><p><img src="${dot}" alt="a dot"></p>`);
});

test('a picture that cannot be embedded is left as its alt text and local addresses never leave', () => {
  const html = '<p><img src="/api/image?path=a.png&amp;from=" alt="the &quot;queue&quot; &gt; stack"></p><p><img src="http://127.0.0.1:1234/x.png" alt=""></p><p><a href="#/n/abc123">a note</a> <a href="/api/image?path=a.png">file</a> <a href="http://localhost:5000/x">local</a> <a href="javascript:evil()">js</a></p>';
  const out = md.copyHtml(html);
  assert.equal(out, '<p>the &quot;queue&quot; &gt; stack</p><p></p><p>a note file local js</p>');
  assert.ok(!/127\.0\.0\.1|\/api\/|localhost|javascript|href/.test(out), out);
});

test('a copied selection never carries script, handlers, styles or inline drawings', () => {
  const dot = 'data:image/png;base64,AAAA';
  const html = `<div onclick="x()" style="color:red"><script>alert(1)</script><style>p{}</style><span class="m" data-x="1">kept <em>text</em></span><svg><script>1</script><text>drawn</text></svg><iframe src="/x">hi</iframe><button>go</button><!-- hidden --><img src='${dot}' alt='ok' onload=evil()><textarea>typed</textarea>after</div>`;
  const out = md.copyHtml(html);
  assert.equal(out, `<div>kept <em>text</em><img src="${dot}" alt="ok">after</div>`);
  assert.ok(!/script|onclick|onload|style|svg|iframe|button|alert|typed|hidden/.test(out), out);
});

test('an attribute holding a > does not end its tag early and bare text stays escaped', () => {
  const out = md.copyHtml('<p title="a>b"><img alt="x>y" src="/api/image?path=q"> 1 &lt; 2 &amp; 3</p>');
  assert.equal(out, '<p>x&gt;y 1 &lt; 2 &amp; 3</p>');
});

test('pictures are bounded one by one and together, the rest left as alt text', () => {
  const big = (n) => `data:image/png;base64,${'A'.repeat(n)}`;
  const html = `<img src="${big(50)}" alt="one"><img src="${big(200)}" alt="two"><img src="${big(50)}" alt="three"><img src="${big(50)}" alt="four">`;
  const out = md.copyHtml(html, { each: 100, all: 150 });
  assert.equal(out, `<img src="${big(50)}" alt="one">two<img src="${big(50)}" alt="three">four`);
  assert.equal(md.copyHtml('<img src="data:text/html;base64,AAAA" alt="no"><img src="data:image/png;base64,A&quot;B" alt="no2">'), 'nono2');
});

test('unbalanced and nested tags come out balanced', () => {
  assert.equal(md.copyHtml('</b><ul><li>a<li>b</ul><p><em>open'), '<ul><li>a<li>b</li></li></ul><p><em>open</em></p>');
  assert.equal(md.copyHtml('<ol start="3"><li>x</li></ol>'), '<ol start="3"><li>x</li></ol>');
});

test('copied text drops its pictures, keeps a real caption and leaves code as it is', () => {
  assert.equal(ed.withoutPictures('Before\n\n![Pasted picture](attachments/a.png)\n\nAfter'), 'Before\n\nAfter');
  assert.equal(ed.withoutPictures('![Pasted picture](a.png)\n\n  ![Screenshot 1|300](b.png)\n\n  - After'), '  - After');
  assert.equal(ed.withoutPictures('A ![The queue|300](a.png "t") and ![[graph.png|250]]'), 'A The queue and');
  const code = '```\n![a](b.png)\n```\n\n\n\n\n`![a](b.png)`';
  assert.equal(ed.withoutPictures(code), code);
  const fenced = '```\na\n\n\n\nb\n```';
  assert.equal(ed.withoutPictures('![Image](x.png)\n\n' + fenced), fenced);
  assert.equal(ed.withoutPictures('A\n\n\n\nB\n\n![Image](x.png)\n\nC'), 'A\n\n\n\nB\n\nC');
  const spaces = ' a' + ' '.repeat(100000) + 'b';
  assert.equal(ed.withoutPictures('![Image](x.png)' + spaces), spaces);
  assert.equal(ed.withoutPictures('![Pasted picture](https://a.b/c.png)'), '![Pasted picture](https://a.b/c.png)');
  assert.equal(ed.withoutPictures(' no picture\n\n\n\n\nhere '), ' no picture\n\n\n\n\nhere ');
});

test('markdown copied from leo is told from anything else on the clipboard', () => {
  const own = 'Before\n\n![Pasted picture](attachments/a.png)\n\nAfter';
  assert.ok(ed.ownCopy(own, 'Before\r\n\r\nAfter\r\n'));
  assert.ok(!ed.ownCopy(own, 'Before\n\nAfter that'));
  assert.ok(!ed.ownCopy(own, ''));
  assert.ok(ed.ownCopy('![The queue](attachments/a.png)\n\n![[b.png]]', ''));
  for (const target of ['http://a.b/c.png', 'https://a.b/c.png', '//a.b/c.png', '<file:///c.png>', 'data:image/png;base64,AA']) {
    assert.ok(!ed.ownCopy(`![a](${target})`, ''), target);
    assert.ok(!ed.ownCopy(`A\n\n![a](${target})`, `A\n\n![a](${target})`), target);
  }
  assert.ok(!ed.ownCopy('![a](b.png) ![[https://a.b/c.png]]', ''));
  assert.ok(!ed.ownCopy(' \n', ''));
  assert.ok(!ed.ownCopy('![a](<'.repeat(20000), ''));
  assert.ok(ed.stored('![a](attachments/a.png) ![b](<attachments/b c.png>) ![[attachments/d.png]]'));
  assert.ok(!ed.stored('![a](attachments/a.png) ![b](img/b.png)'));
  assert.ok(ed.onlyPictures('![The queue](a.png)\n'));
  assert.ok(!ed.onlyPictures('- ![The queue](a.png)'));
});

test('the markdown of one drawn picture is the one its place names', () => {
  const line = 'A ![x](https://a.b/c.png) `![c](d.png)` ![One|300](a.png "t") ![[b.png]]';
  assert.equal(ed.pictureSource(line, 0), '![One|300](a.png "t")');
  assert.equal(ed.pictureSource(line, 1), '![[b.png]]');
  assert.equal(ed.pictureSource(line, 2), '');
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
