const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const vm = require('node:vm');
const { paneHelpers } = require('./page-source');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/web/app/events.js'), 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('const unsaved =');
const end = source.indexOf("window.addEventListener(\n  'scroll',", start);
assert.ok(start > 0 && end > start);

function environment({ draft = false, panel = null } = {}) {
  const windows = {}, documents = {}, sent = [], saved = [], places = [];
  let rendered = 0;
  const context = {
    navigationRequest: 0, seq: 0,
    window: { addEventListener: (name, fn) => { windows[name] = fn; } },
    document: { visibilityState: 'visible', activeElement: null, body: {}, addEventListener: (name, fn) => { documents[name] = fn; } },
    kanban: { panel }, pendingAutosaves: () => [],
    state: { view: 'workspace', session: null },
    app: { contains: () => false },
    saving: { unsaved: () => draft, retry: () => { saved.push('notes'); return Promise.resolve([]); } },
    navigator: { sendBeacon: (...args) => { sent.push(args); return true; } },
    fetch: (...args) => { sent.push(args); return Promise.resolve({ status: 204 }); },
    api: (...args) => { sent.push(args); return Promise.resolve({}); },
    Blob,
    location: { hash: '#/n/kept' },
    localStorage: { setItem: (...args) => places.push(args) },
    route: () => {}, render: async () => { rendered++; }, fail: () => {}, marginLeave: () => {}, marginPage: null,
  };
  vm.createContext(context);
  vm.runInContext(paneHelpers(), context);
  vm.runInContext(source.slice(start, end), context);
  return { context, windows, documents, sent, saved, places, rendered: () => rendered };
}

test('hide, departure and return save work and remember the note', async () => {
  const e = environment({ draft: true });
  for (let i = 0; i < 3; i++) {
    e.context.document.visibilityState = 'hidden';
    e.documents.visibilitychange();
    e.windows.pagehide();
    e.context.document.visibilityState = 'visible';
    e.documents.visibilitychange();
  }
  assert.deepEqual(e.saved, Array(6).fill('notes'));
  assert.equal(e.places.length, 6);
  assert.deepEqual(e.places[0], ['leo-last-place', '#/n/kept']);
  assert.equal(e.rendered(), 3);
  assert.deepEqual(e.sent, []);
});

test('unsaved work retains the close warning and retry while a clean page closes quietly', () => {
  for (const draft of [false, true]) {
    const e = environment({ draft });
    let prevented = false;
    e.windows.beforeunload({ preventDefault: () => { prevented = true; } });
    assert.equal(prevented, draft);
    assert.equal(e.saved.includes('notes'), draft);
    assert.equal(e.sent.length, 0);
  }
});

test('navigation still flushes the departing draft before routing', async () => {
  const e = environment();
  const calls = [];
  const session = { doc: { stop: () => calls.push('stop'), destroy: () => calls.push('destroy') } };
  e.context.state.main = e.context.state.session = session;
  e.context.flush = async (leaving) => { assert.equal(leaving, session); calls.push('flush'); };
  e.context.route = () => calls.push('route');
  await e.windows.hashchange();
  assert.deepEqual(calls, ['stop', 'flush', 'destroy', 'route']);
  assert.deepEqual(e.sent, []);
});

test('returning to a note being edited keeps its draft and focus intact', () => {
  const e = environment({ draft: true });
  e.context.state = { view: 'note', session: { note: { id: 'kept' } } };
  e.documents.visibilitychange();
  assert.equal(e.rendered(), 0);
  assert.deepEqual(e.sent, []);
});

test('an open card panel is saved on hide and keeps the close warning while its text is unsaved', () => {
  for (const dirty of [false, true]) {
    const flushed = [];
    const panel = { dirty: () => dirty, flush: () => { flushed.push('card'); return Promise.resolve([]); } };
    const e = environment({ panel });
    let prevented = false;
    e.windows.beforeunload({ preventDefault: () => { prevented = true; } });
    assert.equal(prevented, dirty);
    assert.deepEqual(flushed, dirty ? ['card'] : []);
    e.windows.pagehide();
    assert.equal(flushed.length, dirty ? 2 : 1);
  }
});
