const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const S = require('../src/web/schedule.js');
const sheets = fs.readFileSync(path.join(__dirname, '../src/web/app/sheets.js'), 'utf8');
const calendar = fs.readFileSync(path.join(__dirname, '../src/web/app/calendar.js'), 'utf8');
const between = (text, from, to) => text.slice(text.indexOf(from), text.indexOf(to, text.indexOf(from)));

const today = S.dayKey(new Date());
const day = (n) => S.dayKey(S.addDays(new Date(), n));

function harness(list) {
  const urls = [];
  const context = {
    S, urls, quick: { due: [] }, drawQuickDue() {},
    api: async (url) => {
      urls.push(url);
      const at = Number(/offset=(\d+)/.exec(url)[1]);
      const limit = Number(/limit=(\d+)/.exec(url)[1]);
      return { tasks: list.slice(at, at + limit), total: list.length };
    },
  };
  vm.createContext(context);
  vm.runInContext(`const TASKS_AHEAD_DAYS = 7;\n${between(calendar, 'function tasksGroups(', 'function finishTaskInWindow(')}\n${between(sheets, 'const QUICK_DUE_ROWS', 'function drawQuickDue(')}\n;globalThis.loadQuickDue = loadQuickDue;`, context);
  return context;
}

const task = (id, date, extra = {}) => ({ id, title: id, date, done: false, ...extra });

test('a long list of earlier tasks is read one page, not all of it', async () => {
  const h = harness(Array.from({ length: 900 }, (_, i) => task(`o${i}`, day(-900 + i))));
  await h.loadQuickDue();
  assert.equal(h.urls.length, 1);
  assert.equal(h.quick.due.length, 6);
  assert.ok(h.quick.due.every((t) => t.late));
});

test('a task due today past the first five hundred is found', async () => {
  const later = S.weekdayOf(day(1));
  const repeating = Array.from({ length: 600 }, (_, i) => task(`r${i}`, '2020-01-01', { repeat: [later] }));
  const h = harness([...repeating, task('now', today), task('soon', day(2)), task('undated', null)]);
  await h.loadQuickDue();
  assert.equal(h.quick.due.map((t) => t.id).join(), ['now'].join());
  assert.ok(h.urls.length > 10);
  assert.equal(h.urls.length <= 14, true);
});

test('the read stops at the first task that is not due', async () => {
  const h = harness([task('a', day(-1)), ...Array.from({ length: 400 }, (_, i) => task(`f${i}`, day(3)))]);
  await h.loadQuickDue();
  assert.deepEqual(h.urls, ['/api/tasks?offset=0&limit=50']);
  assert.equal(h.quick.due.map((t) => t.id).join(), ['a'].join());
});

test('a list that answers without a total is read once', async () => {
  const h = harness([task('a', today)]);
  h.api = async () => ({ tasks: [task('a', today)] });
  await h.loadQuickDue();
  assert.equal(h.quick.due.map((t) => t.id).join(), ['a'].join());
});
