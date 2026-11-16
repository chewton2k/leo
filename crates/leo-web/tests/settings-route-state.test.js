const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
test('the Settings entry stays selected for Keeping and Trash, which live inside Settings', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/side.js'), 'utf8');
  const helper = source.slice(source.indexOf('function sidePlace()'), source.indexOf('function drawSide()'));
  const context = { state:{view:'keeping'},location:{hash:'#/settings'} };
  vm.createContext(context);vm.runInContext(helper, context);
  assert.equal(context.sidePlace(), 'settings');
  context.location.hash = '#/settings/keeping';
  assert.equal(context.sidePlace(), 'settings');
  context.state.view = 'trash';
  context.location.hash = '#/settings/trash';
  assert.equal(context.sidePlace(), 'settings');
});
test('Back from a remembered Keeping section returns to the note instead of looping', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const helper = source.slice(source.indexOf('function back()'), source.indexOf('function chrome('));
  let target;
  const context = { state:{view:'keeping'},location:{hash:'#/settings'},places:{from:'#/settings/keeping',before:'#/n/note'},asideHash:()=>true,goBack:hash=>{target=hash;} };
  vm.createContext(context);vm.runInContext(helper,context);
  context.back();assert.equal(target,'#/n/note');
});
