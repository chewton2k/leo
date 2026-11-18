'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('const QUIT_SETTLE_MS');
const end = source.indexOf('\n};\n', start) + 3;

function page(settle) {
  const said = [];
  const waits = [];
  const context = {
    window: {},
    said,
    waits,
    saving: { settle: (within) => { said.push(`settle ${within}`); return settle(); } },
    saveWebsiteWork: () => said.push('save work'),
    api: async (url, { body }) => { said.push(`${url} ${body.action}`); return {}; },
    setTimeout: (fn, ms) => { waits.push(ms); context.later = fn; return 1; },
    toast: () => said.push('toast'),
    quitText: async () => 'Press again to quit',
  };
  vm.createContext(context);
  vm.runInContext(source.slice(start, end), context);
  return context;
}

test('quit now settles the page then answers settled', async () => {
  let finish;
  const p = page(() => new Promise((resolve) => { finish = resolve; }));
  const answered = p.window.leoFromApp('quit', 'now');
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(p.said, ['save work', 'settle 2800'], 'nothing is answered while the page settles');
  finish(true);
  await answered;
  assert.deepEqual(p.said, ['save work', 'settle 2800', '/api/app settled']);
});

test('quit now still answers when settling hangs', async () => {
  const p = page(() => new Promise(() => {}));
  const answered = p.window.leoFromApp('quit', 'now');
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(p.waits, [2800]);
  assert.ok(p.waits[0] < 3000, 'answers before leo stops waiting');
  assert.equal(p.said.includes('/api/app settled'), false);
  p.later();
  await answered;
  assert.equal(p.said.filter((s) => s === '/api/app settled').length, 1);
});

test('the first quit press still only saves and says press again', async () => {
  const p = page(() => Promise.resolve(true));
  await p.window.leoFromApp('quit', '');
  assert.deepEqual(p.said, ['save work', 'toast']);
});
