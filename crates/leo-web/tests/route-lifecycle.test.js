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
  const context = { openingNote: null, renderGeneration: 0, hideAskFloat: noop,  clearTimeout: noop, cal: {}, document: {body: {classList: {toggle: noop}}}, location: {hash: '#/n/deleted'}, stopPageArrival: noop, library: {}, closeSearch: noop, leaveMap: noop, leaveBoards: noop, chat: {setContext: noop}, recorder: {leave: noop}, activity: {}, drawActivity: noop, app: {innerHTML: 'new draft', querySelectorAll: () => []}, DRAWN: {offline: 'drawn-offline'}, Offline, empty: (icon) => icon === 'drawn-offline' ? 'offline' : 'no drawing', toast: () => calls.push('toast'), go: hash => calls.push(hash), fail: () => calls.push('failure'), loadSideFolders: () => calls.push('sidebar'), newNote: async () => {}, showFolder: async () => {} };
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
test('a page keeps its body classes until the next page draws', async () => {
  const {context} = fixture();
  const toggled = [];
  context.document.body.classList.toggle = (name, on) => toggled.push(name);
  context.location.hash = '#/calendar';
  context.showCalendar = async () => {};
  context.showNote = () => new Promise(() => {});
  await context.render();
  context.location.hash = '#/n/slow';
  context.render();
  context.location.hash = '#/boards';
  context.showBoards = async () => {};
  context.render();
  context.location.hash = '#/settings';
  context.showSettings = async () => {};
  await context.render();
  assert.deepEqual(toggled.filter(name => /^on-(calendar|boards|settings)$/.test(name)), []);
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

function returning(answer) {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8');
  const start = source.indexOf("document.addEventListener('visibilitychange'");
  const end = source.indexOf("window.addEventListener(\n  'scroll'", start);
  const note = { id: 'n1', title: 'T', body: 'text', tags: [], directory: '', pinned: false, version: 3 };
  const seen = { renders: 0, sideLoads: 0, margins: [], failures: 0 };
  let listener;
  const state = { view: 'note', session: { note } };
  const context = {
    document: { visibilityState: 'visible', addEventListener: (type, fn) => { listener = fn; } },
    state, enc: String, busy: () => false, rememberPlace() {}, saveWebsiteWork() {},
    fail: () => { seen.failures++; },
    render: async () => { seen.renders++; },
    loadSideFolders: () => { seen.sideLoads++; },
    marginLoad: (s, current) => { seen.margins.push({ s, current }); return Promise.resolve(); },
    api: async () => answer(note),
  };
  vm.createContext(context);
  vm.runInContext(source.slice(start, end), context);
  const settle = async () => { for (let i = 0; i < 3; i++) await new Promise((resolve) => setImmediate(resolve)); };
  return { listener: () => listener(), seen, state, settle };
}

test('returning to an unchanged open note does not rebuild it', async () => {
  const e = returning((note) => ({ ...note }));
  e.listener();
  await e.settle();
  assert.equal(e.seen.renders, 0);
  assert.equal(e.seen.sideLoads, 1);
  assert.equal(e.seen.margins.length, 1);
  assert.equal(e.seen.margins[0].s, e.state.session);
});

test('returning to an unchanged note fetches the margin notes written while the window was hidden', async () => {
  const e = returning((note) => ({ ...note }));
  e.listener();
  await e.settle();
  const { current } = e.seen.margins[0];
  assert.equal(current(), true);
  e.state.session = { note: e.state.session.note };
  assert.equal(current(), false);
});

test('returning to a note edited elsewhere rebuilds it', async () => {
  const e = returning((note) => ({ ...note, body: 'edited elsewhere' }));
  e.listener();
  await e.settle();
  assert.equal(e.seen.renders, 1);
  assert.equal(e.seen.margins.length, 0);
});

test('returning to a note moved to another folder with the same text rebuilds it', async () => {
  const e = returning((note) => ({ ...note, directory: 'Other' }));
  e.listener();
  await e.settle();
  assert.equal(e.seen.renders, 1);
});

test('returning to a note that is gone rebuilds it so the missing-note path runs', async () => {
  const e = returning(() => { throw Object.assign(new Error('missing'), { status: 404 }); });
  e.listener();
  await e.settle();
  assert.equal(e.seen.renders, 1);
  assert.equal(e.seen.margins.length, 0);
});

test('the background panel updates in place and announces only its state', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8');
  const code = source.slice(source.indexOf('function drawActivity()'), source.indexOf('async function checkActivity()'));
  const painted = [];
  const attrs = {};
  const box = {
    hidden: false, classList: { toggle() {} }, setAttribute: (k, v) => { attrs[k] = v; },
    set innerHTML(value) { throw new Error(`innerHTML rewritten: ${value}`); },
  };
  let now = 1000;
  const context = {
    $: () => box, activity: { tasks: [{ kind: 'combine', label: 'Combining a', step: 'Felix is writing one note', done: 10, total: 100, href: '#combine/1', ready: false }], folded: false, route: '', seen: new Map() },
    hiddenHere: () => false, esc: String, ICON: { chevron: '' }, Math, Date: { now: () => now }, refreshPanel: (_, html) => painted.push(html),
    leoChat: (require('../src/web/chat.js'), globalThis.leoChat),
  };
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.drawActivity = drawActivity;`, context);
  for (let i = 0; i < 3; i++) { context.drawActivity(); now += 1000; }
  assert.equal(painted.length, 3);
  assert.equal(attrs['aria-live'], undefined);
  const said = painted.map((html) => html.match(/<span class="said" role="status">([^<]*)<\/span>/)?.[1]);
  assert.deepEqual(said, ['Working in the background', 'Working in the background', 'Working in the background']);
  assert.deepEqual(painted.map((html) => html.match(/· (\d+ s)/)?.[1]), ['0 s', '1 s', '2 s']);
});
