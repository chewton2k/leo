const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { paneHelpers } = require('./page-source');
const source = fs.readFileSync(path.join(__dirname, '../src/web/app/uploads.js'), 'utf8');
const slice = (from, to) => source.slice(source.indexOf(from), source.indexOf(to));
const head = slice('const UPLOAD_PARALLEL', 'async function shrink(');
const sending = slice('async function sendFile(', 'function watchUpload(');
assert.ok(head && sending, 'missing upload sources');

class Locked extends Error {}
class Offline extends Error {}

function page({ reject = () => false } = {}) {
  const log = { puts: [], posts: [], toasts: [], progress: [], flight: 0, most: 0 };
  const context = {
    Locked, Offline, URL, Math, Promise, Array, Date,
    enc: encodeURIComponent,
    esc: (t) => String(t),
    crypto: { randomUUID: (() => { let n = 0; return () => `id-${++n}`; })() },
    log,
    felix: { felix: () => '' },
    leoChat: { tookFor: () => '1 s' },
    toast: (text) => log.toasts.push(text),
    closeSheet: () => {},
    sheet: () => ({}),
    checkActivity: () => {},
    watchUpload: (id) => { log.watched = id; },
    addingToNote: () => false,
    uploadNote: null,
    $: (q) => ({ value: '', checked: true, ...(q === '#upload-progress' ? { innerHTML: '' } : {}) }),
    fetch: async (url, init) => {
      log.flight += 1;
      log.most = Math.max(log.most, log.flight);
      await new Promise((r) => setTimeout(r, 2));
      log.flight -= 1;
      log.puts.push({ url, init });
      const name = new URL(url, 'http://x').searchParams.get('name');
      return reject(name) ? { ok: false, status: 507, json: async () => ({ error: `There is not enough room on this disk for ${name}.` }) } : { ok: true, status: 204 };
    },
    api: async (url, options) => {
      log.posts.push({ url, options });
      return { id: 'job-1' };
    },
    app: {}, marginPage: null,
  };
  vm.createContext(context);
  vm.runInContext(paneHelpers(), context);
  vm.runInContext(`${head}\nasync function shrink(file) { return { name: file.name, type: file.type, blob: file }; }\n${sending}\npicked = []; this.run = async (files) => { picked = files; await uploadGo(); }; this.sendFiles = sendFiles;`, context);
  return context;
}

test('the upload read-hint listener can run with or without the shared helper', () => {
  const area = {};
  for (const withHelper of [false, true]) {
    let input, updated;
    const document = { addEventListener: (event, fn) => { if (event === 'input') input = fn; } };
    const context = { document };
    if (withHelper) context.updateReadHint = (field) => { updated = field; };
    vm.runInNewContext(slice('const UPLOAD_READS', 'function noteFields('), context);
    input({ target: { closest: () => area } });
    assert.equal(updated, withHelper ? area : undefined);
  }
});

const files = (n) => Array.from({length: n}, (_, i) => ({ name: `file ${i}.pdf`, type: 'application/pdf', size: 10 + i }));

test('25 chosen files send 25 PUTs and one POST, with no file content in the JSON', async () => {
  const c = page();
  await c.run(files(25));
  assert.equal(c.log.puts.length, 25);
  assert.ok(c.log.puts.every((p) => p.init.method === 'PUT' && p.url.startsWith('/api/import/files/') && p.init.body.name));
  assert.equal(c.log.posts.length, 1);
  const body = c.log.posts[0].options.body;
  assert.equal(c.log.posts[0].url, '/api/import');
  assert.equal(body.files.length, 25);
  assert.deepEqual(body.files.map((f) => f.name), files(25).map((f) => f.name));
  assert.ok(body.upload);
  assert.ok(body.files.every((f) => f.item && !('data' in f)));
  assert.equal(new Set(body.files.map((f) => f.item)).size, 25);
  assert.equal(c.log.watched, 'job-1');
});

test('no file count or size is refused by the page', async () => {
  const c = page();
  const big = [{ name: 'book.pdf', type: 'application/pdf', size: 5 * 1024 * 1024 * 1024 }, ...files(40)];
  await c.run(big);
  assert.equal(c.log.puts.length, 41);
  assert.equal(c.log.toasts.length, 0);
});

test('files are sent two at a time', async () => {
  const c = page();
  await c.run(files(10));
  assert.equal(c.log.most, 2);
});

test('a file that fails is named and the others still make their notes', async () => {
  const c = page({ reject: (name) => name === 'file 1.pdf' });
  await c.run(files(3));
  const body = c.log.posts[0].options.body;
  assert.deepEqual(body.files.map((f) => f.name), ['file 0.pdf', 'file 2.pdf']);
  assert.match(c.log.toasts[0], /file 1\.pdf could not be sent/);
});

test('when every file fails nothing is posted', async () => {
  const c = page({ reject: () => true });
  await c.run(files(2));
  assert.equal(c.log.posts.length, 0);
  assert.match(c.log.toasts[0], /file 0\.pdf, file 1\.pdf could not be sent/);
});

test('a picture of any size is sent as one streamed PUT of its blob, with no size message', async () => {
  const c = page();
  const pictures = slice('async function pictureFor(', 'function choosePictures(');
  const calls = [];
  const inserted = [];
  Object.assign(c, {
    state: { session: null },
    shrink: async (file) => ({ name: file.name, type: file.type, blob: file }),
    fail: (e) => calls.push(`failed: ${e.message}`),
  });
  const session = { doc: { insert: (text) => inserted.push(text) } };
  c.state.main = c.state.session = session;
  c.fetch = async (url, init) => {
    c.log.puts.push({ url, init });
    return { ok: true, status: 201, json: async () => ({ path: 'attachments/a.png' }) };
  };
  vm.runInContext(`${pictures}\nthis.addPictures = addPictures;`, c);
  const big = { name: 'scan.png', type: 'image/png', size: 900 * 1024 * 1024 };
  await c.addPictures(session, [big]);
  assert.equal(c.log.puts.length, 1);
  assert.equal(c.log.puts[0].url, '/api/pictures/scan.png');
  assert.equal(c.log.puts[0].init.method, 'PUT');
  assert.equal(c.log.puts[0].init.body, big);
  assert.deepEqual(calls, []);
  assert.deepEqual(inserted, ['![scan](attachments/a.png)']);
});

test('a section added to a note is named by its job and its text is never sent back', async () => {
  const c = page();
  const calls = [];
  const toasts = [];
  Object.assign(c, {
    state: { view: 'workspace', session: null },
    ready: async () => null,
    showNote: async () => {},
    toast: (text) => toasts.push(text),
    api: async (url, options = {}) => {
      calls.push({ url, options });
      return url.endsWith('/insert') ? { version: 'v2', before: 'old' } : { version: 'v1' };
    },
  });
  vm.runInContext(`${slice('async function addSection(', 'async function showLatest(')}\nthis.addSection = addSection;`, c);
  await c.addSection({ id: 'n1', title: 'Heaps' }, 'job-9');
  const insert = calls.find((call) => call.url === '/api/notes/n1/insert');
  assert.deepEqual(JSON.parse(JSON.stringify(insert.options.body)), { job: 'job-9', line: null, base: 'v1' });
  assert.deepEqual(toasts, ['Added to “Heaps”']);
  calls.length = 0;
  await c.addSection({ id: 'n1', title: 'Heaps' }, '');
  assert.equal(calls.length, 0);
});
