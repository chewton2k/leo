const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../src/web/app/popup-menu.js'), 'utf8').replace(/\r\n/g, '\n');
const rule = source.slice(source.indexOf('function closeOnOutsidePress'));

function fixture(leave) {
  const handlers = {};
  const document = { activeElement: null, body: { tag: 'body' }, addEventListener: (type, fn) => (handlers[type] ||= []).push(fn) };
  const context = { document, leave, fail: (e) => { throw e; } };
  vm.createContext(context);
  vm.runInContext(`${rule}\n;closeOnOutsidePress('#panel, .card', () => leave());`, context);
  const target = (hit) => ({ closest: (selector) => (hit && selector.split(', ').includes(hit) ? {} : null) });
  const press = async (hit, { button = 0, primary = true, detail = 1, down = true } = {}) => {
    if (down) for (const fn of handlers.pointerdown) fn({ button, isPrimary: primary, target: target(hit) });
    for (const fn of handlers.click) fn({ detail, target: target(hit) });
    await new Promise((resolve) => setImmediate(resolve));
  };
  return { document, press };
}

test('a primary press and click outside leaves the panel', async () => {
  let left = 0;
  const { press } = fixture(() => { left++; });
  await press(null);
  assert.equal(left, 1);
});

test('presses inside the panel, on a card, or in a menu, sheet or toast leave it open', async () => {
  let left = 0;
  const { press } = fixture(() => { left++; });
  for (const hit of ['#panel', '.card', '.popup-menu', '.select-menu', '.scrim', '.toast']) await press(hit);
  assert.equal(left, 0);
});

test('a press that began elsewhere, a secondary button, a touch that is not primary and a keyboard click do not leave', async () => {
  let left = 0;
  const { press } = fixture(() => { left++; });
  await press(null, { down: false });
  await press(null, { button: 2 });
  await press(null, { primary: false });
  await press(null, { detail: 0 });
  assert.equal(left, 0);
});

test('focus goes to the opener only when it fell to the page', async () => {
  const opener = { focused: 0, focus() { this.focused++; } };
  const { press, document } = fixture(() => opener);
  document.activeElement = document.body;
  await press(null);
  assert.equal(opener.focused, 1);
  document.activeElement = { tag: 'input' };
  await press(null);
  assert.equal(opener.focused, 1);
});

test('a panel that stays open gives nothing to focus', async () => {
  const { press, document } = fixture(() => false);
  document.activeElement = document.body;
  await press(null);
});
