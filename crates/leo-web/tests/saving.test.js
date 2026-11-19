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
  let tries = 0;
  local.setItem = () => { tries++; throw new Error('quota'); };
  const m = manager(async (_, { body }) => ({ ...note(), ...body }), local, { storageError: () => warning++, now: () => 1000 });
  const s = m.open(note());
  m.changed(s, edit('First'));
  m.changed(s, edit('Latest'));
  assert.equal(tries, 1, 'the second edit inside 250 ms does not write');
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

test('typing writes the draft at most four times a second and once more when a save starts', async () => {
  const local = storage();
  let writes = 0;
  const setItem = local.setItem;
  local.setItem = (k, v) => { writes++; setItem(k, v); };
  let clock = 1000;
  const m = manager(async (_, { body }) => ({ ...note(), ...body, version: 'v2' }), local, { now: () => clock });
  const s = m.open(note());
  for (let i = 0; i < 20; i++) { clock += 50; m.changed(s, edit(`typed ${i}`)); }
  assert.equal(writes, 4, 'one write every 250 ms over a second of typing');
  assert.equal(JSON.parse(local.getItem(s.key)).edit.body, 'typed 15');
  assert.equal(s.persistDue, true);
  let sent = null;
  const saved = m.flush(s);
  sent = JSON.parse(local.getItem(s.key)).edit.body;
  assert.equal(writes, 5);
  assert.equal(sent, 'typed 19', 'the waiting draft is written before the save starts');
  await saved;
  assert.equal(local.length, 0);
});

test('the stored draft carries the edit once', () => {
  const local = storage();
  const m = manager(async () => note(), local);
  const s = m.open({ ...note(), body: 'The whole long body' });
  m.changed(s, edit('The whole long body, edited'));
  const stored = JSON.parse(local.getItem(s.key));
  assert.equal(stored.note.body, '');
  assert.equal(stored.edit.body, 'The whole long body, edited');
  assert.equal(stored.note.version, 'v1');
  const again = manager(async () => note(), local).open(note());
  assert.equal(again.edit.body, 'The whole long body, edited');
});

test('persistDue writes a draft that was waiting', async () => {
  const local = storage();
  let clock = 1000;
  const m = manager(async () => { throw Object.assign(new Error('offline'), { status: 500 }); }, local, { now: () => clock });
  const s = m.open(note());
  m.changed(s, edit('first'));
  clock += 10;
  m.changed(s, edit('second'));
  assert.equal(s.persistDue, true);
  assert.equal(JSON.parse(local.getItem(s.key)).edit.body, 'first');
  assert.equal(await m.settle(), true, "stored in this browser counts as kept");
  assert.equal(s.persistDue, false);
  assert.equal(JSON.parse(local.getItem(s.key)).edit.body, 'second');
  assert.equal(s.durableRevision, s.revision);
  const again = manager(async () => note(), local).open(note());
  assert.equal(again.edit.body, 'second');
});

const SMALL_DRAFT = 262144;
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

function fakeIndexedDB({ failOpen = false } = {}) {
  const data = new Map();
  const ctl = { data, puts: 0, failPut: false, hold: false, held: [], made: false };
  const later = (fn) => setTimeout(fn, 0);
  const db = {
    createObjectStore: () => ({}),
    transaction() {
      const tx = { error: null };
      const ops = [];
      const request = (op) => { const r = {}; ops.push(() => { r.result = op(); }); return r; };
      const store = {
        put: (value, key) => { ctl.puts++; return request(() => { if (ctl.failPut) throw new Error('QuotaExceededError'); data.set(key, structuredClone(value)); }); },
        delete: (key) => request(() => data.delete(key)),
        getAllKeys: () => request(() => [...data.keys()]),
        getAll: () => request(() => [...data.values()].map((v) => structuredClone(v))),
      };
      tx.objectStore = () => store;
      const finish = () => {
        try {
          for (const op of ops) op();
          later(() => tx.oncomplete && tx.oncomplete());
        } catch (error) {
          tx.error = error;
          later(() => tx.onerror && tx.onerror());
        }
      };
      later(() => (ctl.hold ? ctl.held.push(finish) : finish()));
      return tx;
    },
  };
  ctl.release = () => { ctl.hold = false; for (const f of ctl.held.splice(0)) f(); };
  return {
    ctl,
    open() {
      const request = {};
      later(() => {
        if (failOpen) { request.error = new Error('blocked'); return request.onerror && request.onerror(); }
        request.result = db;
        if (!ctl.made) { ctl.made = true; request.onupgradeneeded && request.onupgradeneeded(); }
        request.onsuccess();
      });
      return request;
    },
  };
}

function limited() {
  const local = storage();
  const setItem = local.setItem;
  local.setItem = (k, v) => { if (v.length > SMALL_DRAFT * 2) throw new Error('quota'); setItem(k, v); };
  return local;
}

const big = (n, fill = 'a') => fill.repeat(n);
const settled = async () => { for (let i = 0; i < 6; i++) await tick(); };

test('a draft over the small limit is written to the large store whole and read back after a reload', async () => {
  const idb = fakeIndexedDB();
  const local = limited();
  const m = manager(async () => { throw Object.assign(new Error('offline'), { status: 500 }); }, local, { indexedDB: idb });
  await m.ready;
  const s = m.open(note());
  const body = big(SMALL_DRAFT + 10);
  m.changed(s, edit(body));
  await settled();
  assert.equal(local.length, 0, 'nothing large in localStorage');
  assert.equal(idb.ctl.data.get(s.key).edit.body, body);
  assert.equal(idb.ctl.data.get(s.key).note.body, '');
  assert.equal(s.durableRevision, s.revision);
  let told = 0;
  const again = manager(async () => note(), local, { indexedDB: idb, recovered: (n) => { told = n; } });
  await again.ready;
  assert.equal(told, 1);
  const r = again.open(note());
  assert.equal(r.edit.body, body);
  assert.equal(r.note.version, 'v1');
  assert.equal(again.unsaved(), true);
});

test('`settle` waits for a first write still in flight and resolves true once it confirms', async () => {
  const idb = fakeIndexedDB();
  const m = manager(async () => new Promise(() => {}), limited(), { indexedDB: idb });
  await m.ready;
  idb.ctl.hold = true;
  const s = m.open(note());
  m.changed(s, edit(big(SMALL_DRAFT + 1)));
  let done = null;
  m.settle().then((v) => { done = v; });
  await settled();
  assert.equal(done, null, 'still waiting on the write');
  idb.ctl.release();
  await settled();
  assert.equal(done, true);
  assert.equal(idb.ctl.puts, 1);
});

test('a large paste is stored whole, not as a span', async () => {
  const idb = fakeIndexedDB();
  let clock = 1000;
  const m = manager(async () => new Promise(() => {}), limited(), { indexedDB: idb, now: () => clock });
  await m.ready;
  const s = m.open(note());
  m.changed(s, edit('short'));
  clock += 1000;
  const pasted = 'short' + big(1048576, 'p');
  m.changed(s, edit(pasted));
  await settled();
  assert.equal(idb.ctl.data.get(s.key).edit.body, pasted);
});

test('two edits at opposite ends of a 20 MB note are stored whole', async () => {
  const idb = fakeIndexedDB();
  let clock = 1000;
  const m = manager(async () => new Promise(() => {}), limited(), { indexedDB: idb, now: () => clock });
  await m.ready;
  const s = m.open(note());
  const middle = big(20000000, 'm');
  m.changed(s, edit('A' + middle));
  await settled();
  clock += 10;
  const both = 'A' + middle + 'Z';
  m.changed(s, edit(both));
  assert.equal(await m.settle(), true);
  const kept = idb.ctl.data.get(s.key).edit.body;
  assert.equal(kept.length, both.length);
  assert.equal(kept[0], 'A');
  assert.equal(kept.at(-1), 'Z');
});

test('when the large store refuses the write the session is not durable, the message shows once and `settle` saves the note to leo', async () => {
  const idb = fakeIndexedDB();
  idb.ctl.failPut = true;
  let warning = 0;
  const marks = [];
  const sent = [];
  let clock = 1000;
  const m = manager(async (_, { body }) => { sent.push(body.body.length); return { ...note(), ...body, version: 'v2' }; }, limited(), { indexedDB: idb, storageError: () => warning++, mark: (_, t) => marks.push(t), now: () => clock });
  await m.ready;
  const s = m.open(note());
  m.changed(s, edit(big(SMALL_DRAFT * 3)));
  await settled();
  clock += 1000;
  m.changed(s, edit(big(SMALL_DRAFT * 3 + 1)));
  await settled();
  assert.equal(s.persisted, false);
  assert.notEqual(s.durableRevision, s.revision);
  assert.equal(warning, 1);
  assert.ok(marks.includes('Not saved · keep this page open'));
  assert.equal(await m.settle(), true);
  assert.deepEqual(sent, [SMALL_DRAFT * 3 + 1]);
  assert.equal(m.unsaved(), false);
});

test('`settle` resolves false after 5 s when neither store nor leo answers and the message stays', async () => {
  const idb = fakeIndexedDB({ failOpen: true });
  const timers = [];
  const marks = [];
  const m = manager(() => new Promise(() => {}), limited(), { indexedDB: idb, schedule: (fn, ms) => { timers.push({ fn, ms }); return timers.length; }, mark: (_, t) => marks.push(t) });
  await m.ready;
  const s = m.open(note());
  m.changed(s, edit(big(SMALL_DRAFT * 3)));
  let done = null;
  m.settle().then((v) => { done = v; });
  await settled();
  assert.equal(done, null);
  const five = timers.find((t) => t.ms === 5000);
  assert.ok(five, 'a 5 s limit');
  five.fn();
  await settled();
  assert.equal(done, false);
  assert.equal(marks.at(-1), 'Not saved · keep this page open');
  assert.equal(m.unsaved(), true);
});

test('a hide starts the write at once', async () => {
  const idb = fakeIndexedDB();
  let clock = 1000;
  const m = manager(async () => new Promise(() => {}), limited(), { indexedDB: idb, now: () => clock });
  await m.ready;
  const s = m.open(note());
  m.changed(s, edit(big(SMALL_DRAFT + 1)));
  await settled();
  clock += 10;
  m.changed(s, edit(big(SMALL_DRAFT + 2)));
  assert.equal(s.persistDue, true);
  m.retry();
  await settled();
  assert.equal(idb.ctl.puts, 2);
  assert.equal(idb.ctl.data.get(s.key).edit.body.length, SMALL_DRAFT + 2);
});

test('an old draft in localStorage bigger than the limit is read once and its old key is removed only after the large write confirms', async () => {
  const idb = fakeIndexedDB();
  const local = storage();
  const key = 'leo-draft-v1:one';
  local.setItem(key, JSON.stringify({ note: note(), edit: edit(big(SMALL_DRAFT + 5)) }));
  const m = manager(async () => new Promise(() => {}), local, { indexedDB: idb });
  await m.ready;
  const s = m.open(note());
  assert.equal(s.edit.body.length, SMALL_DRAFT + 5);
  idb.ctl.hold = true;
  m.changed(s, edit(big(SMALL_DRAFT + 6)));
  await settled();
  assert.ok(local.getItem(key), 'the old key stays until the write confirms');
  idb.ctl.release();
  await settled();
  assert.equal(local.getItem(key), null);
  assert.equal(idb.ctl.data.get(key).edit.body.length, SMALL_DRAFT + 6);
});

test('a draft that shrinks below the limit goes back to `localStorage`', async () => {
  const idb = fakeIndexedDB();
  const local = storage();
  let clock = 1000;
  const m = manager(async () => new Promise(() => {}), local, { indexedDB: idb, now: () => clock });
  await m.ready;
  const s = m.open(note());
  m.changed(s, edit(big(SMALL_DRAFT + 1)));
  await settled();
  assert.ok(idb.ctl.data.has(s.key));
  clock += 1000;
  m.changed(s, edit('short again'));
  assert.equal(JSON.parse(local.getItem(s.key)).edit.body, 'short again');
  await settled();
  assert.equal(idb.ctl.data.has(s.key), false);
  const again = manager(async () => note(), local, { indexedDB: idb });
  await again.ready;
  assert.equal(again.open(note()).edit.body, 'short again');
});

test('when the large store cannot open the draft falls back and the failure is shown once', async () => {
  const idb = fakeIndexedDB({ failOpen: true });
  let warning = 0;
  const local = storage();
  let clock = 1000;
  const m = manager(async () => new Promise(() => {}), local, { indexedDB: idb, storageError: () => warning++, now: () => clock });
  await m.ready;
  const s = m.open(note());
  m.changed(s, edit(big(SMALL_DRAFT + 1)));
  await settled();
  assert.equal(JSON.parse(local.getItem(s.key)).edit.body.length, SMALL_DRAFT + 1, 'kept in localStorage instead');
  assert.equal(s.durableRevision, s.revision);
  assert.equal(warning, 0);
  const tight = limited();
  const n = manager(async () => new Promise(() => {}), tight, { indexedDB: fakeIndexedDB({ failOpen: true }), storageError: () => warning++, now: () => clock });
  await n.ready;
  const t = n.open(note());
  n.changed(t, edit(big(SMALL_DRAFT * 3)));
  await settled();
  clock += 1000;
  n.changed(t, edit(big(SMALL_DRAFT * 3 + 1)));
  await settled();
  assert.equal(warning, 1);
  assert.equal(t.persisted, false);
});

test('a saved large note leaves no draft behind in either store', async () => {
  const idb = fakeIndexedDB();
  const local = storage();
  const m = manager(async (_, { body }) => ({ ...note(), ...body, version: 'v2' }), local, { indexedDB: idb });
  await m.ready;
  const s = m.open(note());
  idb.ctl.hold = true;
  m.changed(s, edit(big(SMALL_DRAFT + 1)));
  const saved = m.flush(s);
  idb.ctl.release();
  await saved;
  await settled();
  await settled();
  assert.equal(m.unsaved(), false);
  assert.equal(idb.ctl.data.size, 0);
  assert.equal(local.length, 0);
  const again = manager(async () => note(), local, { indexedDB: idb });
  await again.ready;
  assert.equal(again.unsaved(), false);
});

test('a live edit read through getters is kept as it is and survives a conflict copy', async () => {
  let text = 'Typed body';
  let joins = 0;
  const live = { get title() { return 'Note'; }, get body() { joins++; return text; } };
  const requests = [];
  const m = manager(async (path, options) => {
    requests.push(options);
    if (options.method === 'PATCH') throw Object.assign(new Error('conflict'), { status: 409 });
    return { ...note(), ...options.body, id: 'copy', version: 'copy-v1' };
  }, storage(), { now: () => 1000 });
  const s = m.open(note());
  m.changed(s, live);
  joins = 0;
  m.changed(s, live);
  assert.equal(joins, 0, 'a throttled keystroke reads no text');
  assert.equal(s.edit, live);
  text = 'Typed body, more';
  await m.flush(s);
  assert.equal(requests[0].body.body, 'Typed body, more');
  assert.equal(requests[1].body.title, 'Note (conflict from leo)');
  assert.equal(s.note.id, 'copy');
  assert.equal(m.unsaved(), false);
});
