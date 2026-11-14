const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

class El {
  constructor(localName, attrs = {}, children = []) {
    Object.assign(this, { localName, attrs: { ...attrs }, children });
  }
  getAttribute(name) { return name in this.attrs ? this.attrs[name] : null; }
  setAttribute(name, value) { this.attrs[name] = String(value); }
  removeAttribute(name) { delete this.attrs[name]; }
  hasAttribute(name) { return name in this.attrs; }
}

const written = (el) => `<${el.localName}${Object.entries(el.attrs).map(([k, v]) => ` ${k}="${v}"`).join('')}>${el.children.map(written).join('')}</${el.localName}>`;

function load(parsed) {
  const context = {
    window: { leoTheme: { onChange() {}, dark: () => false } },
    document: { body: {}, addEventListener() {} },
    MutationObserver: class { observe() {} },
    DOMParser: class { parseFromString(text) { return parsed(text); } },
    XMLSerializer: class { serializeToString(el) { return written(el); } },
  };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/diagrams.js'), 'utf8'), context);
  return context;
}

const docOf = (root, broken = false) => ({ documentElement: root, getElementsByTagName: (name) => (name === 'parsererror' && broken ? [{}] : []) });

test('each step shows the steps up to it, with its own caption, and keeps a step\'s own opacity', () => {
  const root = new El('svg', { viewBox: '0 0 320 120' }, [
    new El('title'),
    new El('g', { 'data-step': '2', 'data-caption': 'Then it moves' }, [new El('path', { d: 'M0 0' })]),
    new El('g', { 'data-step': '1', 'data-caption': 'First the source', opacity: '.5' }, [new El('circle')]),
    new El('g', { 'data-step': '2' }, [new El('line')]),
    new El('rect'),
  ]);
  const { drawingSteps } = load(() => docOf(root));
  const steps = drawingSteps('<svg/>');
  assert.equal(steps.length, 2);
  assert.deepEqual([...steps.map((s) => s.caption)], ['First the source', 'Then it moves']);
  assert.match(steps[0].svg, /<g data-step="2" data-caption="Then it moves" opacity="0">/);
  assert.match(steps[0].svg, /<g data-step="1" data-caption="First the source" opacity=".5">/);
  assert.match(steps[0].svg, /<g data-step="2" opacity="0">/);
  assert.doesNotMatch(steps[1].svg, /opacity="0"/);
  assert.match(steps[1].svg, /opacity=".5"/);
  assert.match(steps[1].svg, /<rect><\/rect>/);
  assert.match(steps[1].svg, /^<svg viewBox="0 0 320 120" fill="var\(--text\)">/);
});

test('a drawing without steps stays one picture, and one that does not parse is left as it is', () => {
  const plain = new El('svg', {}, [new El('path', { style: 'stroke: var(--accent)' })]);
  const { drawingSteps } = load((text) => (text === 'bad' ? docOf(new El('html'), true) : docOf(plain)));
  assert.deepEqual(JSON.parse(JSON.stringify(drawingSteps('<svg/>'))), [{ svg: '<svg fill="var(--text)"><path style="stroke: var(--accent)"></path></svg>', caption: '' }]);
  assert.equal(drawingSteps('bad'), null);
  const many = new El('svg', {}, Array.from({ length: 13 }, (_, i) => new El('g', { 'data-step': String(i + 1) })));
  const { drawingSteps: tooMany } = load(() => docOf(many));
  assert.equal(tooMany('<svg>all</svg>').length, 1);
});

test('only the theme colours a drawing may name are filled in, anything else becomes the text colour', () => {
  const { inked } = load(() => docOf(new El('svg')));
  const colours = { '--text': '#111', '--accent': '#45f', '--accent-soft': '#eef' };
  const paint = (name) => colours[name];
  assert.equal(inked('stroke: var(--accent); fill:var( --accent-soft )', paint), 'stroke: #45f; fill:#eef');
  assert.equal(inked('stroke: var(--secret-thing, red)', paint), 'stroke: #111');
  assert.equal(inked('no tokens here', paint), 'no tokens here');
});
