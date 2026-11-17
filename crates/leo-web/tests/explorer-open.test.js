const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('compact note selection honors the latest click in either response order', async () => {
  for (const reverse of [false, true]) {
    const pending = new Map(), opened = [];
    const box = { isConnected: true };
    const context = {
      seq: 1, state: { session: null }, openingNote: null,
      api: url => new Promise(resolve => pending.set(url.split('/').pop(), resolve)),
      enc: encodeURIComponent, sideWide: () => false, noteHash: id => `#/n/${id}`,
      go: async hash => { opened.push(hash.split('/').pop()); return true; },
      showNote: async id => {
        opened.push(id);
        context.seq++;
        context.state.session = { note: { id }, doc: { editStart() {} } };
      },
      history: { pushState() {} }, closeSheet: () => { box.isConnected = false; },
      rememberPlace() {}, loadSideFolders: async () => {}, fail: e => { throw e; },
    };
    const source = fs.readFileSync(path.join(__dirname, '../src/web/app/side.js'), 'utf8');
    vm.createContext(context);
    vm.runInContext('let explorerOpenSeq = 0;\n' + source.slice(source.indexOf('async function openExplorerNote('), source.indexOf('function explorerTarget(')), context);
    const row = id => ({ dataset: { id }, closest: () => box });
    const first = context.openExplorerNote(row('first'));
    const latest = context.openExplorerNote(row('latest'));
    for (const id of reverse ? ['latest', 'first'] : ['first', 'latest']) {
      pending.get(id)({ id });
      await (id === 'first' ? first : latest);
    }
    assert.deepEqual(opened, ['latest']);
  }
});

test('reopening the open note from the Folders sheet waits for its save instead of drawing an earlier copy', async () => {
  const fetched = [], went = [];
  let drawn = 0;
  const context = {
    seq: 1, state: { session: { note: { id: 'open' } } },
    api: async url => { fetched.push(url); return { id: 'open' }; },
    enc: encodeURIComponent, sideWide: () => false, noteHash: id => `#/n/${id}`,
    go: async (hash, options) => { went.push([hash, options]); }, fail: e => { throw e; },
    side: { target: 'Folder' }, drawSide() { drawn++; },
  };
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/side.js'), 'utf8');
  vm.createContext(context);
  vm.runInContext('let explorerOpenSeq = 0;\n' + source.slice(source.indexOf('async function openExplorerNote('), source.indexOf('function explorerTarget(')), context);
  await context.openExplorerNote({ dataset: { id: 'open' }, closest: () => ({ isConnected: true }) });
  assert.deepEqual(fetched, []);
  assert.deepEqual(went, [['#/n/open', undefined]]);
  assert.equal(context.side.target, null);
  assert.equal(drawn, 1);
});

const web = path.join(__dirname, '../src/web');
const between = (source, from, to) => {
  const start = source.indexOf(from);
  assert.ok(start >= 0, `missing ${from}`);
  const end = source.indexOf(to, start);
  assert.ok(end > start, `missing ${to}`);
  return source.slice(start, end);
};
const tick = () => new Promise((resolve) => setImmediate(resolve));
const settle = async (done) => { for (let i = 0; i < 200 && !done(); i++) await tick(); };

function explorer(open, answer = (name) => [{ id: `${name}-1`, title: name, created_at: '2020-01-01T00:00:00Z' }]) {
  const source = fs.readFileSync(path.join(web, 'app/side.js'), 'utf8');
  const lines = source.split('\n').filter((line) => /^const (side|SIDE_NOTES_\w+|SIDE_SORTS|SIDE_NEW_MS) =/.test(line)).join('\n');
  const drawHead = between(source, 'function drawSide()', "  const scrolled = box.querySelector('.side-tree');");
  const pieces = [
    lines,
    'const creationDir = (explicit) => explicit ?? side.target ?? (state.view === \'note\' ? state.dir || \'\' : \'\');',
    between(source, 'function sidePlace()', 'function drawSide()'),
    `${drawHead}  drawn++;\n}`,
    between(source, 'function sideHere()', '// Like VS Code'),
    between(source, 'function folderTree(', 'function sideKids('),
    between(source, 'async function loadSideFolders()', 'const sideShown ='),
    between(source, 'function loadSideNotes(', 'function sideNoteMark('),
  ].join('\n');
  const frames = [];
  const reads = new Map();
  const folders = open.map((name) => ({ name, notes: 0 }));
  const context = {
    AbortController, setTimeout, clearTimeout, Date, Math, URLSearchParams, Number, Array, Set, Map, JSON,
    requestAnimationFrame: (fn) => { frames.push(fn); return frames.length; },
    esc: String, ICON: new Proxy({}, { get: () => '' }), sideNoteRows: () => '', loadHomeRecent() {}, drawHomeToday() {}, inkLand() {},
    tasksAtHome: () => false, sideWide: () => true, sideActive: () => true,
    $: () => ({ hidden: false }), sideGrip: {}, sideResize: { fit() {} },
    api: async (url) => {
      if (url === '/api/folders') return folders;
      if (url.startsWith('/api/notes?')) {
        const dir = new URLSearchParams(url.split('?')[1]).get('dir');
        const count = (reads.get(dir) || 0) + 1;
        reads.set(dir, count);
        const items = answer(dir, count);
        return { items, total: items.length };
      }
      throw new Error(url);
    },
  };
  vm.createContext(context);
  vm.runInContext(`let seq = 1, drawn = 0; let state = { view: 'note', dir: '', session: { note: { id: 'n' } } };\n${pieces}\n;globalThis.h = { side, loadSideFolders, bump: () => ++seq, draws: () => drawn, resetDraws: () => { drawn = 0; } };`, context);
  context.h.side.open = new Set(open);
  context.h.flush = () => { while (frames.length) frames.shift()(); };
  return context.h;
}

const idle = (h) => settle(() => !h.side.reading && !h.side.pending.length).then(tick);

test('an unchanged refresh of 8 open folders draws the explorer once', async () => {
  for (const k of [0, 8, 32]) {
    const h = explorer(Array.from({ length: k }, (_, i) => `f${i}`));
    await h.loadSideFolders();
    await idle(h);
    h.flush();
    h.resetDraws();
    h.bump();
    await h.loadSideFolders();
    await idle(h);
    h.flush();
    assert.equal(h.draws(), 1, `${k} open folders`);
  }
});

test('a folder whose notes changed is drawn in one more frame', async () => {
  const names = Array.from({ length: 8 }, (_, i) => `f${i}`);
  const h = explorer(names, (name, count) => [{ id: `${name}-1`, title: name, created_at: '2020-01-01T00:00:00Z' }, ...(name === 'f3' && count > 1 ? [{ id: 'f3-2', title: 'new', created_at: '2020-01-01T00:00:00Z' }] : [])]);
  await h.loadSideFolders();
  await idle(h);
  h.flush();
  h.resetDraws();
  h.bump();
  await h.loadSideFolders();
  await idle(h);
  h.flush();
  assert.equal(h.draws(), 2);
});
