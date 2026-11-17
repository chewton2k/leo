const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
function context() {
  const timers = [];
  const listeners = {};
  const listen = (name, fn) => { listeners[name] = fn; };
  const value = { svg: (paths) => `<svg>${paths}</svg>`, app: { addEventListener() {}, querySelectorAll: () => [] }, esc: text => String(text).replace(/[<>&"]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c])), state: { view: 'folder' }, $: () => null, window: { addEventListener: listen }, document: { activeElement: null, visibilityState: 'visible', addEventListener: listen }, updateOnboardingPermissions() {}, maybeOnboarding() {}, onboardingHere: () => false, setTimeout: (fn, ms) => { timers.push({ fn, ms }); return timers.length; }, clearTimeout: id => { if (id) timers[id - 1] = null; }, api: async () => ({ preferences: {}, updates: {} }), speechBytes: (b) => Math.round(b / 1048576) + ' MB', skeleton: (n) => '<div class="skeleton"></div>'.repeat(n), md: { render: text => String(text).replace(/</g, '&lt;') } };
  vm.createContext(value);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/refresh.js'), 'utf8'), value);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), value);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/app-settings.js'), 'utf8'), value, { filename: path.join(__dirname, '../src/web/app/app-settings.js') });
  return { value, timers, listeners };
}
test('hidden and native-hidden windows make no automatic app-status requests', async () => {
  for (const native of [false, true]) {
    const { value, timers } = context();
    let calls = 0;
    value.api = async () => { calls++; return {}; };
    if (native) value.window.__LEO_VISIBLE__ = false;
    else value.document.visibilityState = 'hidden';
    await value.pollAppStatus(0);
    assert.equal(calls, 0);
    assert.equal(timers.filter(Boolean).length, 0);
  }
});
test('failed status reads retry and ordinary note views use the slower status cadence', async () => {
  const { value, timers } = context();
  value.api = async () => { throw new Error('temporary disconnect'); };
  await value.pollAppStatus(0);
  assert.equal(timers.filter(Boolean).length, 1);
  assert.equal(timers.filter(Boolean)[0].ms, 30000);
});
test('settings, active updates and onboarding retain their responsive cadence', async () => {
  for (const mode of ['settings', 'checking', 'downloading', 'installing', 'onboarding']) {
    const { value, timers } = context();
    if (mode === 'settings') value.state = {view:'settings',section:'general'};
    value.api = async () => ({updates:{state:mode}});
    if (mode === 'onboarding') { value.onboardingHere = () => true; value.ONBOARDING_POLL_MS = 1000; }
    await value.pollAppStatus(0);
    assert.equal(timers.filter(Boolean).length, 1);
    assert.equal(timers.filter(Boolean)[0].ms, mode === 'onboarding' ? 1000 : 2500);
  }
});
test('explicit native actions still run while hidden without restarting automatic work', async () => {
  const { value, timers } = context();
  let calls = 0;
  value.document.visibilityState = 'hidden';
  value.api = async () => { calls++; return {}; };
  await value.appChange({action:'preference'});
  assert.equal(calls, 1);
  assert.equal(timers.filter(Boolean).length, 0);
});
test('hide and resume invalidate late responses and leave only one polling chain', async () => {
  const { value, timers, listeners } = context();
  let finish, calls = 0;
  value.api = () => { calls++; return new Promise(resolve => { finish = resolve; }); };
  const pending = value.pollAppStatus(0);
  value.document.visibilityState = 'hidden';
  listeners.visibilitychange();
  finish({ version: 'stale' });
  await pending;
  assert.equal(timers.filter(Boolean).length, 0);
  assert.equal(vm.runInContext('desktopApp', value), null);
  value.api = async () => { calls++; return {version:'fresh'}; };
  value.document.visibilityState = 'visible';
  listeners.visibilitychange();
  await new Promise(resolve => setImmediate(resolve));
  listeners['leo-visibility']();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(calls, 2);
  assert.equal(timers.filter(Boolean).length, 1);
  assert.equal(vm.runInContext('desktopApp.version', value), 'fresh');
});
test('app polling resumes after successful and failed native actions', async () => {
  const { value, timers } = context();
  await value.appChange({ action: 'microphone' });
  assert.ok(timers.length > 0);
  timers.length = 0;
  value.api = async () => { throw new Error('permission request failed'); };
  await assert.rejects(value.appChange({ action: 'microphone' }));
  assert.ok(timers.length > 0);
});
test('Updates renders ready, busy and release notes safely with independent defaults', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', unsigned: true, preferences: { automatic_updates: true, install_on_quit: true }, updates: { state: 'ready', version: '2.0.0', busy: true, notes: '<script>bad()</script>' }, paths: { notes: '/notes', models: '/models' } });
  assert.ok(html.includes('Restart to update'));
  assert.ok(html.includes('Updates wait for recording and dictation to finish, then leo reopens.'));
  assert.ok(html.includes('Finish recording or dictation'));
  assert.ok(html.includes('checked'));
  assert.equal((html.match(/role="switch"/g) || []).length, 2);
  assert.ok(!html.includes('<script>'));
  assert.ok(html.includes('Accessibility'));
  assert.ok(html.includes('leo update') && html.includes('leo uninstall'));
  assert.ok(html.includes('your notes stay') && html.includes('title="/notes"'));
});

function tail(extra) {
  const events = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8');
  const window = {};
  const messages = [];
  const calls = [];
  const scope = { window, toast: (text, options) => messages.push([text, options]), go: (hash) => calls.push(hash), location: { hash: '#/' }, setTimeout, saveWebsiteWork() {}, ...extra };
  vm.runInNewContext(events.slice(events.indexOf('const APP_SETTINGS =')) + '\nwindow.warnUnsaved = warnUnsaved;', scope);
  return { window, messages, calls };
}
test('a native quit confirmation explains both ways to quit without navigating', async () => {
  const { window, messages } = tail({ go: () => assert.fail('Quit must keep the current page'), api: async () => ({ job: null }) });
  await window.leoFromApp('quit', '');
  assert.equal(messages.length, 1);
  assert.ok(messages[0][0].includes('⌘Q again') && messages[0][0].includes('Quit again') && messages[0][0].includes('3 seconds'));
});
test('a native quit flushes pending autosaves before it toasts', async () => {
  const order = [];
  const { window } = tail({ saveWebsiteWork: () => order.push('save'), toast: () => order.push('toast'), api: async () => ({ job: null }) });
  await window.leoFromApp('quit', '');
  assert.deepEqual(order, ['save', 'toast']);
});
test('quitting says what is lost while a recording is live or its note is being written', async () => {
  const text = async (api) => { const { window, messages } = tail({ api }); await window.leoFromApp('quit', ''); return messages[0][0]; };
  assert.equal(await text(async () => ({ job: { state: 'writing' } })), 'Press ⌘Q again to quit. A note is still being written; you can recover it from Record next time.');
  for (const state of ['starting', 'recording', 'paused']) assert.equal(await text(async () => ({ job: { state } })), 'Press ⌘Q again to quit. The recording stops and you can recover it from Record next time.');
  for (const job of [null, { state: 'done' }, { state: 'failed' }]) assert.ok((await text(async () => ({ job }))).includes('within 3 seconds'));
});
test('a failed or slow recording check keeps the usual quit text', async () => {
  const usual = async (api) => { const { window, messages } = tail({ api }); await window.leoFromApp('quit', ''); return messages[0][0].includes('within 3 seconds'); };
  assert.ok(await usual(async () => { throw new Error('offline'); }));
  assert.ok(await usual(() => new Promise((resolve) => setTimeout(() => resolve({ job: { state: 'writing' } }), 1000))));
});
test('unsaved recordings are announced once with a way to Record, never on the Record page', async () => {
  const run = async (pending, hash = '#/') => {
    const { window, messages, calls } = tail({ api: async () => ({ pending }), location: { hash } });
    await window.warnUnsaved();
    return { messages, calls };
  };
  assert.equal((await run([])).messages.length, 0);
  assert.equal((await run([{ id: 'a' }], '#/record')).messages.length, 0);
  assert.equal((await run([{ id: 'a' }], '#/record/Lectures')).messages.length, 0);
  const one = await run([{ id: 'a' }]);
  assert.equal(one.messages[0][0], "1 recording wasn't saved");
  assert.equal(one.messages[0][1].action, 'Open Record');
  one.messages[0][1].run();
  assert.deepEqual(one.calls, ['#/record']);
  assert.equal((await run([{ id: 'a' }, { id: 'b' }, { id: 'c' }])).messages[0][0], "3 recordings weren't saved");
  const { window, messages } = tail({ api: async () => { throw new Error('offline'); } });
  await window.warnUnsaved();
  assert.equal(messages.length, 0);
});
test('the export text matches what the zip holds', () => {
  const keeping = fs.readFileSync(path.join(__dirname, '../src/web/app/keeping.js'), 'utf8');
  assert.ok(keeping.includes('Your notes and tasks, plus the parts you choose below: uploads with recording transcripts, chats with margin notes, and trash. Settings and keys are never included.'));
});

test('Keeping that could not be opened shows the broken pencil drawing', async () => {
  const keeping = fs.readFileSync(path.join(__dirname, '../src/web/app/keeping.js'), 'utf8');
  const shown = [];
  const ctx = { seq: 0, state: {}, app: { addEventListener() {} }, chrome() {}, $: () => ({}), document: {}, settingsCrumbs: () => '', skeleton: () => 'loading', settingsFrame: (name, html) => html, refreshPanel: (box, html) => shown.push(html), DRAWN: { broken: '<svg class="drawn drawn-broken"></svg>' }, empty: (icon, title) => `${icon}<h3>${title}</h3>`, api: async () => { throw new Error('offline'); } };
  vm.createContext(ctx);
  vm.runInContext(keeping, ctx);
  await assert.rejects(ctx.showKeeping());
  assert.equal(shown.at(-1), '<svg class="drawn drawn-broken"></svg><h3>Keeping could not be opened</h3>');
});
test('the error drawings are single-stroke line drawings like the empty states', () => {
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const ctx = {};
  vm.createContext(ctx);
  vm.runInContext(`${core.slice(core.indexOf('const lineDrawing ='), core.indexOf('const recorder ='))};globalThis.drawn = DRAWN;`, ctx);
  for (const name of ['offline', 'noink', 'broken']) {
    assert.match(ctx.drawn[name], new RegExp(`^<svg class="drawn drawn-${name}" viewBox="0 0 120 92" fill="none" stroke="currentColor" stroke-width="1.5"[^>]*aria-hidden="true">`));
    assert.doesNotMatch(ctx.drawn[name], /fill="(?!none)|#[0-9a-f]{3,6}/i);
  }
});
test('About leo ends with a colophon of plain facts', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'idle' }, paths: { notes: '/n', models: '/m' } });
  const colophon = html.slice(html.indexOf('<div class="about-colophon">'), html.lastIndexOf('</section>'));
  assert.ok(colophon.startsWith('<div class="about-colophon"><p>Your notes are Markdown files on this computer.</p>'));
  assert.deepEqual([...colophon.matchAll(/<span class="set-label">([^<]+)<\/span>/g)].map((m) => m[1]), ['Version', 'Note text', 'Felix’s voice', 'Licences']);
  assert.ok(colophon.includes('<span class="set-value-text">1.2.3</span>'));
  assert.ok(colophon.includes('Instrument Sans <span>· what you write</span>') && colophon.includes('Newsreader <span>· what Felix says in the margin and chat</span>'));
  assert.ok(colophon.includes('<span class="hint">MIT, fonts under the SIL Open Font License</span><a class="set-link" href="https://github.com/chewton2k/leo-cli/blob/main/NOTICE" target="_blank" rel="noopener noreferrer">NOTICE</a>'));
  assert.doesNotMatch(colophon, /set-path/);
});
test('the colophon names the lettering Felix really writes in', () => {
  const { value } = context();
  value.window.leoTheme = { felixLetters: () => 'notes' };
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'idle' }, paths: {} });
  assert.ok(html.includes('Instrument Sans <span>· what Felix says in the margin and chat</span>'));
  assert.ok(!html.includes('Newsreader'));
});
test('the downloading update shows a percent and a bar', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'downloading', version: '2.0.0', bytes: 41200000, total: 96500000 }, paths: {} });
  assert.ok(html.includes('42% · ') && html.includes('<progress value="42"'));
});
test('an available update has one primary Download and no Check now', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'available', version: '2.0.0' }, paths: {} });
  const primary = [...html.matchAll(/<button class="btn primary[^"]*"[^>]*data-app-action="([a-z_]+)"/g)].map((m) => m[1]);
  assert.deepEqual(primary, ['download_update']);
  assert.ok(!html.includes('data-app-action="check_updates"'));
});
test('a ready update offers Restart as the one primary action and Check now stays', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'ready', version: '2.0.0' }, paths: {} });
  assert.deepEqual([...html.matchAll(/<button class="btn primary[^"]*"[^>]*data-app-action="([a-z_]+)"/g)].map((m) => m[1]), ['restart_update']);
  assert.ok(html.includes('data-app-action="check_updates"'));
});
test('idle Updates shows only the last-checked line', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'idle' }, paths: {} });
  assert.ok(html.includes('Not checked yet') && !html.includes('Check for a newer version.'));
});
test('Health loads in shape', () => {
  const { value } = context();
  const html = value.healthCards(null);
  assert.ok(html.includes('Checking…') && html.includes('activity-spin'));
  assert.ok(html.includes('class="skeleton"'));
});
test('the microphone fix button opens the system settings when access is denied', () => {
  const { value } = context();
  value.ICON = new Proxy({}, { get: () => '' });
  const view = { sections: [{ checks: [{ name: 'microphone', state: 'missing', action: 'microphone' }] }] };
  const button = (permissions, platform) => {
    vm.runInContext(`desktopApp = ${JSON.stringify({ platform, permissions })}`, value);
    return value.healthCards(view);
  };
  assert.match(button({ microphone: 'ask' }, 'macos'), />Allow access<\/button>/);
  assert.match(button({ microphone: 'denied' }, 'macos'), />Open System Settings<\/button>/);
  assert.match(button({ microphone: 'denied' }, 'windows'), />Open Settings<\/button>/);
});
test('the notifications fix button opens the system settings only once notifications are denied', () => {
  const { value } = context();
  value.ICON = new Proxy({}, { get: () => '' });
  const view = { sections: [{ checks: [{ name: 'notifications', state: 'warn', action: 'notifications' }] }] };
  const button = (notifications) => {
    vm.runInContext(`desktopApp = ${JSON.stringify({ platform: 'macos', permissions: { microphone: 'denied', notifications } })}`, value);
    return value.healthCards(view);
  };
  assert.match(button('not_requested'), />Allow access<\/button>/);
  assert.match(button('denied'), />Open System Settings<\/button>/);
});
