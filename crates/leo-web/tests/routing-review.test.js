const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { paneHelpers } = require('./page-source');
const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
const events = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8');
function fixture(persisted = true) {
  const calls = [], notices = [];
  const session = { persisted, note: { id: 'draft', directory: '' }, doc: { stop() {}, destroy() { calls.push('destroy'); } } };
  const context = { seq: 1, state: { view: 'note', session, main: session }, app: {}, location: { hash: '#/n/draft' }, history: { state: { leoAt: 3 }, replaceState(_state, _title, hash) { context.location.hash = hash; } }, clearSearchHighlight() {}, searchPassage: null, openingNote: null, route() { calls.push('route'); }, flush: async () => {}, fail: error => calls.push(error.message), toast: (text, options) => notices.push({ text, ...options }), Offline: class extends Error {}, Locked: class extends Error {}, noteHash: id => '#/n/' + id, enc: encodeURIComponent, marginLeave() {} };
  const start = core.includes('function showSaveFailure(') ? core.indexOf('function showSaveFailure(') : core.indexOf('async function go(');
  vm.createContext(context);
  vm.runInContext(paneHelpers(), context);
  vm.runInContext('let navigationRequest = 0;\n' + core.slice(start, core.indexOf('const folderHash', start)), context);
  let hashchange;
  context.window = { addEventListener: (_name, fn) => { hashchange = fn; } };
  const at = events.indexOf("window.addEventListener('hashchange', async");
  vm.runInContext(events.slice(at, events.indexOf('const busy =', at)), context);
  return { context, session, calls, notices, hashchange: () => hashchange() };
}
test('the latest destination wins while the same draft save is in flight', async () => {
  const { context } = fixture();
  let finish; const waiting = new Promise(resolve => { finish = resolve; });
  context.flush = () => waiting;
  const first = context.go('#/n/first'), latest = context.go('#/n/latest');
  finish(); await Promise.all([first, latest]);
  assert.equal(context.location.hash, '#/n/latest');
});
test('persisted drafts offer an explicit escape from every persistent save failure', async () => {
  for (const status of [403, 404, 500]) for (const hash of [false, true]) {
    const { context, session, notices, hashchange } = fixture();
    context.flush = async () => { throw Object.assign(new Error('Save refused'), { status }); };
    if (hash) { context.location.hash = '#/settings/trash'; await hashchange(); }
    else await context.go('#/settings/trash');
    assert.equal(context.state.session, session);
    assert.equal(context.location.hash, '#/n/draft');
    assert.equal(notices.at(-1)?.action, 'Leave, draft kept');
    await notices.at(-1).run();
    assert.equal(context.location.hash, '#/settings/trash');
    assert.equal(context.state.session, null);
    assert.equal(session.persisted, true);
  }
});
test('in-memory drafts and superseded escape actions cannot bypass a save', async () => {
  const memory = fixture(false);
  memory.context.flush = async () => { throw new Error('Save refused'); };
  await memory.context.go('#/settings');
  assert.equal(memory.notices.length, 0);
  assert.equal(memory.context.state.session, memory.session);
  const saved = fixture(); saved.context.flush = memory.context.flush;
  await saved.context.go('#/settings'); const stale = saved.notices.at(-1);
  await saved.context.go('#/search/text');
  await stale.run(); assert.equal(saved.context.location.hash, '#/n/draft');
  saved.session.persisted = false;
  await saved.notices.at(-1).run(); assert.equal(saved.context.location.hash, '#/n/draft');
});

test('mixed Back/hash and direct requests cannot revive an older failed destination', async () => {
  for (const hashFirst of [true, false]) {
    const { context, notices, hashchange } = fixture();
    let refuse; const waiting = new Promise((_resolve, reject) => { refuse = reject; });
    context.flush = () => waiting;
    let first, latest;
    if (hashFirst) {
      context.location.hash = '#/settings'; first = hashchange(); latest = context.go('#/search/latest');
    } else {
      first = context.go('#/settings'); context.location.hash = '#/search/latest'; latest = hashchange();
    }
    refuse(new Error('Save refused')); await Promise.all([first, latest]);
    assert.equal(notices.length, 1);
    assert.equal(context.location.hash, '#/n/draft');
    await notices[0].run(); assert.equal(context.location.hash, '#/search/latest');
  }
});
