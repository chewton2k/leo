'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/saving.js');

function storage() {
  const values = new Map();
  return {
    get length() { return values.size; },
    key: (index) => [...values.keys()][index],
    getItem: (key) => values.get(key) || null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
  };
}
const note = () => ({ id: 'one', title: 'Note', body: 'Original', directory: '', version: 'v1' });
function manager(api, local = storage(), extra = {}) {
  return leoSaving.create({ api, storage: local, schedule: () => 1, cancel: () => {}, ...extra });
}
const edit = (body) => ({ title: 'Note', body });

test('a failed save remains tracked and recovers after a reload', async () => {
  const local = storage();
  const failure = Object.assign(new Error('disk full'), { status: 500 });
  const m = manager(async () => { throw failure; }, local);
  const s = m.open(note());
  m.changed(s, edit('Do not lose this'));
  await assert.rejects(m.flush(s), /disk full/);
  assert.equal(m.unsaved(), true);
  const recovered = manager(async (_, { body }) => ({ ...note(), ...body, version: 'v2' }), local);
  const r = recovered.open(note());
  assert.equal(r.edit.body, 'Do not lose this');
  assert.equal(r.note.version, 'v1');
  await recovered.flush(r);
  assert.equal(recovered.unsaved(), false);
  assert.equal(local.length, 0);
});

test('edits made while a save is in flight are sent after it and retained on failure', async () => {
  let release;
  let calls = 0;
  const local = storage();
  const m = manager(async (_, { body }) => {
    calls++;
    if (calls === 1) await new Promise((resolve) => { release = resolve; });
    else throw new Error('second save failed');
    return { ...note(), ...body, version: 'v2' };
  }, local);
  const s = m.open(note());
  m.changed(s, edit('First edit'));
  const pending = m.flush(s);
  m.changed(s, edit('Latest edit'));
  release();
  await assert.rejects(pending, /second save failed/);
  const again = manager(async () => {}, local).open(note());
  assert.equal(calls, 2);
  assert.equal(again.edit.body, 'Latest edit');
  assert.equal(again.note.version, 'v2');
});

test('a recovered draft uses its original base so external edits create a conflict copy', async () => {
  const local = storage();
  const old = manager(async () => {}, local);
  old.changed(old.open(note()), edit('Phone draft'));
  const requests = [];
  const m = manager(async (path, options) => {
    requests.push({ path, ...options });
    if (options.method === 'PATCH') throw Object.assign(new Error('conflict'), { status: 409 });
    return { ...note(), ...options.body, id: 'copy', version: 'copy-v1' };
  }, local);
  const s = m.open({ ...note(), body: 'Computer edit', version: 'v9' });
  await m.flush(s);
  assert.equal(requests[0].body.base, 'v1');
  assert.equal(requests[1].body.body, 'Phone draft');
  assert.match(requests[1].body.title, /conflict from leo/);
  assert.equal(s.note.id, 'copy');
  assert.equal(local.length, 0);
});

test('offline new-note drafts recover with their folder', async () => {
  const local = storage();
  const m = manager(async () => { throw new Error('offline'); }, local);
  const s = m.open({ ...note(), id: null, directory: 'cs130', version: null });
  m.changed(s, { title: 'Lecture', body: 'BFS' });
  await assert.rejects(m.flush(s));
  const again = manager(async () => {}, local);
  const [draft] = again.drafts();
  assert.equal(draft.note.id, null);
  assert.equal(draft.note.directory, 'cs130');
  assert.deepEqual(draft.edit, { title: 'Lecture', body: 'BFS' });
});

test('a draft saved with tags by an older page still loads and saves', async () => {
  const local = storage();
  local.setItem('leo-draft-v1:one', JSON.stringify({ note: { ...note(), tags: ['exam'] }, edit: { title: 'Note', body: 'Kept', tags: ['exam'] } }));
  local.setItem('leo-draft-v1:two', JSON.stringify({ note: { ...note(), id: 'two' }, edit: { title: 'Two', body: 'Also kept' } }));
  const sent = [];
  const m = manager(async (url, { body }) => { sent.push(body); return { ...note(), id: url.split('/').pop(), ...body, version: 'v2' }; }, local);
  const drafts = m.drafts();
  assert.deepEqual(drafts.map((d) => d.edit.body).sort(), ['Also kept', 'Kept']);
  for (const draft of drafts) await m.flush(m.get(draft.key));
  assert.deepEqual(sent.map((b) => b.body).sort(), ['Also kept', 'Kept']);
  assert.equal(local.length, 0);
});

test('storage failure is visible and the draft stays in memory until saved', async () => {
  let warning = 0;
  const local = storage();
  local.setItem = () => { throw new Error('quota'); };
  const m = manager(async (_, { body }) => ({ ...note(), ...body }), local, { storageError: () => warning++ });
  const s = m.open(note());
  m.changed(s, edit('First'));
  m.changed(s, edit('Latest'));
  assert.equal(warning, 1);
  assert.equal(s.persisted, false);
  assert.equal(m.unsaved(), true);
  await m.flush(s);
  assert.equal(m.unsaved(), false);
});

test('blank new notes are not created and unrelated local storage is untouched', async () => {
  const local = storage();
  local.setItem('other-app', 'keep');
  const m = manager(async () => { assert.fail('blank note was sent'); }, local);
  const s = m.open({ ...note(), id: null, title: '', body: '' });
  m.changed(s, { title: '', body: '' });
  await m.flush(s);
  assert.equal(m.unsaved(), false);
  assert.equal(local.getItem('other-app'), 'keep');
});


test('a retried first save sends the same request id, and a reloaded draft keeps it', async () => {
  const local = storage();
  const bodies = [];
  const lost = async (_, { body }) => { bodies.push(body); throw Object.assign(new Error('answer lost'), { status: 500 }); };
  const m = manager(lost, local);
  const s = m.open({ ...note(), id: null, version: null });
  m.changed(s, edit('Same'));
  await assert.rejects(m.flush(s));
  await assert.rejects(m.flush(s));
  assert.equal(bodies.length, 2);
  assert.ok(bodies[0].request);
  assert.equal(bodies[1].request, bodies[0].request);
  const reloaded = manager(async (_, { body }) => { bodies.push(body); return { ...note(), ...body, id: 'made', version: 'v1' }; }, local);
  await reloaded.flush(reloaded.get(reloaded.drafts()[0].key));
  assert.equal(bodies.length, 3);
  assert.equal(bodies[2].request, bodies[0].request);
});

test('a conflict copy that is retried keeps its request id', async () => {
  const posts = [];
  const m = manager(async (path, options) => {
    if (options.method === 'PATCH') throw Object.assign(new Error('conflict'), { status: 409 });
    posts.push(options.body);
    if (posts.length === 1) throw Object.assign(new Error('answer lost'), { status: 500 });
    return { ...note(), ...options.body, id: 'copy', version: 'copy-v1' };
  });
  const s = m.open(note());
  m.changed(s, edit('Phone draft'));
  await assert.rejects(m.flush(s));
  await m.flush(s);
  assert.equal(posts.length, 2);
  assert.ok(posts[0].request);
  assert.equal(posts[1].request, posts[0].request);
  assert.equal(s.note.id, 'copy');
});

test('the next new note after a saved one gets a new request id', async () => {
  const requests = [];
  const m = manager(async (_, { body }) => {
    requests.push(body.request);
    const { request, ...saved } = body;
    return { ...note(), ...saved, id: 'made' + requests.length, version: 'v1' };
  });
  const first = m.open({ ...note(), id: null, version: null });
  m.changed(first, edit('One'));
  await m.flush(first);
  const second = m.open({ ...note(), id: null, version: null });
  m.changed(second, edit('Two'));
  await m.flush(second);
  assert.equal(requests.length, 2);
  assert.ok(requests[0] && requests[1]);
  assert.notEqual(requests[1], requests[0]);
  assert.equal(first.note.request, undefined);
});

test('new-note drafts work on HTTP origins without randomUUID', async () => {
  const vm = require('node:vm');
  const fs = require('node:fs');
  const sandbox = { crypto: { getRandomValues: (bytes) => require('node:crypto').webcrypto.getRandomValues(bytes) } };
  vm.runInNewContext(fs.readFileSync(require.resolve('../src/web/saving.js'), 'utf8'), sandbox);
  const local = storage();
  const m = sandbox.leoSaving.create({ api: async () => { throw new Error('offline'); }, storage: local, schedule: () => 1, cancel: () => {} });
  const one = m.open({ ...note(), id: null });
  const two = m.open({ ...note(), id: null });
  assert.notEqual(one.key, two.key);
  m.changed(one, edit('First draft'));
  m.changed(two, edit('Second draft'));
  await assert.rejects(m.flush(one));
  await assert.rejects(m.flush(two));
  assert.equal(local.length, 2);
});

test('a discard during a save that then fails does not bring the draft back', async () => {
  const local = storage();
  let fail;
  const scheduled = [];
  const m = manager(() => new Promise((_, reject) => { fail = reject; }), local, { schedule: (fn) => scheduled.push(fn), retryable: () => true });
  const s = m.open(note());
  m.changed(s, edit('Throw this away'));
  const pending = m.flush(s);
  assert.equal(m.discardAll(), 1);
  fail(Object.assign(new Error('offline'), { status: 500 }));
  await pending;
  assert.equal(m.unsaved(), false);
  assert.equal(local.length, 0);
  assert.equal(scheduled.length, 1, 'only the first edit timer, no retry');
});

test('clearing drafts forgets every unsaved edit in this browser and nothing else', async () => {
  const local = storage();
  local.setItem('other-app', 'keep');
  const failure = Object.assign(new Error('offline'), { status: 500 });
  const m = manager(async () => { throw failure; }, local);
  const s = m.open(note());
  m.changed(s, edit('Throw this away'));
  await assert.rejects(m.flush(s), /offline/);
  assert.equal(m.drafts().length, 1);
  assert.equal(m.discardAll(), 1);
  assert.equal(m.unsaved(), false);
  assert.deepEqual(m.drafts(), []);
  assert.equal(local.length, 1);
  assert.equal(local.getItem('other-app'), 'keep');
  assert.equal(manager(async () => note(), local).drafts().length, 0, 'nothing comes back after a reload');
});
