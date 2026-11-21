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
test('a Settings section asks only for what it shows', async () => {
  const settings = fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8');
  const nav = fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8');
  const wanted = { shortcuts: ['/api/dictation'], dictation: ['/api/dictation'], models: ['/api/speech'], calendar: ['/api/calendar'], felix: ['/api/margin'] };
  const asked = [];
  const context = {
    api: async (url) => { asked.push(url); return {}; }, seq: 0, state: {}, localStorage: { setItem() {} }, document: {}, app: {}, appPollGeneration: 0, dictationGeneration: 0,
    settingsSection: (section) => section, showTrash() {}, showKeeping() {}, scheduleAppStatus() {}, chrome() {}, $: () => ({}), settingsCrumbs: () => '', refreshPanel() {}, settingsFrame: () => '', skeleton: () => '',
    drawSettings() {}, loadHealth: async () => {}, refreshPlans() {}, startSpeechPolling() {}, dictationSection: () => false, pollDictation() {}, dictateKeysSeen() {}, fail() {},
  };
  vm.createContext(context);
  vm.runInContext(nav.slice(nav.indexOf('const SETTINGS_SECTIONS'), nav.indexOf('const settingsItems')), context);
  vm.runInContext(settings.slice(settings.indexOf('const SETTINGS_WANTS'), settings.indexOf('const CALENDAR_PROVIDERS')), context);
  const ids = vm.runInContext('SETTINGS_SECTIONS.flatMap((group) => group.items.map((item) => item[0]))', context).filter((id) => id !== 'trash' && id !== 'keeping');
  assert.ok(ids.length >= 8);
  for (const id of ids) {
    asked.length = 0;
    await context.showSettings(id);
    assert.deepEqual([...asked].sort(), ['/api/settings', ...(wanted[id] || [])].sort(), id);
  }
});
