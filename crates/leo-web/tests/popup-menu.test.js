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
      box.querySelector = (selector) => (selector === '.popup-rows' ? list : null);
      box.appendChild = () => {};
      box.addEventListener = () => {};
      box.remove = () => {};
      boxes.push(box);
      return box;
    },
    body: {appendChild: () => {}},
  };
  const context = {document, addEventListener: () => {}, innerWidth: 1000, innerHeight: 800, esc: String, ICON: {}, unfold: () => {}, fold: (box, then) => then?.(), fail: (e) => { throw e; }, setTimeout};
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
