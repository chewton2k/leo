const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../src/web/app/calendar.js'), 'utf8');
const between = (from, to) => source.slice(source.indexOf(from), source.indexOf(to, source.indexOf(from)));

function harness({ tasks, window = [], total, rows = 50 }) {
  const asked = [];
  const context = {
    cal: { data: { tasks }, trayRows: rows }, tasksWin: { tasks, window, total },
    asked, drawn: 0, drawCalendar() { context.drawn++; },
    calDays: () => ['2026-11-03'], calPlan: () => ({ overdue: context.overdue, undated: [] }), overdue: [],
    tasksMore: async () => { asked.push('more'); },
  };
  vm.createContext(context);
  vm.runInContext(`const CAL_TRAY_ROWS = 50;\n${between('function calFind(', 'function calMovable(')}\n${between('async function calTrayMore(', 'function drawCalendar(')}\n;globalThis.calFind = calFind; globalThis.withWindow = withWindow; globalThis.calTrayMore = calTrayMore;`, context);
  return context;
}

test('a task that is only in the window is found and planned with the page', () => {
  const h = harness({ tasks: [{ id: 'a' }], window: [{ id: 'b' }, { id: 'a' }], total: 900 });
  assert.equal(h.calFind('b').id, 'b');
  assert.equal(h.calFind('zzz'), null);
  assert.equal(h.withWindow(h.tasksWin.tasks).map((t) => t.id).join(), 'a,b');
});

test('without a window the page is used as it is', () => {
  const tasks = [{ id: 'a' }];
  const h = harness({ tasks, window: undefined, total: 1 });
  assert.equal(h.withWindow(tasks), tasks);
});

test('Show more in the tray reveals loaded rows before it asks for more', async () => {
  const h = harness({ tasks: Array.from({ length: 500 }, (_, i) => ({ id: `t${i}` })), total: 900 });
  h.overdue = Array.from({ length: 500 }, (_, i) => ({ id: `t${i}` }));
  await h.calTrayMore();
  assert.equal(h.cal.trayRows, 100);
  assert.equal(h.asked.length, 0);
  assert.equal(h.drawn, 1);
});

test('Show more in the tray fetches the next page once every loaded row is shown', async () => {
  const h = harness({ tasks: Array.from({ length: 40 }, (_, i) => ({ id: `t${i}` })), total: 900 });
  h.overdue = Array.from({ length: 40 }, (_, i) => ({ id: `t${i}` }));
  await h.calTrayMore();
  assert.equal(h.asked.join(), 'more');
});
