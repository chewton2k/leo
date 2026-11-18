'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = path.join(__dirname, '../src/web/app');
const read = (file) => fs.readFileSync(path.join(app, file), 'utf8').replace(/\r\n/g, '\n');

function sheetButtons() {
  const source = read('sheets.js');
  const start = source.indexOf('function sheetButtons(');
  const code = source.slice(start, source.indexOf('\n}\n', start) + 3);
  const context = vm.createContext({});
  vm.runInContext(`${code}\nthis.sheetButtons = sheetButtons;`, context);
  return context.sheetButtons;
}

test('a sheet footer has Cancel, a middle button and a primary, all typed buttons', () => {
  const html = sheetButtons()(['Save', 'id="pick-save"'], { middle: '<button type="button" class="btn" id="every">Every time</button>' });
  assert.equal(html, '<div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn" id="every">Every time</button><button type="button" class="btn primary" id="pick-save">Save</button></div>');
  assert.equal((html.match(/<button /g) || []).length, (html.match(/<button type="button" /g) || []).length);
  const danger = sheetButtons()(['Delete', 'data-action="delete"'], { action: 'back', cancel: 'Back', kind: 'danger' });
  assert.equal(danger, '<div class="buttons"><button type="button" class="btn plain" data-action="back">Back</button><button type="button" class="btn danger" data-action="delete">Delete</button></div>');
});

test('a footer with no primary has only the cancel button', () => {
  assert.equal(sheetButtons()(null, { cancel: 'Close' }), '<div class="buttons"><button type="button" class="btn plain" data-action="close">Close</button></div>');
});

test('the Cancel button is written once', () => {
  for (const file of fs.readdirSync(app).filter((f) => f.endsWith('.js'))) {
    const source = read(file);
    const at = source.indexOf('function sheetButtons(');
    const rest = at < 0 ? source : source.slice(0, at) + source.slice(source.indexOf('\n}\n', at));
    assert.equal(rest.includes('data-action="close">Cancel</button>'), false, file);
  }
});
