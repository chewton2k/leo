'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const v8 = require('node:v8');
const vm = require('node:vm');
const { test } = require('node:test');
require('../src/web/saving.js');
const kit = require('../src/web/margin.js');
const ed = require('../src/web/editing.js');
const md = require('../src/web/markdown.js');

const web = path.resolve(__dirname, '../src/web');
const read = (name) => fs.readFileSync(path.join(web, name), 'utf8');
const core = read('app/core.js');
const doc = read('doc.js');

const FULL = process.env.LEO_MEASURE_TYPING === '1';
const SIZES = FULL ? [200000, 2000000, 20000000] : [200000];
const RUNS = { 200000: 31, 2000000: 15, 20000000: 7 };
const UNDO_STEPS = { 200000: 200, 2000000: 200, 20000000: 200 };
const GROWS_ABOVE_MS = 3;
const GROWS_ABOVE_RATIO = 8;

let seed = 7;
const rand = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
const WORDS = Array.from({ length: 3000 }, () => Array.from({ length: 2 + Math.floor(rand() * 8) }, () => String.fromCharCode(97 + Math.floor(rand() * 26))).join(''));

function linesOf(bytes) {
  const lines = [];
  for (let size = 0, b = 0; size < bytes; b++) {
    let line = `Block ${b}:`;
    while (line.length < 247) line += ' ' + WORDS[Math.floor(rand() * WORDS.length)];
    lines.push(line.slice(0, 248), '');
    size += 250;
  }
  return lines;
}

function median(fn, runs) {
  for (let i = 0; i < 2; i++) fn(i);
  const times = [];
  for (let i = 0; i < runs; i++) {
    const t = process.hrtime.bigint();
    fn(i);
    times.push(Number(process.hrtime.bigint() - t) / 1e6);
  }
  times.sort((a, b) => a - b);
  return times[runs >> 1];
}

function storage() {
  const values = new Map();
  let writes = 0;
  let written = 0;
  return {
    get length() { return values.size; },
    key: (i) => [...values.keys()][i],
    getItem: (k) => (values.has(k) ? values.get(k) : null),
    setItem: (k, v) => { writes++; written = v.length; values.set(k, v); },
    removeItem: (k) => values.delete(k),
    stats: () => ({ writes, written }),
  };
}

const coreCtx = { cleanTitle: (t) => String(t).trim(), md, saving: null };
vm.createContext(coreCtx);
vm.runInContext(core.slice(core.indexOf('function snapshot('), core.indexOf('function flush(')), coreCtx);

const positions = new Function('lines', `${doc.slice(doc.indexOf('function lineStart(line)'), doc.indexOf('function hitsOf'))}\n${doc.slice(doc.indexOf('function lineOf(at)'), doc.indexOf('const docLength'))}\nreturn { lineStart, lineOf };`);
const historyOf = new Function('lines', 'editing', 'container', 'onChange', 'outlineLater', 'Date', 'lineStart', `${doc.slice(doc.indexOf('const MOST_UNDO'), doc.indexOf('function restore('))}\nreturn { changed, past, future };`);
const history = (lines, editing, container, onChange, outlineLater, date) => historyOf(lines, editing, container, onChange, outlineLater, date, positions(lines).lineStart);

v8.setFlagsFromString('--expose-gc');
const gc = vm.runInNewContext('gc');

function counting() {
  const real = { stringify: JSON.stringify, parse: JSON.parse };
  const n = { stringify: 0, parse: 0 };
  JSON.stringify = (...a) => { n.stringify++; return real.stringify(...a); };
  JSON.parse = (...a) => { n.parse++; return real.parse(...a); };
  return { n, restore: () => Object.assign(JSON, real) };
}

function counted(lines) {
  const n = { joins: 0 };
  lines.join = function (...a) { n.joins++; return Array.prototype.join.apply(this, a); };
  return n;
}

function typer(lines, h, clock) {
  const mid = lines.length >> 1;
  return () => {
    clock.t += 50;
    const removed = lines.splice(mid, 1, lines[mid] + 'x');
    h.changed({ at: mid, removed, added: [lines[mid]] });
  };
}

function keystroke(bytes) {
  const lines = linesOf(bytes);
  const text = lines.join('\n');
  const runs = RUNS[bytes] || 7;
  const note = { id: 'one', title: 'Note', body: text, directory: '', version: 'v1' };
  const local = storage();
  const clock = { t: 1000 };
  const fakeDate = { now: () => clock.t };
  const m = leoSaving.create({ api: async () => note, storage: local, schedule: () => 1, cancel: () => {}, now: fakeDate.now });
  const s = m.open(note);
  const n = counted(lines);
  s.doc = { source: () => { n.joins++; return Array.prototype.join.call(lines, '\n'); } };
  s.title = { textContent: 'Note' };
  coreCtx.saving = m;
  const untitled = { doc: s.doc, title: { textContent: '' }, note: { ...note, id: null } };
  const editing = { area: {} };
  const container = { classList: { toggle: () => {} } };
  const h = history(lines, editing, container, (t) => coreCtx.changed(s, t), () => {}, fakeDate);
  const type = typer(lines, h, clock);

  type();
  const c = counting();
  n.joins = 0;
  const before = local.stats().writes;
  type();
  c.restore();
  const counts = { joins: n.joins, stringify: c.n.stringify, parse: c.n.parse, writes: local.stats().writes - before, written: local.stats().written };

  const edit = { title: 'Note', body: text };
  const last = lines.length - 1;
  const mid = lines.length >> 1;
  const at = text.length;
  const { lineStart, lineOf } = positions(lines);
  const quote = (b) => lines[b * 2].slice(10, 70);
  const blocks = lines.length / 2;
  const open = Array.from({ length: 24 }, (_, i) => ({ quote: quote(Math.floor((i * blocks) / 24)), prefix: '', suffix: '' }));
  const resolved = Array.from({ length: 48 }, (_, i) => ({ quote: quote(Math.floor((i * blocks) / 48)) + ' ', prefix: '', suffix: '' }));
  const body = JSON.stringify({ ...edit, base: 'v1' });

  const steps = {
    'join of the lines': () => lines.join('\n'),
    'snapshot (titled note, no text handed over)': () => coreCtx.snapshot(s),
    'snapshot (untitled new note: plain text for the title)': () => coreCtx.snapshot(untitled, () => text).title,
    'saving copy (stringify + parse)': () => JSON.parse(JSON.stringify(edit)),
    'draft record (stringify of note and edit)': () => JSON.stringify({ note: s.note, edit }),
    'draft write (at most four a second once throttled)': () => { clock.t += 1000; m.changed(s, edit); },
    'large draft copy (structured clone: the main-thread part of an IndexedDB put)': () => structuredClone({ note: s.note, edit }),
    'lineStart (last line)': () => lineStart(last),
    'lineOf (end of note)': () => lineOf(at),
    'lineStart + lineOf (middle line)': () => lineOf(lineStart(mid) + 3),
    'listPrefix with lines above (last line)': () => ed.listPrefix(lines[last - 1], lines.slice(0, last - 1)),
    'listPrefix with lines above (middle line)': () => ed.listPrefix(lines[mid], lines.slice(0, mid)),
    'listPrefixAt, the run of list lines above (last line)': () => ed.listPrefixAt(lines[last - 1], lines, last - 1),
    'listPrefixAt, the run of list lines above (middle line)': () => ed.listPrefixAt(lines[mid], lines, mid),
    'reading-time line (300 ms after a pause)': () => md.readTime(text),
    'save request body (stringify of the PATCH body)': () => JSON.stringify({ ...edit, base: 'v1' }),
    'margin text search, 24 open + 48 resolved': () => { for (const t of open) kit.find(text, t); for (const r of resolved) kit.find(text, r); },
  };
  const ms = {};
  for (const [name, fn] of Object.entries(steps)) ms[name] = median(fn, runs);
  const perKey = median(type, runs);
  return { counts, ms, perKey, request: body.length };
}

function undoHeap(bytes) {
  const lines = linesOf(bytes);
  const text = lines.join('\n');
  const steps = UNDO_STEPS[bytes] || 10;
  let clock = 1000;
  const fakeDate = { now: () => clock };
  gc();
  const base = process.memoryUsage().heapUsed;
  const editing = { area: {} };
  const h = history(lines, editing, { classList: { toggle: () => {} } }, () => {}, () => {}, fakeDate);
  const last = lines.length - 2;
  const key = (ch) => { const removed = lines.splice(last, 1, lines[last] + ch); h.changed({ at: last, removed, added: [lines[last]] }); };
  for (let i = 0; i < steps; i++) {
    clock += 1100;
    key('x');
  }
  gc();
  const held = process.memoryUsage().heapUsed - base;
  const pushed = h.past.length;
  const inGroup = history(lines, editing, { classList: { toggle: () => {} } }, () => {}, () => {}, { now: () => 5000 });
  for (let i = 0; i < 100; i++) { const removed = lines.splice(last, 1, lines[last] + 'y'); inGroup.changed({ at: last, removed, added: [lines[last]] }); }
  return { steps, pushed, perStep: held / steps, note: text.length, groupPushed: inGroup.past.length };
}

const fmt = (n) => (n < 10 ? n.toFixed(4) : n.toFixed(2));
const mb = (n) => (n / 1048576).toFixed(1);

test('typing cost: every step of a keystroke, and undo memory, at the sizes the note can reach', () => {
  const rows = SIZES.map((bytes) => ({ bytes, ...keystroke(bytes) }));
  const heap = SIZES.map((bytes) => ({ bytes, ...undoHeap(bytes) }));
  for (const r of rows) {
    assert.ok(r.perKey >= 0 && Number.isFinite(r.perKey));
    for (const v of Object.values(r.ms)) assert.ok(Number.isFinite(v) && v >= 0);
  }
  for (const h of heap) assert.ok(h.pushed > 0 && Number.isFinite(h.perStep));
  const small = rows[0];
  const big = rows[rows.length - 1];
  const out = [];
  out.push(`node ${process.version}, runs ${SIZES.map((b) => RUNS[b]).join('/')}`);
  out.push('per keystroke: ' + rows.map((r) => `${mb(r.bytes)} MB: ${r.counts.joins} joins, ${r.counts.stringify} stringify, ${r.counts.parse} parse, ${r.counts.writes} write of ${mb(r.counts.written)} MB`).join(' | '));
  out.push(`| step | ${rows.map((r) => `${mb(r.bytes)} MB ms`).join(' | ')} | grows |`);
  out.push(`|---|${rows.map(() => '---|').join('')}---|`);
  for (const name of Object.keys(small.ms)) {
    const grows = FULL && (big.ms[name] > GROWS_ABOVE_MS || big.ms[name] > GROWS_ABOVE_RATIO * small.ms[name]);
    out.push(`| ${name} | ${rows.map((r) => fmt(r.ms[name])).join(' | ')} | ${FULL ? (grows ? 'yes' : 'no') : 'n/a'} |`);
  }
  out.push(`| one keystroke (doc.js changed, snapshot, saving.changed, as typed) | ${rows.map((r) => fmt(r.perKey)).join(' | ')} | ${FULL ? (big.perKey > GROWS_ABOVE_MS || big.perKey > GROWS_ABOVE_RATIO * small.perKey ? 'yes' : 'no') : 'n/a'} |`);
  out.push(`save request body bytes: ${rows.map((r) => `${mb(r.bytes)} MB: ${r.request}`).join(' | ')}`);
  out.push('undo heap held per typing group: ' + heap.map((h) => `${mb(h.note)} MB note: ${mb(h.perStep)} MB per step over ${h.steps} steps (${h.pushed} records; ${(h.perStep / h.note).toFixed(2)}x the note; 100 keys in one group pushed ${h.groupPushed}; 200 steps = ${(h.perStep * 200 / 1073741824).toFixed(2)} GB)`).join(' | '));
  console.log(out.join('\n'));
});

test('a keystroke joins the note once and copies it without turning it into text', () => {
  const { counts } = keystroke(20000);
  assert.equal(counts.joins, 0);
  assert.equal(counts.parse, 0);
  assert.equal(counts.stringify, 0);
  assert.equal(counts.writes, 0);
});

const editorOf = new Function('lines', 'editing', 'container', 'onChange', 'outlineLater', 'Date', 'lineStart', 'stop', 'draw', 'lineOf', 'selectSpan', 'editSpan',
  `${doc.slice(doc.indexOf('const MOST_UNDO'), doc.indexOf('function historyKey('))}\nreturn { changed, undo, redo, past, future, get lines() { return lines; }, source: joinedText };`);

function editor(text, clock) {
  let api = null;
  const at = () => positions(api.lines);
  api = editorOf(text.split('\n'), { area: {} }, { classList: { toggle: () => {} } }, () => {}, () => {}, { now: () => clock.t },
    (line) => at().lineStart(line), () => {}, () => {}, (offset) => at().lineOf(offset), () => {}, () => {});
  const type = (line, ch) => {
    clock.t += 50;
    const removed = api.lines.splice(line, 1, api.lines[line] + ch);
    api.changed({ at: line, removed, added: [api.lines[line]] });
  };
  return { api, type };
}

test('undo and redo return the text before and after each edit made as a span', () => {
  const clock = { t: 1000 };
  const { api, type } = editor('alpha\nbeta\ngamma', clock);
  const states = [api.source()];
  for (const ch of 'XYZ') type(1, ch);
  states.push(api.source());
  clock.t += 1100;
  for (const ch of 'qr') type(2, ch);
  states.push(api.source());
  clock.t += 50;
  api.lines.splice(0, 1, 'pasted', 'text here', 'alpha');
  api.changed();
  states.push(api.source());
  assert.equal(states[3], 'pasted\ntext here\nalpha\nbetaXYZ\ngammaqr');
  for (let i = 3; i > 0; i--) {
    assert.equal(api.undo(), true);
    assert.equal(api.source(), states[i - 1]);
    assert.equal(api.lines.join('\n'), states[i - 1]);
  }
  assert.equal(api.undo(), false);
  for (let i = 1; i <= 2; i++) {
    assert.equal(api.redo(), true);
    assert.equal(api.source(), states[i]);
  }
  type(0, '!');
  assert.equal(api.redo(), false, 'a new edit clears redo');
  assert.equal(api.undo(), true);
  assert.equal(api.source(), states[2]);
});

test('an undo record is the size of the change, not of the note', () => {
  const clock = { t: 1000 };
  const { api, type } = editor(linesOf(1000000).join('\n'), clock);
  const rows = api.lines.length;
  for (let g = 0; g < 50; g++) {
    clock.t += 1100;
    for (const ch of 'abc') type((g * 97) % rows, ch);
  }
  api.undo();
  const held = api.past.concat(api.future).reduce((n, r) => n + r.removed.length + r.inserted.length, 0);
  assert.equal(api.past.length + api.future.length, 50);
  assert.ok(held < 10240, `${held} characters held`);
});
