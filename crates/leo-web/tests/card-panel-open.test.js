const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/web/app/boards.js'), 'utf8').replace(/\r\n/g, '\n');

function between(from, to) {
  const start = source.indexOf(from);
  const end = source.indexOf(to, start);
  assert.ok(start >= 0 && end > start, `${from} … ${to}`);
  return source.slice(start, end);
}

const loader = between('const kanbanFaceIsWhole', '\n// The card just before');

function load(api) {
  const context = { api, enc: (s) => s };
  vm.createContext(context);
  vm.runInContext(`${loader}\n;globalThis.whole = kanbanFaceIsWhole; globalThis.load = kanbanLoadWhole;`, context);
  return context;
}

test('a card whose description fits its face, or is empty, is editable at once', () => {
  const { whole } = load(() => assert.fail('no read needed'));
  assert.equal(whole({ description: '' }), true);
  assert.equal(whole({}), true);
  assert.equal(whole({ description: 'x'.repeat(240), description_more: false }), true);
  assert.equal(whole({ description: 'x'.repeat(240), description_more: true }), false);
});

test('the whole description replaces the face once the card read returns', async () => {
  const { load: run } = load(() => Promise.resolve({ description: 'y'.repeat(8000) }));
  const card = { id: 'c1', description: 'x'.repeat(240), description_more: true };
  assert.equal(await run(card, () => true), true);
  assert.equal(card.description.length, 8000);
  assert.equal(card.description_more, false);
});

test('a card read that returns after the panel closed changes nothing', async () => {
  const { load: run } = load(() => Promise.resolve({ description: 'y'.repeat(8000) }));
  const card = { id: 'c1', description: 'x'.repeat(240), description_more: true };
  assert.equal(await run(card, () => false), false);
  assert.equal(card.description.length, 240);
  assert.equal(card.description_more, true);
});

test('a card read that fails leaves the card as it was and says why', async () => {
  const { load: run } = load(() => Promise.reject(new Error('gone')));
  const card = { id: 'c1', description: 'x'.repeat(240), description_more: true };
  await assert.rejects(run(card, () => true), /gone/);
  assert.equal(card.description.length, 240);
  assert.equal(card.description_more, true);
});

test('a card read that answers without a description field leaves an empty one', async () => {
  const { load: run } = load(() => Promise.resolve({ id: 'c1' }));
  const card = { id: 'c1', description: 'x'.repeat(240), description_more: true };
  await run(card, () => true);
  assert.equal(card.description, '');
});

test('a title typed before the card read returns is saved alone and never sends the face as the description', async () => {
  const face = 'x'.repeat(240);
  const card = { id: 'c1', title: 'Old', subheading: '', description: face, description_more: true };
  const field = { value: 'New  title' };
  const bodies = [];
  const context = {
    card, doc: null, fields: { title: field, subheading: { value: '' } }, names: ['title', 'subheading', 'description'],
    clearTimeout, timers: {}, saved: { isConnected: false }, problem: { isConnected: false }, title: field, fit() {}, tellOver: () => false, tell() {}, toast() {}, enc: (s) => s,
    Locked: class extends Error {}, Offline: class extends Error {}, KANBAN_RETRY_MS: 3000,
    kanbanWrite: async (url, { body }) => { bodies.push(body); return { ...card, ...body, description: 'the whole description' }; },
    kanbanUpdate: (c, done) => Object.assign(c, done),
  };
  vm.createContext(context);
  vm.runInContext(`${between('  const valueOf =', '  const names = [')}\n;globalThis.__save = save; globalThis.__dirty = dirty;`, context);
  assert.equal(context.__dirty(), true);
  await context.__save('title');
  await context.__save('description');
  assert.equal(JSON.stringify(bodies), JSON.stringify([{ title: 'New title' }]));
});
