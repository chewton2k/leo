'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');
const ed = require('../src/web/editing.js');

const doc = fs.readFileSync(path.resolve(__dirname, '../src/web/doc.js'), 'utf8');
const between = (from, to) => doc.slice(doc.indexOf(from), doc.indexOf(to));

function element() {
  const classes = new Set();
  return {
    dataset: {}, style: {}, className: '', isConnected: false, parentNode: null,
    classList: { contains: (name) => classes.has(name), add: (name) => classes.add(name), remove: (name) => classes.delete(name), toggle: () => {} },
    set innerHTML(value) {}, replaceWith() {}, remove() {},
  };
}

function drawn(count) {
  const counts = { query: 0, queryAll: 0 };
  const container = {
    children: [],
    classList: { toggle() {} },
    insertBefore(el, before) {
      el.parentNode = container;
      el.isConnected = true;
      container.children.splice(before ? container.children.length - 1 : container.children.length, 0, el);
    },
    appendChild(el) { container.insertBefore(el, null); },
    querySelector(selector) {
      counts.query++;
      return selector === '.doc-tail' ? container.children.find((el) => el.dataset.tail) || null : null;
    },
    querySelectorAll() { counts.queryAll++; return container.children.filter((el) => !el.dataset.tail); },
  };
  const body = `${between('function listStarts(', 'function renderBlock(')}\nconst renderBlock = () => '';\n${between('function draw()', 'let outlineTimer')}\nreturn draw;`;
  const lines = Array.from({ length: count }, (_, i) => `## Heading ${i + 1}`);
  const draw = new Function('ed', 'lines', 'container', 'document', 'outlineScroll', 'let blocks = []; let rendered = [];\n' + body)(ed, lines, container, { createElement: element }, () => {});
  draw();
  return { counts, container };
}

for (const count of [3000, 30000]) {
  test(`drawing ${count} blocks looks the note up a constant number of times`, () => {
    const { counts, container } = drawn(count);
    assert.equal(container.children.length, count + 1);
    assert.ok(counts.query <= 4, `container.querySelector ran ${counts.query} times`);
    assert.ok(counts.queryAll <= 2, `container.querySelectorAll ran ${counts.queryAll} times`);
  });
}

function scrolled(count) {
  const counts = { reads: 0, queries: 0, writes: 0, forced: 0 };
  let layoutDirty = false;
  const rows = Array.from({ length: count }, (_, i) => ({ line: i, level: 2, text: `Heading ${i + 1}`, block: i, key: `k${i}` }));
  let scrollTop = 0;
  const rendered = rows.map((row, i) => ({ el: { isConnected: true, dataset: { start: String(i) }, getBoundingClientRect: () => { counts.reads++; return { top: i * 40 - scrollTop }; } } }));
  const button = () => ({ attrs: {}, setAttribute(name, value) { this.attrs[name] = value; }, removeAttribute(name) { delete this.attrs[name]; }, closest: () => null });
  const outline = {
    isConnected: true, hidden: false,
    style: { setProperty() { counts.writes++; layoutDirty = true; } },
    get offsetHeight() { if (layoutDirty) { counts.forced++; layoutDirty = false; } return 300; },
  };
  const outlineNav = { querySelector: () => { counts.queries++; return button(); } };
  const container = { querySelector: () => { counts.queries++; return null; } };
  const root = { innerHeight: 900, get scrollY() { return scrollTop; } };
  const body = `${between('function outlineMark()', 'function outlineEscape')}\n${between('function outlineTop()', 'function outlineScroll()')}\nreturn { outlineActive };`;
  const make = new Function('outline', 'outlineNav', 'container', 'rendered', 'root', 'document', 'performance', 'outlineRows', 'outlinePage', 'outlinePeek', 'outlineTicks', 'OUTLINE_PAGE',
    `const outlinePlaced = { top: null, center: null };\nlet outlineStart = 0, outlineAt = -1, outlineButton = null, outlineTick = null, outlineFrame = 0, outlineLanding = null, outlineFollowed = null, outlineIndex = null;\nconst outlineShows = (index) => index >= outlineStart && index < outlineStart + OUTLINE_PAGE;\n${body}`);
  const document = { getElementById: () => ({ getBoundingClientRect: () => ({ bottom: 64 }) }), documentElement: { scrollHeight: count * 40 + 900 } };
  const { outlineActive } = make(outline, outlineNav, container, rendered, root, document, { now: () => 0 }, rows, () => {}, () => {}, { children: [] }, 24);
  return { counts, frame(top) { scrollTop = top; for (const key of Object.keys(counts)) counts[key] = 0; outlineActive(); return { ...counts }; } };
}

for (const count of [3000, 30000]) {
  test(`a scroll frame over ${count} headings reads few positions, runs few queries and writes no style`, () => {
    const page = scrolled(count);
    page.frame(0);
    page.frame(0);
    let most = { reads: 0, queries: 0, writes: 0, forced: 0 };
    for (let step = 1; step <= 12; step++) {
      const cost = page.frame(step * 40 * Math.floor(count / 13));
      for (const key of Object.keys(most)) most[key] = Math.max(most[key], cost[key]);
    }
    assert.ok(most.reads <= 20, `block positions read per frame: ${most.reads}`);
    assert.ok(most.queries <= 3, `queries per frame: ${most.queries}`);
    assert.equal(most.writes, 0, 'a frame that moved nothing writes no style');
    assert.equal(most.forced, 0, 'no frame reads layout after a style write');
  });
}
