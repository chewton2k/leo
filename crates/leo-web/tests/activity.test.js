const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function context() {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/settings.js'), 'utf8');
  const value = {};
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('function faNiceMax'), source.indexOf('function faFold')), value);
  return value;
}

test('the chart maximum is the next 1, 2, 2.5 or 5 times a power of ten', () => {
  const { faNiceMax } = context();
  for (const [value, max] of [[0, 1], [0.7, 1], [1, 1], [1.5, 2], [2.2, 2.5], [3, 5], [5, 5], [6, 10], [180, 200], [232500, 250000], [250000, 250000], [250001, 500000], [1000, 1000]]) assert.equal(faNiceMax(value), max, String(value));
});

test('models belong to the maker that trains them and shade by rank, a fourth going to Other', () => {
  const { faFamily, faShades } = context();
  for (const [name, family] of [['gpt-6.1-sol', 'openai'], ['o3-mini', 'openai'], ['codex-mini', 'openai'], ['gpt-oss:20b', 'local'], ['claude-sonnet-5-5', 'claude'], ['Opus', 'claude'], ['llama3.1', 'local'], ['', 'local']]) assert.equal(faFamily(name), family, name);
  const shades = faShades({ models: ['gpt-a', 'claude-a', 'gpt-b', 'gpt-c', 'gpt-d', 'llama3.1'] });
  assert.equal(shades['gpt-a'], 'var(--fam-openai-1)');
  assert.equal(shades['gpt-c'], 'var(--fam-openai-3)');
  assert.equal(shades['gpt-d'], undefined);
  assert.equal(shades['claude-a'], 'var(--fam-claude-1)');
  assert.equal(shades['llama3.1'], 'var(--fam-local-1)');
});

test('Felix activity dates use the page language', () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/settings.js'), 'utf8');
  const value = {};
  value.S = require('../src/web/schedule.js');
  value.calLocale = () => 'de-DE';
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('function faToday'), source.indexOf('function faBarName')), value);
  const march = new Date(2026, 2, 5);
  assert.equal(value.faDate('2026-03-05'), march.toLocaleDateString('de-DE', { month: 'short', day: 'numeric' }));
  assert.notEqual(value.faDate('2026-03-05'), march.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }));
  assert.equal(value.faDate('2026-03-05', true), march.toLocaleDateString('de-DE', { weekday: 'short', month: 'short', day: 'numeric' }));
  assert.equal(value.faDate('2026-03-05 14', true), new Date(2026, 2, 5, 14).toLocaleString('de-DE', { weekday: 'short', hour: 'numeric' }));
  assert.equal(value.faExact(1234567), (1234567).toLocaleString('de-DE'));
  assert.equal(value.faToday(), value.S.dayKey(new Date()));
});

test('tick moments older than the tick animation are dropped when a task is ticked', async () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/calendar.js'), 'utf8');
  let clock = 0;
  const value = {
    performance: { now: () => clock }, calFind: () => null, markTaskDone() {}, api: async () => ({}), enc: String,
    tasksWin: { revision: 0 }, taskDoneOn: () => true, state: {}, toast() {}, saving: {}, showNote() {},
  };
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('const TICK_MS'), source.indexOf('function pickerBody')) + '\nglobalThis.size = () => tickedAt.size;', value);
  for (let n = 0; n < 1000; n++) {
    await value.setTaskDone(`t${n}`, true);
    clock += 1000;
  }
  assert.equal(value.size(), 1);
});

test('previews of attached notes are capped at the 200 most recent', () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/chat.js'), 'utf8');
  const value = { request: () => new Promise(() => {}), encodeURIComponent };
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('    const excerpts = new Map();'), source.indexOf('    function drawRefs()')) + '\nglobalThis.excerpts = excerpts;', value);
  for (let n = 0; n < 1000; n++) value.excerptOf({ id: `n${n}`, title: 'x' });
  assert.equal(value.excerpts.size, 200);
  assert.equal(value.excerpts.has('n999'), true);
  assert.equal(value.excerpts.has('n799'), false);
});
