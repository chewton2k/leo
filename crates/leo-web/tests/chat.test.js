'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/chat.js');

const C = globalThis.leoChat;
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

test('a streamed reply is read line by line, keeping a half line for later', () => {
  const first = C.splitLines('{"sources":[]}\n{"t":"Hel');
  assert.deepEqual(first.events, [{ sources: [] }]);
  assert.equal(first.rest, '{"t":"Hel');
  const second = C.splitLines(first.rest + 'lo"}\n{"done":true}\n');
  assert.deepEqual(second.events, [{ t: 'Hello' }, { done: true }]);
  assert.equal(second.rest, '');
  assert.ok(C.splitLines('not json\n').events[0].error);
});

test('a graded answer is marked, and the marker never shows', () => {
  assert.deepEqual(C.grade('[[correct]] Nice, the heap keeps the minimum on top.'), { verdict: 'correct', text: 'Nice, the heap keeps the minimum on top.' });
  assert.deepEqual(C.grade('  [[Incorrect]]\nNot quite.'), { verdict: 'incorrect', text: 'Not quite.' });
  assert.deepEqual(C.grade('[[corr'), { verdict: null, text: '' }, 'a marker still arriving stays hidden');
  assert.deepEqual(C.grade('A [[link]] in the middle'), { verdict: null, text: 'A [[link]] in the middle' });
});

test('citations become note chips, and ones Felix was never given are dropped', () => {
  const sources = [{ n: 1, id: 'a', title: 'Heaps' }, { n: 2, id: 'b', title: 'A <long> title that keeps going on and on' }];
  const html = C.cite('<p>Min on top [n1]. Both [n1, n2]. Nope [n9].</p>', sources, esc);
  assert.equal((html.match(/class="cite"/g) || []).length, 3);
  assert.ok(html.includes('data-id="a"'));
  assert.ok(html.includes('&lt;long&gt;'), 'titles are escaped');
  assert.ok(!html.includes('[n9]'), 'a citation of nothing is not shown');
  assert.ok(html.includes('Nope.</p>'), 'and leaves no gap');
  assert.ok(html.includes('Min on top <button'), 'a kept citation keeps its space');
  assert.deepEqual(C.cited('one [n2] and [n1, n2]', sources).map((s) => s.id), ['a', 'b']);
  assert.deepEqual(C.cited('no citations here n1', sources), []);
});

test('citations are only placed in running text, never in attributes, code or math', () => {
  const sources = [{ n: 1, id: 'a', title: 'Heaps' }];
  const html = C.cite('<p><a href="https://example.com/a[n1]" target="_blank">docs</a> and <code>arr[n1]</code> and <span class="math" data-tex="x[n1]">x[n1]</span> and [n1]</p><pre><code>v[n1]</code></pre>', sources, esc);
  assert.equal((html.match(/class="cite"/g) || []).length, 1, html);
  assert.ok(html.includes('href="https://example.com/a[n1]"'), html);
  assert.ok(html.includes('<code>arr[n1]</code>') && html.includes('<code>v[n1]</code>'), html);
  assert.ok(html.includes('data-tex="x[n1]">x[n1]</span>'), html);
});

test('citations skip block math too', () => {
  const sources = [{ n: 1, id: 'a', title: 'Heaps' }];
  const html = C.cite('<div class="math math-block" data-tex="x[n1]"><span class="katex">x[n1]</span></div><p>see [n1]</p>', sources, esc);
  assert.equal((html.match(/class="cite"/g) || []).length, 1, html);
  assert.ok(html.includes('<span class="katex">x[n1]</span></div>'), html);
});

test('numbered citations share a number per place, keep note and line, and leave no stray digits in copied text', () => {
  const sources = [{ n: 1, id: 'a', title: 'Heaps' }, { n: 2, id: 'b', title: 'Queues' }];
  const text = 'Min on top [n1 L4-6]. Again [n1 L4-6]. Queue [n2]. Other lines [n1 L9]. Gone [n9].';
  const marks = C.numbered(text, sources);
  assert.deepEqual(marks.map((m) => [m.n, m.s.id, m.lines, m.where]), [[1, 'a', '4-6', 'lines 4–6'], [2, 'b', '', ''], [3, 'a', '9-9', 'line 9']]);
  const html = C.cite(`<p>${text}</p>`, sources, esc, [], marks);
  assert.equal((html.match(/class="cite cite-n"/g) || []).length, 4, 'one button per cite, repeats included');
  assert.ok(html.includes('data-line="4-6" title="Heaps, lines 4–6" aria-label="Source 1: Heaps, lines 4–6">1</button>'));
  assert.ok(html.includes('title="Heaps, line 9"'));
  assert.ok(html.includes('>1</button>.'), 'a number sits against its sentence');
  assert.ok(!html.includes('[n9]'), 'a citation of nothing is not shown');
  const plain = html.replace(/<button[^>]*class="cite[^>]*>.*?<\/button>/g, '');
  assert.ok(!/\d/.test(plain.replace(/<[^>]+>/g, '')), 'removing the buttons removes every number');
});

test('the conversation is kept per browser and survives broken storage', () => {
  const values = new Map();
  const storage = { getItem: (k) => values.get(k) || null, setItem: (k, v) => values.set(k, v) };
  C.save(storage, Array.from({ length: 60 }, (_, i) => ({ role: 'user', text: `m${i}` })));
  const back = C.load(storage);
  assert.equal(back.messages.length, 40);
  assert.equal(back.messages[0].text, 'm20');
  const broken = { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } };
  assert.deepEqual(C.load(broken), { id: null, messages: [], refs: [] });
  C.save(broken, []);
});

test('Felix keeps his colour and shape', () => {
  const svg = C.felix(40, 'idle');
  assert.ok(svg.includes('class="felix idle"'));
  const body = /class="felix-skin" x="\d+" y="\d+" width="(\d+)" height="(\d+)"/.exec(svg);
  const ratio = Number(body[1]) / Number(body[2]);
  assert.ok(ratio > 1 && ratio < 1.4, `a box a little wider than tall, not ${ratio}`);
  assert.equal((svg.match(/width="4" height="4"/g) || []).length, 2, 'two eyes');
  assert.match(svg, /class="felix-eyes" shape-rendering="geometricPrecision"/, 'eyes are drawn exactly, so both come out the same size');
  assert.equal((svg.match(/class="felix-arm felix-(left|right)"/g) || []).length, 2, 'two arms');
  assert.match(svg, /<g class="felix-arm felix-left"><rect class="felix-arm-skin"[^>]*\/><g class="felix-claws">/, 'the monster’s claws move with his arms');
  assert.match(svg, /<g class="felix-rawr">/);
});

test('Demon Felix has his own taps, flames, slashes and words, and cute Felix can still surprise', () => {
  const svg = C.felix(40, 'idle');
  for (const part of ['felix-flames', 'felix-slash', 'felix-hehe', 'felix-hi', 'felix-rawr', 'felix-monster']) assert.match(svg, new RegExp(`<g class="${part}">`), part);
  assert.equal(C.DEMON_TAPS.length, C.TAPS.length);
  assert.ok(C.DEMON_TAPS.every((tap) => !C.TAPS.includes(tap) && tap !== 'monster' && tap !== 'sweet'));
  assert.equal(C.MONSTER_ONE_IN, 10);
  assert.ok(C.TICKLED_TOO_LONG > 3 && C.TICKLED_TOO_LONG <= 10);
});

test('typing @ starts a note search, but an email address does not', () => {
  assert.deepEqual(C.mentionAt('compare @heap', 13), { start: 8, query: 'heap' });
  assert.deepEqual(C.mentionAt('@', 1), { start: 0, query: '' });
  assert.deepEqual(C.mentionAt('see @graph trav', 15), { start: 4, query: 'graph trav' });
  assert.equal(C.mentionAt('mail me at a@b.com', 18), null);
  assert.equal(C.mentionAt('@ nothing', 9), null);
  assert.equal(C.mentionAt('@line\nnext', 10), null);
  assert.equal(C.mentionAt('no mention here', 15), null);
});

test('a note is added once, and only up to the limit', () => {
  let refs = C.addRef([], { id: 'a', title: 'Heaps' });
  refs = C.addRef(refs, { id: 'a', title: 'Heaps' });
  assert.deepEqual(refs, [{ id: 'a', title: 'Heaps' }]);
  for (let i = 0; i < 20; i++) refs = C.addRef(refs, { id: `n${i}`, title: '' });
  assert.equal(refs.length, C.MOST_REFS);
  assert.equal(refs[1].title, 'Untitled');
});

test('attached notes are kept with the conversation', () => {
  const store = new Map();
  const storage = { getItem: (k) => store.get(k) || null, setItem: (k, v) => store.set(k, v) };
  C.save(storage, [{ role: 'user', text: 'hi' }], [{ id: 'a', title: 'Heaps' }], 'chat-1234');
  assert.deepEqual(C.load(storage), { id: 'chat-1234', messages: [{ role: 'user', text: 'hi' }], refs: [{ id: 'a', title: 'Heaps' }] });
});

test('a chat saved under an old style opens with its messages', () => {
  for (const mode of ['chat', 'study', 'quiz', 'interview']) {
    const storage = { getItem: () => JSON.stringify({ mode, messages: [{ role: 'user', text: 'hi' }] }), setItem() {} };
    assert.deepEqual(C.load(storage).messages, [{ role: 'user', text: 'hi' }]);
    assert.equal('mode' in C.load(storage), false);
  }
});

test('past chats are grouped the way chat apps group them', () => {
  const now = new Date(2026, 9, 7, 15, 0);
  const at = (days, hour = 10) => ({ id: `c${days}`, title: 't', updated_at: new Date(2026, 9, 7 - days, hour).toISOString() });
  const g = C.groups([at(0), at(1), at(3), at(20), at(90)], now);
  assert.deepEqual(g.map((x) => x.name), ['Today', 'Yesterday', 'Previous 7 days', 'Previous 30 days', 'Older']);
  assert.deepEqual(C.groups([at(0, 1), at(0, 9)], now).map((x) => x.chats.length), [2]);
  assert.deepEqual(C.groups([], now), []);
});

test('a chat id is safe in a file name even without randomUUID', () => {
  const saved = globalThis.crypto;
  Object.defineProperty(globalThis, 'crypto', { value: {}, configurable: true });
  const id = C.newId();
  Object.defineProperty(globalThis, 'crypto', { value: saved, configurable: true });
  assert.match(id, /^[a-z0-9-]{8,64}$/);
  assert.match(C.newId(), /^[a-zA-Z0-9-]{8,64}$/);
});

test('a starter that needs classes names the ones picked', () => {
  const plan = C.STARTERS.find((s) => s.text.includes('review plan'));
  assert.equal(C.starterWords(plan, ['cs130']), 'Make me a 3-day review plan for cs130');
  assert.equal(C.starterWords(plan, ['cs130', 'math61', 'phys1b']), 'Make me a 3-day review plan for cs130, math61 and phys1b');
  for (const s of C.STARTERS) assert.ok(s.ask, `${s.text} asks when nothing is open`);
});

test('splitFiles keeps files already sent out of the box for the next message', () => {
  const docs = [{ id: 'a', name: 'week3.txt' }, { id: 'b', name: 'week4.txt' }, { id: 'c', name: 'old.pdf' }];
  const messages = [
    { role: 'user', text: 'what is in it?', docs: ['week3.txt'], files: ['a'] },
    { role: 'assistant', text: 'heaps', files: ['b'] },
    { role: 'user', text: 'before files had ids', docs: ['old.pdf'] },
  ];
  const { sent, waiting } = C.splitFiles(docs, messages);
  assert.deepEqual(sent.map((d) => d.id), ['a', 'c']);
  assert.deepEqual(waiting.map((d) => d.id), ['b']);
  assert.deepEqual(C.splitFiles(docs, []).waiting.length, 3);
  assert.deepEqual(C.splitFiles(undefined, undefined), { sent: [], waiting: [] });
});

test('asNote keeps the question, links cited notes and drops the grade', () => {
  const sources = [{ n: 1, id: 'a', title: 'Graph traversals' }, { n: 2, id: 'b', title: 'Queues [old]' }];
  const note = C.asNote('  how does   BFS work? ', '[[correct]] It takes vertices from a queue [n1, n2] in order [n9].', sources);
  assert.equal(note.title, 'how does BFS work?');
  assert.equal(note.body, '**Q:** how does BFS work?\n\nIt takes vertices from a queue [[Graph traversals]] [[Queues old]] in order [n9].');
  const long = C.asNote('Explain why breadth first search finds the shortest path in an unweighted graph every time', 'Because.', []);
  assert.ok(long.title.endsWith('…'));
  assert.ok(long.title.length <= 71);
  assert.ok(!long.title.includes('  '));
  assert.deepEqual(C.asNote('', 'Just text', []), { title: 'From Felix', body: 'Just text' });
});

test('reviewPrompt asks each missed question again with the old answer', () => {
  const text = C.reviewPrompt([
    { question: 'What does BFS use?', answer: 'a stack' },
    { question: 'Define a heap.', answer: '' },
  ]);
  assert.match(text, /one at a time/);
  assert.match(text, /\n1\. What does BFS use\? \(last time I said: a stack\)\n2\. Define a heap\.$/);
});

test('each tool gives Felix its own prop, and anything else none', () => {
  assert.equal(C.poseOf('search_notes'), 'tool-search');
  assert.equal(C.poseOf('open_note'), 'tool-open');
  assert.equal(C.poseOf('connected_notes'), 'tool-map');
  assert.equal(C.poseOf('edit_note'), 'tool-edit');
  assert.equal(C.poseOf('create_note'), 'tool-create');
  assert.equal(C.poseOf('delete_everything'), null);
  for (const prop of ['search', 'open', 'map', 'edit', 'create']) assert.ok(C.felix(40).includes(`felix-tool-${prop}`), prop);
});

test('what an answer cost reads as money for keys, model and effort for plans, free for this computer', () => {
  const paid = C.spentLabel({ by: 'Anthropic', model: 'claude-sonnet-5-5', input: 12000, output: 800, cost: 0.032, steps: 2 });
  assert.equal(paid.text, '$0.032 · Anthropic · claude-sonnet-5-5');
  assert.equal(paid.title, '12,000 tokens in, 800 out over 2 steps');
  assert.equal(C.spentLabel({ by: 'OpenAI', model: 'gpt-5-nano', cost: 0.00042, estimated: true }).text, '≈ $0.0004 · OpenAI · gpt-5-nano');
  assert.equal(C.spentLabel({ by: 'OpenAI', model: 'gpt-5.5', cost: 1.234 }).text, '$1.23 · OpenAI · gpt-5.5');
  assert.equal(C.spentLabel({ by: 'Codex', model: 'gpt-6.1-sol', effort: 'high', plan: true }).text, 'Codex · gpt-6.1-sol · high effort · on your plan');
  assert.equal(C.spentLabel({ by: 'Codex', model: 'gpt-6-luna', effort: 'low', plan: true, auto: true }).text, 'Auto · Codex · gpt-6-luna · low effort · on your plan');
  assert.equal(C.spentLabel({ by: 'Claude Code', model: 'claude-opus-5-5', plan: true }).text, 'Claude Code · claude-opus-5-5 · default effort · on your plan');
  assert.equal(C.spentLabel({ by: 'Ollama', model: 'qwen3:8b', local: true, cost: 0 }).text, 'Ollama · qwen3:8b · free on this computer');
  assert.equal(C.spentLabel({ by: 'OpenRouter', model: 'x/y' }).text, 'OpenRouter · x/y · price unknown');
  assert.equal(C.spentLabel(null), null);
  assert.equal(C.spentLabel({ cost: 1 }), null);
});

test('a file card names its type and shows a small example of what is inside', () => {
  assert.deepEqual(C.fileKind('Week 3.pdf'), { label: 'PDF', tone: 'pdf' });
  assert.deepEqual(C.fileKind('essay.DOCX'), { label: 'DOCX', tone: 'doc' });
  assert.deepEqual(C.fileKind('slides.pptx'), { label: 'PPTX', tone: 'slides' });
  assert.deepEqual(C.fileKind('grades.csv'), { label: 'CSV', tone: 'sheet' });
  assert.deepEqual(C.fileKind('board.jpeg'), { label: 'JPG', tone: 'image' });
  assert.deepEqual(C.fileKind('notes.md'), { label: 'MD', tone: 'text' });
  assert.deepEqual(C.fileKind('README'), { label: 'FILE', tone: 'other' });
  const doc = C.fileCard({ name: 'a<b>.pdf', excerpt: 'Week 3\nHeaps' }, esc, { remove: 'd1' });
  assert.match(doc, /class="file-card tone-pdf"/);
  assert.match(doc, /<span class="file-page">Week 3\nHeaps<\/span>/);
  assert.match(doc, /a&lt;b&gt;\.pdf/);
  assert.match(doc, /data-chat="unfile" data-id="d1"/);
  const pic = C.fileCard({ name: 'p.png', thumb: 'data:image/png;base64,AAAA' }, esc);
  assert.match(pic, /<img src="data:image\/png;base64,AAAA"/);
  assert.doesNotMatch(pic, /file-x/);
  const odd = C.fileCard({ name: 'p.png', thumb: 'javascript:alert(1)' }, esc, { reading: true });
  assert.doesNotMatch(odd, /<img/);
  assert.match(odd, /Reading…/);
});

test('Felix has three ways of handling notes and Shift+Tab goes round them', () => {
  assert.deepEqual(C.ACCESS.map((a) => a.id), ['ask', 'auto', 'read']);
  assert.equal(C.accessOf('auto'), 'auto');
  assert.equal(C.accessOf('everything'), 'ask');
  assert.equal(C.accessOf(null), 'ask');
  assert.equal(C.nextAccess('ask'), 'auto');
  assert.equal(C.nextAccess('auto'), 'read');
  assert.equal(C.nextAccess('read'), 'ask');
});

test('notes attached earlier in a chat come along with later questions, newest first', () => {
  const said = (refs) => ({ role: 'user', text: 'q', refs });
  const messages = [said([{ id: 'a', title: 'A' }, { id: 'b', title: 'B' }]), { role: 'assistant', text: 'x', refs: [{ id: 'z' }] }, said([]), said([{ id: 'c', title: 'C' }, { id: 'a', title: 'A' }])];
  assert.deepEqual(C.threadRefs(messages).map((r) => r.id), ['c', 'a', 'b']);
  assert.deepEqual(C.threadRefs([]), []);
  const many = Array.from({ length: 12 }, (_, i) => said([{ id: `n${i}`, title: 'N' }]));
  assert.equal(C.threadRefs(many).length, C.MOST_REFS);
});

test('notes cited in the last three answers are sent along, newest first', () => {
  const said = (text, sources) => ({ role: 'assistant', text, sources });
  const s = (n, id) => ({ n, id, title: id });
  const messages = [
    said('old [n1]', [s(1, 'oldest')]),
    { role: 'user', text: 'q' },
    said('uses [n1] and [n2]', [s(1, 'a'), s(2, 'b')]),
    said('nothing cited', [s(1, 'c')]),
    said('only [n2]', [s(1, 'x'), s(2, 'd')]),
  ];
  assert.deepEqual(C.recentNotes(messages), ['d', 'a', 'b']);
  assert.deepEqual(C.recentNotes([]), []);
});

test('a practice answer is checked kindly: case, accents, spaces and punctuation do not count', () => {
  const mc = { kind: 'multiple_choice', question: 'What does BFS use?', options: ['a stack', 'a queue'], answer: 'a queue' };
  assert.equal(C.checkQuiz(mc, 'A Queue'), true);
  assert.equal(C.checkQuiz(mc, 'a stack'), false);
  const blank = { kind: 'fill_blank', question: "f'(x) = 2x - 4 ___", answer: '\\sin(x)' };
  assert.equal(C.checkQuiz(blank, 'sin x'), null, 'a blank is marked by Felix, who reads what it means');
  assert.equal(C.checkQuiz(blank, 'newest'), null);
  const asked = C.quizSay({ kind: 'fill_blank', question: 'q', answer: 'oldest | earliest' }, 'the earliest one', null);
  assert.ok(asked.includes('Accepted answers: oldest or earliest.'));
  assert.ok(asked.includes('sin x and sin(x)'), 'notation does not decide the mark');
  assert.equal(C.checkQuiz({ kind: 'free_response', answer: 'Because…' }, 'anything'), null, 'free answers are marked by Felix');
});

test('what Felix is told about a practice answer says how it went and how to reply', () => {
  const mc = { kind: 'multiple_choice', question: 'What does BFS use?', answer: 'a queue' };
  const wrong = C.quizSay(mc, 'a stack', false);
  assert.ok(wrong.includes('Question: What does BFS use?'));
  assert.ok(wrong.includes('My answer: a stack'));
  assert.ok(wrong.includes('the right answer is a queue'));
  assert.ok(wrong.includes('[[incorrect]]'));
  assert.ok(C.quizSay(mc, 'a queue', true).includes('[[correct]]'));
  const free = C.quizSay({ kind: 'free_response', question: 'Why a queue?', answer: 'Oldest first.' }, 'it is fair', null);
  assert.ok(free.includes('[[correct]] or [[incorrect]]'));
  assert.ok(free.includes('Oldest first.'));
  assert.ok(C.quizSay({ kind: 'fill_blank', question: 'q', answer: 'oldest | earliest' }, 'x', false).includes('the right answer is oldest.'));
});

test('asking why about an answer explains it and leaves the next question to the user', () => {
  const mc = { kind: 'multiple_choice', question: 'What does BFS use?', answer: 'a queue' };
  const why = C.quizSay(mc, 'a queue', true, true);
  assert.ok(why.includes('[[correct]]'));
  assert.ok(why.includes('do not ask a new practice question'));
  assert.ok(!why.includes('ask the next question with the quiz tool'));
  const marked = C.heldSay({ ...mc, given: 'a stack', correct: false }, false);
  assert.ok(marked.includes('leo marked it wrong; the right answer is a queue'));
  assert.ok(marked.includes('Ask the next question with the quiz tool'));
  const told = C.heldSay({ ...mc, given: 'a stack', correct: false, told: true }, false);
  assert.ok(told.startsWith('Next question, please.'), 'a result already told with a typed message is not told again');
  assert.ok(!told.includes('leo marked it'));
  assert.ok(C.heldSay({ ...mc, given: 'a queue', correct: true, told: true }, true).includes('end_session'));
});

test('tokens read from the cache are named in the cost line tooltip', () => {
  const label = C.spentLabel({ by: 'Anthropic', model: 'claude-sonnet-5-5', input: 12000, cached: 9000, output: 800, cost: 0.01, steps: 3 });
  assert.equal(label.title, '12,000 tokens in (9,000 from the cache), 800 out over 3 steps');
  assert.ok(!C.spentLabel({ by: 'Anthropic', model: 'm', input: 5, output: 1, cost: 0 }).title.includes('cache'));
});

test('what any model is told about an earlier answer includes what Felix did, asked and suggested, and who wrote it', () => {
  const answer = {
    role: 'assistant',
    text: '[[incorrect]] It uses a queue [n1].',
    spent: { by: 'Codex', model: 'gpt-6.1-sol' },
    steps: [{ text: 'Searched your notes for “queue”', found: ['Graph traversals', 'Heaps'] }, 'Opened “Graph traversals”'],
    asks: [{ question: 'Which week?', options: ['1', '2'], answered: '2' }],
    quizzes: [{ kind: 'multiple_choice', question: 'What does BFS use?', options: ['a stack', 'a queue'], answer: 'a queue', given: 'a stack', correct: false }],
    proposals: [{ kind: 'edit', title: 'Graph traversals', find: 'stack', replace: 'queue', state: 'applied' }],
  };
  const text = C.historyText(answer);
  assert.ok(text.startsWith('[Written by Codex · gpt-6.1-sol]\n[What Felix did'), 'notes come first, so clipping a long answer keeps them');
  assert.ok(text.endsWith('\nIt uses a queue [n1].'), 'the verdict marker is not repeated');
  assert.ok(text.includes('[What Felix did for this answer: Searched your notes for “queue” (found: Graph traversals, Heaps); Opened “Graph traversals”]'));
  assert.ok(text.includes('[Felix asked: Which week? (choices: 1 | 2); the user answered: 2]'));
  assert.ok(text.includes('[Practice question (multiple choice): What does BFS use?; choices: a stack | a queue; answer: a queue; answered “a stack”, wrong]'));
  assert.ok(text.includes('Suggested a change to “Graph traversals”: “stack” → “queue”; applied]'));
  assert.equal(C.historyText({ role: 'assistant', text: 'Plain.' }), 'Plain.');
  assert.equal(C.historyText({ role: 'user', text: 'a stack', say: 'Quiz answer. …' }), 'Quiz answer. …');
});

test('notes Felix opened, not only the ones he cited, come along after a switch', () => {
  const messages = [{ role: 'assistant', text: 'No citation here.', sources: [{ n: 1, id: 'opened-1', title: 'A', why: 'opened by Felix' }, { n: 2, id: 'found-1', title: 'B', why: 'found by searching for "x"' }] }];
  assert.deepEqual(C.recentNotes(messages), ['opened-1']);
});

test('a verdict after a short lead-in still counts, and the lead-in is dropped', () => {
  assert.deepEqual(C.grade("I'll mark this answer and give you the next card. [[incorrect]] It uses a queue."), { verdict: 'incorrect', text: 'It uses a queue.' });
  assert.deepEqual(C.grade("I'll mark this [[inc"), { verdict: null, text: "I'll mark this " }, 'a marker still arriving after a lead-in stays hidden');
  const late = `${'word '.repeat(80)}[[correct]] later`;
  assert.equal(C.grade(late).verdict, null, 'a marker deep inside an answer is not a verdict');
});

test('a hint asks for a nudge without the answer, and every mark explains itself', () => {
  const mc = { kind: 'multiple_choice', question: 'Which equation?', options: ['x = 2 sin x', 'x = 0'], answer: 'x = 2 sin x' };
  const hint = C.hintSay(mc);
  assert.ok(hint.includes('Choices: x = 2 sin x | x = 0'));
  assert.ok(hint.includes('Do not give the answer'));
  const wrong = C.quizSay(mc, 'x = 0', false);
  assert.ok(wrong.includes('do not give a hint') && wrong.includes('why each of the other choices is wrong'));
  const right = C.quizSay(mc, 'x = 2 sin x', true);
  assert.ok(right.includes('praise me briefly in your own words') && right.includes('explain why it is right'));
  const free = C.quizSay({ kind: 'free_response', question: 'Why?', answer: 'Because.' }, 'dunno', null);
  assert.ok(free.includes('intuitive shortcut or tip') && free.includes('praise me briefly'));
  for (const said of [right, free]) assert.ok(said.includes('only for what my answer and thinking show (a guess is not reasoning)'));
  for (const said of [wrong, right, free]) assert.doesNotMatch(said, /score/i);
});

test('each further hint knows the hints already given and goes one step further', () => {
  const q = { kind: 'free_response', question: 'Why does BFS find shortest paths?', hints: [{ text: 'Think about the order it visits vertices.' }] };
  const said = C.hintSay(q);
  assert.ok(said.includes('Hints you already gave me:\n1. Think about the order it visits vertices.'));
  assert.ok(said.includes('one step further'));
  assert.ok(said.includes('Do not give the answer'));
  assert.ok(C.hintSay({ kind: 'free_response', question: 'Why?' }).includes('Give me one short hint'));
});




test('a selection in an answer maps back to its whole Markdown lines, fences and math kept', () => {
  const source = 'Intro line.\n\nA **heap** keeps the *minimum* at the root [n1].\n\n```js\nconst a = 1;\nconst b = 2;\n```\n\n$$\nx^2\n$$\nEnd.';
  assert.equal(C.answerLines(source, 'heap keeps the minimum'), 'A **heap** keeps the *minimum* at the root [n1].');
  assert.equal(C.answerLines(source, 'const b = 2;'), '```js\nconst a = 1;\nconst b = 2;\n```');
  assert.equal(C.answerLines(source, 'x2'), '$$\nx^2\n$$');
  assert.equal(C.answerLines(source, 'Intro line.\n\nA heap keeps'), 'Intro line.\n\nA **heap** keeps the *minimum* at the root [n1].');
  assert.equal(C.answerLines(source, 'something Felix never wrote'), 'something Felix never wrote');
  assert.equal(C.answerLines(source, 'A heap keeps the minimum at the root Heaps.'), 'A **heap** keeps the *minimum* at the root [n1].', 'a citation shown as its note title still maps back');
  assert.equal(C.answerLines(source, '   '), '');
});

test('citations become links to the notes, line citations too, unknown ones stay', () => {
  const sources = [{ n: 1, id: 'a', title: 'Heaps [intro]' }, { n: 2, id: 'b', title: 'Graphs' }];
  assert.equal(C.linkCitations('Root [n1]. Both [n1, n2]. Lines [n2 L14-18]. Gone [n9].', sources), 'Root [[Heaps intro]]. Both [[Heaps intro]] [[Graphs]]. Lines [[Graphs]]. Gone [n9].');
});

test('a selected passage travels with its message into the history Felix reads', () => {
  const m = { role: 'user', text: 'Why?', selection: { note: 'a', title: 'Heaps', from: 3, to: 4, text: 'A heap keeps the minimum at the root.' } };
  assert.equal(C.historyText(m), '[Selected passage from “Heaps” lines 3–4: “A heap keeps the minimum at the root.”]\nWhy?');
  assert.equal(C.historyText({ role: 'user', text: 'Hi' }), 'Hi');
});



test('a study session closes an idea on two counted right answers or two misses, and finishes when all are closed', () => {
  const s = C.startSession(['Root', 'Sift down', 'Heap sort', '  ']);
  assert.deepEqual(s.ideas, ['Root', 'Sift down', 'Heap sort']);
  C.scoreSession(s, { idea: 1 }, true);
  assert.equal(s.outcomes[0].state, 'open', 'one right answer is not enough');
  assert.equal(s.outcomes[0].right, 1);
  const again = { idea: 1 };
  C.scoreSession(s, again, true);
  C.scoreSession(s, again, true);
  assert.equal(s.outcomes[0].state, 'first', 'a second right answer closes it, and scoring one quiz twice counts once');
  assert.equal(s.outcomes[0].right, 2);
  C.scoreSession(s, { idea: 2, more: true }, false);
  C.scoreSession(s, { idea: 2, more: true }, true);
  C.scoreSession(s, { idea: 2, sure: 'guess' }, true);
  C.scoreSession(s, { idea: 2, hints: [{ text: 'Look at the root.' }] }, true);
  assert.equal(s.outcomes[1].right, 0, 'a step of a worked problem, a guess and a hinted answer never count');
  C.scoreSession(s, { idea: 2, sure: 'sure' }, true);
  assert.equal(s.outcomes[1].state, 'open');
  C.scoreSession(s, { idea: 2 }, true);
  assert.equal(s.outcomes[1].state, 'done');
  C.scoreSession(s, { idea: 1 }, false);
  assert.equal(s.outcomes[0].state, 'first', 'a closed idea stays closed');
  C.scoreSession(s, { idea: 3 }, null);
  assert.equal(s.outcomes[2].state, 'open', 'no verdict, no change');
  C.scoreSession(s, { idea: 3 }, true);
  assert.deepEqual(C.sessionBody(s), { ideas: ['Root', 'Sift down', 'Heap sort'], closed: [true, true, false], asked: 0, right: [2, 2, 1], misses: [0, 1, 0] });
  assert.deepEqual(C.sessionSummary(s), { total: 3, covered: 2, first: 1, review: [], once: ['Heap sort'], skipped: [] });
  C.scoreSession(s, { idea: 3 }, false);
  C.scoreSession(s, { idea: 3 }, false);
  assert.equal(s.outcomes[2].state, 'review');
  assert.equal(s.state, 'finishing');
  assert.deepEqual(C.sessionSummary(s), { total: 3, covered: 3, first: 1, review: ['Heap sort'], once: [], skipped: [] });
  s.state = 'finished';
  assert.equal(C.sessionBody(s), null);
  const saved = { ideas: ['A'], outcomes: [{ state: 'open', misses: 0 }], asked: 1, state: 'active' };
  C.scoreSession(saved, { idea: 1 }, true);
  assert.equal(saved.outcomes[0].right, 1, 'a session saved before right counts starts from zero');
  const capped = C.startSession(['A', 'B']);
  capped.asked = 30;
  C.scoreSession(capped, { idea: 1 }, true);
  assert.equal(capped.state, 'finishing');
});


test('tool activity follows streamed text without dropping text or splitting Unicode', () => {
  const text = 'Read first.\n\nReview next.\n\nDone.';
  const parts = C.timeline(text, [{ text: 'Read attempt', at: 13 }, { text: 'Read progress', at: 27 }, { text: 'Saved review', at: 27 }]);
  assert.equal(parts.map((p) => p.text || '').join(''), text);
  assert.deepEqual(parts.map((p) => p.text || p.steps.map((s) => s.text)), ['Read first.\n\n', ['Read attempt'], 'Review next.\n\n', ['Read progress', 'Saved review'], 'Done.']);
  const emoji = C.timeline('答😀え', [{ text: 'Looked', at: 2 }]);
  assert.equal(emoji[0].text, '答😀');
  assert.equal(emoji[2].text, 'え');
  const malformed = C.timeline('answer', [null, {}, { text: 'Read', at: -4, found: 'bad' }, { text: 'Done', at: 999 }, { text: 'Later', at: 1 }]);
  assert.equal(malformed.map((p) => p.text || '').join(''), 'answer');
  assert.equal(malformed[0].steps[0].found.length, 0);
  assert.equal(C.timeline('answer', Array(100).fill('Read')).flatMap((p) => p.steps || []).length, 64);
  assert.deepEqual(C.timeline('old answer', ['Legacy step']).map((p) => p.text || p.steps[0].text), ['Legacy step', 'old answer']);
});

test('conversation rows display only escaped titles, including search results', () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const row = source.slice(source.indexOf('    function chatRow(c) {'), source.indexOf('\n    let resumeSeq', source.indexOf('    function chatRow(c) {')));
  const context = vm.createContext({ state: { id: 'chat-one', doomed: null }, escape: esc });
  vm.runInContext(row, context);
  for (const about of ['', 'A response preview that should stay hidden', '<script>bad()</script>']) {
    const html = context.chatRow({ id: 'chat-one', title: 'Graphs <review>', about });
    assert.ok(html.includes('Graphs &lt;review&gt;'));
    assert.ok(!html.includes('chat-history-about'));
    if (about) assert.ok(!html.includes(about));
    assert.ok(html.includes('data-chat="resume"') && html.includes('data-chat="forget"'));
  }
});



test('removing a document keeps the chip gone when the server no longer has it and restores it on a real failure', async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const start = source.indexOf('    async function removeFile(id) {');
  const removeFile = source.slice(start, source.indexOf('\n    let pickTimer', start));
  for (const [status, kept, notices] of [[200, false, 0], [404, false, 0], [401, false, 0], [500, true, 1]]) {
    const state = { id: 'c1', files: [{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }, { id: 'c', name: 'C' }] };
    const noted = [];
    const context = vm.createContext({ state, drawRefs() {}, notify: (message) => noted.push(message), encodeURIComponent, request: async () => ({ ok: status === 200, status }) });
    vm.runInContext(removeFile, context);
    await context.removeFile('b');
    assert.equal(state.files.map((f) => f.id).join(), kept ? 'a,b,c' : 'a,c', String(status));
    assert.equal(noted.length, notices, String(status));
  }
});

test('the request wrapper sends same-origin credentials and reports a locked session', async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const start = source.indexOf('    const request = async (url, init = {}) => {');
  const wrapper = source.slice(start, source.indexOf('\n    };\n', start) + 7);
  for (const [status, locked] of [[401, 1], [200, 0]]) {
    const sent = [];
    let locks = 0;
    const context = vm.createContext({ onLocked: () => { locks += 1; }, fetch: async (url, init) => { sent.push([url, init]); return { ok: status === 200, status }; } });
    vm.runInContext(`${wrapper}\nthis.request = request;`, context);
    const response = await context.request('/api/review', { method: 'POST', body: '{}' });
    assert.equal(response.status, status);
    assert.equal(locks, locked, String(status));
    assert.deepEqual(JSON.parse(JSON.stringify(sent)), [['/api/review', { credentials: 'same-origin', method: 'POST', body: '{}' }]]);
  }
});

test('a failed review-done post puts the sent items back without dropping a newer list', async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const start = source.indexOf('    async function startReview() {');
  const startReview = source.slice(start, source.indexOf('\n    const TOOL_ICONS', start));
  const due = { key: 'a', due: true, notes: [] };
  const later = { key: 'b', due: false, notes: [] };
  const newer = [{ key: 'c', due: true, notes: [] }];
  for (const [ok, reload, expected] of [[false, true, 'a,c'], [false, false, 'a,b'], [true, true, 'c'], [true, false, 'b']]) {
    const state = { review: [due, later], busy: null, refs: [] };
    const context = vm.createContext({ state, MOST_REFS: 8, drawRefs() {}, send() {}, reviewPrompt: () => '', request: async () => { if (reload) state.review = newer; return { ok }; } });
    vm.runInContext(startReview, context);
    await context.startReview();
    assert.equal(state.review.map((m) => m.key).join(), expected, `${ok} ${reload}`);
  }
});

function chatSendFixture(id = 'practice', access = 'ask', before = async () => true, stream = {}) {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const send = source.slice(source.indexOf('    async function send('), source.indexOf('    let leaving ='));
  const drawContext = source.slice(source.indexOf('    function drawContext('), source.indexOf('    const writer = (m) =>'));
  const state = { id, access, messages: [], refs: [], files: [], sent: [], context: null, selection: null };
  const requests = [], prepared = [], notices = [];
  const contextBox = { textContent: '', title: '', hidden: false };
  const input = { value: 'Put the solution in my code area.' };
  const noop = () => {};
  const context = vm.createContext({
    state, input, AbortController, TextDecoder, Date: class extends Date { static now() { return stream.clock ? stream.clock() : Date.now(); } }, working: new Map(), timed: new Set(), MOST_REFS: C.MOST_REFS,
    historyText: C.historyText, modelHistory: C.modelHistory, historyRequest: C.historyRequest, threadRefs: C.threadRefs, recentNotes: C.recentNotes, sessionBody: C.sessionBody, splitLines: C.splitLines, grade: C.grade, scoreSession: C.scoreSession,
    cardOf: doc => doc, currentSession: () => stream.session || null, closePick: noop, drawRefs: noop, fit: noop, awake: noop, mood: noop, thinking: noop, draw: noop, drawChats: noop, remember: noop, compactLater: noop, stopCompacting: noop, react: noop, talking: noop,
    pose: noop, freshIfGone: noop, onSaved: noop, onChanged: noop, onOrganized: noop, queueMicrotask, organize: async card => { card.state = 'applied'; },
    $: () => contextBox, escape: esc,
    onBeforeSend: async args => { prepared.push(args); return before(args); }, notify: message => notices.push(message),
    request: async (_url, options) => {
      requests.push(JSON.parse(options.body));
      const reply = stream.replies ? stream.replies[requests.length - 1] || {} : stream;
      let read = false;
      return { ok: true, status: 200, body: { getReader: () => ({ read: async () => {
        if (read) {
          if (reply.finish) reply.finish(state);
          if (reply.error) throw reply.error;
          return { done: true };
        }
        read = true;
        const events = reply.events || [{ t: 'Ready.' }, { done: true }];
        return { done: false, value: new TextEncoder().encode(events.map(event => JSON.stringify(event)).join('\n') + '\n') };
      } }) } };
    },
  });
  vm.runInContext(send + drawContext, context);
  return { state, input, requests, prepared, notices, contextBox, send: context.send, drawContext: context.drawContext };
}

test('completed and stopped replies retain elapsed time without confusing completion with cancellation', async () => {
  for (const abort of [false, true]) {
    let now = 100;
    const fixture = chatSendFixture('practice', 'ask', undefined, {
      clock: () => now,
      events: [{ proposal: { kind: 'add_task', state: 'applied' } }],
      finish: state => { now = 61250; if (abort) state.busy.abort(); },
    });
    await fixture.send('Fix my code.');
    const answer = fixture.state.messages.at(-1);
    assert.equal(answer.elapsed_ms, 61150);
    assert.equal(Boolean(answer.cancelled), abort);
    assert.equal(answer.error, abort ? 'Stopped.' : undefined);
  }
});

test('successful card-only replies finish without a stopped error for every proposal kind and state', async () => {
  const kinds = ['create', 'edit', 'add_task', 'update_task', 'delete_task', 'restore_task', 'move_note', 'make_folder', 'remove_folder', 'move_folder', 'delete_note', 'restore_note', 'rename_note', 'rename_folder', 'pin_note', 'delete_folder', 'restore_folder', 'add_task', ];
  for (const kind of kinds) {
    for (const state of ['new', 'applied']) {
      const fixture = chatSendFixture('practice', 'ask', undefined, { events: [{ proposal: { kind, state, made: 'note' } }, { done: true }] });
      await fixture.send('Make this change.');
      const answer = fixture.state.messages.at(-1);
      assert.equal(answer.error, undefined, `${kind}: ${state}`);
      assert.equal(answer.pending, false);
      assert.equal(answer.proposals[0].state, state);
    }
  }
  const events = [{ ask: { question: 'Which approach?', options: ['A', 'B'] } }, ...Object.keys(C.QUIZ_KINDS).map(kind => ({ quiz: { question: 'Why?', kind } })), { proposal: { kind: 'add_task', auto: true } }];
  for (const event of events) {
    for (const extra of [{}, { card: { question: 'Explain?', correct: null } }]) {
      const fixture = chatSendFixture('practice', 'ask', undefined, { events: [event, { done: true }] });
      await fixture.send('Continue.', extra);
      assert.equal(fixture.state.messages.at(-1).error, undefined);
      if (extra.card) assert.equal(extra.card.reply.error, undefined);
      assert.equal(fixture.state.busy, null);
    }
  }
});

test('a restarted reply counts retained applied cards and reports empty output when pending cards were discarded', async () => {
  for (const state of ['new', 'applied']) {
    const fixture = chatSendFixture('practice', 'ask', undefined, { events: [{ proposal: { kind: 'add_task', state } }, { reset: true }, { done: true }] });
    await fixture.send('Continue.');
    const answer = fixture.state.messages.at(-1);
    if (state === 'applied') {
      assert.equal(answer.error, undefined);
      assert.equal(answer.proposals.length, 1);
    } else {
      assert.match(answer.error, /finished without an answer.*Try again/);
      assert.equal(answer.proposals.length, 0);
    }
  }
});

test('empty completion, cancellation and provider failures remain distinct even after partial content or cards', async () => {
  const cases = [
    { events: [{ done: true }], expected: /finished without an answer.*Try again/ },
    { events: [{ t: 'A partial answer.' }], finish: state => state.busy.abort(), expected: /^Stopped\.$/ },
    { events: [{ proposal: { kind: 'add_task', state: 'applied' } }], finish: state => state.busy.abort(), expected: /^Stopped\.$/ },
    { events: [], error: { name: 'AbortError' }, expected: /^Stopped\.$/ },
    { events: [{ t: 'A partial answer.' }], error: { name: 'AbortError' }, expected: /^Stopped\.$/ },
    { events: [{ error: 'Stopped.' }], expected: /^Stopped\.$/ },
    { events: [{ proposal: { kind: 'edit' } }], error: new Error('Connection lost. Try again.'), expected: /^Connection lost\. Try again\.$/ },
    { events: [{ error: 'The provider is unavailable. Try again.' }], expected: /^The provider is unavailable\. Try again\.$/ },
  ];
  for (const reply of cases) {
    const fixture = chatSendFixture('practice', 'ask', undefined, reply);
    await fixture.send('Continue.');
    const answer = fixture.state.messages.at(-1);
    assert.match(answer.error, reply.expected);
    assert.equal(answer.pending, false);
    assert.equal(fixture.state.busy, null);
  }
});

test('proposal-only completion sends waiting follow-ups while cancellation restores them without sending', async () => {
  for (const abort of [false, true]) {
    const fixture = chatSendFixture('practice', 'ask', undefined, { replies: [{
      events: [{ proposal: { kind: 'add_task', state: 'applied' } }],
      finish: state => {
        const message = { role: 'user', text: 'Explain its complexity.', steer: 'waiting' };
        state.messages.push(message);
        state.busy.waiting.push(message);
        if (abort) state.busy.abort();
      },
    }] });
    fixture.input.value = '';
    await fixture.send('Change my code.');
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(fixture.requests.length, abort ? 1 : 2);
    assert.equal(fixture.input.value, abort ? 'Explain its complexity.' : '');
    assert.equal(fixture.state.busy, null);
  }
});










test('model history stays append only across quiz answers, hints, next cards and reloads', () => {
  for (const mode of ['chat', 'study']) {
    const messages = [{ role: 'user', text: 'Practise this material.', mode }];
    let quiz = { kind: 'free_response', question: 'Why does the invariant hold?', answer: 'The state preserves the condition.', correct: null };
    messages.push({ role: 'assistant', text: 'Start here.', mode, quizzes: [quiz] });
    let prior = C.modelHistory(messages);
    for (const hint of [true, true, false, false, true, false]) {
      const request = hint ? C.hintSay(quiz) : C.quizSay(quiz, 'It preserves the state.', null);
      const pending = { role: 'assistant', text: '', pending: true, request };
      if (hint) quiz.hints = [...(quiz.hints || []), pending];
      else { quiz.given = 'It preserves the state.'; quiz.reply = pending; }
      const asking = [...C.modelHistory(messages), { role: 'user', text: request }];
      assert.deepEqual(asking.slice(0, prior.length), prior, `${mode}: the prior prefix stays unchanged`);
      pending.text = hint ? 'Trace the state before and after.' : '[[correct]] The invariant remains true.';
      pending.pending = false;
      if (!hint) {
        quiz.correct = true;
        quiz = { kind: 'free_response', question: 'Which case comes next?', answer: 'The boundary case.', correct: null };
        messages.push({ role: 'assistant', text: '', mode, quizzes: [quiz] });
      }
      const completed = C.modelHistory(messages);
      assert.deepEqual(completed.slice(0, asking.length), asking);
      assert.equal(completed.length, asking.length + 1);
      assert.equal(completed.at(-1).role, 'assistant');
      assert.deepEqual(C.modelHistory(JSON.parse(JSON.stringify(messages))), completed);
      prior = completed;
    }
  }
});

test('legacy quiz replies and hints reconstruct stable request turns without changing the cards', () => {
  for (const kind of Object.keys(C.QUIZ_KINDS)) {
    const quiz = { kind, question: 'Choose the invariant.', answer: 'A', options: ['A', 'B'], given: 'A', correct: true, hint: { role: 'assistant', text: 'Look at the boundary.' }, hints: [{ role: 'assistant', text: 'Check the first element.' }], reply: { role: 'assistant', text: '[[correct]] The invariant holds.' } };
    const messages = [{ role: 'assistant', text: '', quizzes: [quiz] }];
    const before = JSON.stringify(messages);
    const history = C.modelHistory(messages);
    assert.equal(history.length, 7);
    assert.deepEqual(history.map(m => m.role), ['assistant', 'user', 'assistant', 'user', 'assistant', 'user', 'assistant']);
    assert.match(history[1].text, /Hint, please/);
    assert.match(history[3].text, /Hints you already gave me/);
    assert.match(history[5].text, /Quiz answer/);
    assert.doesNotMatch(history[0].text, /answered|right|Felix replied/);
    assert.equal(JSON.stringify(messages), before);
    quiz.correct = false;
    assert.deepEqual(C.modelHistory(messages), history);
  }
});

test('canonical history preserves notes, actions and ordinary conversation changes while omitting failed or pending turns', () => {
  const messages = [{ role: 'user', text: 'Change the note.', selection: { title: 'Heaps', text: 'The minimum is first.' } }, { role: 'assistant', text: 'A focused change.', proposals: [{ kind: 'edit', title: 'Heaps', find: 'before', replace: 'after', state: 'new' }] }, { role: 'assistant', text: 'Not sent.', pending: true }, { role: 'assistant', text: 'Failed.', error: 'Stopped.' }, { role: 'user', text: 'Waiting.', steer: 'waiting' }];
  const before = C.modelHistory(messages);
  assert.equal(before.length, 2);
  assert.match(before[0].text, /Selected passage/);
  assert.match(before[1].text, /waiting for the user/);
  messages[1].proposals[0].state = 'applied';
  assert.notDeepEqual(C.modelHistory(messages), before);
  messages[1].proposals[0].state = 'new';
  assert.deepEqual(C.modelHistory(messages), before);
  const long = Array.from({ length: 900 }, (_, i) => ({ role: i % 2 ? 'assistant' : 'user', text: `Turn ${i}` }));
  const kept = C.modelHistory(long);
  assert.ok(kept.length <= 400);
  assert.equal(kept.at(-1).text, 'Turn 899');
});

test('actual quiz sends keep the prior model history stable through hints, marking and saved next questions', async () => {
  const hints = [true, true, false, false, true, false];
  const replies = hints.map(hint => ({ events: [{ t: hint ? 'Check the boundary.' : '[[correct]] Your invariant holds.' }, ...(!hint ? [{ quiz: { kind: 'free_response', question: 'What happens next?', answer: 'The next state.', explain: 'Follow the state.' } }] : []), { done: true }] }));
  const fixture = chatSendFixture('practice', 'auto', undefined, { replies });
  let quiz = { kind: 'free_response', question: 'Why is it preserved?', answer: 'The transition preserves it.', correct: null };
  fixture.state.messages.push({ role: 'user', text: 'Practise this material.' }, { role: 'assistant', text: '', quizzes: [quiz] });
  let previous = null;
  for (const hint of hints) {
    const text = hint ? C.hintSay(quiz) : 'The transition preserves the state.';
    const extra = hint ? { card: quiz, hint: true } : { card: quiz, say: C.quizSay(quiz, text, null) };
    if (!hint) { quiz.given = text; quiz.correct = null; }
    await fixture.send(text, extra);
    const request = fixture.requests.at(-1);
    assert.equal(request.practice, true);
    assert.equal(request.mark, !hint);
    if (previous) {
      assert.deepEqual(request.messages.slice(0, previous.length), previous);
      assert.equal(request.messages.length, previous.length + 2);
    }
    const nested = hint ? quiz.hints.at(-1) : quiz.reply;
    assert.equal(nested.request, request.messages.at(-1).text);
    assert.match(nested.request, hint ? /Hint, please/ : /Quiz answer/);
    assert.deepEqual(C.modelHistory(JSON.parse(JSON.stringify(fixture.state.messages))), C.modelHistory(fixture.state.messages));
    previous = request.messages;
    if (!hint) quiz = fixture.state.messages.at(-1).quizzes[0];
  }
  assert.match(C.modelHistory(fixture.state.messages).map(m => m.text).join('\n'), /\[\[correct\]\]/);
});

test('a finished trace says what was opened, used and changed in one line', () => {
  const step = (tool, text, found = []) => ({ tool, text, found });
  const opened = [step('open_note', 'Opened “A”'), step('open_note', 'Read part 2 of 3 of “B” (lines 40–80)'), step('open_note', 'Read part 3 of 3 of “B” (lines 80–120)'), step('open_note', 'Opened “C”'), step('open_note', 'Opened “D”')];
  assert.equal(C.traceSummary(opened, ['A', 'B', 'C'], 0), 'Opened 4 notes, used 3');
  assert.equal(C.traceSummary(opened, ['A', 'B', 'C', 'D'], 0), 'Opened 4 notes');
  assert.equal(C.traceSummary(opened, [], 0), 'Opened 4 notes');
  assert.equal(C.traceSummary(opened, ['A', 'Not opened'], 0), 'Opened 4 notes, used 1');
  assert.equal(C.traceSummary(opened.slice(0, 1), ['A'], 1), 'Opened 1 note · made 1 change');
  assert.equal(C.traceSummary([step('read_document', 'Read “a.pdf”')], [], 0), 'Opened 1 source');
  assert.equal(C.traceSummary([step('search_notes', 'Searched')], [], 2), 'Searched your notes · made 2 changes');
  assert.equal(C.traceSummary([step('search_chats', 'Searched')], [], 0), 'Searched your chats');
  assert.equal(C.traceSummary([step('read_document', 'Read part 1 of 3 of “a.pdf”'), step('read_document', 'Read part 2 of 3 of “a.pdf”')], [], 0), 'Opened 1 source');
  assert.equal(C.traceSummary([step('read_transcript', 'Looked for “x” in the recording of “T”'), step('read_transcript', 'Looked for “y” in the recording of “T”'), step('open_note', 'Opened “A”')], ['T'], 0), 'Opened 2 notes, used 1');
  assert.equal(C.traceSummary([step('calculate', 'Worked it out')], [], 0), '1 step');
  assert.equal(C.traceSummary([{ ...step('open_note', 'Looked for “x”'), failed: true }, step('search_notes', 'Searched')], [], 0), 'Searched your notes');
});

test('the live step says what Felix is doing now, and anything unknown keeps his words', () => {
  const step = (tool, text, failed = false) => ({ tool, text, failed, found: [] });
  assert.equal(C.doing(step('open_note', 'Read “Markov chains”')), 'Reading “Markov chains”…');
  assert.equal(C.doing(step('open_note', 'Read part 2 of 3 of “B” (lines 40–80)')), 'Reading part 2 of 3 of “B” (lines 40–80)…');
  assert.equal(C.doing(step('search_notes', 'Searched your notes for “lab safety”')), 'Searching your notes for “lab safety”…');
  assert.equal(C.doing(step('connected_notes', 'Followed connections from “Heaps”')), 'Following connections from “Heaps”…');
  assert.equal(C.doing(step('edit_note', 'Suggested a change to “Heaps”')), 'Suggesting a change to “Heaps”…');
  assert.equal(C.doing(step('read_transcript', 'Looked for “x” in the recording of “T”')), 'Looking for “x” in the recording of “T”…');
  assert.equal(C.doing(step('calculate', 'Worked out the numbers.')), 'Working out the numbers…');
  assert.equal(C.doing(step('open_note', 'Looked for “x”', true)), 'Looked for “x”', 'a failed step keeps its words');
  assert.equal(C.doing(step('quiz', 'Wanted to change a note (read only)')), 'Wanted to change a note (read only)');
  assert.equal(C.doing(step('', '')), '');
});

test('only an empty search with nothing cited is marked as not from the notes', () => {
  const empty = [{ tool: 'search_notes', text: 'Searched', found: [] }];
  assert.equal(C.notFromNotes(empty, 'Wear goggles.', 0), true);
  assert.equal(C.notFromNotes(empty, 'Wear goggles.', 1), false);
  assert.equal(C.notFromNotes(empty, 'Beyond your notes: wear goggles.', 0), false);
  assert.equal(C.notFromNotes(empty, '  ', 0), false);
  assert.equal(C.notFromNotes([], 'Hello.', 0), false);
  assert.equal(C.notFromNotes([{ tool: 'search_notes', text: 'Searched', found: ['A'] }], 'Wear goggles.', 0), false);
  assert.equal(C.notFromNotes([...empty, { tool: 'open_note', text: 'Opened “A”', found: [] }], 'Wear goggles.', 0), false);
  assert.equal(C.notFromNotes([{ tool: 'calculate', text: 'Worked', found: [] }], '4', 0), false);
  assert.equal(C.notFromNotes([...empty, { tool: 'open_note', text: 'Looked for “x”', found: [], failed: true }], 'Wear goggles.', 0), true);
});

const USAGE_DAYS = {
  models: [
    { day: '2026-11-07', name: 'big', calls: 2, input: 1000, cached: 750, output: 40 },
    { day: '2026-11-06', name: '', calls: 1, input: 0, cached: 0, output: 0 },
    { day: '2026-11-05', name: '<b>', calls: NaN, input: -1, cached: 5, output: Infinity },
    { day: '2026-11-01', name: 'small', calls: 3, input: 300, cached: 9999, output: 30 },
    { day: '2026-09-01', name: 'old', calls: 5, input: 50, cached: 0, output: 5 },
    { name: 'nameless', calls: 1 },
    null,
  ],
  kinds: [
    { day: '2026-11-07', name: 'answer', calls: 3, input: 0, cached: 0, output: 0 },
    { day: '2026-11-07', name: '<script>', calls: 9 },
    { day: '2026-11-01', name: 'margin_read', calls: 2 },
  ],
};

test('a usage period adds up tokens, states the cache share and drops garbage', () => {
  const week = C.usagePeriod(USAGE_DAYS, 7, '2026-11-07');
  assert.deepEqual(week.totals, { calls: 6, input: 1300, cached: 1050, output: 70, share: 1050 / 1300 });
  assert.deepEqual(week.models.map((m) => [m.name, m.tokens]), [['big', 1040], ['small', 330], ['', 0], ['<b>', 0]]);
  assert.deepEqual(week.kinds.map((k) => [k.name, k.calls]), [['answer', 3], ['margin_read', 2]]);
  assert.equal(week.bars.length, 7);
  assert.equal(week.bars[6].to, '2026-11-07');
  assert.deepEqual(week.bars[6].by, [['big', 1040]]);
  const month = C.usagePeriod(USAGE_DAYS, 30, '2026-11-07');
  assert.equal(month.bars.length, 30);
  assert.ok(!month.models.some((m) => m.name === 'old'));
  const quarter = C.usagePeriod(USAGE_DAYS, 90, '2026-11-07');
  assert.equal(quarter.bars.length, 13);
  assert.equal(quarter.bars[12].to, '2026-11-07');
  assert.equal(quarter.bars[12].from, '2026-11-01');
  assert.equal(quarter.bars[0].from, '2026-08-10');
  assert.ok(quarter.models.some((m) => m.name === 'old'));
});

test('a 24 hour period has one bar per hour and crosses midnight', () => {
  const hours = { models: [
    { day: '2026-11-07 01', name: 'big', calls: 2, input: 100, cached: 50, output: 10 },
    { day: '2026-11-06 03', name: 'big', calls: 1, input: 10, cached: 0, output: 1 },
    { day: '2026-11-06 01', name: 'gone', calls: 9, input: 9, cached: 0, output: 9 },
  ], kinds: [{ day: '2026-11-07 01', name: 'answer', calls: 2 }] };
  const day = C.usagePeriod(hours, 1, '2026-11-07 02');
  assert.equal(day.length, 1);
  assert.equal(day.bars.length, 24);
  assert.equal(day.bars[0].from, '2026-11-06 03');
  assert.equal(day.bars[23].to, '2026-11-07 02');
  assert.equal(day.bars[22].calls, 2);
  assert.deepEqual(day.totals, { calls: 3, input: 110, cached: 50, output: 11, share: 50 / 110 });
  assert.deepEqual(day.kinds.map((k) => [k.name, k.calls]), [['answer', 2]]);
  assert.deepEqual(C.usagePeriod(hours, 1, '2026-11-07').bars, []);
});

test('an empty or unreadable usage period is empty and never throws', () => {
  for (const days of [null, undefined, {}, { models: 'x', kinds: 5 }, { models: [], kinds: [] }]) {
    const none = C.usagePeriod(days, 30, '2026-11-07');
    assert.equal(none.totals.calls, 0);
    assert.equal(none.totals.share, 0);
    assert.deepEqual(none.models, []);
    assert.deepEqual(none.kinds, []);
  }
  assert.deepEqual(C.usagePeriod(USAGE_DAYS, 30, 'garbage').bars, []);
  assert.equal(C.usagePeriod(USAGE_DAYS, 12, '2026-11-07').length, 30);
});

test('colours go to the models in use this week first, and the rest fold into Other', () => {
  const days = { models: [
    { day: '2026-11-07', name: 'now', calls: 1, input: 10, cached: 0, output: 0 },
    { day: '2026-10-01', name: 'then', calls: 1, input: 9000, cached: 0, output: 0 },
    { day: '2026-10-02', name: 'more', calls: 1, input: 500, cached: 0, output: 0 },
    { day: '2026-10-03', name: 'least', calls: 1, input: 1, cached: 0, output: 0 },
  ], kinds: [] };
  const ranked = C.usageRank(days, '2026-11-07');
  assert.deepEqual(ranked.models, ['now', 'then', 'more', 'least']);
  const items = C.usagePeriod(days, 90, '2026-11-07').models.map((m) => ({ name: m.name, value: m.tokens, calls: m.calls }));
  const folded = C.usageFold(items, ranked.models.slice(0, 3));
  assert.deepEqual(folded.map((s) => [s.name, s.rank, s.value]), [['then', 1, 9000], ['more', 2, 500], ['now', 0, 10], ['Other', -1, 1]]);
  assert.equal(folded[3].calls, 1);
  assert.deepEqual(C.usageFold([{ name: 'x', value: 0 }], ['x']), []);
});

test('usage kinds keep counts as numbers and refuse arbitrary labels', () => {
  const rows = C.usageKinds([
    { purpose: 'answer', calls: 3, input: 12000, cached: 10000, output: 400, median_ms: 3200, p95_ms: 5000 },
    { purpose: 'retry', calls: 1, failures: 1, unreported: 1, median_ms: 200, p95_ms: 200 },
    { purpose: '<script>', calls: 1 },
    { purpose: 'summary', calls: NaN, estimated: 2, failures: -3 },
  ]);
  assert.equal(rows.length, 3);
  assert.deepEqual(rows[0], { purpose: 'answer', label: 'Answers', calls: 3, failures: 0, unreported: 0, estimated: 0, median_ms: 3200, p95_ms: 5000 });
  assert.equal(rows[1].failures, 1);
  assert.equal(rows[1].unreported, 1);
  assert.deepEqual([rows[2].calls, rows[2].failures, rows[2].estimated], [0, 0, 2]);
  assert.deepEqual(C.usageKinds(null), []);
});

test('a confirmed verdict overrides a conflicting hidden marker without changing the explanation', async () => {
  for (const verdict of ['correct', 'incorrect', 'ungraded']) {
    const previous = verdict === 'correct' ? 'incorrect' : 'correct';
    const fixture = chatSendFixture('practice', 'ask', undefined, { events: [{ t: `[[${previous}]] Review the queue invariant.` }, { verdict }, { done: true }] });
    const quiz = { kind: 'free_response', question: 'What leaves first?', answer: 'The oldest item.', correct: null };
    fixture.state.messages.push({ role: 'user', text: 'Practise queues.' }, { role: 'assistant', text: '', quizzes: [quiz] });
    await fixture.send('The oldest item.', { card: quiz, say: C.quizSay(quiz, 'The oldest item.', null) });
    assert.equal(quiz.correct, verdict === 'ungraded' ? null : verdict === 'correct');
    assert.equal(C.grade(quiz.reply.text).verdict, verdict === 'ungraded' ? null : verdict);
    assert.equal(C.grade(quiz.reply.text).text.trim(), 'Review the queue invariant.');
  }
});

test('an answer Felix marks right counts toward its idea only when it was neither a guess nor one of more parts', async () => {
  for (const [extra, right] of [[{ sure: 'guess' }, 0], [{ more: true }, 0], [{ sure: 'sure' }, 1]]) {
    const session = C.startSession(['Queue order', 'Visited nodes']);
    const fixture = chatSendFixture('practice', 'ask', undefined, { session, events: [{ t: '[[correct]] The oldest item leaves first.' }, { done: true }] });
    const quiz = { kind: 'free_response', question: 'What leaves first?', answer: 'The oldest item.', correct: null, idea: 1, ...extra };
    fixture.state.messages.push({ role: 'user', text: 'Practise queues.' }, { role: 'assistant', text: '', quizzes: [quiz] });
    await fixture.send('The oldest item.', { card: quiz, say: C.quizSay(quiz, 'The oldest item.', null) });
    assert.equal(quiz.correct, true);
    assert.equal(session.outcomes[0].right, right, JSON.stringify(extra));
  }
});

test('a question on the idea just missed after Next question still keeps the idea open', async () => {
  for (const [correct, more] of [[false, true], [true, undefined]]) {
    const fixture = chatSendFixture('practice', 'ask', undefined, { events: [{ quiz: { kind: 'free_response', question: 'And now?', answer: 'x', idea: 1 } }, { done: true }] });
    fixture.state.messages.push({ role: 'user', text: 'Practise queues.' }, { role: 'assistant', text: '', quizzes: [{ kind: 'multiple_choice', question: 'What leaves first?', options: ['oldest', 'newest'], answer: 'oldest', idea: 1, given: 'newest', correct }] });
    await fixture.send('Next question, please.');
    assert.equal(fixture.state.messages.at(-1).quizzes[0].more, more, String(correct));
  }
});

test('a question on the idea just missed comes right after the explanation, so it keeps the idea open', async () => {
  for (const [verdict, idea, more] of [['incorrect', 1, true], ['incorrect', 2, undefined], ['correct', 1, undefined]]) {
    const fixture = chatSendFixture('practice', 'ask', undefined, { events: [{ t: `[[${verdict}]] The oldest item leaves first.` }, { quiz: { kind: 'free_response', question: 'And now?', answer: 'x', idea } }, { done: true }] });
    const quiz = { kind: 'free_response', question: 'What leaves first?', answer: 'The oldest item.', correct: null, idea: 1, given: 'The newest item.' };
    fixture.state.messages.push({ role: 'user', text: 'Practise queues.' }, { role: 'assistant', text: '', quizzes: [quiz] });
    await fixture.send('The newest item.', { card: quiz, say: C.quizSay(quiz, 'The newest item.', null) });
    assert.equal(fixture.state.messages.at(-1).quizzes[0].more, more, `${verdict} ${idea}`);
  }
});

test('a quiz answer leo already marked tells Felix not to repeat it, and how to go on', () => {
  const right = C.heldSay({ question: 'What does BFS use?', given: 'a queue', correct: true, answer: 'a queue', kind: 'multiple_choice' }, false);
  assert.match(right, /leo marked it right/);
  assert.match(right, /do not repeat or re-mark it/);
  assert.match(right, /Ask the next question with the quiz tool/);
  const wrong = C.heldSay({ question: 'Why?', given: 'because', correct: false, answer: 'a queue', feedback: 'Close.', kind: 'free_response' }, true);
  assert.match(wrong, /wrong; the right answer is a queue/);
  assert.match(wrong, /explanation and feedback/);
  assert.match(wrong, /finish it now with end_session/);
});

test('a prediction is an attempt before teaching, so it is never marked', async () => {
  const quiz = { kind: 'free_response', predict: true, question: 'What has to change?', answer: 'The pointer.', correct: null };
  const say = C.quizSay(quiz, 'the index', null);
  assert.match(say, /My prediction: the index/);
  assert.match(say, /do not mark it/);
  assert.doesNotMatch(say, /\[\[correct\]\]/);
  assert.match(C.historyText({ role: 'assistant', text: 'Try this.', quizzes: [quiz] }), /\[Prediction question/);

  const fixture = chatSendFixture('practice', 'ask', undefined, { events: [{ t: '[[correct]] You said the index; it is the pointer.' }, { quiz: { kind: 'free_response', question: 'Check', answer: 'x' } }, { done: true }] });
  quiz.given = 'the index';
  fixture.state.messages.push({ role: 'assistant', text: '', quizzes: [quiz] });
  await fixture.send(quiz.given, { card: quiz, say });
  assert.equal(fixture.requests[0].mark, false);
  assert.equal(fixture.requests[0].practice, true);
  assert.equal(quiz.correct, null);
  assert.equal(fixture.state.messages.at(-1).quizzes[0].question, 'Check');
});

test('how sure the user felt and their thinking reach Felix on the answer line', () => {
  const mc = { kind: 'multiple_choice', question: 'Which?', options: ['a', 'b'], answer: 'a', given: 'b', correct: false, sure: 'guess', thinking: 'first\nsecond' };
  for (const said of [C.quizSay(mc, 'b', false), C.heldSay(mc, false)]) assert.match(said, /My answer: b \(how sure I felt: guessing\) My thinking: first second/);
  assert.match(C.heldSay(mc, false).split('\n').slice(0, 3).join('\n'), /first second/, 'a held result keeps them when it is folded into the next message');
  assert.match(C.quizSay({ ...mc, sure: null, thinking: undefined }, 'b', false), /My answer: b\n/);
  assert.deepEqual(Object.keys(C.SURE), ['sure', 'think', 'guess']);
  assert.match(C.quizSay({ ...mc, surePct: 35, thinking: undefined }, 'b', false), /My answer: b \(how sure I felt: 35%\)\n/);
  assert.deepEqual([0, 39, 40, 74, 75, 100].map(C.sureOf), ['guess', 'guess', 'think', 'think', 'sure', 'sure']);
});

test('a streamed list marker waits until it has text', () => {
  assert.equal(C.held('Intro\n-'), 'Intro');
  assert.equal(C.held('Intro\n- '), 'Intro');
  assert.equal(C.held('Intro\n  *'), 'Intro');
  assert.equal(C.held('Steps\n1.'), 'Steps');
  assert.equal(C.held('Steps\n2)'), 'Steps');
  assert.equal(C.held('-'), '');
  assert.equal(C.held('Intro\n- first'), 'Intro\n- first');
  assert.equal(C.held('Pay 5 -'), 'Pay 5 -');
  assert.equal(C.held('Intro\n- first\n-'), 'Intro\n- first');
  assert.equal(C.held('Apple\nPear\n\n3'), 'Apple\nPear\n');
  assert.equal(C.held('Apple\nPear\n\n12'), 'Apple\nPear\n');
  assert.equal(C.held('Steps\nc'), 'Steps');
  assert.equal(C.held('Steps\nbb'), 'Steps');
  assert.equal(C.held('Steps\nA)'), 'Steps');
  assert.equal(C.held('Steps\nok'), 'Steps\nok');
  assert.equal(C.held('Steps\nApple'), 'Steps\nApple');
  assert.equal(C.held('Steps\n3 apples'), 'Steps\n3 apples');
  assert.equal(C.held('Pay 5'), 'Pay 5');
});

function answers(messages, { busy = null, send = () => {}, clipboard = null, fetchOk = true } = {}) {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8');
  const cut = (from, to) => source.slice(source.indexOf(from), source.indexOf(to, source.indexOf(from)));
  const code = `${cut('    function bubble(m, i) {', '    function onNote()')}\n${cut('    function lastPair()', '    async function startWith(')}`;
  const calls = [];
  const input = { value: '', focus() { calls.push('focus'); } };
  const context = {
    ...C, marks: new Map(), state: { messages, busy, preparingSend: false, id: 'chat-12345678', refs: [], sent: [], files: [], selection: null }, drawRefs() {}, drawings: {}, escape: esc, render: (t) => `<p>${esc(t)}</p>`, input,
    root: { navigator: { clipboard } }, upload: Promise.resolve(), openTraces: new Set(), working: new Map(), timed: new Set(), tick() {}, workingFor: () => '', tookFor: () => '', writer: () => '', writerOf: () => '',
    stepChevron: '', MOST_REPLY_STEPS: 64, clipped: (t) => t, stored: () => messages.filter((m) => !m.pending), fit() {}, persist() {}, draw() {}, send,
    notify: (text) => calls.push(`notify:${text}`), setTimeout: () => 0,
    request: async (url, init) => { calls.push(['fetch', url, JSON.parse(init.body)]); return { ok: fetchOk }; },
  };
  vm.createContext(context);
  for (let tries = 0; tries < 40; tries++) {
    try { vm.runInContext(`${code}\n;globalThis.__bubble = bubble; globalThis.__fns = { copyAnswer, takeBack, retryAnswer, editQuestion, lastPair };`, context); break; }
    catch (e) { if (!(e instanceof ReferenceError)) throw e; context[e.message.split(' ')[0]] = () => ''; }
  }
  return { context, calls, input, html: () => messages.map((m, i) => context.__bubble(m, i)), ...context.__fns };
}

function deciding(messages) {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8');
  const cut = (from, to) => source.slice(source.indexOf(from), source.indexOf(to, source.indexOf(from)));
  const code = `${cut('    const inflight', '    async function undo(')}\n${cut('    async function saveAnswer(', '    function lastPair()')}`;
  const calls = [];
  const context = {
    asNote: C.asNote, retiredPractice: C.retiredPractice, state: { messages, context: null }, notify() {}, remember() {}, draw() {}, onSaved() {}, onChanged() {}, onOrganized() {},
    request: async (url) => { calls.push(url); await new Promise((r) => setImmediate(r)); return { ok: true, json: async () => ({ id: 'made', version: 'v' }) }; },
  };
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.__fns = { decide, saveAnswer };`, context);
  return { calls, ...context.__fns };
}

test('a double click on Create, Do it or Save as note sends one request', async () => {
  const messages = [...pair(), { role: 'assistant', text: 'ok', proposals: [{ kind: 'create', title: 'T', state: 'new' }, { kind: 'move_note', state: 'new' }] }];
  const box = deciding(messages);
  await Promise.all([box.decide(2, 0, true), box.decide(2, 0, true)]);
  assert.deepEqual(box.calls, ['/api/notes']);
  assert.equal(messages[2].proposals[0].state, 'applied');
  await Promise.all([box.decide(2, 1, true), box.decide(2, 1, true)]);
  assert.deepEqual(box.calls.slice(1), ['/api/felix/act']);
  await Promise.all([box.saveAnswer(1), box.saveAnswer(1)]);
  assert.equal(box.calls.length, 3);
  assert.equal(messages[1].saved, 'made');
});

test('a chat save that fails is reported once and sends only the messages leo keeps', async () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8');
  const from = source.indexOf('    let upload = Promise.resolve();');
  const code = source.slice(from, source.indexOf('    async function loadChats()', from));
  const sent = [];
  const told = [];
  const messages = Array.from({ length: 450 }, (_, i) => ({ role: 'user', text: `m${i}` }));
  const context = {
    marks: new Map(), markOf: C.markOf, toSave: C.toSave, savedAs: C.savedAs, state: { id: 'chat-1', refs: [], messages, chats: null }, stored: () => messages, persist() {}, known: new Set(), gone: new Set(), drawChats() {}, loadChats() {}, setTimeout: () => 0,
    NAME_CHECKS: [], notify: (t) => told.push(t),
    request: async (url, init) => { sent.push(JSON.parse(init.body)); return { ok: false }; },
  };
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.__remember = remember;`, context);
  await context.__remember();
  await context.__remember();
  assert.equal(sent.length, 2);
  assert.equal(sent[0].messages.length, 450);
  assert.equal(sent[0].messages[0].text, 'm0');
  assert.equal(told.length, 1);
});

test('a history save that cannot reach leo warns once', async () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8');
  const from = source.indexOf('    let upload = Promise.resolve();');
  const code = source.slice(from, source.indexOf('    async function loadChats()', from));
  const told = [];
  let reachable = false;
  const messages = [{ role: 'user', text: 'hi' }, { role: 'assistant', text: 'hello' }];
  const context = {
    marks: new Map(), markOf: C.markOf, toSave: C.toSave, savedAs: C.savedAs, state: { id: 'chat-1', refs: [], messages, chats: null }, stored: () => messages, persist() {}, known: new Set(), gone: new Set(), drawChats() {}, loadChats() {}, setTimeout: () => 0,
    NAME_CHECKS: [], notify: (t) => told.push(t),
    request: async () => {
      if (!reachable) throw new TypeError('Failed to fetch');
      return { ok: true, json: async () => ({ id: 'chat-1', named: true, count: 2, updated_at: '2026-11-17T00:00:00Z' }) };
    },
  };
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.__remember = remember;`, context);
  await context.__remember();
  await context.__remember();
  assert.equal(told.length, 1);
  reachable = true;
  await context.__remember();
  reachable = false;
  await context.__remember();
  assert.equal(told.length, 2);
});

const pair = () => [{ role: 'user', text: 'What is a heap?' }, { role: 'assistant', text: '[n1] A heap keeps the minimum on top.', sources: [{ n: 1, id: 'a', title: 'Heaps' }] }];

test('every answer offers Copy, the last one Retry, and the last question Edit', () => {
  const { html } = answers([...pair(), ...pair()]);
  const [q1, a1, q2, a2] = html();
  assert.doesNotMatch(q1, /data-chat="edit"/);
  assert.match(q2, /data-chat="edit"/);
  assert.match(a1, /data-chat="copy"/);
  assert.doesNotMatch(a1, /data-chat="retry"/);
  assert.match(a2, /data-chat="copy"[^>]*>Copy<\/button><button class="msg-act" data-chat="retry">Retry<\/button>/);
  assert.doesNotMatch(a2, /disabled/);
});

test('Copy, Retry and Edit are disabled while Felix is working', () => {
  const [q, a] = answers(pair(), { busy: {} }).html();
  assert.match(q, /data-chat="edit" disabled/);
  assert.match(a, /data-chat="copy" data-i="1" disabled/);
  assert.match(a, /data-chat="retry" disabled/);
  const pending = answers([...pair(), { role: 'user', text: 'More' }, { role: 'assistant', text: '', pending: true }]).html();
  assert.doesNotMatch(pending.join(''), /data-chat="retry"|data-chat="edit"/);
});

test('a writer failure shows the sentence in Felix\u2019s voice with a Set up writing AI button, and other errors keep their look', () => {
  const sentence = "leo can't write yet: no writing AI is ready (Ollama isn't running).";
  const [, said, , other] = answers([...pair(), { role: 'user', text: 'Hi' }, { role: 'assistant', text: '', error: 'Felix finished without an answer. Try again.' }]).html();
  assert.doesNotMatch(said, /Set up writing AI/);
  assert.match(other, /<div class="msg-error">Felix finished without an answer\. Try again\.<\/div>/);
  assert.doesNotMatch(other, /Set up writing AI/);
  const [, failed] = answers([{ role: 'user', text: 'Quiz me' }, { role: 'assistant', text: '', error: sentence }]).html();
  assert.match(failed, /<div class="prose"><p>leo can&#39;t write yet|<div class="prose"><p>leo can't write yet/);
  assert.match(failed, /<a class="btn primary sm" href="#\/settings\/writing">Set up writing AI<\/a>/);
  assert.doesNotMatch(failed, /msg-error/);
  const drawn = answers([{ role: 'user', text: 'Quiz me' }, { role: 'assistant', text: '', error: sentence }]);
  drawn.context.drawings = { noink: '<svg class="drawn drawn-noink"></svg>' };
  assert.match(drawn.html()[1], /<svg class="drawn drawn-noink"><\/svg><div class="prose"><p>leo can/);
  const plain = answers([{ role: 'user', text: 'Hi' }, { role: 'assistant', text: '', error: 'Felix finished without an answer. Try again.' }]);
  plain.context.drawings = drawn.context.drawings;
  assert.doesNotMatch(plain.html()[1], /drawn-noink/);
  assert.equal(C.noAi(sentence), true);
  assert.equal(C.noAi('Stopped.'), false);
  assert.equal(C.noAi(undefined), false);
});

test('a failed upload shows the writer sentence with the setup link, and other errors stay as they were', async () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const sentence = "leo can't write yet: no writing AI is chosen.";
  for (const [error, link] of [[sentence, true], ['The file is empty.', false]]) {
    const open = { innerHTML: '' };
    const context = {
      api: async () => ({ state: 'failed', error }), enc: encodeURIComponent, esc, noAi: C.noAi, felix: { felix: () => '' },
      $: (sel) => (sel === '.upload-working' ? open : null), setTimeout: (fn) => { fn(); return 1; }, clearTimeout() {},
    };
    vm.createContext(context);
    const sheets = fs.readFileSync(path.join(__dirname, '../src/web/app/sheets.js'), 'utf8').replace(/\r\n/g, '\n');
    const footer = sheets.indexOf('function sheetButtons(');
    vm.runInContext(sheets.slice(footer, sheets.indexOf('\n}\n', footer) + 3), context);
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/uploads.js'), 'utf8'), context);
    context.watchUpload('job', '');
    await new Promise(setImmediate);
    assert.ok(open.innerHTML.includes(`<p class="upload-error">${esc(error)}</p>`));
    assert.equal(open.innerHTML.includes('<a class="btn plain" href="#/settings/writing">Set up writing AI</a>'), link);
  }
});

test('Copy writes the answer as Markdown without citation marks or the grading marker', async () => {
  let written = null;
  const box = answers(pair(), { clipboard: { writeText: async (t) => { written = t; } } });
  const button = { textContent: 'Copy' };
  await box.copyAnswer(1, button);
  assert.equal(written, C.linkCitations(C.grade(pair()[1].text).text, pair()[1].sources).trim());
  assert.equal(button.textContent, 'Copied');
  const busy = answers(pair(), { busy: {}, clipboard: { writeText: async () => { throw new Error('no'); } } });
  await busy.copyAnswer(1, button);
  assert.equal(busy.calls.length, 0, 'nothing happens while streaming');
  const refused = answers(pair(), { clipboard: { writeText: async () => { throw new Error('Denied'); } } });
  await refused.copyAnswer(1, { textContent: 'Copy' });
  assert.deepEqual(refused.calls, ['notify:Could not copy: Denied']);
});

test('Retry drops the last pair on the server and sends the question again', async () => {
  const sent = [];
  const messages = [...pair(), ...pair()];
  const box = answers(messages, { send: (t) => sent.push(t) });
  await box.retryAnswer();
  assert.deepEqual(box.calls, [['fetch', '/api/chats/chat-12345678/drop', { count: 2, total: 4 }]]);
  assert.equal(messages.length, 2);
  assert.deepEqual(sent, ['What is a heap?']);
});

test('Edit puts the question back in the composer and drops the pair', async () => {
  const messages = pair();
  const box = answers(messages);
  box.input.value = 'draft';
  await box.editQuestion();
  assert.equal(messages.length, 0);
  assert.equal(box.input.value, 'What is a heap?\n\ndraft');
  assert.deepEqual(box.calls.at(-1), 'focus');
});

const withAttachments = () => [{
  role: 'user', text: 'Compare these', refs: [{ id: 'n1', title: 'Heaps' }], selection: { note: 'n2', title: 'Trees', from: 1, to: 2, text: 'a tree' },
  files: ['d1'], docs: ['Notes.pdf'], cards: [{ name: 'Notes.pdf', excerpt: 'Lecture one' }],
}, pair()[1]];

test('Retry and Edit keep a question\u2019s notes, passage and documents', async () => {
  for (const [name, again] of [['retryAnswer', true], ['editQuestion', false]]) {
    const sent = [], messages = withAttachments();
    const box = answers(messages, { send: (t) => sent.push(t) });
    assert.ok(box.context.state.sent.push({ id: 'd1', name: 'Notes.pdf' }) && box.lastPair() === 0);
    box.context.state.refs = [{ id: 'n3', title: 'Other' }];
    await box[name]();
    const { state } = box.context;
    assert.deepEqual(state.refs.map((r) => r.id), ['n1', 'n3']);
    assert.equal(state.selection.note, 'n2');
    assert.equal(state.sent.length, 0);
    assert.deepEqual(JSON.parse(JSON.stringify(state.files)), [{ name: 'Notes.pdf', excerpt: 'Lecture one', id: 'd1', status: 'ready' }]);
    assert.deepEqual(sent, again ? ['Compare these'] : []);
    assert.equal(box.input.value, again ? '' : 'Compare these');
  }
});

test('a quiz, next or steer question still has no Retry or Edit', () => {
  for (const extra of [{ quiz: {} }, { next: true }, { steer: true }]) {
    assert.equal(answers([{ ...pair()[0], ...extra }, pair()[1]]).lastPair(), -1);
  }
});

test('a refused or busy take-back keeps the chat as it is', async () => {
  const sent = [];
  const messages = pair();
  const refused = answers(messages, { fetchOk: false, send: (t) => sent.push(t) });
  await refused.retryAnswer();
  assert.equal(messages.length, 2);
  assert.deepEqual(sent, []);
  assert.match(refused.calls.at(-1), /^notify:/);
  const busy = answers(messages, { busy: {}, send: (t) => sent.push(t) });
  await busy.editQuestion();
  await busy.retryAnswer();
  assert.deepEqual(busy.calls, []);
  assert.equal(messages.length, 2);
});

test('a step Felix got wrong and then fixed with the same tool is not shown', () => {
  const tried = { text: 'Tried to ask you something', tool: 'quiz', failed: true };
  const asked = { text: 'Asked you a practice question', tool: 'quiz', failed: false };
  const read = { text: 'Opened “Heaps”', tool: 'open_note', failed: false };
  assert.deepEqual(C.fixedSteps([tried, read, asked]), [read, asked]);
  assert.deepEqual(C.fixedSteps([read, tried]), [read, tried], 'a failure Felix never fixed stays visible');
  assert.deepEqual(C.fixedSteps([asked, tried]), [asked, tried], 'only a later success hides it');
  const choice = { text: 'Asked you a question', tool: 'ask_user', failed: false };
  assert.deepEqual(C.fixedSteps([tried, read, choice]), [read, choice], 'a question card fixes a failed quiz');
  const lost = { text: 'Retried ask user: "question" is empty', tool: 'ask_user', failed: true };
  assert.deepEqual(C.fixedSteps([read, lost]), [read, lost], 'while Felix works the step keeps its words');
  const said = "Felix couldn't make that question. Ask again.";
  assert.deepEqual(C.fixedSteps([read, lost, tried], true).map((s) => s.text), [read.text, said, said]);
  const broken = { text: 'Retried open note: no such note', tool: 'open_note', failed: true };
  assert.deepEqual(C.fixedSteps([broken], true), [broken], 'other failures keep their words');
});

test('an edit card shows proof marks, falls back to both texts when marks would hide a change, and marks an addition at the end', () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8');
  const marks = source.slice(source.indexOf('  const DELETE_MARK'), source.indexOf('\n', source.indexOf('  const CARET_MARK')));
  const from = source.indexOf('    const proofMarks = new WeakMap();');
  const code = source.slice(from, source.indexOf('\n    }\n', source.indexOf('    function proposalCard(', from)) + 7);
  const escape = (t) => String(t).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const context = { escape, render: escape, retiredPractice: () => false, state: { messages: [{ role: 'assistant' }] }, root: { leoMarkdown: require('../src/web/markdown.js') } };
  vm.createContext(context);
  vm.runInContext(`${marks}\n${code}\n;globalThis.card = proposalCard;`, context);
  const marked = context.card({ kind: 'edit', title: 'Heaps', find: 'the newest one', replace: 'the oldest one', state: 'new' }, 0, 0);
  assert.match(marked, /class="proposal-marked">the <del>newest<svg class="proposal-loop"/);
  assert.match(marked, /<ins><svg class="proposal-caret"[^]*?<\/svg>oldest<\/ins> one<\/div>/);
  assert.doesNotMatch(marked, /proposal-old|proposal-new/);
  const spacing = context.card({ kind: 'edit', title: 'Heaps', find: 'one\ntwo', replace: 'one two', state: 'new' }, 0, 0);
  assert.match(spacing, /<div class="proposal-old">one\ntwo<\/div><div class="proposal-new">one two<\/div>/);
  assert.doesNotMatch(spacing, /proposal-marked/);
  const added = context.card({ kind: 'edit', title: 'Heaps', find: '', replace: 'Ask about it.', state: 'new' }, 0, 0);
  assert.match(added, /<div class="proposal-note">Added at the end:<\/div><div class="proposal-marked"><ins><svg class="proposal-caret"/);
  const same = { kind: 'edit', title: 'Heaps', find: 'a b', replace: 'a c', state: 'new' };
  assert.equal(context.card(same, 0, 0), context.card(same, 0, 0), 'a redraw reuses the marks');
});

test('a quiz keeps the full thinking draft and tells the user how much Felix reads', async () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/chat.js'), 'utf8');
  const slice = (from, to) => source.slice(source.indexOf(from), source.indexOf(to));
  const start = '💭'.repeat(2000);
  const thinking = start + 'THINKING_TAIL';
  const q = { kind: 'free_response', question: 'Why?', answer: 'Order' };
  const sent = [];
  const context = require('node:vm').createContext({
    ...C, MOST_THINKING: 2000, SURE: {}, escape: esc,
    state: { messages: [{ quizzes: [q] }], drafts: { 'think:0:0': thinking }, streak: 0 },
    draw() {}, remember() {}, mood() {}, currentSession: () => null, scoreSession() {},
    request: async (_, { body }) => {
      sent.push(JSON.parse(body));
      return { ok: true, json: async () => ({ correct: true }) };
    },
  });
  require('node:vm').runInContext(
    slice('    function thinkingBox(', '    function quizCard(') +
    slice('    const checking = new WeakSet()', '    function continueQuiz(') +
    '\nthis.box = thinkingBox; this.answer = answerQuiz; thinkingOpen.add(state.messages[0].quizzes[0]);',
    context);
  await context.answer(0, 0, 'Order');
  assert.equal(sent[0].thinking, thinking);
  assert.equal(q.thinking, thinking);
  const box = context.box('data-i="0" data-k="0"', thinking);
  assert.doesNotMatch(box, /maxlength/);
  assert.match(box, /Felix reads the first 2,000 characters/);
  const prompt = C.quizSay(q, q.given, true);
  assert.ok(prompt.includes(start));
  assert.ok(!prompt.includes('THINKING_TAIL'));
});

test('quiz answer fields and the note picker have accessible names', () => {
  const source = require('node:fs').readFileSync(require('node:path').join(__dirname, '../src/web/chat.js'), 'utf8');
  assert.match(source, /id="chat-pick-search" placeholder="Find a note…" aria-label="Find a note"/);
  const code = source.slice(source.indexOf('    function quizCard('), source.indexOf('    const checking = new WeakSet()'));
  const context = { ...C, escape: esc, state: { drafts: {} }, checking: new WeakSet(), thinkingOpen: new WeakSet(), markedAt: new WeakMap(), inline: (t) => esc(t), render: (t) => esc(t), cite: (t) => t, hintsOf: () => [] };
  const vm = require('node:vm');
  vm.createContext(context);
  for (let tries = 0; tries < 40; tries++) {
    try { vm.runInContext(`${code}\n;globalThis.__card = quizCard;`, context); break; }
    catch (e) { if (!(e instanceof ReferenceError)) throw e; context[e.message.split(' ')[0]] = () => ''; }
  }
  for (const [kind, name] of [['fill_blank', 'The missing words'], ['free_response', 'Your answer, in your own words']]) {
    let html;
    for (let tries = 0; tries < 40 && html === undefined; tries++) {
      try { html = context.__card({ kind, question: 'Why?', answer: 'x' }, 0, 0, false, []); }
      catch (e) { if (e.name !== 'ReferenceError') throw e; context[e.message.split(' ')[0]] = ''; }
    }
    assert.match(html, new RegExp(`<(input|textarea) class="quiz-input"[^>]* aria-label="${name}"`));
  }
});

test("a refused reply's sentence is read one way at the three sites", async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const line = source.split('\n').find((l) => l.trim().startsWith('const sentenceOf = async'));
  const context = vm.createContext({});
  vm.runInContext(`${line}\nthis.sentenceOf = sentenceOf;`, context);
  assert.equal(await context.sentenceOf({ json: async () => ({ error: 'That note is gone.' }) }), 'That note is gone.');
  assert.equal(await context.sentenceOf({ json: async () => { throw new SyntaxError('not JSON'); } }), '');
  assert.equal(await context.sentenceOf({ json: async () => ({}) }), '');
  assert.equal(source.includes('said = (await'), false);
  assert.equal((source.match(/const said = await sentenceOf\(/g) || []).length, 3);
});

function goneFixture({ id = 'chat-1', synced = true, reply = async () => ({ ok: true, status: 200 }), messages = [{ role: 'user', text: 'Explain heaps' }, { role: 'assistant', text: 'A heap is a tree.' }], typed = '' } = {}) {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const from = source.indexOf('    let upload = Promise.resolve();');
  const saving = source.slice(from, source.indexOf('    async function loadChats()', from));
  const at = source.indexOf('    async function checkStored() {');
  const checking = source.slice(at, source.indexOf('    async function resume(', at));
  const calls = [], told = [], begun = [];
  const state = { id, refs: [], messages: messages.slice(), chats: null, busy: null, preparingSend: false };
  const input = { value: typed };
  const context = vm.createContext({
    state, input, marks: new Map(), markOf: C.markOf, toSave: C.toSave, savedAs: C.savedAs, NAME_CHECKS: [], setTimeout: () => 0,
    known: new Set(synced ? [id] : []), gone: new Set(),
    stored: () => state.messages, persist() {}, drawChats() {}, loadChats() {}, fit() {},
    notify: (t) => told.push(t),
    begin: () => { begun.push(state.id); state.id = `fresh-${begun.length}`; state.messages = []; },
    request: async (url, init = {}) => { calls.push([init.method || 'GET', url, init.body ? JSON.parse(init.body) : null]); return reply(url, init, calls.length); },
  });
  vm.runInContext(`${saving}\n${checking}\nthis.remember = remember; this.checkStored = checkStored; this.freshIfGone = freshIfGone;`, context);
  return { context, state, input, calls, told, begun, known: context.known, gone: context.gone };
}

test('a stored chat leo no longer has starts a new chat', async () => {
  const f = goneFixture({ reply: async () => ({ ok: false, status: 404 }) });
  await f.context.checkStored();
  assert.deepEqual(f.begun, ['chat-1']);
  assert.equal(f.known.has('chat-1'), false);
  assert.equal(f.gone.has('chat-1'), true);
  assert.deepEqual(f.calls.map((c) => c.slice(0, 2)), [['GET', '/api/chats/chat-1']]);
});

test('a stored chat leo still has is kept', async () => {
  const f = goneFixture({ reply: async () => ({ ok: true, status: 200 }) });
  await f.context.checkStored();
  assert.deepEqual(f.begun, []);
  assert.equal(f.state.id, 'chat-1');
});

test('an unreachable leo, a 401 and a 500 keep the stored chat', async () => {
  for (const reply of [async () => { throw new TypeError('Failed to fetch'); }, async () => ({ ok: false, status: 401 }), async () => ({ ok: false, status: 500 })]) {
    const f = goneFixture({ reply });
    await f.context.checkStored();
    assert.deepEqual(f.begun, []);
    assert.equal(f.state.messages.length, 2);
  }
});

test('a stored chat that never reached leo is kept without asking', async () => {
  const f = goneFixture({ synced: false, reply: async () => ({ ok: false, status: 404 }) });
  await f.context.checkStored();
  assert.deepEqual(f.begun, []);
  assert.equal(f.calls.length, 0);
});

test('a chat that changed while the check ran is kept', async () => {
  let f;
  f = goneFixture({ reply: async () => { f.state.messages.push({ role: 'user', text: 'and again' }); return { ok: false, status: 404 }; } });
  await f.context.checkStored();
  assert.deepEqual(f.begun, []);
  f = goneFixture({ reply: async () => { f.state.busy = {}; return { ok: false, status: 404 }; } });
  await f.context.checkStored();
  assert.deepEqual(f.begun, []);
});

test('a save answered gone starts a new chat and puts the last message back in the box', async () => {
  const f = goneFixture({ reply: async () => ({ ok: false, status: 410 }) });
  await f.context.remember();
  assert.equal(f.calls[0][2].existed, true);
  assert.deepEqual(f.begun, ['chat-1']);
  assert.equal(f.input.value, 'Explain heaps');
  assert.equal(f.told.length, 1);
  assert.match(f.told[0], /Your last message is in the box/);
  assert.equal(f.known.has('chat-1'), false);
});

test('a save queued before a gone answer is not sent after it', async () => {
  const f = goneFixture({ reply: async () => ({ ok: false, status: 410 }) });
  f.state.busy = {};
  const first = f.context.remember();
  const second = f.context.remember();
  await first;
  await second;
  assert.equal(f.calls.length, 1);
  assert.deepEqual(f.begun, []);
});

test('a chat found gone while a reply runs starts fresh when the reply ends and the final save of the old id is not sent', async () => {
  const f = goneFixture({ reply: async () => ({ ok: false, status: 410 }) });
  f.state.busy = {};
  const old = { id: 'chat-1', refs: [], messages: f.state.messages.slice() };
  await f.context.remember(old);
  assert.deepEqual(f.begun, []);
  f.state.busy = null;
  f.context.freshIfGone();
  assert.deepEqual(f.begun, ['chat-1']);
  await f.context.remember({ ...old, messages: [...old.messages, { role: 'assistant', text: 'more' }] });
  assert.equal(f.calls.length, 1);
  assert.notEqual(f.state.id, 'chat-1');
  const source = require('node:fs').readFileSync(require.resolve('../src/web/chat.js'), 'utf8');
  assert.match(source, /state\.busy = null;\n\s+freshIfGone\(\);/);
});

test('a first save says leo did not have the chat, and a later one says it did', async () => {
  const f = goneFixture({ synced: false, reply: async () => ({ ok: true, status: 200, json: async () => ({ id: 'chat-1', named: true, count: 2 }) }) });
  await f.context.remember();
  await f.context.remember();
  assert.deepEqual(f.calls.map((c) => c[2].existed), [false, true]);
});

test('the saved flag round-trips', () => {
  const store = new Map();
  const storage = { getItem: (k) => store.get(k) ?? null, setItem: (k, v) => store.set(k, v) };
  C.save(storage, [{ role: 'user', text: 'hi' }], [], 'id', true);
  assert.equal(C.load(storage).synced, true);
  C.save(storage, [{ role: 'user', text: 'hi' }], [], 'id');
  assert.equal('synced' in C.load(storage), false);
  assert.equal(JSON.parse(store.get([...store.keys()][0])).synced, undefined);
});

test('the note picker asks for 20 results', async () => {
  const source = require('node:fs').readFileSync(require('node:path').join(__dirname, '../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const start = source.indexOf('    async function findNotes(query) {');
  const code = source.slice(start, source.indexOf('\n    }\n', start) + 6);
  const asked = [];
  const context = { state: { refs: [{ id: 'b' }] }, request: async (url) => { asked.push(url); return { ok: true, json: async () => [{ id: 'a' }, { id: 'b' }] }; } };
  const vm = require('node:vm');
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.find = findNotes;`, context);
  const found = await context.find(' heaps ');
  assert.deepEqual(asked, ['/api/search?q=heaps&limit=20']);
  assert.deepEqual(found.map((n) => n.id), ['a']);
});

test('an eleventh document is queued and no limit on documents is left in the page', () => {
  const source = require('node:fs').readFileSync(require('node:path').join(__dirname, '../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  assert.ok(!source.includes('MOST_FILES') && !source.includes('holds up to'));
  const start = source.indexOf('    function addFiles(list) {');
  const code = source.slice(start, source.indexOf('\n    }\n', start) + 6);
  const queued = [];
  const notices = [];
  const context = { state: { files: Array.from({ length: 10 }, (_, i) => ({ id: `f${i}` })), sent: [] }, addFile: (f) => queued.push(f), notify: (m) => notices.push(m) };
  const vm = require('node:vm');
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.add = addFiles;`, context);
  context.add(['eleventh.txt', 'twelfth.txt']);
  assert.deepEqual(queued, ['eleventh.txt', 'twelfth.txt']);
  assert.deepEqual(notices, []);
});

test('elapsed time is worded 42 s below a minute and 1:05 from a minute up', () => {
  assert.equal(C.tookFor(42000), '42 s');
  assert.equal(C.tookFor(65000), '1:05');
  assert.equal(C.tookFor(0), '0 s');
  assert.equal(C.tookFor(-3000), '0 s');
  const fs = require('node:fs');
  const path = require('node:path');
  for (const name of ['app/events.js', 'app/uploads.js']) {
    assert.ok(!fs.readFileSync(path.join(__dirname, '../src/web', name), 'utf8').includes("padStart(2, '0')"), `${name} words elapsed time itself`);
  }
});

function fakeChat(count) {
  let rows = Array.from({ length: count }, (_, i) => ({ role: i % 2 ? 'assistant' : 'user', text: `m${i}` }));
  const puts = [];
  return {
    puts,
    rows: () => rows,
    set: (next) => { rows = next; },
    put(body) {
      puts.push(body);
      if (body.from === undefined) {
        if (body.messages.length < rows.length) return { status: 409, count: rows.length };
        rows = body.messages.slice();
      } else {
        if (body.from > rows.length) return { status: 409, count: rows.length };
        rows = [...rows.slice(0, body.from), ...body.messages];
      }
      return { status: 200, count: rows.length };
    },
    page(before) {
      const to = Math.min(before ?? rows.length, rows.length);
      const from = Math.max(0, to - 200);
      return JSON.parse(JSON.stringify({ messages: rows.slice(from, to), from, count: rows.length }));
    },
  };
}

test('a save sends only the messages added since the last save', () => {
  const first = [{ role: 'user', text: 'q1' }, { role: 'assistant', text: 'a1' }];
  const mark = C.savedAs({ first: 0 }, first, 2);
  assert.deepEqual(C.toSave(mark, [...first, { role: 'user', text: 'q2' }]), { from: 2, messages: [{ role: 'user', text: 'q2' }] });
  assert.deepEqual(C.toSave({ first: 0, saved: 0, held: [] }, first), { from: 0, messages: first });
});

test('a long chat opened at its newest page keeps a new message', () => {
  const server = fakeChat(1000);
  const page = server.page();
  const mark = C.markOf(page);
  assert.deepEqual([mark.first, mark.saved], [800, 1000]);
  const plan = C.toSave(mark, [...page.messages, { role: 'user', text: 'one more' }]);
  assert.equal(plan.from, 1000);
  assert.equal(plan.messages.length, 1);
  assert.equal(server.put(plan).status, 200);
  const back = server.rows();
  assert.equal(back.length, 1001);
  assert.equal(back[1000].text, 'one more');
  assert.deepEqual(back.slice(0, 1000).map((m) => m.text), Array.from({ length: 1000 }, (_, i) => `m${i}`));
});

test('a message appended after loading an earlier page is saved at the right index', () => {
  const server = fakeChat(1000);
  const newest = server.page();
  let mark = C.markOf(newest);
  const older = server.page(mark.first);
  mark = { first: older.from, saved: mark.saved, held: [...older.messages.map((m) => JSON.stringify(m)), ...mark.held] };
  const local = [...older.messages, ...newest.messages, { role: 'user', text: 'after scrolling up' }];
  const plan = C.toSave(mark, local);
  assert.equal(plan.from, 1000);
  assert.deepEqual(plan.messages, [{ role: 'user', text: 'after scrolling up' }]);
  server.put(plan);
  assert.equal(server.rows()[1000].text, 'after scrolling up');
  assert.equal(server.rows().length, 1001);
});

test('an earlier message changed with the count and the last message unchanged is saved', () => {
  const server = fakeChat(1000);
  const page = server.page();
  const mark = C.markOf(page);
  const local = JSON.parse(JSON.stringify(page.messages));
  local[150].quiz = { given: 'a stack', correct: false };
  const plan = C.toSave(mark, local);
  assert.equal(plan.from, 950);
  assert.equal(plan.messages.length, 50);
  server.put(plan);
  assert.equal(server.rows().length, 1000);
  assert.equal(server.rows()[950].quiz.given, 'a stack');
  assert.equal(server.rows()[999].text, 'm999');
});

function savingSlice(server, state, noted) {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const start = source.indexOf('    let upload = Promise.resolve();');
  const code = source.slice(start, source.indexOf('\n    function freshIfGone() {', start));
  const body = { scrollTop: 0, scrollHeight: 0 };
  const request = async (url, init = {}) => {
    const before = /before=(\d+)/.exec(url);
    if (init.method === 'PUT') {
      const answer = server.put(JSON.parse(init.body));
      return { ok: answer.status === 200, status: answer.status, json: async () => ({ id: state.id, named: true, count: answer.count, updated_at: '2026-11-18T00:00:00Z' }) };
    }
    return { ok: true, status: 200, json: async () => server.page(before ? Number(before[1]) : undefined) };
  };
  const context = vm.createContext({
    state, body, request, encodeURIComponent, JSON, Number, Math, Array, Boolean, Promise, setTimeout,
    marks: new Map(), known: new Set([state.id]), gone: new Set(), unsaved: new Set(), naming: new Set(), NAME_CHECKS: [],
    markOf: C.markOf, toSave: C.toSave, savedAs: C.savedAs,
    stored: () => state.messages.filter((m) => !m.pending), persist() {}, drawChats() {}, loadChats() {}, freshIfGone() {}, notify: (m) => noted.push(m),
    draw() { body.scrollHeight = state.messages.length * 10; },
  });
  vm.runInContext(`${code}\nthis.remember = remember; this.earlier = earlier; this.marks = marks; this.upload = () => upload;`, context);
  return { context, body };
}

test('a refused save reloads and resends only the unsent messages, never a whole list', async () => {
  const server = fakeChat(1000);
  const page = server.page();
  const state = { id: 'chat-long-0001', messages: page.messages, refs: [], busy: null };
  const noted = [];
  const { context } = savingSlice(server, state, noted);
  context.marks.set(state.id, C.markOf(page));
  server.set(server.rows().slice(0, 998));
  state.messages = [...state.messages, { role: 'user', text: 'mine' }];
  await context.remember();
  assert.ok(server.puts.every((p) => Number.isInteger(p.from)), 'no whole list is ever sent');
  assert.deepEqual(server.puts.map((p) => [p.from, p.messages.length]), [[1000, 1], [998, 1]]);
  assert.equal(server.rows().length, 999);
  assert.equal(server.rows()[998].text, 'mine');
  assert.equal(state.messages.at(-1).text, 'mine');
  assert.equal(state.messages.length, 201);
  assert.deepEqual([context.marks.get(state.id).first, context.marks.get(state.id).saved], [798, 999]);
  assert.deepEqual(noted, []);
});

test('scrolling to the top loads the page before', async () => {
  const server = fakeChat(1000);
  const page = server.page();
  const state = { id: 'chat-long-0002', messages: page.messages, refs: [], busy: null };
  const { context, body } = savingSlice(server, state, []);
  context.marks.set(state.id, C.markOf(page));
  body.scrollHeight = 2000;
  body.scrollTop = 30;
  await context.earlier();
  assert.equal(state.messages.length, 400);
  assert.equal(state.messages[0].text, 'm600');
  assert.equal(state.messages[399].text, 'm999');
  assert.equal(context.marks.get(state.id).first, 600);
  assert.equal(body.scrollTop, 30 + 2000, 'what was on screen stays put');
  state.messages.push({ role: 'user', text: 'new' });
  await context.remember();
  assert.deepEqual(server.puts.map((p) => [p.from, p.messages.length]), [[1000, 1]]);
});

test('a refused save while Felix is answering waits for the reply instead of redrawing the chat', async () => {
  const server = fakeChat(1000);
  const page = server.page();
  const streaming = { role: 'assistant', text: 'Thinking', pending: true };
  const state = { id: 'chat-long-0003', messages: [...page.messages, { role: 'user', text: 'mine' }, streaming], refs: [], busy: {} };
  const { context } = savingSlice(server, state, []);
  context.marks.set(state.id, C.markOf(page));
  server.set(server.rows().slice(0, 998));
  await context.remember();
  assert.ok(state.messages.includes(streaming), 'the answer being written stays on screen');
  assert.deepEqual(server.puts.map((p) => [p.from, p.messages.length]), [[1000, 1]]);
  state.busy = null;
  streaming.pending = false;
  await context.remember();
  assert.deepEqual(server.puts.slice(1).map((p) => [p.from, p.messages.length]), [[1000, 2], [998, 2]]);
  assert.equal(server.rows().length, 1000);
  assert.deepEqual(server.rows().slice(-2).map((m) => m.text), ['mine', 'Thinking']);
  assert.equal(state.messages.at(-1), streaming, 'the same message object stays on screen');
});

test('an attached file of any size is sent as one streamed PUT of its blob and never refused for its size', async () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8');
  const from = source.indexOf('    async function readFile(');
  const code = source.slice(from, source.indexOf('    function cardOf(', from));
  const requests = [];
  const notices = [];
  const big = { name: 'book.pdf', type: 'application/pdf', size: 3 * 1024 * 1024 * 1024 };
  const context = {
    encodeURIComponent,
    JSON,
    state: { id: 'chat-12345678', files: [{ key: 'k', name: 'book.pdf', status: 'reading' }] },
    drawRefs() {},
    thumbOf: async () => null,
    prepare: async (file) => ({ name: file.name, type: file.type, blob: file }),
    request: async (url, init) => {
      requests.push({ url, init });
      return { ok: true, status: 201, json: async () => ({ id: 'doc-1', name: 'book.pdf', chars: 9 }) };
    },
    sentenceOf: async () => '',
    notify: (text) => notices.push(text),
  };
  vm.createContext(context);
  vm.runInContext(`${code}\nthis.readFile = readFile;`, context);
  const doc = await context.readFile(big, 'k', 'chat-12345678');
  assert.equal(requests.length, 1);
  assert.equal(requests[0].url, '/api/chats/chat-12345678/files/book.pdf?type=application%2Fpdf');
  assert.equal(requests[0].init.method, 'PUT');
  assert.equal(requests[0].init.body, big);
  assert.equal('headers' in requests[0].init, false);
  assert.deepEqual(notices, []);
  assert.equal(doc.id, 'doc-1');
});

test('a chat drawing is inked on first insert and stays inked when the reply is redrawn', () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const records = [];
  class N {
    constructor(tag, attrs = {}, kids = []) {
      Object.assign(this, { nodeType: 1, localName: tag, tagName: tag.toUpperCase(), isConnected: true, parentElement: null, childNodes: [], attrs: new Map(Object.entries(attrs)) });
      this.append(...kids);
      const data = (k) => `data-${String(k)}`;
      this.dataset = new Proxy({}, { get: (o, k) => this.attrs.get(data(k)), has: (o, k) => this.attrs.has(data(k)), set: (o, k, v) => (this.setAttribute(data(k), v), true), deleteProperty: (o, k) => this.attrs.delete(data(k)) || true });
      this.classList = { contains: (c) => (this.getAttribute('class') || '').split(' ').includes(c), add: (c) => this.setAttribute('class', `${this.getAttribute('class') || ''} ${c}`.trim()) };
    }
    get attributes() { return [...this.attrs].map(([name, value]) => ({ name, value })); }
    get children() { return this.childNodes; }
    get textContent() { return ''; }
    get outerHTML() { return `<${this.localName}${this.attributes.map((a) => ` ${a.name}="${a.value}"`).join('')}>${this.childNodes.map((n) => n.outerHTML).join('')}</${this.localName}>`; }
    set src(v) { this.setAttribute('src', v); }
    getAttribute(n) { return this.attrs.has(n) ? this.attrs.get(n) : null; }
    hasAttribute(n) { return this.attrs.has(n); }
    removeAttribute(n) { this.attrs.delete(n); }
    setAttribute(n, v) {
      this.attrs.set(n, String(v));
      if (n === 'src') records.push({ type: 'attributes', target: this, addedNodes: [] });
    }
    append(...kids) {
      for (const k of kids) {
        k.parentElement = this;
        this.childNodes.push(k);
        records.push({ type: 'childList', addedNodes: [k] });
      }
    }
    closest() { return null; }
    matches(sel) { return sel === 'figure.drawing > img' && this.localName === 'img' && this.parentElement.classList.contains('drawing'); }
    querySelector(sel) { return this.childNodes.find((k) => k.localName === sel) || null; }
    querySelectorAll(sel) { return this.childNodes.flatMap((k) => [...(k.matches(sel) ? [k] : []), ...k.querySelectorAll(sel)]); }
  }
  let observe;
  const context = vm.createContext({
    window: { leoTheme: { onChange() {}, dark: () => true } },
    document: { body: {}, documentElement: {}, addEventListener() {}, querySelectorAll: () => [] },
    MutationObserver: class { constructor(fn) { observe = fn; } observe() {} },
    DOMParser: class { parseFromString() { return { documentElement: new N('svg', { viewBox: '0 0 320 80' }, [new N('path', { stroke: 'var(--text)' })]), getElementsByTagName: () => [] }; } },
    XMLSerializer: class { serializeToString(el) { return el.outerHTML; } },
    getComputedStyle: () => ({ getPropertyValue: (name) => (name === '--text' ? '#e9e7e2' : '') }),
    setTimeout: () => 0,
    clearTimeout() {},
  });
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/diagrams.js'), 'utf8'), context);
  const raw = `data:image/svg+xml;charset=utf-8,${encodeURIComponent('<svg viewBox="0 0 320 80"><path stroke="var(--text)"></path></svg>')}`;
  const reply = () => new N('div', { class: 'prose' }, [new N('figure', { class: 'drawing' }, [new N('img', { src: raw, alt: '' })])]);
  const prose = reply();
  const [fig] = prose.childNodes;
  const [img] = fig.childNodes;
  const inked = () => {
    const src = decodeURIComponent(img.getAttribute('src'));
    return fig.classList.contains('inked') && src.includes('stroke="#e9e7e2"') && !src.includes('var(--');
  };
  records.length = 0;
  observe([{ type: 'childList', addedNodes: [prose] }]);
  assert.ok(inked(), 'the first insert is inked');
  const fresh = reply();
  records.length = 0;
  C.settle(prose, fresh);
  assert.ok(!inked(), 'a redraw puts the raw drawing back');
  observe(records.splice(0));
  assert.ok(inked(), 'and the drawing is inked again before it paints');
  observe(records.splice(0));
  assert.equal(records.length, 0, 'inking it again does not loop');
});

test('a marked quiz offers Visualize, asking with the selection toolbar\'s words', () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const vm = require('node:vm');
  const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8').replace(/\r\n/g, '\n');
  const code = source.slice(source.indexOf('    function quizCard('), source.indexOf('    const checking = new WeakSet()'));
  const context = vm.createContext({ ...C, escape: esc, state: { drafts: {} }, checking: new WeakSet(), thinkingOpen: new WeakSet(), markedAt: new WeakMap(), inline: (t) => esc(t), render: (t) => esc(t), cite: (t) => t, hintsOf: () => [], quizMark: () => '', sureText: () => '' });
  vm.runInContext(`${code}\n;globalThis.__card = quizCard;`, context);
  const q = { kind: 'multiple_choice', question: 'Which?', options: ['A', 'B'], answer: 'A', explain: 'Because.', given: 'A', correct: true, held: true };
  let html;
  for (let tries = 0; tries < 40 && html === undefined; tries++) {
    try { html = context.__card(q, 0, 0, false, []); }
    catch (e) { if (e.name !== 'ReferenceError') throw e; context[e.message.split(' ')[0]] = ''; }
  }
  assert.match(html, /<div class="quiz-next">.*data-chat="quiz-why"[^>]*>Ask Felix why<\/button><button type="button" class="btn plain sm" data-chat="quiz-draw">Visualize<\/button><\/div>/);
  const said = /const VISUALIZE = '([^']+)';/.exec(source)[1];
  assert.match(source, /what === 'quiz-draw'\) \{\n\s+send\(VISUALIZE\);/);
  assert.equal(said, /draw: '([^']+)'/.exec(fs.readFileSync(path.join(__dirname, '../src/web/app/passages.js'), 'utf8'))[1]);
  const seeing = /SEE_WORDS: \[&str; \d+\] = \[([^\]]+)\]/.exec(fs.readFileSync(path.join(__dirname, '../src/drawing.rs'), 'utf8'))[1].match(/"(\w+)"/g).map((w) => w.slice(1, -1));
  assert.ok(said.toLowerCase().split(/\W+/).some((w) => seeing.includes(w)), 'the server knows the user asked to see something');
});
