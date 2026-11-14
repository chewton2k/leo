const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
test('late Settings saves cannot redraw a different page on success or failure', async () => {
  for (const failed of [false, true]) {
    let finish;
    const context = { app: { addEventListener() {} }, seq: 1, state: { view: 'settings', settings: {} }, document: { addEventListener() {} }, api: () => new Promise((resolve, reject) => { finish = () => failed ? reject(new Error('failed')) : resolve({settings:{},message:'Saved'}); }), window: {}, toast() {} };
    vm.createContext(context);
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
    let draws = 0;
    context.drawSettings = () => { draws++; };
    const pending = context.changeSetting({set:'effort',value:'high'}).catch(() => {});
    context.seq++;
    context.state = {view:'folder',settings:{}};
    finish();
    await pending;
    assert.equal(draws, 0);
  }
});
test('the writing card says Ready only when the writing check is ready', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, {get: () => ''}), esc: text => String(text), choiceCards: () => '' };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const task = { task: 'writing', provider: 'ollama', ready: true, choices: [], models: [] };
  for (const [writing, label] of [['ready', 'Ready'], ['no_model', 'Not set up'], [undefined, 'Not set up']]) {
    context.state = { settings: { writing: writing && { state: writing } } };
    assert.ok(context.taskCard(task).includes(`>${label}</span>`), writing);
  }
});
test('the writing card says where notes go for each provider and stays quiet for custom ones', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, {get: () => ''}), esc: text => String(text), choiceCards: () => '', state: { settings: {} } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const line = (provider, task = 'writing') => (context.taskCard({ task, provider, choices: [], models: [] }).match(/<p class="set-note">([^<]*)<\/p>/) || [])[1];
  assert.equal(line('claude_code'), 'Notes and transcripts you send go to Anthropic through Claude Code.');
  assert.equal(line('codex'), 'Notes and transcripts you send go to OpenAI through Codex.');
  assert.equal(line('ollama'), 'Ollama runs on this computer; nothing leaves it.');
  assert.equal(line('my-custom'), undefined);
});
test('late onboarding completion cannot close a newer sheet', async () => {
  let finish, closed = 0;
  const context = { desktopApp: null, appChange: () => new Promise(resolve => { finish = resolve; }), closeSheet() { closed++; }, document: { querySelector: () => null } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  context.maybeOnboarding = () => {};
  const pending = context.finishOnboarding({dataset:{step:'0'}});
  finish();
  await pending;
  assert.equal(closed, 0);
});

test('the note footer keeps its note tools without the bottom Ask button', () => {
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const script = core.slice(core.indexOf('function noteActions('), core.indexOf('async function showNote('));
  const context = { ON_MAC: true, ICON: new Proxy({}, {get: () => '<svg></svg>'}), esc: text => String(text) };
  vm.createContext(context);
  vm.runInContext(script, context);
  const html = context.noteActions('note-id', true);
  assert.doesNotMatch(html, /data-action="(?:ask-selection|note-map)"/);
  for (const name of ['Show transcript', 'Show tasks', 'Add picture', 'Move to folder', 'Share', 'Move to trash']) assert.ok(html.includes('aria-label="' + name + '"'));
});

test('Codex and Claude Code models list newest release first, then by power', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const order = (ids) => JSON.stringify(context.modelsByRelease(ids.map((id) => ({ id }))).map((m) => m.id));
  assert.equal(order(['gpt-6-luna', 'gpt-6-sol', 'gpt-6.1-sol', 'gpt-6-astra']), JSON.stringify(['gpt-6.1-sol', 'gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna']));
  assert.equal(order(['claude-haiku-5-5', 'claude-sonnet-5-5', 'claude-opus-5-5', 'claude-fable-5-1']), JSON.stringify(['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5', 'claude-fable-5-1']));
  assert.equal(order(['plain', 'gpt-6.9-a', 'gpt-6.10-a']), JSON.stringify(['gpt-6.10-a', 'gpt-6.9-a', 'plain']));
});
test('the calendar card guides each provider and checks the address before saving', () => {
  const listeners = {};
  const nodes = {};
  const field = (extra = {}) => ({ value: '', attrs: {}, setAttribute(k, v) { this.attrs[k] = v; }, ...extra });
  const context = { app: { addEventListener() {} }, document: { addEventListener: (type, fn) => (listeners[type] = [...(listeners[type] || []), fn]) }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: text => String(text), $: sel => nodes[sel] || null, segmented: ({ current, choices, field: f }) => `<seg ${f} ${current}>${choices.map(c => c[1]).join('|')}</seg>`, state: {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const firstLink = { google: 'https://calendar.google.com/calendar/r/settings', outlook: 'https://outlook.live.com/calendar/0/options/calendar/SharedCalendars', apple: 'https://www.icloud.com/calendar' };
  for (const [provider, url] of Object.entries(firstLink)) {
    context.state.calendarProvider = provider;
    const html = context.calendarCard({ calendars: [] });
    assert.equal((html.match(/<li>/g) || []).length, 3, provider);
    assert.ok(html.includes(`<li>Open <a href="${url}" target="_blank" rel="noopener noreferrer">`), provider);
    assert.ok(html.includes('<seg data-cal-provider ' + provider + '>Google|Outlook|Apple</seg>'));
  }
  assert.ok(context.calendarSteps('apple').includes('webcal://'));
  assert.ok(context.calendarSteps('outlook').includes('Publish a calendar'));
  const steps = { outerHTML: '' };
  nodes['.cal-steps'] = steps;
  nodes['#calendar-link'] = field();
  nodes['#calendar-error'] = { textContent: 'old' };
  for (const fn of listeners.change) fn({ target: { closest: () => ({ value: 'outlook' }) } });
  assert.equal(context.state.calendarProvider, 'outlook');
  assert.ok(steps.outerHTML.includes('outlook.live.com') && !steps.outerHTML.includes('google.com'));
  assert.equal(nodes['#calendar-error'].textContent, '');
  const share = 'That is the share link. Paste the secret address that ends in .ics.';
  const generic = 'Paste the calendar address that starts with https:// or webcal://.';
  assert.equal(context.calendarAddressError('https://calendar.google.com/calendar/u/0?cid=abc', 'google'), share);
  assert.equal(context.calendarAddressError('https://calendar.google.com/calendar?cid=abc', 'google'), share);
  assert.equal(context.calendarAddressError('hello', 'google'), generic);
  assert.equal(context.calendarAddressError('webcal://p01-caldav.icloud.com/published/2/abc', 'apple'), '');
  assert.equal(context.calendarAddressError('https://calendar.google.com/calendar/ical/me/private-abc/basic.ics', 'google'), '');
  context.state.calendarProvider = 'google';
  nodes['#calendar-link'].value = 'https://calendar.google.com/calendar/u/0?cid=abc';
  return context.addCalendar().then(() => {
    assert.equal(nodes['#calendar-error'].textContent, share);
    assert.equal(nodes['#calendar-link'].attrs['aria-invalid'], 'true');
  });
});
test('the calendar card offers to record when an event starts, on by default once connected', async () => {
  const listeners = {};
  const sent = [];
  const context = { app: { addEventListener() {} }, document: { addEventListener: (type, fn) => (listeners[type] = [...(listeners[type] || []), fn]) }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: text => String(text), segmented: () => '', settingsSwitch: (attrs, on) => `<input ${attrs}${on ? ' checked' : ''}>`, rel: () => 'today', api: async (url, options) => { sent.push([url, options]); return { offer: options.body.offer }; }, state: {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const row = /<label class="set-row" data-find="calendar-offer"><span class="grow">Offer to record when an event starts<span class="set-desc">[^<]*<\/span><\/span><input data-cal-offer( checked)?><\/label>/;
  const linked = [{ id: 'a', name: 'Work', added_at: '2026-11-01T00:00:00Z' }];
  assert.doesNotMatch(context.calendarCard({ calendars: [] }), /calendar-offer/);
  assert.equal(context.calendarCard({ calendars: linked }).match(row)[1], ' checked');
  assert.equal(context.calendarCard({ calendars: linked, offer: true }).match(row)[1], ' checked');
  assert.equal(context.calendarCard({ calendars: linked, offer: false }).match(row)[1], undefined);
  const box = { checked: false, matches: (at) => at === '[data-cal-offer]' };
  await Promise.all(listeners.change.map((fn) => fn({ target: box })));
  assert.deepEqual(JSON.parse(JSON.stringify(sent)), [['/api/calendar', { method: 'PATCH', body: { offer: false } }]]);
  assert.equal(context.state.calendar.offer, false);
});

test('Ollama setup guides an install, waits for the app, and lists models that fit', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, {get: () => ''}), esc: text => String(text), choiceCards: () => '', ON_MAC: true };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const missing = context.ollamaSetup({ installed: false, running: false, models: [] });
  assert.match(missing, /Ollama runs AI models on this Mac\. It isn’t installed yet\./);
  assert.match(missing, /<a class="btn primary sm" href="https:\/\/ollama\.com\/download" target="_blank"/);
  assert.match(missing, /lists models that fit this Mac\. No Terminal needed\./);
  context.ON_MAC = false;
  assert.match(context.ollamaSetup({ installed: false }), /on this computer\. It isn’t installed yet/);
  context.ON_MAC = true;
  assert.equal(context.ollamaSetup({ installed: true, running: false }), '');
  context.state = { settings: {}, ollama: { installed: false } };
  const card = context.taskCard({ task: 'writing', provider: 'ollama', note: 'Ollama is not installed on this computer.', choices: [], models: [] });
  assert.doesNotMatch(card, /Ollama is not installed on this computer/);
  assert.match(card, /id="ollama-setup"/);
  context.state.ollama = { installed: true, running: false };
  assert.match(context.taskCard({ task: 'writing', provider: 'ollama', note: 'Ollama is not running: open the Ollama app on this computer.', choices: [], models: [] }), /open the Ollama app/);
  const models = [
    { name: 'qwen3:4b', bytes: 2500000000, memory_gb: 8, about: 'Faster, shorter notes', fits: true, pulled: true },
    { name: 'qwen3:8b', bytes: 5200000000, memory_gb: 16, about: 'Better notes', fits: true, recommended: true },
    { name: 'gemma3:27b', bytes: 17000000000, memory_gb: 32, about: 'Best quality, slow', fits: false },
  ];
  const running = context.ollamaSetup({ installed: true, running: true, memory_bytes: 17179869184, free_disk_bytes: 48000000000, models }, 'qwen3:4b');
  assert.match(running, /This Mac has 16 GB of memory and 48 GB free\./);
  assert.match(running, /data-model="qwen3:4b" aria-pressed="false"><span class="onboarding-model-head"><b>qwen3:4b<\/b><span class="chip">Fits this Mac<\/span><span class="onboarding-model-state"><span class="chip accent">In use<\/span>/);
  assert.match(running, /data-model="qwen3:8b" aria-pressed="false"><span class="onboarding-model-head"><b>qwen3:8b<\/b><span class="chip">Recommended<\/span>/);
  assert.match(running, /data-model="gemma3:27b" aria-pressed="false" disabled><span class="onboarding-model-head"><b>gemma3:27b<\/b><span class="chip">Too big for this Mac<\/span>/);
  assert.match(running, /2\.5 GB · needs 8 GB of memory/);
  assert.match(running, /17 GB · needs 32 GB of memory/);
  assert.doesNotMatch(running, /onboarding-download/);
  assert.doesNotMatch(running, /data-action="ollama-pull"/, 'nothing downloads until a card is chosen and Download is pressed');
  const chosen = context.ollamaSetup({ installed: true, running: true, models }, 'qwen3:4b', 'qwen3:8b');
  assert.match(chosen, /class="onboarding-model on" data-action="ollama-pick" data-model="qwen3:8b" aria-pressed="true"/);
  assert.match(chosen, /<button class="btn primary" data-action="ollama-pull" data-model="qwen3:8b">Download · 5\.2 GB<\/button>/);
  assert.match(context.ollamaSetup({ installed: true, running: true, models }, 'qwen3:8b', 'qwen3:4b'), /data-action="ollama-pull" data-model="qwen3:4b">Use<\/button>/);
  assert.doesNotMatch(context.ollamaSetup({ installed: true, running: true, models }, 'qwen3:4b', 'qwen3:4b'), /data-action="ollama-pull"/);
  assert.doesNotMatch(context.ollamaSetup({ installed: true, running: true, models }, 'qwen3:4b', 'gemma3:27b'), /data-action="ollama-pull"/);
  let panel = '';
  Object.assign(context, { onboardingHere: () => false, $: () => ({}), refreshPanel: (box, html) => { panel = html; }, writingModel: () => 'qwen3:4b' });
  context.state = { ollama: { installed: true, running: true, models } };
  context.ollamaPick({ dataset: { model: 'qwen3:8b' } });
  assert.match(panel, /data-model="qwen3:8b" aria-pressed="true"/);
  assert.match(panel, />Download · 5\.2 GB</);
  const pulling = context.ollamaSetup({ installed: true, running: true, models, pull: { model: 'qwen3:8b', state: 'pulling', completed: 3120000000, total: 5200000000 } });
  assert.match(pulling, /class="onboarding-model on" data-action="ollama-pick" data-model="qwen3:8b" aria-pressed="true"/);
  assert.doesNotMatch(pulling, /Download · /);
  assert.match(pulling, /<span class="chip accent">Downloading 60%<\/span>/);
  assert.match(pulling, /Downloading qwen3:8b · 3\.1 GB of 5\.2 GB<\/span><progress value="60" max="100"/);
  assert.match(pulling, /data-action="ollama-cancel">Cancel<\/button>/);
  assert.doesNotMatch(pulling, /This Mac has/);
  const failed = context.ollamaSetup({ installed: true, running: true, models, pull: { model: 'qwen3:8b', state: 'failed', error: "There isn't enough free disk space for this model." } });
  assert.match(failed, /set-note warn">There isn't enough free disk space/);
  assert.doesNotMatch(failed, /ollama-cancel/);
});

test('Settings → Writing AI signs in, waits for the probe and turns the status Ready', async () => {
  const timers = [], calls = [];
  let clock = 0, signedIn = false;
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: (text) => String(text), choiceCards: () => '', ON_MAC: true, seq: 1, Date: { now: () => clock },
    setTimeout: (fn, ms) => { timers.push([fn, ms]); return timers.length; }, clearTimeout() {},
    appChange: async (change) => { calls.push(JSON.stringify(change)); },
    api: async (url) => { calls.push(url); return { writing: { state: signedIn ? 'ready' : 'not_signed_in' }, candidates: [{ id: 'claude_code', installed: true, signed_in: signedIn }] }; } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const task = { task: 'writing', provider: 'claude_code', choices: [], models: [] };
  context.state = { view: 'settings', section: 'writing', settings: { writing: { state: 'not_signed_in' }, plans: [{ provider: 'claude_code', installed: true, signed_in: false }] } };
  let card = '';
  context.drawSettings = (page) => { card = context.taskCard(task); };
  assert.match(context.taskCard(task), /data-action="writing-sign-in" data-provider="claude_code">Sign in</);
  await context.writingSignIn({ dataset: { provider: 'claude_code' } });
  assert.deepEqual(calls, ['{"action":"sign_in","provider":"claude_code"}']);
  assert.match(card, /disabled>Waiting for sign-in…/);
  assert.equal(timers[0][1], 3000);
  signedIn = true;
  clock += 3000;
  await timers.shift()[0]();
  assert.match(card, /set-status ok">Ready/);
  assert.doesNotMatch(card, /writing-sign-in|Waiting/);
  assert.equal(timers.length, 0);
  Object.assign(context.state.settings.plans[0], { installed: false, signed_in: null });
  assert.ok(context.taskCard(task).includes('href="https://claude.com/claude-code"'));
  context.state.settings.plans[0] = { provider: 'codex', installed: false };
  assert.ok(context.taskCard({ ...task, provider: 'codex' }).includes('<a class="btn plain sm" href="https://github.com/openai/codex" target="_blank" rel="noopener noreferrer">Install Codex</a>'));
});

test('leaving Settings stops the sign-in check', async () => {
  const timers = [], calls = [];
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, seq: 1, Date: { now: () => 0 }, drawSettings() {},
    setTimeout: (fn, ms) => { timers.push([fn, ms]); return timers.length; }, clearTimeout() {},
    appChange: async () => {}, api: async (url) => { calls.push(url); return {}; } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  context.drawSettings = () => {};
  context.state = { view: 'settings', section: 'writing', settings: { plans: [] } };
  await context.writingSignIn({ dataset: { provider: 'codex' } });
  context.state = { view: 'note' };
  await timers.shift()[0]();
  assert.deepEqual(calls, []);
  assert.equal(timers.length, 0);
});
