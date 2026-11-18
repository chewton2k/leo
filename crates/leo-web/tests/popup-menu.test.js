const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
function element(attrs = {}, tag = 'div') {
  const kept = new Map(Object.entries(attrs));
  const el = {
    tag, kids: [], style: {}, isConnected: true, focused: 0,
    hasAttribute: (name) => kept.has(name),
    getAttribute: (name) => kept.get(name),
    setAttribute: (name, value) => kept.set(name, String(value)),
    removeAttribute: (name) => kept.delete(name),
    matches: (selector) => selector === 'button:not([aria-expanded])' && tag === 'button' && !kept.has('aria-expanded'),
    contains: (target) => target === el || el.kids.some((kid) => kid.contains(target)),
    focus: () => { el.focused += 1; },
    getBoundingClientRect: () => ({left: 10, bottom: 20}),
    offsetWidth: 100, offsetHeight: 100,
  };
  return el;
}
function fixture() {
  const handlers = {};
  const boxes = [];
  const document = {
    addEventListener: (type, fn) => (handlers[type] ||= []).push(fn),
    createElement: () => {
      const box = element();
      const list = {innerHTML: '', querySelectorAll: () => [], querySelector: () => null};
      const typed = {};
      const input = {value: '', focus: () => {}, addEventListener: (type, fn) => { typed[type] = fn; }};
      const listeners = {};
      box.list = list;
      box.type = (value) => { input.value = value; typed.input(); };
      box.rows = () => [...list.innerHTML.matchAll(/data-row="(\d+)"[^>]*>.*?<span class="grow">(.*?)<\/span>/g)].map((m) => ({index: m[1], text: m[2]}));
      box.press = (text) => {
        const row = box.rows().find((r) => r.text === text);
        listeners.click({target: {closest: () => ({dataset: {row: row.index}, disabled: false})}});
      };
      box.key = (key) => {
        const event = {key, target: input, preventDefault: () => {}, stopPropagation: () => {}};
        listeners.keydown(event);
      };
      box.querySelector = (selector) => (selector === '.popup-rows' ? list : selector === 'input' && box.innerHTML.includes('<input') ? input : null);
      box.appendChild = () => {};
      box.addEventListener = (type, fn) => { listeners[type] = fn; };
      box.remove = () => {};
      boxes.push(box);
      return box;
    },
    body: {appendChild: () => {}},
  };
  const context = {document, addEventListener: () => {}, innerWidth: 1000, innerHeight: 800, esc: String, ICON: {}, unfold: () => {}, fold: (box, then) => then?.(), fail: (e) => { throw e; }, setTimeout, clearTimeout};
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/popup-menu.js'), 'utf8'), context);
  const fire = (type, event) => (handlers[type] || []).forEach((fn) => fn(event));
  const open = () => boxes.filter((box) => box.className === 'popup-menu');
  const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
  const press = async (target, whenClicked = () => {}) => {
    fire('pointerdown', {target, button: 0});
    fire('click', {target});
    whenClicked();
    await tick();
  };
  const show = (owner, options = {}) => context.popupMenu(owner, {label: 'Menu', rows: [{text: 'One'}], ...options});
  return {context, boxes, fire, open, press, show};
}
test('clicking the owner again closes its menu and gives the owner focus back', async () => {
  const {open, press, show} = fixture();
  const owner = element({'aria-haspopup': 'menu', 'aria-expanded': 'false'}, 'button');
  show(owner);
  assert.equal(open().length, 1);
  assert.equal(owner.getAttribute('aria-expanded'), 'true');
  await press(owner, () => show(owner));
  assert.equal(open().length, 0);
  assert.equal(owner.getAttribute('aria-expanded'), 'false');
  assert.equal(owner.focused, 1);
  await press(owner, () => show(owner));
  assert.equal(open().length, 1);
});
test('a click outside closes the menu on pointerdown', () => {
  const {fire, open, show} = fixture();
  const owner = element({'aria-haspopup': 'menu'}, 'button');
  show(owner);
  fire('pointerdown', {target: element(), button: 0});
  assert.equal(open().length, 0);
});
test('a menu whose owner click does not reopen it closes once that click ends', async () => {
  const {open, press, show} = fixture();
  const tree = element();
  const row = element({}, 'button');
  tree.kids.push(row);
  show(tree);
  await press(row);
  assert.equal(open().length, 0);
});
test('a click inside the open menu leaves it open', async () => {
  const {open, press, show} = fixture();
  show(element({'aria-haspopup': 'menu'}, 'button'));
  await press(open()[0]);
  assert.equal(open().length, 1);
});
test('context menus reopen at the new point and a different owner replaces the menu', () => {
  const {boxes, open, show} = fixture();
  const owner = element();
  show(owner, {point: {x: 30, y: 40}});
  show(owner, {point: {x: 200, y: 300}});
  assert.equal(open().length, 1);
  assert.equal(boxes[1].style.left, '200px');
  assert.equal(boxes[1].style.top, '300px');
  show(element());
  assert.equal(open().length, 1);
  assert.equal(open()[0], boxes[2]);
});
test('a different menu on the same owner replaces the open one instead of toggling it', () => {
  const {boxes, open, show} = fixture();
  const owner = element({'aria-haspopup': 'menu'}, 'button');
  show(owner, {label: 'This note'});
  show(owner, {label: 'Move to folder'});
  assert.deepEqual(open(), [boxes[1]]);
});
test('owners without aria-haspopup report aria-expanded while open and are restored after', () => {
  const {fire, show} = fixture();
  const plain = element({}, 'button');
  show(plain);
  assert.equal(plain.getAttribute('aria-expanded'), 'true');
  fire('pointerdown', {target: element(), button: 0});
  assert.equal(plain.hasAttribute('aria-expanded'), false);
  const folder = element({'aria-expanded': 'true'}, 'button');
  show(folder);
  fire('pointerdown', {target: element(), button: 0});
  assert.equal(folder.getAttribute('aria-expanded'), 'true');
});

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const answer = (names, more = false) => ({rows: names.map((text) => ({text})), more, loaded: names.length});
function paged(options = {}) {
  const f = fixture();
  const asked = [];
  const waiting = [];
  const page = (query, offset) => new Promise((resolve) => {
    asked.push([query, offset]);
    waiting.push(resolve);
  });
  f.show(element(), {find: 'Find', rows: [{text: 'First'}], more: true, loaded: 1, page, ...options});
  return {...f, box: f.boxes[0], asked, waiting, page};
}
test('a menu without a page filters its rows by what is typed', () => {
  const {boxes, show} = fixture();
  show(element(), {find: 'Find', rows: [{text: 'Alpha'}, {text: 'Beta'}]});
  boxes[0].type('bet');
  assert.deepEqual(boxes[0].rows().map((r) => r.text), ['Beta']);
});
test('typing asks the page once after a pause and drops an older answer', async () => {
  const {box, asked, waiting} = paged();
  box.type('a');
  box.type('ab');
  await sleep(250);
  assert.deepEqual(asked, [['ab', 0]]);
  box.type('abc');
  await sleep(250);
  assert.deepEqual(asked, [['ab', 0], ['abc', 0]]);
  waiting[1](answer(['Newer']));
  await sleep(0);
  waiting[0](answer(['Older']));
  await sleep(0);
  assert.deepEqual(box.rows().map((r) => r.text), ['Newer']);
});
test('an empty box draws the first rows again and an empty answer says nothing matches', async () => {
  const {box, asked, waiting} = paged();
  box.type('zzz');
  await sleep(250);
  waiting[0](answer([]));
  await sleep(0);
  assert.match(box.list.innerHTML, /Nothing matches/);
  box.type('');
  assert.deepEqual(box.rows().map((r) => r.text), ['First', 'Show more']);
  assert.equal(asked.length, 1);
});
test('Show more keeps the menu open, asks for the next page and appends', async () => {
  const {box, asked, waiting, open} = paged();
  box.press('Show more');
  assert.deepEqual(asked, [['', 1]]);
  waiting[0](answer(['First', 'Second'], false));
  await sleep(0);
  assert.deepEqual(box.rows().map((r) => r.text), ['First', 'Second']);
  assert.equal(open().length, 1);
});
test('Show more under a typed query asks the page for that query', async () => {
  const {box, asked, waiting} = paged();
  box.type('q');
  await sleep(250);
  waiting[0](answer(['One', 'Two'], true));
  await sleep(0);
  box.press('Show more');
  assert.deepEqual(asked, [['q', 0], ['q', 2]]);
});
test('Enter while a search is pending picks nothing', async () => {
  let picked = 0;
  const {box, waiting} = paged({rows: [{text: 'First', run: () => { picked += 1; }}]});
  box.type('x');
  box.key('Enter');
  await sleep(250);
  box.key('Enter');
  await sleep(0);
  assert.equal(picked, 0);
  waiting[0](answer(['Found']));
  await sleep(0);
  assert.deepEqual(box.rows().map((r) => r.text), ['Found']);
});
