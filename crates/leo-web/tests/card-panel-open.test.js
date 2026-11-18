const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/web/app/boards.js'), 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf("  description.innerHTML = skeleton(3, 'board');");
const end = source.indexOf('  const doc = leoDoc.mount(', start);
assert.ok(start > 0 && end > start);

function run(api) {
  const card = { id: 'c1', description: 'x'.repeat(240) };
  const description = { innerHTML: '' };
  const box = { isConnected: true, hidden: false, innerHTML: 'panel' };
  const mounted = [];
  const context = {
    card, description, box, id: 'c1', kanban: { opening: null }, enc: (s) => s, api,
    skeleton: () => 'lines', document: { body: { classList: { remove() {} } } },
    leoDoc: { mount: (_, options) => mounted.push(options.source) },
  };
  vm.createContext(context);
  const done = vm.runInContext(`(async () => {\n${source.slice(start, end)}\nleoDoc.mount(null, { source: card.description });\n})()`, context);
  return { card, description, box, mounted, done };
}

test('the panel loads the whole description before it can be edited', async () => {
  let release;
  const open = run(() => new Promise((resolve) => { release = resolve; }));
  assert.equal(open.description.innerHTML, 'lines');
  assert.deepEqual(open.mounted, [], 'nothing is mounted while the card loads');
  release({ description: 'y'.repeat(8000) });
  await open.done;
  assert.equal(open.mounted[0].length, 8000);
  assert.equal(open.card.description.length, 8000);
});

test('a card that cannot be loaded closes the panel and leaves the face alone', async () => {
  const open = run(() => Promise.reject(new Error('gone')));
  await assert.rejects(open.done, /gone/);
  assert.equal(open.box.hidden, true);
  assert.deepEqual(open.mounted, []);
  assert.equal(open.card.description.length, 240);
});
