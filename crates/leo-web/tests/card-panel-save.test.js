const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/web/app/boards.js'), 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('  const running = {};');
const end = source.indexOf('  const names = [', start);
assert.ok(start > 0 && end > start);

test('card panel saves of one field run one after another, so the newest text lands last', async () => {
  const card = { id: 'c1', description: '' };
  const field = { text: 'first' };
  const writes = [];
  const releases = [];
  const context = {
    card, clearTimeout, timers: {}, saved: { isConnected: false }, problem: { isConnected: false }, title: {}, fit() {}, tellOver: () => false, tell() {}, toast() {}, enc: (s) => s,
    Locked: class extends Error {},
    valueOf: () => field.text,
    kanbanWrite: (url, { body }) => new Promise((resolve) => { writes.push(body.description); releases.push(() => resolve(body)); }),
    kanbanUpdate: (c, done) => Object.assign(c, done),
  };
  vm.createContext(context);
  vm.runInContext(`${source.slice(start, end)}\n;globalThis.__save = save;`, context);
  const first = context.__save('description');
  await new Promise((r) => setImmediate(r));
  field.text = 'second';
  const second = context.__save('description');
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(writes, ['first'], 'the second save waits for the first');
  releases[0]();
  await first;
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(writes, ['first', 'second']);
  releases[1]();
  await second;
  assert.equal(card.description, 'second');
});

test('a card title save that cannot reach leo retries after 3 s and says so', async () => {
  class Offline extends Error {}
  const card = { id: 'c1', title: 'old' };
  const saved = { isConnected: true, textContent: '' };
  const toasts = [];
  const pending = [];
  const timers = {};
  let attempts = 0;
  const context = {
    card, clearTimeout: () => {}, timers, saved, problem: { isConnected: false }, title: {}, fit() {}, tellOver: () => false, tell() {}, enc: (s) => s,
    toast: (message) => toasts.push(message),
    Offline,
    Locked: class extends Error {},
    fail() {},
    KANBAN_RETRY_MS: 3000,
    setTimeout: (run, ms) => { pending.push({ run, ms }); return pending.length; },
    valueOf: () => 'new',
    kanbanWrite: async (url, { body }) => {
      if (++attempts < 3) throw new Offline('Cannot reach leo');
      return body;
    },
    kanbanUpdate: (c, done) => Object.assign(c, done),
  };
  vm.createContext(context);
  vm.runInContext(`${source.slice(start, end)}\n;globalThis.__save = save;`, context);
  assert.equal(await context.__save('title'), false);
  assert.equal(saved.textContent, 'Not saved, will retry');
  assert.deepEqual(pending.map((p) => p.ms), [3000]);
  assert.equal(toasts.length, 1);
  pending.shift().run();
  await new Promise((r) => setImmediate(r));
  assert.equal(attempts, 2);
  assert.equal(toasts.length, 1, 'the error is told once, not on every try');
  assert.deepEqual(pending.map((p) => p.ms), [3000]);
  pending.shift().run();
  await new Promise((r) => setImmediate(r));
  assert.equal(attempts, 3);
  assert.equal(card.title, 'new');
  assert.equal(saved.textContent, 'Saved');
  assert.equal(pending.length, 0);
});
