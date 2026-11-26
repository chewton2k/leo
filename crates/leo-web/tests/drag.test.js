const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function fixture({document = {}, ...extra} = {}) {
  const listeners = {};
  const context = {side: {root: ''}, folderLabel: value => value, felix:{NOTE_DRAG:'application/x-leo-note'},clearTimeout(){},document:{
    addEventListener(type, fn) { listeners[type] = fn; }, querySelectorAll:() => [],
    body:{classList:{remove(){},add(){},toggle(){}}}, ...document
  }, ...extra};
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/drag.js'), 'utf8'), context);
  return listeners;
}

test('only intentional note and folder sources can start a native drag', () => {
  const listeners = fixture();
  for (const kind of ['note', 'folder', 'link', 'image', 'selection', 'control']) {
    let prevented = false;
    const data = new Map();
    const source = {dataset:{id:'note-id',title:'My note',from:'',dir:'A'}, addEventListener(){}};
    listeners.dragstart({target:{closest(selector) {
      return kind === 'note' && selector.startsWith('.card') || kind === 'folder' && selector.startsWith('[data-action') ? source : null;
    }},preventDefault(){prevented = true;},dataTransfer:{setData:(key,value) => data.set(key,value)}});
    assert.equal(prevented, !['note','folder'].includes(kind), kind);
    assert.equal(data.has('application/x-leo-note'), kind === 'note', kind);
    assert.equal(data.has('application/x-leo-folder'), kind === 'folder', kind);
  }
});

test('holding a note drag on another note row arms combine, and leaving the row disarms it', () => {
  const timers = [];
  const classes = new Set();
  const row = {dataset:{id:'b', title:'Graphs', from:'', dropDir:''}, style:{setProperty(){}},
    classList:{add:(...names) => names.forEach(n => classes.add(n)), remove:(...names) => names.forEach(n => classes.delete(n)), toggle:(name, on) => on ? classes.add(name) : classes.delete(name)},
    closest:(selector) => selector === '.side-tree' ? tree : (selector.includes('side-note') || selector.includes('[data-drop-dir]') && !selector.includes('side-folder')) ? row : null};
  const tree = {getBoundingClientRect:() => ({top:-100, bottom:100})};
  const listeners = fixture({setTimeout: (fn) => { timers.push(fn); return timers.length; }, document:{querySelectorAll:(selector) => selector.includes('combine') && classes.size ? [row] : []}});
  const source = {dataset:{id:'a', title:'BFS', from:''}, addEventListener(){}};
  listeners.dragstart({target:{closest:(selector) => selector.startsWith('.card') ? source : null}, dataTransfer:{setData(){}}});
  const over = (target) => {
    let allowed = false;
    listeners.dragover({target, clientY:0, defaultPrevented:false, preventDefault(){allowed = true;}, dataTransfer:{types:['application/x-leo-note']}});
    return allowed;
  };
  assert.equal(over(row), false, 'a quick pass over a note in the same folder is not a drop');
  assert.ok(classes.has('combine-wait'));
  assert.equal(timers.length, 1);
  over(row);
  assert.equal(timers.length, 1, 'staying on the row keeps the one timer');
  timers[0]();
  assert.ok(classes.has('combine-armed') && !classes.has('combine-wait'));
  assert.equal(over(row), true, 'an armed row takes the drop');
  over({closest:() => null});
  assert.equal(classes.size, 0);
  assert.equal(over(row), false, 'coming back starts the hold again');
  assert.equal(timers.length, 2);
});

function clock() {
  const list = [];
  return {
    list,
    setTimeout(fn, ms) { list.push({ fn, ms }); return list.length; },
    async fire() {
      for (const t of list.splice(0)) t.fn();
      for (let i = 0; i < 5; i++) await Promise.resolve();
    },
  };
}

function combineWatch(visible, answer, line = null) {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/drag.js'), 'utf8');
  const code = source.slice(source.indexOf('const LONG_COMBINE'), source.indexOf('async function forgetCombine'));
  const timer = clock();
  const asked = [];
  const toasts = [];
  const closed = [];
  const context = {
    setTimeout: timer.setTimeout, appVisible: () => visible.now, enc: String, checkActivity() {}, reviewCombine: async () => {}, fail() {},
    $: () => line, closeSheet: () => closed.push(true),
    toast: (text) => toasts.push(text), api: async (url) => { asked.push(url); return answer(); },
  };
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.watchCombine = watchCombine; globalThis.combinesPaused = combinesPaused; globalThis.combinePart = combinePart;`, context);
  return { context, timer, asked, toasts, closed };
}

test('a hidden window pauses the combine watch', async () => {
  const { context, timer, asked } = combineWatch({ now: false }, () => ({ state: 'working' }));
  context.watchCombine('j1');
  for (let i = 0; i < 4; i++) await timer.fire();
  assert.equal(asked.length, 0);
  assert.equal(context.combinesPaused.has('j1'), true);
});

test('a combine paused while hidden and ready on return toasts once', async () => {
  const visible = { now: false };
  const { context, timer, asked, toasts } = combineWatch(visible, () => ({ state: 'ready', with_title: 'A', into_title: 'B' }));
  context.watchCombine('j1');
  await timer.fire();
  assert.equal(asked.length, 0);
  visible.now = true;
  if (context.combinesPaused.delete('j1')) context.watchCombine('j1');
  await timer.fire();
  await timer.fire();
  assert.equal(asked.length, 1);
  assert.equal(toasts.length, 1);
});

test('a long combine shows which part it is on while it works and closes its sheet when it ends', async () => {
  const line = { textContent: 'Part 1 of 30' };
  let job = { state: 'working', parts: 30, done: 4 };
  const { context, timer, closed, toasts } = combineWatch({ now: true }, () => job, line);
  context.watchCombine('j1');
  await timer.fire();
  assert.equal(line.textContent, 'Part 5 of 30');
  job = { state: 'working', parts: 30, done: 30 };
  await timer.fire();
  assert.equal(line.textContent, 'Part 30 of 30');
  assert.equal(closed.length, 0);
  job = { state: 'ready', with_title: 'A', into_title: 'B' };
  await timer.fire();
  assert.equal(closed.length, 1);
  assert.equal(toasts.length, 1);
});

function combineStart(answer) {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/drag.js'), 'utf8');
  const code = source.slice(source.indexOf('async function combineNow'), source.indexOf('const combinesWatched'));
  const sheets = [];
  const toasts = [];
  const asked = [];
  const handlers = {};
  const open = { innerHTML: '' };
  const context = {
    esc: (text) => text, enc: String, checkActivity() {}, watchCombine() {}, fail(e) { throw e; },
    sheetButtons: (primary, { cancel } = {}) => `[${cancel || 'Cancel'}${primary ? `|${primary[0]}` : ''}]`,
    sheet: (html) => { sheets.push(html); return {}; },
    closeSheet() { sheets.push('closed'); },
    $: (selector) => selector === '.scrim .combine' ? open : selector === '#combine-stop' ? { addEventListener: (type, fn) => { handlers.stop = fn; } } : { disabled: false },
    toast: (text) => toasts.push(text),
    api: async (url, options) => { asked.push([url, options && options.method]); return answer(url); },
  };
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.combineNow = combineNow;`, context);
  return { context, sheets, toasts, asked, handlers, open };
}

test('a combine of more than twenty requests names the count and offers Stop; a short one only toasts', async () => {
  const long = combineStart(() => ({ id: 'j1', parts: 21 }));
  await long.context.combineNow({ id: 'a' }, { id: 'b' });
  assert.equal(long.toasts.length, 0);
  const html = long.sheets.at(-1);
  assert.match(html, /This combine takes 21 requests to the AI/);
  assert.match(html, /Part 1 of 21/);
  assert.match(html, /\[Keep going\|Stop combining\]/);
  const short = combineStart(() => ({ id: 'j2', parts: 20 }));
  await short.context.combineNow({ id: 'a' }, { id: 'b' });
  assert.deepEqual(short.sheets, ['closed']);
  assert.match(short.toasts[0], /Combining in the background/);
});

test('Stop deletes the working combine and says neither note changed', async () => {
  const run = combineStart((url) => url.endsWith('/combine') ? { id: 'j1', parts: 40 } : null);
  await run.context.combineNow({ id: 'a' }, { id: 'b' });
  await run.handlers.stop();
  assert.deepEqual(run.asked.at(-1), ['/api/combines/j1', 'DELETE']);
  assert.equal(run.sheets.at(-1), 'closed');
  assert.equal(run.toasts.at(-1), 'Combining was stopped. Neither note was changed.');
});

test('a refused start shows the engine\u2019s sentence in the sheet and starts nothing', async () => {
  const run = combineStart(() => { throw new Error('Two combines are already running. Let one finish, then try again.'); });
  await run.context.combineNow({ id: 'a' }, { id: 'b' });
  assert.match(run.open.innerHTML, /The notes could not be combined/);
  assert.match(run.open.innerHTML, /Two combines are already running/);
  assert.equal(run.toasts.length, 0);
});

test('opening a combine that is still working shows its part and offers Stop', async () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/drag.js'), 'utf8');
  const start = source.indexOf('const combinePart');
  const review = source.indexOf('async function reviewCombine');
  const code = `${source.slice(start, source.indexOf('const combinesWatched'))}\n${source.slice(review, source.indexOf("if (job.state !== 'ready')", review))}}`;
  const sheets = [];
  const context = {
    esc: (text) => text, enc: String, fail(e) { throw e; }, checkActivity() {}, closeSheet() {}, toast() {},
    sheetButtons: (primary, { cancel } = {}) => `[${cancel}|${primary[0]}]`,
    sheet: (html) => { sheets.push(html); return {}; },
    $: () => ({ addEventListener() {} }),
    api: async () => ({ id: 'j9', state: 'working', parts: 7, done: 2 }),
  };
  vm.createContext(context);
  vm.runInContext(`${code}\n;globalThis.reviewCombine = reviewCombine;`, context);
  await context.reviewCombine('j9');
  assert.match(sheets.at(-1), /Part 3 of 7/);
  assert.match(sheets.at(-1), /\[Keep going\|Stop combining\]/);
});

test('while a folder is focused, its top level marks the New note row, as the whole library does unfocused', () => {
  for (const root of ['', 'Work']) {
    const group = {};
    const tree = {querySelectorAll: () => []};
    const explorer = {querySelector: (selector) => (selector === '.side-group' ? group : null)};
    const context = {side: {root}, sideKids: () => null, felix: {}, document: {addEventListener() {}}};
    vm.createContext(context);
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/drag.js'), 'utf8'), context);
    const background = {dataset: {dropDir: ''}, closest: (selector) => (selector === '.side-tree' ? tree : selector.startsWith('.side-explorer') ? explorer : null)};
    assert.equal(context.marksOf(background).row, group, root || 'unfocused');
  }
});
