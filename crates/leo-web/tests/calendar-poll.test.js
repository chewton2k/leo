const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../src/web/app/calendar.js'), 'utf8');
const between = (from, to) => {
  const start = source.indexOf(from);
  assert.ok(start >= 0, `missing ${from}`);
  const end = source.indexOf(to, start);
  assert.ok(end > start, `missing ${to}`);
  return source.slice(start, end);
};
const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

function harness({ view = 'calendar', visible = true } = {}) {
  const list = [];
  let next = 1;
  let now = 1000000;
  const asked = [];
  const context = {
    setTimeout(fn, ms) { const id = next++; list.push({ id, fn, ms }); return id; },
    clearTimeout(id) { const at = list.findIndex((t) => t.id === id); if (at >= 0) list.splice(at, 1); },
    Date: class extends Date { static now() { return now; } },
    JSON, enc: String, S: require('../src/web/schedule.js'), asked,
    appVisible: () => context.visible, visible,
    Locked: class {}, drawCalendar() {}, drawTasksWindow() {}, loadHomeWeek() {}, tasksAtHome: () => false, calWindow: () => ({ from: 'f', to: 't' }),
    api: async (url) => { asked.push(url.split('?')[0]); return url.startsWith('/api/schedule') ? { events: [], tasks: [] } : { tasks: [] }; },
  };
  vm.createContext(context);
  const code = [
    'const CAL_POLL_MS = 150000;',
    'const CAL_TASKS_GRACE_MS = 5000;',
    between('async function loadSchedule(', 'function calSettled()'),
    between('function calendarShown()', 'document.addEventListener'),
    between('async function loadTasksWindow()', "setTimeout(() => loadTasksWindow().catch"),
  ].join('\n');
  vm.runInContext(
    `let seq = 1, state = { view: ${JSON.stringify(view)} }; const cal = { view: 'week', polls: 0, loadedAt: 0 }; const tasksWin = { load: 0, revision: 0, tasks: null, stale: false, timer: 0, at: 0 }; const homeWeek = { at: 0 };\n${code}\n;globalThis.h = { loadSchedule, loadTasksWindow, calendarShown, tasksWin, state, cal };`,
    context,
  );
  return {
    h: context.h, asked, list, context,
    async period() {
      now += 150000;
      const due = list.filter((t) => t.ms === 150000);
      for (const t of due) { list.splice(list.indexOf(t), 1); t.fn(); }
      await settle();
    },
  };
}

test('one 150 s period on the Calendar asks for the schedule and not the tasks', async () => {
  const t = harness();
  await t.h.loadSchedule(1);
  await t.h.loadTasksWindow();
  t.asked.length = 0;
  await t.period();
  assert.deepEqual(t.asked, ['/api/schedule']);
});

test('leaving the Calendar resumes the tasks poll', async () => {
  const t = harness();
  await t.h.loadSchedule(1);
  await t.h.loadTasksWindow();
  t.h.state.view = 'folder';
  t.asked.length = 0;
  await t.period();
  assert.deepEqual(t.asked, ['/api/tasks']);
});

test('coming back to a hidden Calendar does not ask for tasks separately', async () => {
  const t = harness();
  await t.h.loadSchedule(1);
  await t.h.loadTasksWindow();
  t.context.visible = false;
  await t.period();
  assert.equal(t.h.tasksWin.stale, true);
  t.context.visible = true;
  t.asked.length = 0;
  t.list.length = 0;
  t.h.cal.loadedAt = 1e12;
  t.h.calendarShown();
  await settle();
  assert.deepEqual(t.asked.filter((url) => url === '/api/tasks'), []);
  assert.equal(t.h.tasksWin.stale, false);
  assert.ok(t.list.some((timer) => timer.ms === 150000), 'a tasks timer is armed');
});

test('coming back outside the Calendar still reloads stale tasks', async () => {
  const t = harness();
  await t.h.loadTasksWindow();
  t.context.visible = false;
  await t.period();
  assert.equal(t.h.tasksWin.stale, true);
  t.context.visible = true;
  t.h.state.view = 'folder';
  t.asked.length = 0;
  t.h.calendarShown();
  await settle();
  assert.deepEqual(t.asked, ['/api/tasks']);
});
