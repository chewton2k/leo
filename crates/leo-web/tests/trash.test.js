const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function page() {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/trash.js'), 'utf8');
  const helpers = source.slice(source.indexOf('function trashCounted('), source.indexOf('function drawTrash('));
  const context = {
    plural: (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`,
    esc: (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;'),
    rel: () => 'today',
    ICON: { trash: '<svg></svg>' },
  };
  vm.createContext(context);
  vm.runInContext(helpers, context);
  return context;
}

test('the trash counts notes and pictures', () => {
  const { trashCounted } = page();
  assert.equal(trashCounted([{ kind: 'note' }, { kind: 'note' }]), '2 notes');
  assert.equal(trashCounted([{ kind: 'picture' }]), '1 picture');
  assert.equal(trashCounted([{ kind: 'note' }, { kind: 'picture' }, { kind: 'picture' }]), '1 note and 2 pictures');
});

test('a picture removed from its note is a row with Restore and Delete for good', () => {
  const { trashRow } = page();
  const row = trashRow({ id: 'picture:20261117-101500-slide.png', title: '20261117-101500-slide.png', kind: 'picture', readable: true }, false, new Set());
  assert.match(row, /Picture · removed from its note/);
  assert.match(row, /data-trash-keep data-id="picture:20261117-101500-slide.png">Restore/);
  assert.match(row, /data-action="trash-forget" data-id="picture:20261117-101500-slide.png"/);
  assert.doesNotMatch(row, /data-action="restore"/);
  const picked = trashRow({ id: 'picture:x.png', title: 'x.png', kind: 'picture' }, true, new Set(['picture:x.png']));
  assert.match(picked, /data-trash-pick value="picture:x.png" checked/);
});

test('a trashed note says how many pictures it carries', () => {
  const { trashRow } = page();
  const row = trashRow({ id: 'n1', title: 'Lecture', kind: 'note', directory: 'cs', readable: true, pictures: 2 }, false, new Set());
  assert.match(row, /\/cs · deleted today · 2 pictures/);
  assert.match(row, /data-action="restore" data-id="n1"/);
  assert.doesNotMatch(trashRow({ id: 'n2', title: 'Plain', kind: 'note', readable: true }, false, new Set()), /picture/);
});

test('an older picture no note uses can only be deleted, and a removed note\'s files can be restored', () => {
  const { trashRow, trashCounted } = page();
  const unused = trashRow({ id: 'unused:20261117-101500-old.png', title: '20261117-101500-old.png', kind: 'unused', readable: true }, false, new Set());
  assert.match(unused, /Picture no note uses/);
  assert.doesNotMatch(unused, /Restore/);
  assert.match(unused, /data-action="trash-forget" data-id="unused:20261117-101500-old.png"/);
  const held = trashRow({ id: 'held:n1', title: 'Files of a removed note', kind: 'held', readable: true }, false, new Set());
  assert.match(held, /Kept for a while after its note left/);
  assert.match(held, /data-trash-keep data-id="held:n1">Restore/);
  assert.equal(trashCounted([{ kind: 'note' }, { kind: 'unused' }, { kind: 'held' }]), '1 note, 1 picture and 1 set of files');
});

test('deleting for good sends the revisions the page showed', () => {
  const { trashRevisions } = page();
  const items = [
    { id: 'n1', kind: 'note', revision: 'aa' },
    { id: 'unused:x.png', kind: 'unused', revision: 'bb' },
    { id: 'held:n2', kind: 'held', revision: 'cc' },
    { id: 'held:n3', kind: 'held' },
  ];
  assert.deepEqual({ ...trashRevisions(items, ['n1'], false) }, { n1: 'aa' });
  assert.deepEqual({ ...trashRevisions(items, ['held:n2'], false) }, { 'held:n2': 'cc' });
  assert.deepEqual({ ...trashRevisions(items, [], true) }, { n1: 'aa', 'unused:x.png': 'bb', 'held:n2': 'cc' });
});

test('a trash item that changed while the confirmation was open is asked about again', async () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/trash.js'), 'utf8');
  const body = source.slice(source.indexOf('function trashRevisions('), source.indexOf('function trashRow(')) +
    source.slice(source.indexOf('function trashAsk('), source.indexOf('async function trashRestorePicked('));
  const sent = [];
  const asked = [];
  const toasts = [];
  const context = {
    state: { items: [{ id: 'n1', kind: 'note', title: 'Lecture', revision: 'old' }] },
    plural: (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`,
    esc: (s) => s,
    trashCounted: (items) => `${items.length} notes`,
    closeSheet: () => {},
    confirmSheet: (sheet) => asked.push(sheet),
    toast: (text) => toasts.push(text),
    api: async (_path, { body }) => {
      sent.push(body);
      const error = new Error('Something in the trash changed since this page showed it.');
      error.status = 409;
      throw error;
    },
    showTrash: async () => {
      context.state.items = [{ id: 'n1', kind: 'note', title: 'Lecture', revision: 'new' }];
    },
  };
  vm.createContext(context);
  vm.runInContext(body, context);
  context.trashAsk(['n1'], false);
  await context.trashDeleteNow();
  assert.deepEqual({ ...sent[0].revisions }, { n1: 'old' });
  assert.match(toasts[0], /changed since this page showed it/);
  assert.equal(asked.length, 2);
  assert.deepEqual({ ...context.state.trashPending.revisions }, { n1: 'new' });
});
