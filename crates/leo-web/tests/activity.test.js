const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function context() {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/settings.js'), 'utf8');
  const value = {};
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('function faNiceMax'), source.indexOf('function faSlices')), value);
  return value;
}

test('the chart maximum is the next 1, 2, 2.5 or 5 times a power of ten', () => {
  const { faNiceMax } = context();
  for (const [value, max] of [[0, 1], [0.7, 1], [1, 1], [1.5, 2], [2.2, 2.5], [3, 5], [5, 5], [6, 10], [180, 200], [232500, 250000], [250000, 250000], [250001, 500000], [1000, 1000]]) assert.equal(faNiceMax(value), max, String(value));
});

test('models belong to the maker that trains them and every one gets its own shade, never Other', () => {
  const { faFamily, faShades } = context();
  for (const [name, family] of [['gpt-6.1-sol', 'openai'], ['o3-mini', 'openai'], ['codex-mini', 'openai'], ['gpt-oss:20b', 'local'], ['claude-sonnet-5-5', 'claude'], ['Opus', 'claude'], ['llama3.1', 'local'], ['', 'local']]) assert.equal(faFamily(name), family, name);
  const gpts = Array.from({ length: 8 }, (_, i) => `gpt-${i}`);
  const locals = Array.from({ length: 8 }, (_, i) => `local${i}:7b`);
  const shades = faShades([...gpts, 'claude-a', ...locals]);
  assert.equal(shades['gpt-0'], 'var(--fam-openai-1)');
  assert.equal(shades['gpt-6'], 'var(--fam-openai-7)');
  assert.equal(shades['gpt-7'], 'color-mix(in srgb, var(--fam-openai-1), var(--text) 30%)');
  assert.equal(shades['claude-a'], 'var(--fam-claude-1)');
  assert.equal(shades['local2:7b'], 'var(--fam-local-3)');
  assert.equal(shades['local7:7b'], 'color-mix(in srgb, var(--fam-local-2), var(--text) 60%)');
  const every = Object.values(shades);
  assert.equal(new Set(every).size, every.length, 'no two models share a colour');
});

test('costs come from the dated price table, models on this computer are free and unknown ones have no price', () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/settings.js'), 'utf8');
  const value = { window: { leoChat: { money: (n) => `$${n.toFixed(2)}` } } };
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('function faCost'), source.indexOf('const FA_SHADES')), value);
  const price = value.faPrice({ prices: { as_of: '2026-11-21', models: { 'gpt-6.2-sol': [200, 10, 1000] } } });
  assert.deepEqual([...price('gpt-6.2-sol')], [2, 0.1, 10]);
  assert.deepEqual([...price('qwen3:8b')], [0, 0, 0]);
  assert.deepEqual([...price('gpt-oss:20b')], [0, 0, 0]);
  for (const name of ['gpt-oss:120b-cloud', 'gpt-5', 'claude-opus-9', 'gemini-3.8-flash', 'openai/gpt-5', 'llama3.1', '']) assert.equal(price(name), null, name);
  assert.equal(value.faCost({ cost: 1.5, unpriced: 0 }), '≈ $1.50');
  assert.equal(value.faCost({ cost: 1.5, unpriced: 20 }), '≈ $1.50+');
  assert.equal(value.faCost({ cost: 0.00004, unpriced: 0 }), '< $0.01');
  assert.equal(value.faCost({ cost: 0, unpriced: 20 }), 'no price');
  assert.equal(value.faCost({ cost: 0, unpriced: 0, tokens: 9 }), 'free');
  assert.equal(value.faCost({ cost: 0, unpriced: 0, tokens: 0 }), '–');
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
