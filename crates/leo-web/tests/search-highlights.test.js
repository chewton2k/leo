const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

function fixture(texts, supported = true) {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/core.js'), 'utf8');
  const nodes = texts.map(textContent => ({ textContent, parentElement: { closest: () => null, scrollIntoView() {} } }));
  const registry = new Map();
  const timers = new Map();
  let next = 0;
  const context = {
    CSS: supported ? { highlights: registry } : {}, Highlight: class extends Array { constructor(...ranges) { super(...ranges); } }, NodeFilter: { SHOW_TEXT: 4 },
    document: {
      createTreeWalker() { let index = 0; return { nextNode: () => nodes[index++] }; },
      createRange() { return { setStart(node, at) { this.node = node; this.startContainer = node; this.start = at; }, setEnd(node, at) { this.end = at; }, getBoundingClientRect: () => ({ top: 120, bottom: 140 }) }; },
    },
    window: { innerHeight: 900, addEventListener() {} },
    setTimeout(fn) { const id = ++next; timers.set(id, fn); return id; }, clearTimeout(id) { timers.delete(id); },
  };
  vm.createContext(context);
  const start = source.indexOf('const SEARCH_HIGHLIGHT_MS');
  assert.notEqual(start, -1, 'search highlighting helper exists');
  vm.runInContext(source.slice(start, source.indexOf('async function go(', start)), context);
  return { ...context, registry, timers };
}

test('search highlights literal query words, with Unicode offsets, without editing text', () => {
  const value = fixture(['😀 math a+b Math <img>', 'a+b']);
  assert.equal(value.highlightSearch({}, 'math a+b <img>'), true);
  const ranges = value.registry.get('leo-search');
  assert.deepEqual(Array.from(ranges, range => range.node.textContent.slice(range.start, range.end)), ['math', 'a+b', 'Math', '<img>', 'a+b']);
  assert.equal(ranges[0].start, 3);
  assert.equal(value.timers.size, 1);
  value.timers.values().next().value();
  assert.equal(value.registry.size, 0);
});

test('new highlights replace old ranges and cancel their expiry', () => {
  const value = fixture(['math physics']);
  value.highlightSearch({}, 'math');
  value.highlightSearch({}, 'physics');
  assert.equal(value.registry.get('leo-search').length, 1);
  assert.equal(value.timers.size, 1);
  value.clearSearchHighlight();
  assert.equal(value.registry.size, 0);
  assert.equal(value.timers.size, 0);
});

test('highlighting bounds repeated matches and handles missing matches and support', () => {
  const value = fixture(['a '.repeat(1000)]);
  value.highlightSearch({}, 'a');
  assert.equal(value.registry.get('leo-search').length, 300);
  assert.equal(value.highlightSearch({}, 'absent'), false);
  assert.equal(value.registry.size, 0);
  assert.equal(value.highlightSearch({}, ''), false);
  assert.equal(fixture(['math'], false).highlightSearch({}, 'math'), false);
});

test('search result and Quick Leo navigation carry their own query only to a note', async () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/core.js'), 'utf8');
  const start = source.indexOf('const SEARCH_HIGHLIGHT_MS');
  const end = source.indexOf('const folderHash', start);
  const value = fixture([]);
  Object.assign(value, { seq: 0, history: { pushState() {}, replaceState() {} }, saving: { flush: async () => true }, state: { view: 'search', query: 'math' }, location: { hash: '#/search/math' }, route() {} });
  vm.createContext(value);
  vm.runInContext(source.slice(start, end) + '\nconst pending = () => searchPassage;', value);
  await vm.runInContext("go('#/n/one');", value);
  assert.equal(vm.runInContext('pending().query', value), 'math');
  await vm.runInContext("state = { view: 'note' }; go('#/n/two', { search: 'physics' });", value);
  assert.equal(vm.runInContext('pending().query', value), 'physics');
  await vm.runInContext("go('#/n/three');", value);
  assert.equal(vm.runInContext('pending()', value), null);
  await vm.runInContext("go('#/calendar', { search: 'math' });", value);
  assert.equal(vm.runInContext('pending()', value), null);
});

test('the query bound keeps the final Unicode character whole', () => {
  const value = fixture(['😀']);
  assert.equal(value.highlightSearch({}, ' '.repeat(1999) + '😀'), true);
  const range = value.registry.get('leo-search')[0];
  assert.equal(range.node.textContent.slice(range.start, range.end), '😀');
});

test('a match deep inside a tall paragraph scrolls the range into view', () => {
  const value = fixture(['long paragraph needle']);
  let top = 2000;
  value.document.createRange = () => ({ setStart(node, at) { this.startContainer = node; }, setEnd() {}, getBoundingClientRect: () => ({ top, bottom: top + 20 }) });
  value.window.scrollBy = options => { top -= options.top; };
  assert.equal(value.highlightSearch({}, 'needle'), true);
  assert.ok(top >= 80 && top + 20 <= value.window.innerHeight);
});

test('the facts line under the title is never searched, so a match lands in the note text', () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/core.js'), 'utf8');
  const skipped = [];
  const value = fixture([]);
  const nodes = [{ textContent: '120 words · 1 min read', parentElement: { closest: (q) => { skipped.push(q); return q.includes('.note-facts') ? {} : null; }, scrollIntoView() {} } }, { textContent: 'words in the body', parentElement: { closest: () => null, scrollIntoView() {} } }];
  value.document.createTreeWalker = () => { let i = 0; return { nextNode: () => nodes[i++] }; };
  assert.equal(value.highlightSearch({}, 'words'), true);
  assert.equal(value.registry.get('leo-search').length, 1);
  assert.equal(value.registry.get('leo-search')[0].node, nodes[1]);
  assert.match(source, /closest\('textarea, script, style, \.katex-mathml, \.note-facts'\)/);
});
