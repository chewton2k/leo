const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const plain = (value) => JSON.parse(JSON.stringify(value));
const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
function withKeys(context, mac = true) {
  context.ON_MAC = mac;
  vm.runInContext(core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('async function dictateHint')), context);
  return context;
}
test('dictation controls escape history and preserve explicit text settings', () => {
  const context = { app: { addEventListener() {} }, settingsOption: (value, label) => `<option>${label}</option>`, ICON: { chevron: '' }, esc: value => value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;'), state: {}, window: {}, document: { activeElement: null, addEventListener() {} }, setTimeout() {} };
  vm.createContext(context);
  withKeys(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  const html = context.dictationCards({ settings: { shortcut: 'option+space', custom_words: ['東京'], mode: 'both', popup: 'live' }, recent: [{ id: 1, text: '<img src=x onerror=bad()>' }], shortcut: {} });
  assert.ok(html.includes('&lt;img'));
  assert.ok(!html.includes('<img'));
  assert.ok(html.includes('東京'));
  assert.ok(html.includes('data-dictation="custom_words"'));
  for (const retired of ['Lid closed', 'closed_microphone', 'trailing_space', 'data-dictation="enter"']) assert.ok(!html.includes(retired));
  assert.ok(html.includes('class="switch" role="switch"'));
  assert.ok(!/type="checkbox"(?![^>]*class="switch")/.test(html));
  assert.ok(html.includes('<input type="hidden" data-dictation="mode" value="both">'));
  assert.ok(html.includes('role="radiogroup" aria-label="Popup"'));
  assert.ok(/<input type="checkbox" class="switch" role="switch" data-dictation="writing" data-on="quick">/.test(html));
  assert.ok(html.includes('Adds a few seconds'));
  assert.ok(html.includes('Names and terms leo should spell your way, up to 200. Some models also listen for them; the first ones count most.'));
  for (const gone of ['Use shortcut', 'dictation-use-shortcut', 'dictation-resume', 'Change shortcut', 'dictation-record', 'dictation-shortcut-field', 'data-dictation="shortcut"']) assert.ok(!html.includes(gone));
  const row = context.dictationShortcutRow({ settings: { shortcut: 'option+space' }, shortcut: {} });
  assert.ok(row.includes('<kbd>\u2325</kbd><kbd>Space</kbd>'));
  assert.ok(row.includes('data-action="dictation-record">Change</button>'));
  assert.ok(row.includes('data-find="shortcut-dictation"'));
  assert.ok(row.includes('<input type="hidden" data-dictation="shortcut" value="option+space">'));
  const recording = context.dictationShortcutRow({ settings: { shortcut: 'option+space' }, shortcut: { recording: true, held: 'command+shift' } });
  assert.ok(recording.includes('shortcut-field recording'));
  assert.ok(recording.includes('Press your keys\u2026'));
  assert.ok(recording.includes('<kbd>\u2318</kbd><kbd>\u21e7</kbd>'));
  assert.ok(!recording.includes('<kbd>Space</kbd>'));
  assert.ok(recording.includes('>Esc to cancel</span>'));
  const windows = withKeys(vm.createContext({ esc: (value) => value }), false);
  assert.deepEqual(plain(vm.runInContext("shortcutParts('ctrl_left+alt+super+shift+fn+space+k')", windows)), ['Ctrl', 'Alt', 'Win', 'Shift', 'Fn', 'Space', 'K']);
  assert.deepEqual(plain(vm.runInContext("shortcutParts('')", windows)), []);
  assert.deepEqual(plain(vm.runInContext("shortcutParts('command_right+option+fn')", withKeys(vm.createContext({}), true))), ['\u2318', '\u2325', 'Fn']);
  assert.ok(html.includes('Matches the tone of the app you dictate into (only the app’s name is used).'));
  assert.ok(!/id="dictation-position-row" hidden/.test(html));
  assert.ok(/id="dictation-position-row" hidden/.test(context.dictationCards({ settings: { popup: 'off' }, recent: [], shortcut: {} })));
  assert.ok(!html.includes('data-action="dictation-save"'));
  assert.ok(!html.includes('Save after editing'));
  assert.ok(!html.includes('No writing AI is set up'));
  const unready = context.dictationCards({ settings: { writing: 'quick' }, writing_ready: false, recent: [], shortcut: {} });
  assert.ok(unready.includes('No writing AI is set up'));
  assert.ok(unready.includes('>Set up writing AI</a>'));
  const chosen = context.dictationCards({ settings: { writing: 'selected' }, recent: [], shortcut: {} });
  assert.ok(chosen.includes('data-dictation="writing" data-on="selected" checked'));
  const fields = (checked, on) => [{ dataset: { dictation: 'writing', on }, type: 'checkbox', checked }];
  context.state = { dictation: { settings: { writing: 'selected' } } };
  context.app.querySelectorAll = () => fields(false, 'selected');
  assert.equal(context.dictationDraft().writing, 'off');
  context.app.querySelectorAll = () => fields(true, 'selected');
  assert.equal(context.dictationDraft().writing, 'selected');
  context.app.querySelectorAll = () => fields(true, 'quick');
  assert.equal(context.dictationDraft().writing, 'quick');
});
function shortcutContext(shortcut = {}, saved = 'option+space') {
  const calls = [], handlers = {}, delays = [], status = { textContent: '', classes: new Set(), classList: { toggle: (name, on) => status.classes[on ? 'add' : 'delete'](name) } };
  const field = { innerHTML: '', classList: { toggle() {} } }, hidden = { dataset: { dictation: 'shortcut' }, value: saved };
  const context = {
    app: { addEventListener() {}, querySelectorAll: () => [hidden] },
    window: { addEventListener: (type, fn, capture) => { handlers[type] = { fn, capture }; } },
    ICON: {}, esc: (value) => value, document: { activeElement: null, addEventListener() {} },
    state: { view: 'settings', section: 'dictation', dictation: { settings: { shortcut: saved }, shortcut, recent: [] } }, seq: 1,
    setTimeout: (fn, ms) => { delays.push(ms); }, clearTimeout() {}, appVisible: () => true, pulse() {}, refreshPanel() {}, fail: (e) => { throw e; },
    $: (selector) => ({ '#dictation-shortcut-status': status, '#dictation-shortcut-field': field, '[data-dictation="shortcut"]': hidden, '#dictation-result': { classList: { remove() {}, add() {} } }, '#dictation-recent': {} })[selector] || null,
    api: async (url, options = {}) => {
      if (options.body) calls.push(plain(options.body));
      if (options.body && options.body.action === 'save' && context.refuse) throw new Error(context.refuse);
      const settings = options.body && options.body.action === 'save' ? options.body.settings : context.state.dictation.settings;
      return { settings, shortcut: context.after || {}, recent: [] };
    },
  };
  vm.createContext(context);
  withKeys(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  return { context, calls, handlers, hidden, status, field, delays };
}
const settle = async () => { for (let n = 0; n < 6; n++) await new Promise((resolve) => setImmediate(resolve)); };
test('a captured chord saves at once, says so, and is not saved twice', async () => {
  const { context, calls, hidden, status } = shortcutContext({ recording: true, held: 'option' });
  context.after = { recording: false, captured: 'command+shift+k' };
  context.updateDictationStatus(context.state.dictation);
  assert.equal(status.textContent, 'Esc to cancel');
  assert.deepEqual(calls, []);
  const captured = { settings: { shortcut: 'option+space' }, shortcut: { recording: false, captured: 'command+shift+k' } };
  context.updateDictationStatus(captured);
  await settle();
  assert.deepEqual(calls, [{ action: 'save', settings: { shortcut: 'command+shift+k' } }]);
  assert.equal(hidden.value, 'command+shift+k');
  assert.equal(status.textContent, 'Saved. Hold or press \u2318 \u21e7 K to dictate.');
  assert.ok(status.classes.has('ok') && !status.classes.has('bad'));
  context.updateDictationStatus(captured);
  await settle();
  assert.equal(calls.length, 1);
  context.updateDictationStatus({ settings: { shortcut: 'command+shift+k' }, shortcut: { recording: true, held: '' } });
  context.updateDictationStatus(captured);
  await settle();
  assert.equal(calls.length, 2);
});
test('the saved message follows the activation mode and the captured chord wins over a stale field', async () => {
  for (const [mode, text] of [['hold', 'Saved. Hold \u2318 K to dictate.'], ['tap', 'Saved. Press \u2318 K to start and stop.'], ['both', 'Saved. Hold or press \u2318 K to dictate.']]) {
    const { context, calls, status } = shortcutContext({ recording: true, held: '' });
    context.state.dictation.settings.mode = mode;
    context.app.querySelectorAll = () => [{ dataset: { dictation: 'shortcut' }, value: 'option+space' }];
    context.updateDictationStatus(context.state.dictation);
    context.updateDictationStatus({ settings: { shortcut: 'option+space', mode }, shortcut: { recording: false, captured: 'command+k' } });
    await settle();
    assert.equal(calls[0].settings.shortcut, 'command+k');
    assert.equal(status.textContent, text);
  }
});
test('leaving Settings or hiding the window forgets a recording, so a later chord is not saved', async () => {
  const left = (context) => { context.state.view = 'notes'; return context.pollDictation(1, vm.runInContext('dictationGeneration', context)); };
  const hidden = (context, handlers) => { context.appVisible = () => false; handlers['leo-visibility'].fn(); };
  for (const leave of [left, hidden]) {
    const { context, calls, handlers } = shortcutContext({ recording: true, held: '' });
    context.updateDictationStatus(context.state.dictation);
    await leave(context, handlers);
    context.state.view = 'settings';
    context.appVisible = () => true;
    context.updateDictationStatus({ settings: { shortcut: 'option+space' }, shortcut: { recording: false, captured: 'fn' } });
    await settle();
    assert.deepEqual(calls, []);
  }
});
test('a chord left over from before this page saw a recording is never saved', async () => {
  const { context, calls } = shortcutContext();
  context.updateDictationStatus({ settings: { shortcut: 'option+space' }, shortcut: { recording: false, captured: 'fn' } });
  await settle();
  assert.deepEqual(calls, []);
});
test('a refused chord keeps the old shortcut, shows the refusal and resumes it', async () => {
  const { context, calls, hidden, status } = shortcutContext();
  context.refuse = 'Another app already uses that shortcut.';
  context.updateDictationStatus({ settings: { shortcut: 'option+space' }, shortcut: { recording: true, held: 'fn' } });
  context.updateDictationStatus({ settings: { shortcut: 'option+space' }, shortcut: { recording: false, captured: 'fn' } });
  await settle();
  assert.deepEqual(calls.map((c) => c.action), ['save', 'resume_shortcut']);
  assert.equal(hidden.value, 'option+space');
  assert.equal(status.textContent, 'Another app already uses that shortcut.');
  assert.ok(status.classes.has('bad') && !status.classes.has('ok'));
});
test('while recording every key is held back from the page and Escape or an outside click cancels', async () => {
  const { context, calls, handlers } = shortcutContext({ recording: true, held: '' });
  context.after = { recording: true, held: '' };
  assert.ok(handlers.keydown.capture && handlers.keyup.capture && handlers.click.capture);
  const key = (type, name) => { const e = { type, key: name, prevented: false, stopped: false, preventDefault() { e.prevented = true; }, stopImmediatePropagation() { e.stopped = true; } }; handlers[type].fn(e); return e; };
  for (const name of [' ', 'Enter', 'k', 'Tab']) {
    const e = key('keydown', name);
    assert.ok(e.prevented && e.stopped, name);
  }
  assert.ok(key('keyup', ' ').prevented);
  assert.deepEqual(calls, []);
  assert.ok(key('keydown', 'Escape').prevented);
  await settle();
  assert.deepEqual(calls, [{ action: 'resume_shortcut' }]);
  handlers.click.fn({ target: { closest: () => ({}) } });
  await settle();
  assert.equal(calls.length, 1);
  handlers.click.fn({ target: { closest: () => null } });
  await settle();
  assert.deepEqual(calls.map((c) => c.action), ['resume_shortcut', 'resume_shortcut']);
  context.state.dictation.shortcut = {};
  assert.ok(!key('keydown', ' ').prevented);
  assert.ok(!key('keydown', 'Escape').prevented);
  handlers.click.fn({ target: { closest: () => null } });
  await settle();
  assert.equal(calls.length, 2);
});
test('the page polls every 150 ms while recording and every 600 ms otherwise', () => {
  for (const [shortcut, wait] of [[{ recording: true }, 150], [{}, 600]]) {
    const { context, delays } = shortcutContext(shortcut);
    context.pollDictation(1, vm.runInContext('dictationGeneration', context));
    assert.deepEqual(delays, [wait]);
  }
});
function editContext(words, recent) {
  const listeners = {}, calls = [], panel = { html: '' }, field = { value: words.join('\n') }, result = { textContent: '' };
  const context = {
    app: { addEventListener: (name, fn) => { listeners[name] = fn; }, querySelectorAll: () => [{ dataset: { dictation: 'custom_words' }, value: field.value }] },
    ICON: { plus: '' }, esc: (value) => value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;'),
    state: { dictation: { settings: { custom_words: words }, recent } }, window: {}, document: { activeElement: null, addEventListener() {} }, setTimeout() {}, clearTimeout() {},
    refreshPanel: (box, html) => { panel.html = html; },
    $: (selector) => selector === '#dictation-recent' ? {} : selector === '#dictation-result' ? result : selector === '[data-dictation="custom_words"]' ? field : null,
    fail: (e) => { throw e; },
    api: async (url, options) => {
      calls.push(options.body);
      const edited = context.state.dictation.recent.map((item) => item.id === options.body.id ? { ...item, text: options.body.text, edited: true } : item);
      return { settings: { custom_words: words }, recent: edited };
    },
    seq: 1, updateDictationStatus() {}, appVisible: () => false, pulse() {},
  };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  vm.runInContext('state.view = "settings"', context);
  context.field = field;
  return { context, listeners, calls, panel, field, result };
}
const type = (context, text) => vm.runInContext(`dictationEdit.text = ${JSON.stringify(text)}`, context);
const chipWords = (html) => [...html.matchAll(/data-word="([^"]*)"/g)].map((m) => m[1]);
test('editing a recent dictation offers only its new words as custom words', async () => {
  const { context, calls, panel } = editContext(['Felix'], [{ id: 7, text: 'ship it to leo seal I users, Felix says' }]);
  context.startDictationEdit({ dataset: { id: '7' } });
  assert.ok(panel.html.includes('data-edit-text'));
  type(context, 'ship it to Leo CLI users, Felix says Acme Corp now Zed Labs today Extra Word');
  await context.saveDictationEdit();
  assert.deepEqual(plain(calls), [{ action: 'edit', id: 7, text: 'ship it to Leo CLI users, Felix says Acme Corp now Zed Labs today Extra Word' }]);
  assert.deepEqual(chipWords(panel.html), ['Leo CLI', 'Acme Corp now Zed Labs today Extra Word']);
  assert.ok(panel.html.includes('Edited'));
  assert.ok(!panel.html.includes('data-edit-text'));
  assert.ok(panel.html.includes('Add \u201cLeo CLI\u201d to custom words'));
});
test('edit offers skip known words, punctuation-only changes and cap at three', () => {
  const offers = (before, after, known) => plain(editContext(known, []).context.dictationWordOffers(before, after, known));
  assert.deepEqual(offers('a leo b', 'a Leo b', ['leo']), []);
  assert.deepEqual(offers('hello users', 'hello users.', []), []);
  assert.deepEqual(offers('x y', 'x Aaa y Bbb y Ccc y Ddd', []), ['Aaa', 'Bbb', 'Ccc']);
  assert.deepEqual(offers('x y', 'x A1 y Bb y Cc', []), []);
  assert.deepEqual(offers('x y', 'x what y Zed', []), ['Zed']);
  assert.deepEqual(offers('x y', 'x it y The y okay', []), []);
  assert.deepEqual(offers('x y', 'x very much y', []), []);
  assert.deepEqual(offers('x y', 'x Noarlunga y', []), ['Noarlunga']);
  assert.deepEqual(offers('x y', 'x Q y Zed', []), ['Zed']);
  assert.deepEqual(offers('x y', 'x Zed y zed', []), ['Zed']);
  assert.deepEqual(offers('x y', 'x 東京 y', []), ['東京']);
  assert.deepEqual(offers('x y', 'x 京 y Bb', []), []);
});
test('an edited dictation keeps its original behind a Show original toggle', () => {
  const { context, panel } = editContext([], [{ id: 7, text: 'Ship the Noarlunga deck.', edited: true, original: 'Ship the no longer deck.' }, { id: 8, text: 'Plain', edited: true, original: null }, { id: 9, text: 'Fresh' }]);
  context.showDictationRecent();
  assert.ok(panel.html.includes('Edited \u00b7 <button type="button" class="link-btn" data-action="dictation-original" data-id="7" aria-expanded="false">Show original</button>'));
  assert.ok(!panel.html.includes('Original:'));
  assert.equal((panel.html.match(/dictation-original/g) || []).length, 1);
  assert.ok(panel.html.includes('<span class="hint">Edited</span>'));
  context.toggleDictationOriginal({ dataset: { id: '7' } });
  assert.ok(panel.html.includes('>Hide original</button>'));
  assert.ok(panel.html.includes('<p class="set-note" data-refresh-key="original-7">Original: Ship the no longer deck.</p>'));
  context.toggleDictationOriginal({ dataset: { id: '7' } });
  assert.ok(panel.html.includes('>Show original</button>'));
  assert.ok(!panel.html.includes('Original:'));
});
test('an original is escaped', () => {
  const { context, panel } = editContext([], [{ id: 1, text: 'x', edited: true, original: '<img src=x onerror=bad()>' }]);
  context.toggleDictationOriginal({ dataset: { id: '1' } });
  assert.ok(panel.html.includes('Original: &lt;img'));
  assert.ok(!panel.html.includes('<img'));
});
test('accepting an offer adds the word once, respects the limit, and Escape cancels', async () => {
  const { context, listeners, calls, panel, field, result } = editContext(['Felix'], [{ id: 7, text: 'leo seal I' }]);
  context.startDictationEdit({ dataset: { id: '7' } });
  type(context, 'Leo CLI');
  await context.saveDictationEdit();
  const saved = [];
  context.saveDictation = () => { saved.push(field.value); return Promise.resolve(); };
  const chip = { dataset: { id: '7', word: 'Leo CLI' } };
  await context.addDictationWord(chip);
  assert.equal(field.value, 'Felix\nLeo CLI');
  assert.deepEqual(chipWords(panel.html), []);
  assert.equal(saved.length, 1);
  vm.runInContext("dictationOffers[7] = ['Leo CLI']", context);
  await context.addDictationWord(chip);
  assert.equal(field.value, 'Felix\nLeo CLI');
  field.value = Array.from({ length: 200 }, (_, n) => `w${n}`).join('\n');
  vm.runInContext("dictationOffers[7] = ['Other']", context);
  await context.addDictationWord({ dataset: { id: '7', word: 'Other' } });
  assert.equal(field.value.split('\n').length, 200);
  assert.ok(!field.value.includes('Other'));
  assert.ok(result.textContent.includes('full'));
  assert.deepEqual(chipWords(panel.html), []);
  context.state.dictation.recent = [{ id: 7, text: 'leo seal I' }];
  const before = calls.length;
  context.startDictationEdit({ dataset: { id: '7' } });
  type(context, 'something else');
  let stopped = false;
  listeners.keydown({ target: { matches: () => true }, key: 'Escape', preventDefault() {}, stopPropagation() { stopped = true; } });
  assert.equal(vm.runInContext('dictationEdit', context), null);
  assert.ok(stopped);
  assert.equal(calls.length, before);
  assert.ok(!panel.html.includes('data-edit-text'));
});

test('clearing history asks first', () => {
  const calls = [];
  const context = { app: { addEventListener() {} }, confirmSheet: (options) => calls.push(options), state: {}, window: {}, document: { activeElement: null, addEventListener() {} }, setTimeout() {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  context.clearDictationsAsk();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].action, 'dictation-clear-now');
  assert.equal(calls[0].yes, 'Clear history');
});
test('popup live and minimal states keep useful errors and timers', () => {
  const { display } = require('../../leo-desktop/dist/dictation.js');
  assert.equal(display({ state: 'listening', seconds: 62.9, text: 'hello' }).timer, '1:02');
  assert.equal(display({ state: 'listening', minimal: true, committed: 'hello', tentative: 'there' }).text, '');
  assert.equal(display({ state: 'error', minimal: true, message: 'Choose a microphone.' }).text, 'Choose a microphone.');
  assert.equal(display({ state: 'error', message: 'x' }).label, 'Dictation stopped');
  assert.equal(display({ state: 'loading' }).label, 'Loading model…');
  const starting = display({ state: 'starting' });
  assert.equal(starting.label, 'Starting microphone…');
  assert.equal(starting.recording, false);
  assert.equal(display({ state: 'writing', committed: 'hi' }).label, 'Polishing with AI…');
  assert.equal(display({ state: 'writing', committed: 'hi' }).caret, false);
  const notice = display({ state: 'notice', message: 'The writing AI took too long.', committed: 'hi' });
  assert.equal(notice.label, 'Pasted without AI writing');
  assert.equal(notice.text, 'The writing AI took too long.');
  assert.equal(notice.caret, false);
  assert.equal(display({ state: 'notice', title: 'Nothing heard', message: 'x' }).label, 'Nothing heard');
  assert.equal(display({ state: 'notice', title: '', message: 'x' }).label, 'Pasted without AI writing');
  assert.equal(display({ state: 'error', title: 'Nothing heard', message: 'x' }).label, 'Dictation stopped');
  assert.equal(display({ state: 'listening', top: true }).top, true);
});
function popupPage() {
  const dir = path.join(__dirname, '../../leo-desktop/dist');
  const nodes = {}, listeners = {};
  const node = (id) => nodes[id] || (nodes[id] = {
    id, textContent: '', hidden: false, dataset: {}, style: {}, children: [], scrollTop: 0, scrollHeight: 0, clientHeight: 0,
    classList: { toggle() {} }, appendChild(child) { this.children.push(child); }, addEventListener() {}, setAttribute() {}, removeAttribute() {},
  });
  const root = {
    document: { body: node('body'), documentElement: node('html'), getElementById: node, createElement: () => ({ style: {} }) },
    addEventListener: (name, fn) => { listeners[name] = fn; }, setInterval() {},
    __TAURI_INTERNALS__: { invoke: () => new Promise(() => {}), transformCallback: (fn) => { root.emit = fn; return 1; } },
  };
  vm.runInNewContext(fs.readFileSync(path.join(dir, 'dictation.js'), 'utf8'), root);
  return { node, listeners, emit: (payload) => root.emit({ payload }) };
}
test('popup blocks its context menu, shows the notice title and drops stale revisions', () => {
  const page = popupPage();
  let prevented = false;
  page.listeners.contextmenu({ preventDefault() { prevented = true; } });
  assert.equal(prevented, true);
  page.emit({ state: 'listening', rev: 5, seconds: 3 });
  assert.equal(page.node('wave').hidden, false);
  assert.equal(page.node('timer').textContent, '0:03');
  page.emit({ state: 'notice', rev: 6, title: 'Check the paste', message: 'Leo could not confirm the paste.' });
  assert.equal(page.node('label').textContent, 'Check the paste');
  assert.equal(page.node('message').textContent, 'Leo could not confirm the paste.');
  assert.equal(page.node('wave').hidden, true);
  assert.equal(page.node('timer').textContent, '');
  page.emit({ state: 'listening', rev: 4 });
  assert.equal(page.node('label').textContent, 'Check the paste');
});
test('popup page hides live words from screen readers and the red bars outside listening', () => {
  const dir = path.join(__dirname, '../../leo-desktop/dist');
  const html = fs.readFileSync(path.join(dir, 'dictation.html'), 'utf8');
  for (const id of ['timer', 'committed', 'tentative', 'caret']) assert.match(html, new RegExp(`<span id="${id}" aria-hidden="true"`));
  for (const id of ['label', 'message']) assert.doesNotMatch(html, new RegExp(`<span id="${id}"[^>]*aria-hidden`));
  assert.match(html, /<div id="pill" role="status" aria-live="polite">/);
  assert.match(fs.readFileSync(path.join(dir, 'dictation.css'), 'utf8'), /#wave\[hidden\] \{ display: none; \}/);
});
test('popup shows committed and tentative words like Handy and keeps them while writing', () => {
  const { display, smooth, height } = require('../../leo-desktop/dist/dictation.js');
  const live = display({ state: 'listening', committed: 'Hello there.', tentative: 'how are' });
  assert.equal(live.text, 'Hello there. how are');
  assert.equal(live.committed, 'Hello there. ');
  assert.equal(live.caret, true);
  assert.equal(live.open, true);
  const writing = display({ state: 'transcribing', committed: 'Hello there.', tentative: '' });
  assert.equal(writing.text, 'Hello there. ');
  assert.equal(writing.caret, false);
  assert.equal(display({ state: 'listening', tentative: 'hi' }).text, 'hi');
  assert.equal(display({ state: 'listening', minimal: true, committed: 'hi' }).open, false);
  assert.equal(display({ state: 'listening' }).caret, false);
  assert.deepEqual(smooth([1, 0], [0, 1]), [0.7, 0.3]);
  assert.equal(height(0), 3);
  assert.equal(height(1), 18);
  assert.equal(height(5), 18);
});

for (const debounced of [false, true]) {
  test(`${debounced ? 'debounced' : 'queued'} dictation saves keep their draft when navigation replaces page state`, async () => {
    const listeners = {}, timers = [], calls = [];
    const field = { dataset: { dictation: 'custom_words' }, value: 'Charlton\n東京' };
    const context = {
      app: { addEventListener: (name, listener) => { listeners[name] = listener; }, querySelectorAll: () => [field] },
      state: { view: 'settings', dictation: { settings: { shortcut: 'fn' } } }, seq: 1,
      clearTimeout() {}, setTimeout: callback => { timers.push(callback); },
      api: async (url, options) => { calls.push({ url, ...options }); return { settings: options.body.settings }; },
      fail: error => { throw error; },
    };
    vm.createContext(context);
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
    let saving;
    if (debounced) listeners.input({ target: field });
    else saving = context.saveDictation();
    context.state = { view: 'settings', section: 'calendar' };
    context.seq++;
    if (debounced) timers.shift()();
    await (saving || vm.runInContext('dictationSaving', context));
    assert.equal(calls.length, 1);
    assert.deepEqual(JSON.parse(JSON.stringify(calls[0].body.settings)), { shortcut: 'fn', custom_words: ['Charlton', '東京'] });
    assert.equal(context.state.dictation, undefined);
  });
}

test('a debounced custom-words save sends the shortcut saved since, not the one it was typed with', async () => {
  const listeners = {}, timers = [], calls = [];
  const field = { dataset: { dictation: 'custom_words' }, value: 'Charlton' };
  const context = {
    app: { addEventListener: (name, listener) => { listeners[name] = listener; }, querySelectorAll: () => [field] },
    state: { view: 'settings', dictation: { settings: { shortcut: 'fn' } } }, seq: 1,
    clearTimeout() {}, setTimeout: callback => { timers.push(callback); },
    api: async (url, options) => { calls.push(options.body); return { settings: options.body.settings }; },
    fail: error => { throw error; },
  };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  listeners.input({ target: field });
  context.state.dictation.settings = { shortcut: 'command+k' };
  timers.shift()();
  context.state.view = 'notes';
  await vm.runInContext('dictationSaving', context);
  assert.deepEqual(plain(calls[0].settings), { shortcut: 'command+k', custom_words: ['Charlton'] });
});
