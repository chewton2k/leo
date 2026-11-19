const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const read = (name) => fs.readFileSync(path.join(__dirname, '../src/web/app', name), 'utf8');

class El {
  constructor(page, tag, classes = '', data = {}) {
    Object.assign(this, { page, tag, classes: new Set(classes.split(' ').filter(Boolean)), dataset: { ...data }, parent: null, kids: [], nodeType: 1, text: '' });
    this.classList = { contains: (name) => this.classes.has(name), add: (name) => this.classes.add(name), remove: (name) => this.classes.delete(name) };
  }
  add(...kids) {
    for (const kid of kids) { kid.parent = this; this.kids.push(kid); }
    return this;
  }
  get parentElement() { return this.parent; }
  get isConnected() { return this.parent ? this.parent.isConnected : this === this.page.root; }
  remove() { this.parent.kids.splice(this.parent.kids.indexOf(this), 1); this.parent = null; }
  within(test) { for (let at = this; at; at = at.parent) if (test(at)) return at; return null; }
  closest(selector) { return this.within(CLOSEST[selector] || (() => { throw new Error(`closest ${selector}`); })); }
  matches(selector) { return (MATCH[selector] || (() => { throw new Error(`matches ${selector}`); }))(this); }
  descendants() { return this.kids.flatMap((kid) => [kid, ...kid.descendants()]); }
  querySelectorAll(selector) {
    const all = this.descendants();
    this.page.examined += all.length;
    return all.filter((el) => el.matches(selector));
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
  removeAttribute(name) { delete this.dataset[name.replace(/^data-/, '')]; }
  get firstChild() { return this.kids[0] || null; }
  appendChild(kid) { return this.add(kid); }
  set className(value) { this.classes = new Set(value.split(' ').filter(Boolean)); }
  get textContent() { return this.text; }
  set innerHTML(html) { this.html = html; }
}

const is = (el, tag, classes = []) => el.tag === tag && classes.every((name) => el.classes.has(name));
const MATCH = {
  '.math:not([data-drawn])': (el) => el.classes.has('math') && el.dataset.drawn === undefined,
  'figure.diagram:not([data-drawn])': (el) => is(el, 'figure', ['diagram']) && el.dataset.drawn === undefined,
  'figure.diagram:not([data-drawn]), figure.drawing > img': (el) => MATCH['figure.diagram:not([data-drawn])'](el) || MATCH['figure.drawing > img'](el),
  'figure.diagram[data-drawn]': (el) => is(el, 'figure', ['diagram']) && el.dataset.drawn !== undefined,
  'figure.drawing > img': (el) => el.tag === 'img' && Boolean(el.parent) && is(el.parent, 'figure', ['drawing']),
  'figure.drawing:not(.pending, [data-inked]) > img': (el) => el.tag === 'img' && Boolean(el.parent) && is(el.parent, 'figure', ['drawing']) && !el.parent.classes.has('pending') && el.parent.dataset.inked === undefined,
  'figure.drawing.inked': (el) => is(el, 'figure', ['drawing', 'inked']),
  '.diagram-art, .diagram-error': (el) => el.classes.has('diagram-art') || el.classes.has('diagram-error'),
  '.diagram-src code': (el) => el.tag === 'code' && Boolean(el.parent) && el.parent.classes.has('diagram-src'),
};
const CLOSEST = {
  '.msg.pending, .editing': (el) => (el.classes.has('msg') && el.classes.has('pending')) || el.classes.has('editing'),
  '.msg.pending, .viewer-diagram': (el) => (el.classes.has('msg') && el.classes.has('pending')) || el.classes.has('viewer-diagram'),
  '.viewer-diagram': (el) => el.classes.has('viewer-diagram'),
  '.doc': (el) => el.classes.has('doc'),
  '.margin-note': (el) => el.classes.has('margin-note'),
  '.margin-note, .msg.leo > .prose': (el) => el.classes.has('margin-note'),
};

function build(file, blocks) {
  const page = { examined: 0, scans: 0, timers: [], observers: [] };
  page.root = new El(page, 'body');
  const bulk = new El(page, 'div', 'doc');
  for (let at = 0; at < blocks; at++) {
    const block = new El(page, 'div', 'blk');
    if (at % 50 === 0) block.add(new El(page, 'span', 'math', { drawn: 'yes' }));
    if (at % 500 === 0) block.add(new El(page, 'figure', 'diagram', { drawn: 'yes' }));
    bulk.add(block);
  }
  page.root.add(bulk);
  page.chat = new El(page, 'div', 'chat');
  page.root.add(page.chat);
  page.document = {
    body: page.root,
    documentElement: {},
    addEventListener() {},
    querySelectorAll: (selector) => {
      page.scans++;
      const all = page.root.descendants();
      page.examined += all.length;
      return all.filter((el) => el.matches(selector));
    },
    createElement: (tag) => new El(page, tag),
    getElementById: () => null,
  };
  const context = {
    document: page.document,
    window: { katex: { renderToString: (tex) => `<k>${tex}</k>` }, mermaid: { initialize() {}, render: async () => ({ svg: '<svg></svg>' }) }, leoTheme: { onChange: (fn) => { page.theme = fn; }, dark: () => false } },
    MutationObserver: class { constructor(fn) { page.observers.push(fn); } observe() {} },
    setTimeout: (fn, ms) => { page.timers.push(fn); return page.timers.length; },
    clearTimeout: (id) => { if (id) page.timers[id - 1] = null; },
    getComputedStyle: () => ({ getPropertyValue: () => '' }),
    stillMotion: () => true,
  };
  vm.createContext(context);
  vm.runInContext(`${read(file)}\n;globalThis.__scan = { ${file === 'math.js' ? 'math, waitingMath, drawMath' : 'diagrams, waitingDiagrams, stepDrawings, drawDiagrams'} };`, context);
  page.run = context.__scan;
  page.batch = (...nodes) => { for (const fn of page.observers) fn(nodes.map((n) => ({ addedNodes: [n] }))); };
  page.settle = async () => {
    for (const fn of page.timers.splice(0)) if (fn) await fn();
    await new Promise(setImmediate);
  };
  page.reset = () => { page.scans = 0; page.examined = 0; };
  return page;
}

const SIZES = [1900, 19000, 190000];

test('math added in one batch is found without scanning the document', async () => {
  const seen = [];
  for (const blocks of SIZES) {
    const page = build('math.js', blocks);
    page.reset();
    const added = new El(page, 'p').add(new El(page, 'span', 'math', { tex: 'x^2' }), new El(page, 'span', 'math', { tex: 'y' }));
    page.chat.add(added);
    page.batch(added);
    await page.settle();
    const [first, second] = added.kids;
    assert.equal(first.dataset.drawn, 'yes');
    assert.equal(second.dataset.drawn, 'yes');
    console.log(`math scan: ${blocks} blocks: ${page.scans} document scans, ${page.examined} elements examined per streamed batch`);
    seen.push([blocks, page.scans, page.examined]);
  }
  for (const [blocks, scans, examined] of seen) {
    assert.equal(scans, 0, `${blocks} blocks`);
    assert.ok(examined <= 10, `${blocks} blocks examined ${examined}`);
  }
});

test('math added inside a pending reply is drawn when the reply ends', async () => {
  const page = build('math.js', 200);
  const reply = new El(page, 'div', 'msg leo pending');
  const formula = new El(page, 'span', 'math', { tex: 'a' });
  reply.add(formula);
  page.chat.add(reply);
  page.batch(reply);
  await page.settle();
  assert.equal(formula.dataset.drawn, undefined, 'a pending reply is left alone');
  assert.equal(page.run.math.waiting.size, 1, 'and stays waiting');
  reply.classList.remove('pending');
  const next = new El(page, 'p');
  page.chat.add(next);
  page.batch(next);
  await page.settle();
  assert.equal(formula.dataset.drawn, 'yes');
});

test('a drawn or removed element leaves the set', async () => {
  const page = build('math.js', 200);
  const kept = new El(page, 'span', 'math', { tex: 'k' }), gone = new El(page, 'span', 'math', { tex: 'g' }), editing = new El(page, 'span', 'math', { tex: 'e' });
  const row = new El(page, 'p').add(kept, gone);
  const draft = new El(page, 'div', 'editing').add(editing);
  page.chat.add(row, draft);
  gone.remove();
  page.batch(row, draft);
  assert.deepEqual([...page.run.waitingMath()], [kept]);
  assert.equal(page.run.math.waiting.size, 2, 'the removed one is gone; the one being edited waits');
  await page.settle();
  assert.equal(kept.dataset.drawn, 'yes');
  assert.deepEqual([...page.run.waitingMath()], []);
  assert.equal(page.run.math.waiting.size, 1, 'only the one being edited waits');
});

test('diagrams added in one batch are found without scanning the document', async () => {
  const seen = [];
  for (const blocks of SIZES) {
    const page = build('diagrams.js', blocks);
    page.reset();
    const figure = new El(page, 'figure', 'diagram').add(new El(page, 'pre', 'diagram-src').add(Object.assign(new El(page, 'code'), { text: 'graph TD; A-->B' })));
    const drawing = new El(page, 'figure', 'drawing').add(Object.assign(new El(page, 'img'), { getAttribute: () => 'plain.png' }));
    const added = new El(page, 'div').add(figure, drawing);
    page.chat.add(added);
    page.batch(added);
    await page.settle();
    assert.equal(figure.dataset.drawn, 'yes');
    assert.equal(drawing.dataset.inked, '');
    console.log(`diagram scan: ${blocks} blocks: ${page.scans} document scans, ${page.examined} elements examined per streamed batch`);
    seen.push([blocks, page.scans, page.examined]);
  }
  for (const [blocks, scans, examined] of seen) {
    assert.equal(scans, 0, `${blocks} blocks`);
    assert.ok(examined <= 20, `${blocks} blocks examined ${examined}`);
  }
});

test('a diagram added inside a pending reply is drawn when the reply ends', async () => {
  const page = build('diagrams.js', 200);
  const reply = new El(page, 'div', 'msg leo pending');
  const figure = new El(page, 'figure', 'diagram').add(new El(page, 'pre', 'diagram-src').add(Object.assign(new El(page, 'code'), { text: 'graph TD; A-->B' })));
  reply.add(figure);
  page.chat.add(reply);
  page.batch(reply);
  await page.settle();
  assert.equal(figure.dataset.drawn, undefined);
  reply.classList.remove('pending');
  const next = new El(page, 'p');
  page.chat.add(next);
  page.batch(next);
  await page.settle();
  assert.equal(figure.dataset.drawn, 'yes');
});

test('a drawing that is pending when added is inked once it is ready', async () => {
  const page = build('diagrams.js', 200);
  const img = Object.assign(new El(page, 'img'), { getAttribute: () => 'plain.png' });
  const figure = new El(page, 'figure', 'drawing pending').add(img);
  page.chat.add(figure);
  page.batch(figure);
  await page.settle();
  assert.equal(figure.dataset.inked, undefined);
  figure.classList.remove('pending');
  const next = new El(page, 'p');
  page.chat.add(next);
  page.batch(next);
  await page.settle();
  assert.equal(figure.dataset.inked, '');
});

test('a drawn or removed diagram leaves the set, and one in a viewer waits', async () => {
  const page = build('diagrams.js', 200);
  const source = () => new El(page, 'pre', 'diagram-src').add(Object.assign(new El(page, 'code'), { text: 'graph TD; A-->B' }));
  const here = new El(page, 'figure', 'diagram').add(source());
  const gone = new El(page, 'figure', 'diagram').add(source());
  const viewer = new El(page, 'div', 'viewer-diagram').add(new El(page, 'figure', 'diagram').add(source()));
  const row = new El(page, 'div').add(here, gone);
  page.chat.add(row, viewer);
  gone.remove();
  page.batch(row, viewer);
  assert.deepEqual([...page.run.waitingDiagrams()], [here]);
  await page.settle();
  assert.equal(here.dataset.drawn, 'yes');
  assert.deepEqual([...page.run.waitingDiagrams()], []);
  assert.equal(page.run.diagrams.waiting.size, 1, 'only the one in the viewer waits');
});

test('a theme change redraws every diagram', async () => {
  const page = build('diagrams.js', 200);
  const figure = new El(page, 'figure', 'diagram').add(new El(page, 'pre', 'diagram-src').add(Object.assign(new El(page, 'code'), { text: 'graph TD; A-->B' })));
  page.chat.add(figure);
  page.batch(figure);
  await page.settle();
  assert.equal(figure.dataset.drawn, 'yes');
  const before = figure.kids.find((kid) => kid.classes.has('diagram-art'));
  page.reset();
  page.theme();
  await page.settle();
  const art = figure.kids.filter((kid) => kid.classes.has('diagram-art'));
  assert.equal(page.scans, 2, 'the theme path scans the drawn drawings and diagrams once each');
  assert.equal(art.length, 1);
  assert.notEqual(art[0], before, 'the picture was drawn again');
  assert.equal(figure.dataset.drawn, 'yes');
});
