const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
function fixture() {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8');
  const calls = [];
  class Offline extends Error {}
  const noop = () => {};
  const context = { openingNote: null, renderGeneration: 0, hideAskFloat: noop,  clearTimeout: noop, cal: {}, closeSource: noop, document: {body: {classList: {toggle: noop}}}, location: {hash: '#/n/deleted'}, stopPageArrival: noop, library: {}, closeSearch: noop, leaveMap: noop, leaveBoards: noop, chat: {setContext: noop}, recorder: {leave: noop}, activity: {}, drawActivity: noop, app: {innerHTML: 'new draft', querySelectorAll: () => []}, DRAWN: {offline: 'drawn-offline'}, Offline, empty: (icon) => icon === 'drawn-offline' ? 'offline' : 'no drawing', toast: () => calls.push('toast'), go: hash => calls.push(hash), fail: () => calls.push('failure'), loadSideFolders: () => calls.push('sidebar'), newNote: async () => {}, showFolder: async () => {} };
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('async function render()'), source.indexOf("window.addEventListener('online'")), context);
  return {context, calls, Offline};
}
test('late route errors cannot redirect or overwrite a newer draft', async () => {
  for (const kind of ['missing', 'offline', 'other']) {
    const {context, calls, Offline} = fixture();
    let reject;
    context.showNote = () => new Promise((_, fail) => { reject = fail; });
    const pending = context.render();
    context.location.hash = '#/new';
    await context.render();
    calls.length = 0;
    reject(kind === 'missing' ? Object.assign(new Error('missing'), {status:404}) : kind === 'offline' ? new Offline() : new Error('failed'));
    await pending;
    assert.deepEqual(calls, [], kind);
    assert.equal(context.app.innerHTML, 'new draft', kind);
  }
});
test('an older visit to the same route cannot handle its late error or success', async () => {
  for (const failed of [false, true]) {
    const {context, calls} = fixture();
    let finish;
    context.showNote = () => new Promise((resolve, reject) => { finish = () => failed ? reject(Object.assign(new Error('missing'), {status:404})) : resolve(); });
    const pending = context.render();
    context.showNote = async () => {};
    await context.render();
    calls.length = 0;
    finish();
    await pending;
    assert.deepEqual(calls, []);
  }
});
test('the active route still handles missing notes and connection errors', async () => {
  for (const kind of ['missing', 'offline', 'other']) {
    const {context, calls, Offline} = fixture();
    context.showNote = async () => { throw kind === 'missing' ? Object.assign(new Error('missing'), {status:404}) : kind === 'offline' ? new Offline() : new Error('failed'); };
    await context.render();
    assert.deepEqual(calls, kind === 'missing' ? ['toast', '#/'] : kind === 'other' ? ['failure'] : []);
    assert.equal(context.app.innerHTML, kind === 'offline' ? 'offline' : 'new draft');
  }
});

test('arriving on a page keeps a sheet opened there and closes one left from before', async () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8');
  const closed = [];
  const context = { location: { hash: '#/search/x' }, closeSheet: (now) => closed.push(now), drawn: {}, window: { scrollTo() {} }, render: async () => {}, $: () => open };
  let open = { openedOn: '#/search/x' };
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('async function route('), source.indexOf('const activity =')), context);
  await context.route(true);
  assert.deepEqual(closed, []);
  await context.route();
  assert.deepEqual(closed, [true]);
  open = { openedOn: '#/n/before' };
  await context.route(true);
  assert.deepEqual(closed, [true, true]);
});
