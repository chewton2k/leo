const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../src/web/app/side.js'), 'utf8');
const start = source.indexOf('const COMBINE_PAGE');
assert.ok(start >= 0, 'missing COMBINE_PAGE');
const slice = source.slice(start, source.indexOf('async function afterExplorerNote(', start));
function library(notes) {
  const asked = [];
  const context = {
    seq: 1, ICON: {note: ''}, URLSearchParams, enc: encodeURIComponent, asked, menus: [], toasts: [], sheets: [],
    api: async (url) => {
      asked.push(url);
      const query = new URL(url, 'http://x').searchParams;
      const word = (query.get('q') || '').toLowerCase();
      const hits = notes.filter((n) => n.title.toLowerCase().includes(word)).sort((a, b) => a.title.localeCompare(b.title) || a.id.localeCompare(b.id));
      return hits.slice(Number(query.get('offset') || 0), Number(query.get('offset') || 0) + Number(query.get('limit')));
    },
    toast: (text) => context.toasts.push(text),
    popupMenu: (owner, options) => context.menus.push(options),
    combineSheet: (from, into) => context.sheets.push([from, into]),
  };
  vm.createContext(context);
  vm.runInContext(slice, context);
  return context;
}
const make = (count, title = (i) => `Note ${String(i).padStart(4, '0')}`) => Array.from({length: count}, (_, i) => ({id: `id-${String(i).padStart(4, '0')}`, title: title(i), directory: `folder ${i}`}));
const self = {id: 'me', title: 'Me'};
test('a note after the 1,000th can be found by typing its name', async () => {
  const c = library(make(1200));
  await c.explorerCombineMenu(self, {});
  const answer = await c.menus[0].page('Note 1100', 0);
  assert.deepEqual([...answer.rows.map((r) => r.text)], ['Note 1100']);
});
test('the first page asks for 50 and no more', async () => {
  const c = library(make(1200));
  await c.explorerCombineMenu(self, {});
  assert.equal(c.asked.length, 1);
  assert.match(c.asked[0], /limit=51/);
  assert.equal(c.menus[0].rows.length, 50);
  assert.equal(c.menus[0].more, true);
  assert.equal(c.menus[0].loaded, 50);
});
test('a library of a few notes has no Show more', async () => {
  const c = library(make(3));
  await c.explorerCombineMenu(self, {});
  assert.equal(c.menus[0].rows.length, 3);
  assert.equal(c.menus[0].more, false);
});
test('a search asks the page search with the offset, one more than it shows', async () => {
  const c = library(make(200));
  await c.explorerCombineMenu(self, {});
  await c.menus[0].page('note', 0);
  assert.match(c.asked[1], /^\/api\/search\?q=note&brief=true&limit=51&offset=0$/);
});
test('more than 50 notes with one title in different folders: Show more reaches the 51st and its row opens the combine sheet', async () => {
  const c = library(make(120, () => 'Same'));
  await c.explorerCombineMenu(self, {});
  let answer = {rows: c.menus[0].rows, more: c.menus[0].more, loaded: c.menus[0].loaded};
  assert.equal(answer.rows.length, 50);
  assert.ok(answer.rows.every((r) => / · folder \d+$/.test(r.text)));
  answer = await c.menus[0].page('', answer.loaded);
  assert.equal(answer.rows.length, 100);
  assert.equal(answer.more, true);
  assert.equal(answer.rows[50].text, 'Same · folder 50');
  answer.rows[50].run();
  assert.equal(JSON.stringify(c.sheets), JSON.stringify([[{id: 'me', title: 'Me'}, {id: 'id-0050', title: 'Same'}]]));
});
test('Show more under a typed query pages the search', async () => {
  const c = library(make(120));
  await c.explorerCombineMenu(self, {});
  const first = await c.menus[0].page('note', 0);
  assert.equal(first.rows.length, 50);
  const next = await c.menus[0].page('note', first.loaded);
  assert.equal(next.rows.length, 100);
  assert.match(c.asked[2], /offset=50$/);
});
test('the note being combined is never listed', async () => {
  const c = library([...make(5), {id: 'me', title: 'Me', directory: ''}]);
  await c.explorerCombineMenu(self, {});
  assert.equal(c.menus[0].rows.length, 5);
});
test('a library of only this note says there is nothing to combine with', async () => {
  const c = library([{id: 'me', title: 'Me', directory: ''}]);
  await c.explorerCombineMenu(self, {});
  assert.equal(c.menus.length, 0);
  assert.equal(c.toasts.length, 1);
});
test('opening the menu with 50,000 notes does 50 rows of work', async () => {
  const c = library(make(50000));
  const began = performance.now();
  await c.explorerCombineMenu(self, {});
  console.log(`combine menu opened over 50,000 notes: ${(performance.now() - began).toFixed(1)} ms (including the fake api's own sort)`);
  assert.equal(c.menus[0].rows.length, 50);
});
