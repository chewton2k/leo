const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const read = (file) => fs.readFileSync(path.join(__dirname, '../src/web/app', file), 'utf8');
const slice = (text, from, to) => text.slice(text.indexOf(from), text.indexOf(to, text.indexOf(from)));

function home() {
  const field = {value: '', listeners: {}, addEventListener(type, fn) { this.listeners[type] = fn; }};
  const quick = {value: '', events: 0, dispatchEvent() { quick.events += 1; }};
  const context = {Event: class {}, $: (sel) => (sel === '#home-ask' ? field : quick), quickLeo() { context.opened += 1; }, opened: 0};
  vm.runInNewContext(`${slice(read('core.js'), 'function wireHomeAsk(', '\nlet moreWatch')}\nwireHomeAsk();`, context);
  return {field, quick, context};
}

test('typing then Enter opens Quick Leo with the text', () => {
  const {field, quick, context} = home();
  field.value = 'what is on my plate';
  let prevented = false;
  field.listeners.keydown({key: 'Enter', preventDefault() { prevented = true; }});
  assert.equal(context.opened, 1);
  assert.equal(quick.value, 'what is on my plate');
  assert.equal(quick.events, 1);
  assert.ok(prevented);
});

test('clicking the field opens Quick Leo, and other keys do not', () => {
  const {field, quick, context} = home();
  field.listeners.keydown({key: 'a', preventDefault() {}});
  field.listeners.keydown({key: 'Enter', shiftKey: true, preventDefault() {}});
  assert.equal(context.opened, 0);
  field.listeners.click();
  assert.equal(context.opened, 1);
  assert.equal(quick.events, 0);
});

function chooser() {
  const buttons = {};
  const calls = [];
  const context = {
    sheet: () => 'box',
    sheetButtons: () => '',
    $: (sel) => ({addEventListener: (_t, fn) => { buttons[sel] = fn; }}),
    uploadSheet: async (...args) => { calls.push(['upload', ...args]); },
    closeSheet: () => calls.push(['close']),
    chat: {attachFiles: (list) => calls.push(['ask', list])},
    fail: (e) => { throw e; },
  };
  vm.runInNewContext(`${slice(read('uploads.js'), 'function dropChoice(', '\n// The note a sheet')}\nthis.dropChoice = dropChoice;`, context);
  return {buttons, calls, context};
}

test('the drop sheet sends files to Felix or to the note maker', () => {
  const {buttons, calls, context} = chooser();
  context.dropChoice(['a.pdf']);
  buttons['#drop-ask']();
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [['close'], ['ask', ['a.pdf']]]);
  calls.length = 0;
  buttons['#drop-make']();
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [['upload', ['a.pdf']]]);
});

function dropped(view, target) {
  const listeners = {};
  const calls = [];
  const context = {
    folderLabel: (v) => v, felix: {NOTE_DRAG: 'x-note'}, clearTimeout() {},
    document: {addEventListener(type, fn) { listeners[type] = fn; }, querySelectorAll: () => [], body: {classList: {remove() {}, add() {}, toggle() {}}}},
    state: {view},
    $: () => null,
    uploadSheet: async (...args) => { calls.push(['upload', ...args]); },
    dropChoice: (files) => calls.push(['choose', files]),
    fail() {},
  };
  vm.createContext(context);
  vm.runInContext(read('drag.js'), context);
  const files = [{name: 'a.pdf'}];
  listeners.drop({target: {closest: () => target || null}, dataTransfer: {types: ['Files'], files}, preventDefault() {}});
  return JSON.parse(JSON.stringify(calls));
}

test('files dropped with no note open ask first; on a note they are added as before', () => {
  assert.deepEqual(dropped('workspace'), [['choose', [{name: 'a.pdf'}]]]);
  assert.deepEqual(dropped('note'), [['upload', [{name: 'a.pdf'}], null, {add: true}]]);
  assert.deepEqual(dropped('workspace', {dataset: {action: 'home'}}), [['upload', [{name: 'a.pdf'}], '']]);
});
