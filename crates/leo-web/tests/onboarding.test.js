const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
test('permission steps share one page while existing completion flags stay compatible', () => {
  const context = {};
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  assert.equal(context.nextOnboarding({ onboarding: [false, false, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, false, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, false] }), 2);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true] }), -1);
  assert.equal(context.nextOnboarding({ onboarding: [false, true, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [false, false, true] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [false, true, true] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, false, true] }), 0);
  assert.equal(context.nextOnboarding({}), 0);
});
function setup(candidates, speech, ollamaApi) {
  const el = (extra = {}) => ({ innerHTML: '', textContent: '', disabled: false, ...extra });
  const nodes = { '#onboarding-writing': el(), '#onboarding-writer-status': el(), '#onboarding-use': el({ disabled: true }) };
  const calls = [];
  const settings = fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8');
  const context = {
    ON_MAC: true, seq: 0, ICON: new Proxy({}, { get: () => '' }), desktopApp: null, state: {}, here: 3, calls,
    document: { querySelector: (sel) => sel === `[data-onboarding-step="${context.here}"]` ? {} : null },
    $: (sel) => nodes[sel] || null,
    choiceCards: (options) => `cards:${options.current}:${options.choices.map((c) => c[0]).join(',')}`,
    refreshPanel: (box, html) => { box.innerHTML = html; },
    ollamaSetup: (view, current) => `<ollama-setup ${view?.pull?.state || ''} ${current}>`,
    appChange: async (change) => { calls.push(['app', change.step]); },
    api: async (url, options = {}) => {
      calls.push([url, options.body && JSON.parse(JSON.stringify(options.body))]);
      if (url === '/api/writing?probe=signin') return { writing: { state: 'not_chosen' }, candidates };
      if (url === '/api/speech') return speech(options.body);
      if (url === '/api/writing/ollama') return ollamaApi(options.body);
      return {};
    },
    closeSheet() {}, setTimeout() {}, clearTimeout() {},
  };
  vm.createContext(context);
  vm.runInContext(settings.slice(settings.indexOf('function providerCard('), settings.indexOf('function taskCard(')), context);
  vm.runInContext(settings.slice(settings.indexOf('async function ollamaChange('), settings.indexOf('const PLAN_STALE_MS')), context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  context.maybeOnboarding = () => {};
  return { context, nodes, calls };
}
const settle = async () => { for (let i = 0; i < 10; i++) await new Promise(setImmediate); };
const claude = (signed_in) => ({ id: 'claude_code', installed: true, signed_in });
const codex = (signed_in) => ({ id: 'codex', installed: true, signed_in });
const ollama = (models) => ({ id: 'ollama', installed: true, running: true, models });

test('the writing step comes after the old steps and only for people who have not finished it', () => {
  const { context } = setup([]);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true], writing_done: false }), 3);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, false], writing_done: false }), 2);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true], writing_done: true }), -1);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true] }), -1);
});

test('the writing step preselects a signed-in Claude Code, then Codex, then Ollama with a model', async () => {
  for (const [candidates, chosen, use, ready, status] of [
    [[codex(true), claude(true), ollama(['qwen3:4b'])], 'claude_code', 'Use Claude Code', true, /set-status ok">Ready/],
    [[codex(true), claude(false), ollama(['qwen3:4b'])], 'codex', 'Use Codex', true, /set-status ok">Ready/],
    [[codex(null), claude(false), ollama(['qwen3:4b'])], 'ollama', 'Use Ollama', true, /set-status ok">Ready/],
    [[codex(null), claude(false), ollama([])], 'codex', 'Use Codex', true, /set-status missing">Sign-in unknown/],
    [[codex(false), claude(false), ollama([])], 'claude_code', 'Use Claude Code', false, /Sign in needed/],
  ]) {
    const { context, nodes, calls } = setup(candidates, null, () => ({ installed: true, running: true, pulled: candidates[2].models }));
    await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
    assert.ok(nodes['#onboarding-writing'].innerHTML.startsWith(`cards:${chosen}:claude_code,codex,ollama`));
    assert.equal(nodes['#onboarding-use'].textContent, use);
    assert.equal(nodes['#onboarding-use'].disabled, !ready);
    assert.match(nodes['#onboarding-writer-status'].innerHTML, status);
    assert.equal(calls[0][0], '/api/writing?probe=signin');
  }
});

test('Use saves the writing provider like Settings and finishes the step; Skip only finishes it', async () => {
  const { context, calls } = setup([claude(true)]);
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  await context.onboardingUse();
  assert.deepEqual(calls.slice(1), [['/api/settings', { set: 'provider', task: 'writing', value: 'claude_code' }], ['app', 3]]);
  const skipped = setup([claude(false)]);
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', skipped.context);
  await skipped.context.onboardingUse();
  await skipped.context.finishOnboarding({ dataset: { step: '3' } });
  assert.deepEqual(skipped.calls.slice(1), [['app', 3]]);
});

test('Download and use finishes the speech step once the model is downloaded', async () => {
  let downloaded = false;
  const model = () => ({ id: 'base', name: 'Base', languages: ['en'], size: 1, verified: downloaded, in_use: false });
  const { context, calls } = setup([], (body) => {
    if (body?.action === 'download') return { models: [{ ...model(), progress: { state: 'downloading' } }] };
    downloaded = true;
    return { models: [model()] };
  });
  context.here = 2;
  await vm.runInContext('loadOnboardingModels(onboardingGeneration)', context);
  downloaded = false;
  await context.onboardingModel({ dataset: { verb: 'download' } });
  await settle();
  assert.deepEqual(calls.filter((c) => c[1]?.action).map((c) => c[1].action), ['download', 'use']);
  assert.deepEqual(calls.at(-1), ['app', 2]);
});

test('the dictation hint names the real shortcut the way the page shows keys and stays hidden without dictation', async () => {
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  for (const [mac, shortcut, text] of [
    [true, 'option+space', 'Press ⌥Space anywhere to dictate.'],
    [false, 'ctrl+space', 'Press Ctrl+Space anywhere to dictate.'],
    [true, '⌥Space', 'Press ⌥Space anywhere to dictate.'],
    [true, 'fn', 'Press Fn anywhere to dictate.'],
    [true, null, ''],
  ]) {
    const context = { ON_MAC: mac, api: async () => { if (!shortcut) throw new Error('Dictation is not available.'); return { settings: { shortcut } }; } };
    vm.createContext(context);
    vm.runInContext(core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('function wireHomeAsk(')), context);
    const el = { hidden: true, isConnected: true, textContent: '' };
    await context.dictateHint(el);
    assert.equal(el.textContent, text);
    assert.equal(el.hidden, !shortcut);
  }
});

test('the dictation hint asks once and follows a changed shortcut', async () => {
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  let asked = 0;
  const context = { ON_MAC: true, api: async () => { asked++; return { settings: { shortcut: 'option+space' } }; } };
  vm.createContext(context);
  vm.runInContext(core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('function wireHomeAsk(')), context);
  const el = () => ({ hidden: true, isConnected: true, textContent: '' });
  const first = el(), second = el(), third = el();
  await context.dictateHint(first);
  await context.dictateHint(second);
  assert.equal(asked, 1);
  assert.equal(second.textContent, 'Press ⌥Space anywhere to dictate.');
  context.dictateKeysSeen({ settings: { shortcut: 'ctrl+space' } });
  await context.dictateHint(third);
  assert.equal(asked, 1);
  assert.equal(third.textContent, 'Press ⌃Space anywhere to dictate.');
});

test('with no writing AI on the computer, setup preselects Ollama and says so', async () => {
  const absent = [{ id: 'claude_code', installed: false }, { id: 'codex', installed: false }, { id: 'ollama', installed: false, running: false, models: [] }];
  const found = { textContent: 'leo found these on this computer.' };
  const { context, nodes } = setup(absent, null, () => ({ installed: false, running: false, pulled: [] }));
  nodes['#onboarding-found'] = found;
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  assert.ok(nodes['#onboarding-writing'].innerHTML.startsWith('cards:ollama:'));
  assert.equal(found.textContent, 'None of these is on this computer yet. Ollama runs here and needs no account.');
  const some = setup([claude(false), ...absent.slice(1)]);
  some.nodes['#onboarding-found'] = { textContent: 'leo found these on this computer.' };
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', some.context);
  assert.ok(some.nodes['#onboarding-writing'].innerHTML.startsWith('cards:claude_code:'));
  assert.equal(some.nodes['#onboarding-found'].textContent, 'leo found these on this computer.');
});

test('the denied microphone text says Settings on Windows and System Settings elsewhere', () => {
  for (const [platform, name] of [['windows', 'Settings'], ['macos', 'System Settings']]) {
    const { context } = setup([]);
    context.desktopApp = { platform };
    const row = context.onboardingPermissionRow('microphone', '', 'Microphone', 'why');
    assert.ok(row.includes(`leo is turned off in ${name}. Turn it on there`), row);
  }
});

test('clicking an Ollama card in setup only selects it', async () => {
  const { context, nodes, calls } = setup([ollama([])], null, () => ({ installed: true, running: true, pulled: [] }));
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  await settle();
  const before = calls.length;
  context.ollamaPick({ dataset: { model: 'qwen3:4b' } });
  assert.equal(vm.runInContext('ollamaSelected', context), 'qwen3:4b');
  assert.equal(calls.length, before);
  assert.ok(nodes['#onboarding-writer-status'].innerHTML.includes('<ollama-setup'));
});

function clocked(candidates) {
  const made = setup(candidates), timers = [], { context } = made;
  let clock = 0;
  context.Date = { now: () => clock };
  context.setTimeout = (fn, ms) => { timers.push([fn, ms]); return timers.length; };
  context.appChange = async (change) => { made.calls.push(['app', JSON.parse(JSON.stringify(change))]); };
  const next = async (ms = 3000) => { clock += ms; const [fn] = timers.shift(); await fn(); };
  return { ...made, timers, next, at: (ms) => { clock = ms; } };
}
const probes = (calls) => calls.filter((c) => c[0] === '/api/writing?probe=signin').length;

test('Sign in starts a 3 s sign-in check and the row turns Ready when the probe says signed in', async () => {
  const found = [claude(false), codex(false)];
  const { context, nodes, calls, timers, next } = clocked(found);
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  assert.match(nodes['#onboarding-writer-status'].innerHTML, /data-action="onboarding-sign-in" data-provider="claude_code">Sign in</);
  await context.onboardingSignIn({ dataset: { provider: 'claude_code' } });
  assert.deepEqual(calls.find((c) => c[0] === 'app'), ['app', { action: 'sign_in', provider: 'claude_code' }]);
  assert.match(nodes['#onboarding-writer-status'].innerHTML, /<button class="btn plain sm" disabled>Waiting for sign-in…<\/button>/);
  await next();
  assert.equal(probes(calls), 2);
  assert.equal(timers.length, 1);
  found[0].signed_in = true;
  await next();
  assert.match(nodes['#onboarding-writer-status'].innerHTML, /set-status ok">Ready/);
  assert.equal(nodes['#onboarding-use'].disabled, false);
  assert.equal(timers.length, 0);
});

test('waiting for sign-in gives up after two minutes with Check again, and Check again asks once', async () => {
  const found = [claude(false)];
  const { context, nodes, calls, timers, next, at } = clocked(found);
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  await context.onboardingSignIn({ dataset: { provider: 'claude_code' } });
  for (let i = 0; i < 39; i++) await next();
  assert.ok(timers.every(([, ms]) => ms === 3000));
  assert.match(nodes['#onboarding-writer-status'].innerHTML, /Waiting for sign-in…/);
  await next();
  assert.equal(timers.length, 0);
  assert.equal(probes(calls), 41);
  const status = nodes['#onboarding-writer-status'].innerHTML;
  assert.match(status, /Still waiting\. Finish signing in, then press Check again\./);
  assert.match(status, /data-provider="claude_code" data-check="1">Check again</);
  at(500000);
  await context.onboardingSignIn({ dataset: { provider: 'claude_code', check: '1' } });
  assert.equal(calls.filter((c) => c[0] === 'app').length, 1);
  assert.equal(timers[0][1], 0);
  await next(0);
  assert.equal(probes(calls), 42);
  assert.equal(timers.length, 0);
  assert.match(nodes['#onboarding-writer-status'].innerHTML, /Check again/);
});

test('closing setup stops the sign-in check', async () => {
  const { context, calls, timers, next } = clocked([claude(false)]);
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  await context.onboardingSignIn({ dataset: { provider: 'claude_code' } });
  context.here = 0;
  await next();
  assert.equal(probes(calls), 1);
  assert.equal(timers.length, 0);
});

test('a missing coding app offers its install page', async () => {
  for (const [c, href, name] of [[{ id: 'claude_code', installed: false }, 'https://claude.com/claude-code', 'Install Claude Code'], [{ id: 'codex', installed: false }, 'https://github.com/openai/codex', 'Install Codex']]) {
    const { context, nodes } = setup([c]);
    await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
    const status = nodes['#onboarding-writer-status'].innerHTML;
    assert.ok(status.includes(`<a class="btn plain sm" href="${href}" target="_blank" rel="noopener noreferrer">${name}</a>`), status);
    assert.match(status, /Not installed/);
    const app = { claude_code: 'Opening Code in the Claude app once also works.', codex: 'Installing the ChatGPT app also works.' }[c.id];
    assert.ok(status.includes(`${name.replace('Install ', '')} is not installed on this Mac. ${app}`), status);
    context.ON_MAC = false;
    assert.ok(context.onboardingWriterStatus(c).includes('is not installed on this computer.<'), context.onboardingWriterStatus(c));
  }
});

test('an installed coding app whose sign-in leo cannot tell still lets setup Use it', async () => {
  const { context, nodes, calls } = setup([claude(null)]);
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  assert.match(nodes['#onboarding-writer-status'].innerHTML, /couldn’t tell whether you’re signed in/);
  assert.match(nodes['#onboarding-writer-status'].innerHTML, /data-action="onboarding-sign-in" data-provider="claude_code">Sign in</);
  assert.equal(nodes['#onboarding-use'].disabled, false);
  await context.onboardingUse();
  assert.deepEqual(calls.slice(1), [['/api/settings', { set: 'provider', task: 'writing', value: 'claude_code' }], ['app', 3]]);
});

test('setup shows an Ollama pull and its cancel at once, and Use saves the pulled or picked model', async () => {
  const catalog = [{ name: 'qwen3:4b', pulled: false }, { name: 'gemma3:4b', pulled: true }];
  let view = { installed: true, running: true, pulled: [], models: catalog, pull: null };
  const posted = [];
  const { context, nodes, calls } = setup([ollama([])], null, (body) => {
    if (!body) return view;
    posted.push(body.action);
    return { ...view, pull: body.action === 'pull' ? { model: body.model, state: 'pulling' } : { model: 'qwen3:4b', state: 'cancelled' } };
  });
  const status = () => nodes['#onboarding-writer-status'].innerHTML;
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  await settle();
  assert.match(status(), /No model/);
  await context.ollamaPull({ dataset: { model: 'qwen3:4b' } });
  assert.match(status(), /<ollama-setup pulling >/);
  await context.ollamaPull({ dataset: { model: 'qwen3:4b' } });
  assert.deepEqual(posted, ['pull']);
  await context.ollamaChange({ action: 'cancel' });
  assert.match(status(), /<ollama-setup cancelled >/);
  await context.ollamaPull({ dataset: { model: 'qwen3:4b' } });
  view = { ...view, pulled: ['gemma3:4b', 'qwen3:4b'], models: [{ ...catalog[0], pulled: true }, catalog[1]], pull: { model: 'qwen3:4b', state: 'done' } };
  await vm.runInContext('loadOnboardingOllama(onboardingGeneration)', context);
  assert.match(status(), /set-status ok">Ready.*<ollama-setup done qwen3:4b>/);
  assert.equal(nodes['#onboarding-use'].disabled, false);
  await context.ollamaPull({ dataset: { model: 'gemma3:4b' } });
  assert.match(status(), /<ollama-setup done gemma3:4b>/);
  assert.ok(!calls.some((c) => c[0] === '/api/settings'));
  await context.onboardingUse();
  assert.deepEqual(calls.filter((c) => c[0] === '/api/settings'), [['/api/settings', { set: 'provider', task: 'writing', value: 'ollama', model: 'gemma3:4b' }]]);
  assert.deepEqual(calls.at(-1), ['app', 3]);
});
