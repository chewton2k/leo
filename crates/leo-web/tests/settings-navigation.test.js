const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
function helpers(listeners = {}) {
  const context = { Event, esc: value => String(value).replace(/[<>&"]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c])), localStorage: { getItem: () => 'dictation', setItem() {} }, document: { addEventListener: (type, handler) => (listeners[type] = listeners[type] || []).push(handler) }, app: { querySelector: () => null }, svg: paths => `<svg>${paths}</svg>`, ICON: {}, stillMotion: () => false };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), context);
  return context;
}
test('all grouped Settings routes resolve, with Keeping and Trash under Data', () => {
  const h = helpers();
  for (const section of ['general', 'shortcuts', 'dictation', 'models', 'writing', 'calendar', 'keeping', 'trash', 'health', 'updates']) assert.equal(h.settingsSection(section), section);
  assert.equal(h.settingsSection(''), 'dictation');
  assert.equal(h.settingsSection('../bad'), 'general');
  const html = h.settingsFrame('keeping', '<section>Keeping content</section>');
  for (const label of ['leo', 'Your notes', 'Data', 'Trash', 'This app', 'Speech models', 'Updates &amp; about']) assert.ok(html.includes(label), label);
  assert.ok(html.includes('href="#/settings/keeping" aria-current="page"'));
  assert.ok(!html.includes('Phone'));
  assert.ok(html.includes('Keeping content'));
  assert.ok(html.includes('<h1>Keeping</h1>'));
  assert.ok(html.includes('<span class="settings-nav-pill" aria-hidden="true"></span>'));
  assert.ok(html.includes('class="settings-section arrive"'));
  assert.equal((html.match(/class="settings-link/g) || []).length, 12);
  assert.ok(html.includes('<button type="button" class="settings-link" data-action="usage">'), 'Usage opens from the This app group');
  assert.equal((html.match(/<svg>/g) || []).length, 9, 'the search icon, Usage and every section without a shared icon draw their own');
  assert.ok(html.includes('<nav class="settings-nav" aria-label="Settings sections">'));
  assert.ok(html.includes('id="settings-search" type="text" role="combobox" aria-label="Search settings"'));
  assert.ok(html.includes('<div class="settings-results" id="settings-results"></div>'));
});
test('switches stay checkboxes and segmented choices carry their value', () => {
  const h = helpers();
  const on = h.settingsSwitch('data-x="1"', true, 'Thing');
  assert.ok(on.includes('type="checkbox" class="switch" role="switch" aria-label="Thing" data-x="1" checked'));
  assert.ok(!h.settingsSwitch('data-x="1"', false).includes('checked'));
  const field = h.segmented({ label: 'Mode', choices: [['a', 'A'], ['b', 'B<'], ['c', 'C']], current: 'b', field: 'data-mode="m"' });
  assert.ok(field.includes('<input type="hidden" data-mode="m" value="b">'));
  assert.ok(field.includes('style="--count:3;--at:1"'));
  assert.ok(field.includes('role="radiogroup" aria-label="Mode"'));
  assert.ok(field.includes('data-value="b" role="radio" aria-checked="true" tabindex="0"'));
  assert.ok(field.includes('B&lt;') && !field.includes('B<'));
  const unknown = h.segmented({ label: 'Mode', choices: [['a', 'A'], ['b', 'B']], current: 'zzz', field: 'data-mode="m"' });
  assert.ok(unknown.includes('--at:0'));
  const buttons = h.segmented({ label: 'Theme', choices: [['light', 'Light'], ['dark', 'Dark']], current: 'dark', action: 'theme-pick', key: 'theme' });
  assert.ok(buttons.includes('data-action="theme-pick" data-theme="dark" aria-pressed="true"'));
  assert.ok(!buttons.includes('type="hidden"') && buttons.includes('role="group"'));
});

const sources = ['settings.js', 'app-settings.js', 'dictation.js', 'keeping.js', 'trash.js', 'margin-notes.js'].map(name => fs.readFileSync(path.join(__dirname, '../src/web/app', name), 'utf8')).join('\n');
test('every searchable setting names a real section and a target the page draws', () => {
  const h = helpers();
  const index = vm.runInContext('SETTINGS_INDEX', h);
  const sections = vm.runInContext('settingsItems()', h).map(([id]) => id);
  const ids = new Set();
  for (const entry of index) {
    assert.ok(sections.includes(entry.section), `${entry.id} lists section ${entry.section}, which does not exist`);
    assert.ok(entry.label.trim() && entry.keywords === entry.keywords.toLowerCase(), `${entry.id} needs a label and lower-case keywords`);
    assert.ok(!ids.has(entry.id), `${entry.id} is listed twice`);
    ids.add(entry.id);
    assert.ok(sources.includes(`data-find="${entry.id}"`) || sources.includes(`'${entry.id}'`), `${entry.id} has no data-find target in the page`);
  }
  for (const section of sections) assert.ok(index.some(entry => entry.section === section), `${section} has nothing searchable`);
});
test('search is local, case-insensitive and ranks label matches first', () => {
  const h = helpers();
  const ids = query => Array.from(h.settingsMatches(query), entry => entry.id);
  assert.deepEqual(ids(''), []);
  assert.deepEqual(ids('   '), []);
  assert.deepEqual(ids('zzzz'), []);
  assert.deepEqual(ids('THEME'), ids('theme'));
  assert.equal(ids('theme')[0], 'theme');
  assert.ok(ids('dark').includes('theme'), 'keywords match');
  assert.ok(ids('note width').includes('note-width') && !ids('note width').includes('theme'), 'every word must match');
  assert.deepEqual(ids('popup'), ['dictation-popup']);
  assert.deepEqual(ids('position'), ['dictation-popup']);
  assert.equal(ids('trash')[0], 'trash-notes');
});
test('the frame draws results as a listbox, an empty message, and keeps the query', () => {
  const h = helpers();
  vm.runInContext("settingsFind.query = 'dictation'; settingsFind.active = 1", h);
  let html = h.settingsFrame('general', '');
  assert.ok(html.includes('value="dictation"') && html.includes('aria-expanded="true" aria-controls="settings-listbox" aria-activedescendant="settings-result-1"'));
  assert.ok(html.includes('role="listbox" aria-label="Settings search results"'));
  const second = Array.from(h.settingsMatches('dictation'), entry => entry.id)[1];
  assert.ok(html.includes(`id="settings-result-1" aria-selected="true" data-find-pick="${second}"`));
  assert.ok(html.includes('id="settings-result-0" aria-selected="false"'));
  assert.ok(html.includes('settings-result-where">Dictation<'));
  assert.ok(html.includes('<nav class="settings-nav" aria-label="Settings sections" data-searching>'));
  vm.runInContext("settingsFind.query = '<b>'", h);
  html = h.settingsFrame('general', '');
  assert.ok(html.includes('role="status">No settings match “&lt;b&gt;”.</p>') && html.includes('aria-expanded="false"') && !html.includes('aria-controls'));
});
test('choice cards are real radios in a group, with a hidden field that fires change', () => {
  const listeners = {};
  const h = helpers(listeners);
  const html = h.choiceCards({ label: 'Where "notes" go', current: 'right', field: 'data-where="x"', choices: [['both', 'Both', 'Middle.', '<svg></svg>'], ['right', 'Right<', 'Beside.'], ['none', 'None']] });
  assert.ok(html.includes('role="radiogroup" aria-label="Where &quot;notes&quot; go" style="--count:3;--at:1"'));
  assert.ok(html.includes('<input type="hidden" data-where="x" value="right">'));
  assert.equal((html.match(/type="radio"/g) || []).length, 3);
  assert.ok(html.includes('value="right" checked') && !html.includes('value="both" checked'));
  assert.ok(html.includes('<span class="choice-diagram" aria-hidden="true"><svg></svg></span>') && html.includes('Right&lt;'));
  assert.equal((html.match(/choice-diagram/g) || []).length, 1);
  assert.ok(h.choiceCards({ label: 'L', current: 'zzz', choices: [['a', 'A'], ['b', 'B']] }).includes('--at:0'));
  const name = html.match(/name="(choice-\d+)"/)[1];
  assert.notEqual(h.choiceCards({ label: 'L', current: 'a', choices: [['a', 'A']] }).match(/name="(choice-\d+)"/)[1], name, 'each group has its own radio name');
  const radios = ['both', 'right', 'none'].map((value, i) => ({ value, checked: i === 1 }));
  const picks = radios.map(radio => ({ classList: { on: false, toggle(_, on) { this.on = on; }, contains() { return this.on; } }, querySelector: () => radio }));
  const set = {}, field = { value: 'right', heard: [], dispatchEvent(e) { this.heard.push([e.type, e.bubbles]); } }, ring = { style: {}, offsetWidth: 0 };
  const box = { querySelectorAll: () => picks, querySelector: selector => (selector.includes('hidden') ? field : ring), style: { getPropertyValue: (key) => set[key], setProperty: (key, value) => { set[key] = value; } } };
  h.syncChoiceCards(box);
  assert.equal(set['--at'], 1);
  field.value = 'none';
  h.syncChoiceCards(box, 0);
  assert.deepEqual(radios.map(radio => radio.checked), [false, false, true]);
  assert.equal(set['--at'], 2);
  const radio = Object.assign(radios[0], { closest: selector => (selector === '.choice-cards' ? box : radio) });
  listeners.change.at(-1)({ target: { closest: selector => (selector.includes('radio') ? radio : null) } });
  assert.equal(field.value, 'both');
  assert.deepEqual(field.heard, [['change', true]]);
  assert.equal(set['--at'], 0);
  listeners.change.at(-1)({ target: { closest: () => null } });
  assert.equal(field.heard.length, 1, 'other change events are ignored');
});
test('a reset button shows only while the value differs and names its setting', () => {
  const h = helpers();
  const shown = h.settingReset('Note width', 'width', true), hidden = h.settingReset('Note width', 'width', false);
  assert.ok(shown.includes(' data-on ') && !hidden.includes('data-on'));
  for (const html of [shown, hidden]) assert.ok(html.includes('data-action="setting-reset" data-reset="width" aria-label="Reset Note width to default"'));
});

test('Settings motion asks stillMotion, the one place that reads Reduced Motion', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8');
  assert.ok(!source.includes('prefers-reduced-motion'));
  assert.ok(source.includes('stillMotion()'));
});

test('a chord is read from the key event in a fixed modifier order, and aria names it', () => {
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const load = (context) => {
    vm.createContext(context);
    vm.runInContext(core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('async function dictateHint')), context);
    return context;
  };
  for (const [mac, event, chord] of [
    [true, { key: 'K', metaKey: true }, 'cmd+k'],
    [true, { key: 'E', code: 'KeyE', metaKey: true, shiftKey: true }, 'cmd+shift+e'],
    [true, { key: '∫', code: 'KeyB', metaKey: true, altKey: true }, 'cmd+alt+b'],
    [false, { key: '!', code: 'Digit1', ctrlKey: true, shiftKey: true }, 'ctrl+shift+1'],
    [false, { key: ' ', ctrlKey: true }, 'ctrl+space'],
    [false, { key: 'Shift', shiftKey: true }, ''],
  ]) {
    const context = load({ ON_MAC: mac, localStorage: { getItem: () => JSON.stringify({ quick: 'ctrl+j' }) } });
    assert.equal(context.chordOf(event), chord);
    assert.equal(context.pressed({ key: 'j', ctrlKey: true }, 'quick'), true);
    assert.equal(context.pressed({ key: 'k', ctrlKey: true }, 'quick'), false);
    assert.equal(context.shortcutKeys('new'), mac ? 'cmd+n' : 'ctrl+n');
  }
  const blocked = load({ ON_MAC: true, localStorage: { getItem: () => { throw new Error('blocked'); } } });
  assert.equal(blocked.shortcutKeys('quick'), 'cmd+k');
  assert.equal(blocked.shortcutAria('cmd+ctrl+alt+shift+k'), 'Meta+Control+Alt+Shift+K');
});

test('a choice that cannot work is disabled and says so, and a group can keep its own radio name', () => {
  const h = helpers();
  const html = h.choiceCards({ label: 'What to record', name: 'rec-kind', current: 'microphone', choices: [['microphone', 'Microphone', 'Your voice'], ['screen', 'Screen', 'Not on this computer', '', '', { disabled: true }]] });
  assert.equal((html.match(/name="rec-kind"/g) || []).length, 2);
  assert.ok(html.includes('value="screen" disabled aria-disabled="true">'));
  assert.ok(html.includes('Not on this computer'));
  assert.ok(!html.includes('value="microphone" checked disabled'));
  const buttons = h.choiceCards({ label: 'L', action: 'pick', key: 'v', current: 'a', choices: [['a', 'A'], ['b', 'B', '', '', '', { disabled: true }]] });
  assert.ok(buttons.includes('data-v="b" aria-pressed="false" disabled aria-disabled="true">'));
  const source = fs.readFileSync(path.join(__dirname, '../src/web/recording.js'), 'utf8');
  assert.ok(source.includes("choiceCards({ label: 'What to record', name: 'rec-kind'"));
  assert.ok(!source.includes('class="rec-kind"'));
});
