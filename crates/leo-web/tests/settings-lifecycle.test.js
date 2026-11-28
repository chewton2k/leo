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

test('the note menu keeps every note tool, with Read through and Share as second steps and trash last', () => {
  const sheets = fs.readFileSync(path.join(__dirname, '../src/web/app/sheets.js'), 'utf8');
  const script = sheets.slice(sheets.indexOf('function shareRows('), sheets.indexOf('async function share('));
  const shown = [];
  const marginPage = { through: null, changing: false, persona: 'reader', s: { note: { id: 'note-id' } } };
  const context = { marginPage, marginPrefs: { personas: [{ id: 'reader', name: 'Reader', icon: 'book' }, { id: 'critic', name: 'Critic', icon: 'target' }] }, PERSONA_ICON: { book: '<svg></svg>', target: '<svg></svg>' }, ICON: new Proxy({}, { get: () => '<svg></svg>' }), state: { view: 'note', session: { note: { id: 'note-id', recorded: true } } }, popupMenu: (owner, { rows }) => shown.push(rows), marginPrintable: () => [], copyMarkdown() {}, saveMarkdown() {}, share() {}, split: () => false, closed: [], here: () => 'focused', paneClose: (p) => context.closed.push(p) };
  vm.createContext(context);
  vm.runInContext(script, context);
  context.noteMenu({});
  assert.equal(JSON.stringify(shown[0].filter((row) => row.text).map((row) => row.text)), JSON.stringify(['Show transcript', 'Tasks', 'Add picture', 'Move to folder', 'Read through', 'Share', 'Move to trash']));
  const through = shown[0].find((row) => row.text === 'Read through');
  assert.equal(JSON.stringify(through.next.map((row) => [row.text, row.checked])), JSON.stringify([['Reader', true], ['Critic', false]]));
  assert.equal(through.off, false);
  assert.equal(JSON.stringify(shown[0][5].next.map((row) => row.text)), JSON.stringify(['Copy as Markdown', 'Save .md file', 'Export as PDF']));
  Object.assign(marginPage, { changing: true });
  context.noteMenu({});
  assert.equal(shown[1].find((row) => row.text === 'Read through').off, true);
  Object.assign(marginPage, { changing: false, through: {} });
  context.noteMenu({});
  assert.equal(shown[2].find((row) => row.text === 'Stop reading through').next, undefined);
  assert.ok(shown[0].at(-1).danger);
  let loads = 0;
  Object.assign(context, { marginPrefs: null, marginLoadPrefs: () => loads++ });
  Object.assign(marginPage, { through: null });
  context.noteMenu({});
  const idle = shown[3].findIndex((row) => row.text === 'Read through');
  assert.equal(shown[3][idle].off, true);
  assert.match(shown[3][idle + 1].note, /did not load/);
  assert.equal(loads, 1);
  assert.equal(shown[0].some((row) => row.text === 'Close this pane'), false);
  context.split = () => true;
  context.noteMenu({});
  const texts = shown[4].filter((row) => row.text).map((row) => row.text);
  assert.equal(JSON.stringify(texts.slice(-2)), JSON.stringify(['Close this pane', 'Move to trash']));
  shown[4].find((row) => row.text === 'Close this pane').run();
  assert.deepEqual([...context.closed], ['focused']);
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
  const context = { app: { addEventListener() {} }, document: { addEventListener: (type, fn) => (listeners[type] = [...(listeners[type] || []), fn]) }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: text => String(text), $: sel => nodes[sel] || null, segmented: ({ current, choices, field: f }) => `<seg ${f} ${current}>${choices.map(c => c[1]).join('|')}</seg>`, settingsSwitch: () => '', state: {} };
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
  nodes['#calendar-error'] = { innerHTML: 'old' };
  for (const fn of listeners.change) fn({ target: { closest: () => ({ value: 'outlook' }) } });
  assert.equal(context.state.calendarProvider, 'outlook');
  assert.ok(steps.outerHTML.includes('outlook.live.com') && !steps.outerHTML.includes('google.com'));
  assert.equal(nodes['#calendar-error'].innerHTML, '');
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
    assert.equal(nodes['#calendar-error'].innerHTML, `${share} <a href="https://calendar.google.com/calendar/u/0/r/settings/calendar/abc" target="_blank" rel="noopener noreferrer">Open this calendar's settings</a>`);
    assert.equal(nodes['#calendar-link'].attrs['aria-invalid'], 'true');
    nodes['#calendar-link'].value = 'https://calendar.google.com/calendar/u/0?cid=a"b';
    return context.addCalendar();
  }).then(() => {
    assert.equal(nodes['#calendar-error'].innerHTML, share);
    nodes['#calendar-link'].value = 'hello';
    return context.addCalendar();
  }).then(() => assert.equal(nodes['#calendar-error'].innerHTML, generic));
});
test('the morning briefing shows its time, wishes and Make one now only when on, and says when no writing AI is chosen', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: text => String(text), $: () => null, segmented: () => '', settingsSwitch: (attrs, on) => `<switch ${attrs}${on ? ' checked' : ''}>`, state: {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const off = context.calendarCard({ calendars: [] });
  assert.ok(off.includes('<switch data-cal-briefing="on">'));
  assert.ok(!off.includes('data-cal-briefing="at"') && !off.includes('briefing-now'));
  const on = context.calendarCard({ calendars: [], briefing: { on: true, at: '07:30', wants: 'x'.repeat(2001) } });
  assert.ok(on.includes('<switch data-cal-briefing="on" checked>'));
  assert.ok(on.includes('type="time" data-cal-briefing="at" value="07:30"'));
  assert.ok(on.includes('data-reads="2000"') && on.includes('data-read-hint role="status">Felix reads the first 2,000 characters'));
  assert.ok(on.includes('data-action="briefing-now">Make one now'));
  assert.ok(!on.includes('Choose a writing AI'));
  context.state.settings = { writing: { state: 'not_chosen' } };
  const none = context.calendarCard({ calendars: [], briefing: { on: true, at: '08:00', wants: '' } });
  assert.ok(none.includes('Choose a writing AI in <a href="#/settings/writing">Settings → Writing AI</a> first.'));
  assert.ok(none.includes('data-action="briefing-now" disabled'));
});
test('pasting into the calendar link connects it once, and typing never does', () => {
  const listeners = {};
  const nodes = {};
  const sent = [];
  const context = { app: { addEventListener() {} }, document: { addEventListener: (type, fn) => (listeners[type] = [...(listeners[type] || []), fn]) }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: text => String(text), $: sel => nodes[sel] || null, api: async (url, options) => { sent.push(options.body.link); return { calendars: [] }; }, fail: () => sent.push('failed'), state: {}, seq: 1 };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const button = { disabled: false };
  nodes['#calendar-link'] = { id: 'calendar-link', value: 'https://calendar.google.com/calendar/ical/me/private-abc/basic.ics', setAttribute() {} };
  nodes['#calendar-error'] = { innerHTML: '' };
  nodes['[data-action="calendar-add"]'] = button;
  const typed = (inputType) => listeners.input.forEach((fn) => fn({ target: nodes['#calendar-link'], inputType }));
  typed('insertText');
  assert.deepEqual(sent, []);
  typed('insertFromPaste');
  typed('insertFromPaste');
  assert.deepEqual(sent, ['https://calendar.google.com/calendar/ical/me/private-abc/basic.ics']);
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
  assert.match(context.calendarCard({ calendars: [] }), /data-cal-tasks checked/);
  assert.match(context.calendarCard({ calendars: [] }), /Remind me/);
  sent.length = 0;
  await Promise.all(listeners.change.map((fn) => fn({ target: { checked: false, matches: (at) => at === '[data-cal-tasks]' } })));
  await Promise.all(listeners.change.map((fn) => fn({ target: { value: '10', matches: (at) => at === '[data-cal-lead]' } })));
  assert.deepEqual(JSON.parse(JSON.stringify(sent)), [['/api/calendar', { method: 'PATCH', body: { tasks_remind: false } }], ['/api/calendar', { method: 'PATCH', body: { lead: 10 } }]]);
});

test('a missing coding app names the desktop app it comes with, on a Mac only', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: (text) => String(text), choiceCards: () => '', ON_MAC: true, state: { settings: {} } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const card = (provider, installed, missing = !installed) => context.taskCard({ task: 'writing', provider, choices: [], models: [], signin: { installed, missing, text: installed ? 'Uses your plan.' : 'It is not installed on this computer.' } });
  assert.match(card('codex', false), /It is not installed on this computer\. Installing the ChatGPT app also works\.</);
  assert.match(card('claude_code', false), /Opening Code in the Claude app once also works\.</);
  for (const provider of ['codex', 'claude_code']) for (const html of [card(provider, true), card(provider, false, false)]) assert.doesNotMatch(html, /also works/);
  context.ON_MAC = false;
  for (const provider of ['codex', 'claude_code']) assert.doesNotMatch(card(provider, false), /also works/);
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
  assert.doesNotMatch(card, /writing-sign-in/);
  assert.doesNotMatch(card, /Waiting/);
  assert.equal(timers.length, 0);
  Object.assign(context.state.settings.plans[0], { installed: false, signed_in: null });
  assert.ok(context.taskCard(task).includes('href="https://claude.com/claude-code"'));
  context.state.settings.plans[0] = { provider: 'codex', installed: false };
  assert.ok(context.taskCard({ ...task, provider: 'codex' }).includes('<a class="btn plain sm" href="https://github.com/openai/codex" target="_blank" rel="noopener noreferrer">Install Codex</a>'));
});

test('a refused login shows Sign in until Test connection passes', async () => {
  const timers = [], calls = [];
  let refused = false, fail = true, plans = 0;
  const out = { id: 'test-writing', className: '', textContent: '', isConnected: true };
  const page = () => ({ writing: { state: 'ready' }, plans: [{ provider: 'codex', installed: true, signed_in: !refused, refused }] });
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: (text) => String(text), choiceCards: () => '', ON_MAC: true, seq: 1,
    $: () => out, setTimeout: (fn) => timers.push(fn), clearTimeout() {}, appChange: async (change) => { calls.push(change.action); },
    api: async (url) => {
      calls.push(url);
      if (url === '/api/settings') { plans++; return page(); }
      refused = fail;
      if (fail) throw new Error('Please log out and sign in again.');
      return { message: 'codex answered' };
    } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const task = { task: 'writing', provider: 'codex', choices: [], models: [], signin: { installed: true, text: 'Uses your ChatGPT plan.' } };
  context.state = { view: 'settings', section: 'writing', settings: page() };
  let card = context.taskCard(task);
  context.drawSettings = () => { card = context.taskCard(task); Object.assign(out, { className: 'set-result', textContent: '' }); };
  assert.match(card, />Ready</);
  assert.doesNotMatch(card, /Sign in|sign-in/);
  const test = () => context.testAi({ dataset: { task: 'writing' } });
  await test();
  assert.match(card, />Sign in needed</);
  assert.match(card, /Sign in, then press Test connection\.<\/span><button [^>]*data-action="writing-sign-in" data-provider="codex">Sign in</);
  assert.deepEqual([out.className, out.textContent], ['set-result bad', 'Please log out and sign in again.']);
  calls.length = 0;
  await context.writingSignIn({ dataset: { provider: 'codex' } });
  assert.deepEqual([calls, timers.length], [['sign_in'], 0]);
  assert.match(card, />Sign in needed</);
  fail = false;
  await test();
  assert.match(card, />Ready</);
  assert.doesNotMatch(card, /writing-sign-in/);
  assert.deepEqual([out.className, out.textContent, plans], ['set-result ok', 'codex answered', 2]);
  await test();
  assert.equal(plans, 2);
  fail = true;
  const leaving = test();
  context.state = { view: 'note' };
  await leaving;
  assert.match(card, />Ready</);
  context.state = { view: 'settings', section: 'writing', settings: page() };
  context.state.settings.plans[0].signed_in = false;
  context.state.settings.plans[0].refused = false;
  refused = fail = false;
  await test();
  assert.match(card, />Ready</);
  const api = context.api;
  const button = { dataset: { task: 'writing' }, disabled: false };
  let disabledWhileReading = null;
  context.api = async (url) => {
    if (url === '/api/settings') disabledWhileReading = button.disabled;
    return api(url);
  };
  fail = true;
  await context.testAi(button);
  assert.deepEqual([disabledWhileReading, button.disabled], [true, true], 'Test connection stays off until the card is redrawn with a fresh button');
  context.api = async (url) => {
    if (url === '/api/settings/test') out.isConnected = false;
    return api(url);
  };
  const reads = plans;
  card = 'another provider';
  button.disabled = false;
  await context.testAi(button);
  assert.deepEqual([card, plans, button.disabled], ['another provider', reads, false], 'a card switched to another provider meanwhile keeps its own result');
});

test('a sign-in check under way never turns a refused login Ready', async () => {
  const timers = [];
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: (text) => String(text), choiceCards: () => '', seq: 1, Date: { now: () => 0 },
    setTimeout: (fn) => timers.push(fn), clearTimeout() {}, appChange: async () => {},
    api: async () => ({ writing: { state: 'ready' }, candidates: [{ id: 'codex', installed: true, signed_in: true }] }) };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const plan = { provider: 'codex', installed: true, signed_in: null, refused: false };
  const card = () => context.taskCard({ task: 'writing', provider: 'codex', choices: [], models: [] });
  context.state = { view: 'settings', section: 'writing', settings: { writing: { state: 'ready' }, plans: [plan] } };
  context.drawSettings = () => {};
  await context.writingSignIn({ dataset: { provider: 'codex' } });
  assert.match(card(), /Waiting for sign-in/);
  Object.assign(plan, { signed_in: false, refused: true });
  for (const late of [false, true]) {
    vm.runInContext(`signInWait.late = ${late}`, context);
    assert.match(card(), /Sign in needed[\s\S]*Sign in, then press Test connection[\s\S]*data-provider="codex">Sign in</);
  }
  await timers.shift()();
  assert.match(card(), /Sign in needed/);
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

test('the Note width row says what is stored and keeps its Reset reachable', () => {
  const stored = { width: 'wide', page: null };
  const leoTheme = { noteWidth: () => stored.width, pageWidth: () => stored.page, setNoteWidth(width) { stored.width = width; stored.page = null; } };
  const desc = { textContent: '' }, button = { attrs: {}, setAttribute(name, value) { this.attrs[name] = value; } };
  const row = { querySelector: (selector) => selector === '.set-desc' ? desc : button };
  const context = { app: { addEventListener() {} }, document: { addEventListener() {}, querySelector: () => row }, window: { leoTheme }, ICON: new Proxy({}, {get: () => ''}), esc: text => String(text) };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const html = () => context.widthRow();
  assert.match(html(), /^<div class="set-row width-row"[^>]*>.*The default\./);
  assert.match(html(), /aria-label="Reset Note width to default" aria-disabled="true"/);
  for (const [width, page, words] of [['wide', 812, 'Up to 812 px wide.'], ['comfortable', null, 'Comfortable.'], ['full', null, 'Full width.'], ['full', 700, 'Full width.']]) {
    Object.assign(stored, { width, page });
    assert.ok(html().includes(words), `${width} ${page}`);
    assert.ok(!html().includes('aria-disabled'), `${width} ${page} can be reset`);
  }
  assert.ok(!html().includes('drag a mark'));
  Object.assign(stored, { width: 'comfortable', page: null });
  context.resetWidth();
  assert.equal(stored.width, 'wide');
  assert.match(desc.textContent, /^The default\./);
  assert.equal(button.attrs['aria-disabled'], 'true');
});
