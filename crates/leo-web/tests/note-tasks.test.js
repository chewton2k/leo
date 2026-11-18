const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../src/web/app/calendar.js'), 'utf8');
const actions = fs.readFileSync(path.join(__dirname, '../src/web/app/actions.js'), 'utf8');
const start = source.indexOf('const NOTE_TASKS_PAGE');
const code = source.slice(start, source.indexOf('async function openTaskAnywhere('));

function harness({ checkboxes, tracked = [], pages = null }) {
  const body = Array.from({ length: checkboxes }, (_, i) => `- [ ] item ${i}`).join('\n');
  const rows = { html: '', foot: null };
  const urls = [];
  const sheets = [];
  const all = tracked.map((line) => ({ id: `task-${line}`, line, note: { id: 'n1' } }));
  const context = {
    state: { view: 'note', session: { note: { id: 'n1' }, doc: { source: () => body } } },
    md: { MARKER: String.raw`[-*+]` },
    ready: async () => true, Locked: class {}, enc: encodeURIComponent, esc: String, toast() {},
    urls, sheets,
    api: async (url) => {
      urls.push(url);
      const at = Number(/offset=(\d+)/.exec(url)?.[1] || 0);
      const limit = Number(/limit=(\d+)/.exec(url)?.[1] || 500);
      return pages ? pages(at, limit, all) : { tasks: all.slice(at, at + limit), total: all.length };
    },
    sheet: (html) => { sheets.push(html); rows.html = html; },
    sheetButtons: () => '',
    go() {},
    $: (selector) => (selector === '#note-task-rows' ? { insertAdjacentHTML: (_, html) => { rows.html += html; } } : selector === '#note-task-foot' ? rows.foot : null),
  };
  rows.foot = { innerHTML: '', removed: false, remove() { this.removed = true; } };
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.noteTasksSheet = noteTasksSheet; globalThis.noteTasksMore = typeof noteTasksMore === 'function' ? noteTasksMore : null; globalThis.promoteTask = promoteTask;`, context);
  context.rows = rows;
  return context;
}

const count = (html) => (html.match(/class="set-row"/g) || []).length;

test('a note with 2,000 checkboxes lists them all', async () => {
  const h = harness({ checkboxes: 2000 });
  await h.noteTasksSheet();
  let guard = 0;
  while (!h.rows.foot.removed && guard++ < 100) await h.noteTasksMore();
  assert.equal(count(h.rows.html), 2000);
});

test('the first page is 100 rows with a Show more that says how many are left', async () => {
  const h = harness({ checkboxes: 250 });
  await h.noteTasksSheet();
  assert.equal(count(h.sheets[0]), 100);
  assert.match(h.sheets[0], /data-action="note-tasks-more"/);
  assert.match(h.sheets[0], /150 more/);
});

test('Show more adds the next page and removes itself at the end', async () => {
  const h = harness({ checkboxes: 250 });
  await h.noteTasksSheet();
  await h.noteTasksMore();
  assert.equal(count(h.rows.html), 200);
  assert.equal(h.rows.foot.removed, false);
  assert.match(h.rows.foot.innerHTML, /50 more/);
  await h.noteTasksMore();
  assert.equal(count(h.rows.html), 250);
  assert.equal(h.rows.foot.removed, true);
});

test('a note with 100 checkboxes or fewer has no Show more', async () => {
  const h = harness({ checkboxes: 100 });
  await h.noteTasksSheet();
  assert.equal(count(h.sheets[0]), 100);
  assert.doesNotMatch(h.sheets[0], /note-tasks-more/);
});

test('the tracked lookup does not search the task list per checkbox', async () => {
  const lines = Array.from({ length: 10000 }, (_, i) => i);
  const h = harness({ checkboxes: 100, tracked: lines });
  vm.runInContext('Array.prototype.find = function () { throw new Error("find was called"); };', h);
  await h.noteTasksSheet();
  assert.equal((h.sheets[0].match(/In your tasks/g) || []).length, 100);
  assert.match(h.sheets[0], /data-id="task-99"/);
});

test('a note with 620 tracked checkboxes knows all 620', async () => {
  const lines = Array.from({ length: 620 }, (_, i) => i);
  const h = harness({ checkboxes: 620, tracked: lines });
  await h.noteTasksSheet();
  while (!h.rows.foot.removed) await h.noteTasksMore();
  assert.equal((h.rows.html.match(/In your tasks/g) || []).length, 620);
  assert.match(h.rows.html, /data-id="task-619"/);
  assert.deepEqual(h.urls, ['/api/tasks?note=n1&offset=0&limit=500', '/api/tasks?note=n1&offset=500&limit=500']);
});

test('a library that answers without a total is read once', async () => {
  const h = harness({ checkboxes: 3, tracked: [0], pages: (at, limit, all) => ({ tasks: all.slice(at, at + limit) }) });
  await h.noteTasksSheet();
  assert.equal(h.urls.length, 1);
  assert.match(h.sheets[0], /data-id="task-0"/);
});

test('promoting a checkbox far down keeps the rows already shown', async () => {
  const h = harness({ checkboxes: 450 });
  await h.noteTasksSheet();
  await h.noteTasksMore();
  await h.noteTasksMore();
  await h.noteTasksMore();
  assert.equal(count(h.rows.html), 400);
  const asked = h.sheets.length;
  const real = h.api;
  h.api = async (url, opts) => (opts && opts.method === 'POST' ? { title: 'item 350' } : real(url, opts));
  await h.promoteTask('350');
  assert.equal(h.sheets.length, asked + 1);
  assert.equal(count(h.sheets[asked]), 400);
});

test('the Show more action is wired', () => {
  assert.match(actions, /'note-tasks-more': \(\) => noteTasksMore\(\)/);
});
