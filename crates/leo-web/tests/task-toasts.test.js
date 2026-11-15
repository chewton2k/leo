const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../src/web/app/calendar.js'), 'utf8');
const saveDate = source.slice(source.indexOf('async function saveDate('), source.indexOf('\n}\n', source.indexOf('async function saveDate(')) + 3);

function harness() {
  const toasts = [];
  const context = {
    S: require('../src/web/schedule.js'), calLocale: () => 'en-US', toasts, api: async () => ({}), enc: encodeURIComponent, esc: String,
    toast: (text) => toasts.push(text), closeSheet() {}, loadTasksWindow: async () => {}, state: { view: 'folder' }, cal: {}, seq: 1, Locked: class {},
  };
  vm.createContext(context);
  vm.runInContext(`${saveDate}\n;globalThis.saveDate = saveDate;`, context);
  return context;
}

test('a date toast names the task and says whether it was set, moved, kept or cleared', async () => {
  const h = harness();
  await h.saveDate({ id: 't', title: 'Essay', date: '' }, { date: '2026-11-20' });
  await h.saveDate({ id: 't', title: 'Essay', date: '2026-11-18' }, { date: '2026-11-20' });
  await h.saveDate({ id: 't', title: 'Essay', date: '2026-11-20' }, { date: '2026-11-20', time: '09:00' });
  await h.saveDate({ id: 't', title: '', date: '2026-11-20' }, { date: '' });
  assert.deepEqual(h.toasts, ['Set “Essay” for Fri, Nov 20', 'Moved “Essay” to Fri, Nov 20', 'Saved “Essay”', 'Date cleared']);
});
