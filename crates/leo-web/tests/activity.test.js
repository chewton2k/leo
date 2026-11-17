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
