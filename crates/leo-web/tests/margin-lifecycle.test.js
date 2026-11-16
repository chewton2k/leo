const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { pageHtml } = require('./page-source');
const kit = require('../src/web/margin.js');
require('../src/web/chat.js');
const FAILED = "Felix could not read your note just now. Try again in a moment.";

function harness() {
  let settle;
  const timers = new Map();
  let next = 0;
  const context = {
    window: { leoMargin: kit }, document: { hidden: false, addEventListener() {}, documentElement: { addEventListener() {} } },
    chat: { isBusy: () => false }, saving: { unsaved: () => false },
    flush: async () => {}, api: () => new Promise((resolve, reject) => { settle = { resolve, reject }; }),
    enc: encodeURIComponent, Locked: class extends Error {}, AbortController,
    setTimeout: (fn, delay) => { const id = ++next; fn.delay = delay; timers.set(id, fn); return id; },
    clearTimeout: (id) => timers.delete(id), cancelAnimationFrame() {}, drawSide() {}, sideRedrawSoon() {}, queueMicrotask,
    $: () => null, svg: (paths) => paths, ICON: { book: '' }, stillMotion: () => false, fail: (e) => { throw e; }, ON_MAC: true, MOTION_MS: 180, POP_FROM: .95,
    noAi: globalThis.leoChat.noAi, esc: (text) => String(text), choiceCards: (options) => JSON.stringify(options), segmented: () => '', settingsSwitch: () => '',
  };
  vm.createContext(context);
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  vm.runInContext(core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('async function dictateHint')), context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/margin-notes.js'), 'utf8'), context);
  const state = vm.runInContext('marginPage', context);
  const session = () => ({ note: { id: 'a', version: 'abc' }, doc: { source: () => 'A changed paragraph long enough to read.' } });
  Object.assign(state, { s: session(), mode: 'along', baseline: [] });
  const showReading = context.marginShowReading;
  context.marginShowReading = () => {};
  context.marginRedraw = () => {};
  return { context, state, session, timers, showReading, settle: () => settle };
}

test('an obsolete read cannot clear a newer note reading state or timer', async () => {
  for (const rejected of [false, true]) {
    const h = harness();
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    h.state.s = h.session();
    h.state.reading = true;
    h.state.faceTimer = h.context.setTimeout(() => {});
    const timer = h.state.faceTimer;
    if (rejected) h.settle().reject(new Error('provider failed'));
    else h.settle().resolve({ note: null });
    await pending;
    assert.equal(h.state.reading, true);
    assert.ok(h.timers.has(timer));
  }
});

test('the explorer redraws when a read along starts and when it ends, so its reading dot follows the read', async () => {
  for (const failed of [false, true]) {
    const h = harness();
    const seen = [];
    h.context.sideRedrawSoon = () => seen.push(Boolean(h.state.reading));
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    assert.deepEqual(seen, [true]);
    if (failed) h.settle().reject(new Error('provider failed'));
    else h.settle().resolve({ note: null });
    await pending;
    await new Promise(setImmediate);
    assert.deepEqual(seen, [true, false]);
  }
  const h = harness();
  const seen = [];
  h.context.sideRedrawSoon = () => seen.push('redraw');
  const pending = h.context.marginRead(h.state.s);
  assert.deepEqual(seen, [], 'the explorer redraws outside the read, so its errors never become a failed read');
  await new Promise(setImmediate);
  h.settle().resolve({ note: null });
  await pending;
  await new Promise(setImmediate);
  assert.deepEqual(seen, ['redraw', 'redraw']);
});

test('read completion distinguishes silence from failure without losing failed blocks', async () => {
  for (const failed of [false, true]) {
    const h = harness();
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    h.settle().resolve({ note: null, failed });
    await pending;
    assert.equal(h.state.reading, false);
    assert.equal(h.state.baseline.length, failed ? 0 : 1);
    assert.equal(h.state.status, failed ? FAILED : 'Felix read it; nothing to add.');
  }
});

test('a read that leaves a margin note says nothing about having read', async () => {
  const h = harness();
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  h.settle().resolve({ note: { id: 't1', messages: [] } });
  await pending;
  assert.equal(h.state.status, '');
});

test('a read refused because the margin is full keeps the baseline and says why', async () => {
  const line = "Felix's margin is full. Resolve a note and he'll read again.";
  const h = harness();
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  h.settle().resolve({ note: null, failed: true, error: line });
  await pending;
  assert.equal(h.state.baseline.length, 0);
  assert.equal(h.state.status, line);
});

test('a read that frees notes removes them and tells the server which thread is open', async () => {
  const h = harness();
  const bodies = [];
  const real = h.context.api;
  h.context.api = (path, init) => { bodies.push(init.body); return real(path, init); };
  h.context.toast = () => { throw new Error('a tidied note gets no toast'); };
  Object.assign(h.state, { threads: [{ id: 'old', quote: 'a' }, { id: 'open', quote: 'b' }], open: 'open' });
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  h.settle().resolve({ note: { id: 't1', messages: [] }, resolved: ['old'] });
  await pending;
  assert.equal(bodies[0].keep, 'open');
  assert.deepEqual(h.state.threads.map((t) => t.id), ['open', 't1']);
  assert.deepEqual([...h.state.resolved].map((r) => r.id), ['old']);
  assert.equal(h.state.status, '');
});

test('a read never drops the open note or one with typed text, and tells the server they stay open', async () => {
  const h = harness();
  const calls = [];
  h.context.api = (path, init) => new Promise((resolve) => { calls.push({ path, body: init.body, resolve }); });
  h.context.$ = (sel, root) => (sel === 'textarea' && root ? root.area : null);
  h.context.toast = () => { throw new Error('a tidied note gets no toast'); };
  Object.assign(h.state, {
    gone: ['lost'],
    threads: [{ id: 'old', quote: 'a' }, { id: 'open', quote: 'b' }, { id: 'typed', quote: 'c' }, { id: 'empty', quote: 'd' }],
    open: 'open',
  });
  h.state.els.set('typed', { remove() {}, area: { value: 'half a thought' } });
  h.state.els.set('empty', { remove() {}, area: { value: '  ' } });
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  calls.shift().resolve({ note: { id: 't1', messages: [] }, resolved: ['old', 'open', 'typed', 'empty'] });
  for (const id of ['open', 'typed']) {
    await new Promise(setImmediate);
    const back = calls.shift();
    assert.ok(back.path.endsWith(`/threads/${id}`));
    assert.equal(back.body.status, 'open');
    assert.deepEqual([...back.body.gone], ['lost']);
    back.resolve({ id, status: 'open', quote: 'x', messages: [] });
  }
  await pending;
  assert.equal(calls.length, 0);
  assert.deepEqual(h.state.threads.map((t) => t.id), ['open', 'typed', 't1']);
  assert.deepEqual([...h.state.resolved].map((r) => r.id).sort(), ['empty', 'old']);
});

test('a read that saved nothing still removes the notes it resolved', async () => {
  const h = harness();
  h.context.toast = () => {};
  Object.assign(h.state, { threads: [{ id: 'old', quote: 'a' }] });
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  h.settle().resolve({ note: null, resolved: ['old'] });
  await pending;
  assert.deepEqual(h.state.threads, []);
  assert.deepEqual([...h.state.resolved].map((r) => r.id), ['old']);
});

test('a double click or held Enter on a resolved passage reopens it once', async () => {
  const h = harness();
  const calls = [];
  h.context.api = (path, init) => new Promise((resolve) => { calls.push(resolve); });
  Object.assign(h.state, { resolved: [{ id: 'r1', quote: 'q' }] });
  const first = h.context.marginReopen('r1');
  const second = h.context.marginReopen('r1');
  await second;
  assert.equal(calls.length, 1);
  calls[0]({ id: 'r1', status: 'open', quote: 'q', messages: [] });
  await first;
  assert.deepEqual([...h.state.threads].map((t) => t.id), ['r1']);
  const again = h.context.marginReopen('r1');
  calls[1]({ id: 'r1', status: 'open', quote: 'q', messages: [] });
  await again;
  assert.deepEqual([...h.state.threads].map((t) => t.id), ['r1']);
  assert.equal(h.state.reopening.size, 0);
});

test('reopening a resolved passage sends the notes the page cannot place and opens the returned thread', async () => {
  const h = harness();
  const bodies = [];
  const real = h.context.api;
  h.context.api = (path, init) => { bodies.push(init.body); return real(path, init); };
  h.context.toast = () => { throw new Error('reopening gets no toast'); };
  Object.assign(h.state, { resolved: [{ id: 'r1', quote: 'q' }, { id: 'r2', quote: 'p' }], gone: ['lost'] });
  const pending = h.context.marginReopen('r1');
  h.settle().resolve({ id: 'r1', status: 'open', quote: 'q', messages: [] });
  await pending;
  assert.equal(bodies[0].status, 'open');
  assert.equal(bodies[0].gone[0], 'lost');
  assert.deepEqual([...h.state.resolved].map((r) => r.id), ['r2']);
  assert.deepEqual([...h.state.threads].map((t) => t.id), ['r1']);
  assert.equal(h.state.open, 'r1');
});

test('mode updates retain confirmed state after failure and ignore departed notes', async () => {
  for (const departed of [false, true]) {
    const h = harness();
    h.state.mode = 'ask';
    h.context.marginControl = () => {};
    h.context.marginModeMenu = () => {};
    const pending = h.context.marginChangeMode('along');
    if (departed) {
      h.context.marginLeave();
      h.state.s = h.session();
      h.state.changing = true;
      h.settle().resolve({ mode: 'along' });
    } else h.settle().reject(new Error('save failed'));
    await pending;
    assert.equal(h.state.mode, 'ask');
    assert.equal(h.state.changing, departed);
    if (!departed) assert.equal(h.state.status, 'Could not save this mode. Try again.');
  }
});

test('refused reads keep pending changes and show a safe pause reason', async () => {
  for (const [why, status] of [['daily', 'Paused for today: daily limit reached'], ['felix', 'Paused while Felix answers'], ['unknown', FAILED]]) {
    const h = harness();
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    const error = new Error('private provider detail');
    error.data = { why };
    h.settle().reject(error);
    await pending;
    assert.equal(h.state.baseline.length, 0);
    assert.equal(h.state.status, status);
  }
});

test('Undo after Dismiss toasts the server reason when it fails', async () => {
  const h = harness();
  const toasts = [];
  h.context.toast = (text, options) => toasts.push([text, options]);
  const pending = h.context.marginStatus({ id: 't1', quote: 'q' }, 'dismissed');
  h.settle().resolve({});
  await pending;
  assert.equal(toasts[0][0], 'Dismissed Felix’s note');
  const undo = toasts[0][1].run();
  h.settle().reject(new Error('Felix’s margin is full. Resolve a note first.'));
  await undo;
  assert.equal(toasts[1][0], 'Felix’s margin is full. Resolve a note first.');
  assert.equal(toasts[1][1].bad, true);
});

test('an ask over the length limit says it goes to the chat', () => {
  const h = harness();
  h.context.chat.isOpen = () => false;
  h.state.layer = {};
  assert.equal(h.context.marginAsk(h.state.s, { text: 'x'.repeat(1001), from: 1 }), false);
  assert.equal(h.state.status, 'Long selections go to the chat');
  h.state.status = '';
  assert.equal(h.context.marginAsk(h.state.s, { text: 'x', from: 1 }), false);
  assert.equal(h.state.status, '');
});

test('a stale 409 without a reason says the note changed and a retry follows the next edit', async () => {
  const h = harness();
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  const stale = new Error('leo answered 409.');
  stale.status = 409;
  h.settle().reject(stale);
  await pending;
  assert.equal(h.state.baseline.length, 0);
  assert.match(h.state.status, /note changed/i);
  assert.match(h.state.status, /next edit/i);
  assert.doesNotMatch(h.state.status, /Writing AI/);
  h.state.loaded = true;
  h.context.marginEdited(h.state.s);
  assert.equal(h.timers.get(h.state.timer).delay, 4000);
});

test('a refusal with a reason is never mistaken for a stale version', async () => {
  for (const [status, why, expected] of [[409, 'daily', 'Paused for today: daily limit reached'], [429, 'soon', 'Felix reads again in a moment'], [403, 'unedited', 'Reads changes after you edit']]) {
    const h = harness();
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    const refused = new Error('refused');
    refused.status = status;
    refused.data = { why };
    h.settle().reject(refused);
    await pending;
    assert.equal(h.state.status, expected);
  }
});

function placeholder(h) {
  const el = { removed: false, leaving: false, classList: { add: (name) => { el.leaving = name === 'leaving'; } }, remove() { el.removed = true; } };
  h.state.readingAt = 0;
  h.state.readingEl = el;
  return el;
}

async function finish(h, answer) {
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  const el = placeholder(h);
  if (answer instanceof Error) h.settle().reject(answer);
  else h.settle().resolve(answer);
  await pending;
  return el;
}

test('the dots wait 200 ms, then mark the last changed block, and only while that read is current', () => {
  for (const current of [true, false]) {
    const h = harness();
    const source = 'First block here.\n\nThe changed paragraph long enough.';
    h.state.reading = true;
    h.showReading(h.state.s, source, ['The changed paragraph long enough.']);
    const timer = h.timers.get(h.state.faceTimer);
    assert.equal(timer.delay, 200);
    assert.equal(h.state.readingAt, null);
    if (!current) h.state.reading = false;
    timer();
    assert.equal(h.state.readingAt, current ? source.indexOf('The changed') : null);
  }
});

test('silence fades the reading dots and says Felix read it, for a moment', async () => {
  const h = harness();
  const el = await finish(h, { note: null });
  assert.equal(h.state.status, 'Felix read it; nothing to add.');
  assert.equal(h.state.brief, true);
  assert.equal(h.state.readingAt, null);
  assert.equal(h.state.readingEl, null);
  assert.equal(el.leaving, true);
  assert.equal(el.removed, false);
  const fade = [...h.timers.values()].find((fn) => fn.delay === 120);
  fade();
  assert.equal(el.removed, true);
});

test('a failed or refused read drops the dots at once and keeps its status', async () => {
  for (const answer of [{ note: null, failed: true }, Object.assign(new Error('refused'), { data: { why: 'daily' } })]) {
    const h = harness();
    const el = await finish(h, answer);
    assert.equal(el.removed, true);
    assert.equal(el.leaving, false);
    assert.equal(h.state.readingEl, null);
    assert.notEqual(h.state.status, '');
  }
});

test('a comment replaces the dots without a fade', async () => {
  const h = harness();
  const el = await finish(h, { note: { id: 'n1', quote: 'changed paragraph', messages: [{ from: 'felix', text: 'Hi' }] } });
  assert.equal(el.removed, true);
  assert.equal(el.leaving, false);
  assert.equal(h.state.threads.length, 1);
  assert.ok(h.state.arriving.has('n1'));
});

test('leaving the note forgets the reading dots and remembered sides', () => {
  const h = harness();
  placeholder(h);
  h.state.sides = { a: 'left' };
  h.context.marginLeave();
  assert.equal(h.state.readingAt, null);
  assert.equal(h.state.readingEl, null);
  assert.equal(Object.keys(h.state.sides).length, 0);
});

test('a brief status clears after its bounded timer, but never a newer status, another note or a departed page', () => {
  for (const change of ['none', 'newer', 'other note', 'left']) {
    const h = harness();
    h.context.marginSetStatus('Nothing to add this time', true);
    const timer = h.timers.get(h.state.statusTimer);
    if (change === 'newer') h.context.marginSetStatus('Paused for today');
    else if (change === 'other note') h.state.s = h.session();
    else if (change === 'left') h.context.marginLeave();
    const status = change === 'none' ? '' : h.state.status;
    timer();
    assert.equal(h.state.status, status, change);
  }
});

test('a new edit clears a brief status but not a failure', () => {
  const h = harness();
  h.state.loaded = true;
  h.context.marginSetStatus('Nothing to add this time', true);
  h.context.marginEdited(h.state.s);
  assert.equal(h.state.status, '');
});

function replyHarness() {
  const h = harness();
  const box = { textContent: '', hidden: true };
  const send = { disabled: false };
  const el = { isConnected: true, dataset: { id: 't1' } };
  const area = { value: '', style: {}, scrollHeight: 20, form: { querySelector: () => send }, closest: () => el, focus() {} };
  let count = 0;
  Object.assign(h.context, { $: (sel) => (sel === 'textarea' ? area : sel === '.margin-problem' ? box : null), marginDraw() {}, requestId: () => `request-${++count}` });
  h.state.threads = [{ id: 't1', messages: [] }];
  h.state.draft = null;
  const submit = async (text) => {
    area.value = text;
    await h.context.marginSubmit({ preventDefault() {}, target: { closest: () => el } });
  };
  return { h, box, send, area, submit };
}

test('a failed reply puts the text back, shows the error and keeps Send enabled, and a retry keeps its request', async () => {
  const { h, box, send, area, submit } = replyHarness();
  const bodies = [];
  h.context.api = async (url, options) => { bodies.push(options.body); throw new Error('Felix could not answer.'); };
  await submit('Why?');
  assert.equal(area.value, 'Why?');
  assert.equal(box.textContent, 'Felix could not answer.');
  assert.equal(box.hidden, false);
  assert.equal(send.disabled, false);
  assert.equal(h.state.thinking.size, 0);
  await submit('Why?');
  await submit('Why not?');
  assert.deepEqual(bodies.map((body) => body.request), ['request-1', 'request-1', 'request-2']);
});

test('a reply refused as busy keeps the text and says Felix is still answering', async () => {
  const { h, box, area, submit } = replyHarness();
  h.context.api = async () => { throw Object.assign(new Error('Felix is already answering this.'), { status: 409, data: { why: 'busy' } }); };
  await submit('Why?');
  assert.equal(area.value, 'Why?');
  assert.equal(box.textContent, 'Felix is still answering this.');
});

test('an edit waits the plain pace, then the other half only when the changed block is unfinished', () => {
  for (const [text, baseline, more] of [
    ['A changed paragraph long enough to read.', [], false],
    ['A changed paragraph still being written', [], true],
    ['A changed paragraph long enough to read.\n\nAn older tail already read and unfinished', ['An older tail already read and unfinished'], false],
  ]) {
    const h = harness();
    h.state.loaded = true;
    h.state.baseline = baseline;
    h.state.s.doc.source = () => text;
    h.context.marginEdited(h.state.s);
    const armed = h.state.timer;
    assert.equal(h.timers.get(armed).delay, 4000);
    h.timers.get(armed)();
    if (more) {
      assert.notEqual(h.state.timer, armed);
      assert.equal(h.timers.get(h.state.timer).delay, 2000);
      assert.ok(!h.state.reading);
      h.timers.get(h.state.timer)();
    }
    assert.ok(h.state.reading);
  }
});

test('coming back to the window arms the plain pace and checks the changed block when it fires', () => {
  const h = harness();
  h.state.s.doc.source = () => 'A changed paragraph still being written';
  h.context.marginShown();
  const armed = h.state.timer;
  assert.equal(h.timers.get(armed).delay, 4000);
  h.timers.get(armed)();
  assert.equal(h.timers.get(h.state.timer).delay, 2000);
});

test('a new edit leaves a failure status alone', () => {
  const h = harness();
  h.state.loaded = true;
  h.context.marginSetStatus(FAILED);
  h.context.marginEdited(h.state.s);
  assert.equal(h.state.status, FAILED);
});

test('When you ask says how to ask, with the real shortcut and no promise of automatic reading', () => {
  for (const [mac, keys] of [[true, '⌘⇧E'], [false, 'Ctrl+Shift+E']]) {
    const h = harness();
    h.context.ON_MAC = mac;
    const html = h.context.felixSettingsCards({ gap: 90, gaps: [10, 15, 30, 90, 180], cap: 40, caps: [20, 40, 80, 0], today: 0, default_mode: 'ask', layout: 'right', pace: 'regular' }, { tasks: [] });
    assert.ok(html.includes(`Select text, then Ask Felix or ${keys}.`));
    assert.ok(html.includes('Reads along sends the note’s title and the text you change to your writing AI.'));
    assert.ok(!/automatic/i.test(html));
  }
});

test('Reading pace is a row of picture cards that carry the pause and save to the pace field', () => {
  const html = harness().context.felixSettingsCards({ gap: 90, gaps: [90], cap: 40, caps: [40], today: 0, default_mode: 'ask', layout: 'right', pace: 'slow' }, { tasks: [] });
  const cards = JSON.parse(html.match(/<span class="set-label">Reading pace<\/span>(\{.*?\})<\/div>/)[1]);
  assert.equal(cards.field, 'data-margin-pref="pace"');
  assert.equal(cards.current, 'slow');
  assert.deepEqual(cards.choices.map(([value, title, about]) => [value, title, about]), [['fast', 'Fast', 'After 2 sec of quiet, longer mid-sentence.'], ['regular', 'Regular', 'After 4 sec of quiet, longer mid-sentence.'], ['slow', 'Slow', 'After 8 sec of quiet, longer mid-sentence.']]);
  assert.ok(cards.choices.every((choice) => choice[3].includes('<svg')));
});

test('the page keeps only the asking affordances it really has', () => {
  const passages = fs.readFileSync(path.join(__dirname, '../src/web/app/passages.js'), 'utf8');
  for (const label of ['<span>Felix this</span>', '>Ask Felix</button>', '>Visualize</button>']) assert.ok(passages.includes(label), label);
  assert.match(passages, /pressed\(e, 'ask'\)/);
});

test('the mascot and the reading stub are gone from the margin script and styles', () => {
  const script = fs.readFileSync(path.join(__dirname, '../src/web/app/margin-notes.js'), 'utf8');
  const page = pageHtml();
  assert.match(page, /\.settings-layout[\s\S]*<\/html>/);
  for (const dead of ['felix.felix', 'margin-head', 'margin-face', 'readFace']) assert.equal(script.includes(dead), false, dead);
  for (const dead of ['.margin-reading', '.margin-head', 'margin-reveal', '@keyframes margin-draw', '.chat-history-new', '.settings-layout::before']) assert.equal(page.includes(dead), false, dead);
});

test('leaving with a queued hover lets the next note schedule and paint its own hover', () => {
  const h = harness();
  const frames = new Map();
  let id = 0;
  h.context.requestAnimationFrame = fn => { frames.set(++id, fn); return id; };
  h.context.cancelAnimationFrame = id => frames.delete(id);
  h.context.marginPaint = () => {};
  const layer = () => ({ remove() {}, parentElement: { getBoundingClientRect: () => ({ left: 0, top: 0 }), style: { removeProperty() {} } } });
  const move = key => h.context.marginHover({ clientX: 2, clientY: 2, pointerType: 'mouse', target: { closest: () => ({ dataset: { id: key } }) } });
  h.state.layer = layer();
  move('old');
  assert.equal(frames.size, 1);
  h.context.marginLeave();
  assert.equal(frames.size, 0);
  h.state.layer = layer();
  move('new');
  assert.equal(frames.size, 1);
  frames.get(h.state.pointer)();
  assert.equal(h.state.hover, 'new');
  assert.equal(h.state.pointer, 0);
});

 test('preference failure keeps the remembered layout or defaults to both', async () => {
  for (const saved of [null, 'right', 'marks', 'both']) {
    const h = harness();
    h.context.localStorage = { getItem: () => saved };
    h.context.api = async () => { throw new Error('offline'); };
    assert.equal((await h.context.marginLoadPrefs()).layout, saved || 'both');
  }
});

test('switching to When you ask during the save sends nothing, and a late answer leaves no status', async () => {
  const h = harness();
  let sent = 0;
  h.context.api = () => { sent++; return new Promise(() => {}); };
  h.context.flush = async () => { h.state.mode = 'ask'; };
  await h.context.marginRead(h.state.s);
  assert.equal(sent, 0);

  const late = harness();
  const pending = late.context.marginRead(late.state.s);
  await new Promise(setImmediate);
  late.state.mode = 'ask';
  late.settle().resolve({ note: null });
  await pending;
  assert.equal(late.state.status, '');
});

test('a mode picked before the first save is kept, then sent once the note exists', async () => {
  const h = harness();
  const calls = [];
  h.context.api = async (url, options = {}) => {
    calls.push([options.method || 'GET', url, options.body]);
    if (url === '/api/margin') return { layout: 'both', pace: 'regular', default_mode: 'ask' };
    return options.method === 'POST' ? { mode: options.body.mode } : { mode: 'ask', threads: [] };
  };
  h.context.marginControl = () => {};
  h.context.marginModeMenu = () => {};
  h.state.s.note = {};
  h.state.mode = 'ask';
  await h.context.marginChangeMode('along');
  assert.equal(h.state.mode, 'along');
  assert.deepEqual(calls, []);
  await h.context.marginCreated(h.state.s, { id: 'n1' });
  assert.equal(JSON.stringify(calls.at(-1)), JSON.stringify(['POST', '/api/notes/n1/margin', { mode: 'along' }]));
  assert.equal(h.state.mode, 'along');
  assert.equal(h.state.loaded, true);
});

function through(mode = 'ask') {
  const h = harness();
  const source = Array.from({ length: 14 }, (_, i) => `Paragraph number ${i + 1} has enough words to read.`).join('\n\n');
  const doc = { source: () => source, glow() {} };
  Object.assign(h.state, { mode, s: { note: { id: 'a', version: 'v' }, doc } });
  const toasts = [];
  const calls = [];
  const waiting = [];
  Object.assign(h.context, {
    toast: (text, options) => toasts.push([text, options]), stillMotion: () => false, ICON: {}, iconSwap: (host, change) => change(),
    api: (url, options = {}) => { calls.push([url, options.body]); return new Promise((resolve, reject) => { waiting.push({ resolve, reject }); options.signal?.addEventListener('abort', () => reject(new Error('aborted'))); }); },
  });
  const thread = (id, n) => ({ id, quote: `Paragraph number ${n} has`, prefix: '', suffix: '', messages: [] });
  const answer = async (value) => { await new Promise(setImmediate); waiting.shift().resolve(value); await new Promise(setImmediate); };
  return { ...h, source, toasts, calls, thread, answer, waiting };
}

test('a read-through whose note is left draws nothing', async () => {
  const h = through();
  const run = h.context.marginThroughClick();
  await h.answer({ notes: [h.thread('t1', 1)] });
  h.state.s = h.session();
  await h.answer({ notes: [h.thread('t2', 5)] });
  await run;
  assert.equal(h.state.threads.length, 0);
  assert.equal(h.toasts.length, 0);
  assert.equal(h.state.through, null);
});

test('Stop aborts the in-flight part at once and keeps and reveals what earlier parts saved', async () => {
  const h = through();
  const run = h.context.marginThroughClick();
  await h.answer({ notes: [h.thread('t1', 1)] });
  await new Promise(setImmediate);
  h.state.through.started = 0;
  await h.context.marginThroughClick();
  await run;
  assert.equal(h.calls.length, 2);
  assert.equal(JSON.stringify(h.state.threads.map((t) => t.id)), JSON.stringify(['t1']));
  assert.equal(h.toasts[0][0], 'Stopped. Felix added 1 margin note');
  assert.equal(h.state.through, null);
});

test('Stop during the only part ends the run with nothing added', async () => {
  const h = through();
  const run = h.context.marginThroughClick();
  await new Promise(setImmediate);
  h.state.through.started = 0;
  await h.context.marginThroughClick();
  await run;
  assert.equal(h.calls.length, 1);
  assert.equal(h.state.threads.length, 0);
  assert.equal(h.toasts.length, 0);
  assert.equal(h.state.through, null);
});

test('threads are revealed top to bottom after the run, not per part', async () => {
  const h = through();
  const run = h.context.marginThroughClick();
  await h.answer({ notes: [h.thread('late', 12)] });
  await h.answer({ notes: [h.thread('mid', 7)] });
  assert.equal(h.state.threads.length, 0);
  await h.answer({ notes: [h.thread('early', 2)] });
  await run;
  assert.equal(JSON.stringify(h.state.threads.map((t) => t.id)), JSON.stringify(['early']));
  assert.equal(JSON.stringify([...h.timers.values()].map((fn) => fn.delay)), '[80,160]');
  for (const fn of [...h.timers.values()]) fn();
  assert.equal(JSON.stringify(h.state.threads.map((t) => t.id)), JSON.stringify(['early', 'mid', 'late']));
  assert.equal(h.toasts[0][0], 'Felix added 3 margin notes');
});

test('marginRead waits while a read-through is going', async () => {
  const h = through('along');
  h.state.through = {};
  await h.context.marginRead(h.state.s);
  assert.equal(h.calls.length, 0);
  assert.equal(h.state.again, true);
});

test('a read-through in When you ask mode never changes the mode', async () => {
  const h = through('ask');
  const run = h.context.marginThroughClick();
  for (let i = 0; i < 3; i++) await h.answer({ notes: [] });
  await run;
  assert.equal(h.state.mode, 'ask');
  assert.equal(h.state.status, 'Nothing to add this time');
  assert.equal(h.timers.size, 1);
});

function passages(room = true) {
  const fake = () => {
    const on = {};
    const node = {
      hidden: false, dataset: {}, style: {}, value: '', offsetWidth: 200, offsetHeight: 30, focused: false,
      setAttribute() {}, addEventListener: (kind, fn) => { on[kind] = fn; }, classList: { toggle() {} },
      fire: (kind, extra = {}) => { const e = { stopped: false, preventDefault() {}, stopPropagation() { e.stopped = true; }, ...extra }; on[kind]?.(e); return e; },
      focus() { node.focused = true; },
      querySelector: () => node.child || (node.child = fake()),
    };
    return node;
  };
  const picked = { text: 'offices were built for supervision', from: 1, to: 1 };
  const s = { note: { id: 'n' }, doc: { selection: () => picked, selectionBox: () => ({ x: 100, y: 100, top: 80 }) }, title: { textContent: 'Essay' } };
  const asked = [];
  const opened = [];
  const attached = [];
  const context = {
    document: { createElement: fake, body: { appendChild() {} }, addEventListener() {} },
    window: { addEventListener() {}, innerHeight: 800 }, ICON: { chat: '' }, ON_MAC: true, state: { view: 'note', session: s },
    $: () => ({ getBoundingClientRect: () => ({ left: 0, right: 600 }) }), toast() {}, cleanTitle: (t) => t,
    marginAsk: (...args) => (room ? asked.push(args) > 0 : false),
    marginAskFirst: (...args) => opened.push(args) > 0 && room,
    chat: { attachSelection: (x) => attached.push(x) },
  };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/passages.js'), 'utf8'), context);
  const pill = vm.runInContext('askFloat', context);
  const press = (ask) => pill.fire('click', { target: { closest: () => ({ dataset: { ask } }) } });
  return { pill, asked, opened, attached, s, picked, press };
}

test('Felix this and Visualize ask about the selection at once with their own message, and go to the chat without room', () => {
  for (const [kind, question] of [['this', 'Respond to this passage.'], ['draw', 'Draw this.']]) {
    const p = passages();
    p.press(kind);
    assert.equal(p.pill.hidden, true);
    assert.deepEqual([...p.asked[0]], [p.s, p.picked, question]);
    const none = passages(false);
    none.press(kind);
    assert.equal(none.attached[0].text, none.picked.text);
  }
});

test('Ask Felix opens a question box for the passage in the margin and asks nothing yet', () => {
  const p = passages();
  p.press('ask');
  assert.equal(p.pill.hidden, true);
  assert.deepEqual([...p.opened[0]], [p.s, p.picked]);
  assert.equal(p.asked.length, 0);
  assert.equal(p.attached.length, 0);
});

test('without room in the margin Ask Felix goes straight to the chat', () => {
  const p = passages(false);
  p.press('ask');
  assert.equal(p.opened.length, 1);
  assert.equal(p.attached.length, 1);
  assert.equal(p.attached[0].text, p.picked.text);
});

test('the persona chip names Felix once and the mode chip follows it without a prefix', () => {
  const h = harness();
  const fake = () => ({ dataset: { ready: '1' }, textContent: '', title: '', innerHTML: '', setAttribute(k, v) { this[k] = v; }, querySelectorAll: () => [] });
  const els = Object.fromEntries(['mode', 'mode-menu', 'mode-label', 'persona', 'persona-menu', 'persona-icon', 'persona-label'].map((id) => [`#margin-${id}`, fake()]));
  els['#margin-mode-icon'] = { dataset: {}, innerHTML: '' };
  h.context.$ = (sel) => els[sel] || null;
  h.context.marginThroughControl = () => {};
  h.context.iconSwap = (icon, paint) => paint();
  vm.runInContext("marginPrefs = { personas: [{ id: 'reader', name: 'Reader', icon: 'book', details: 'Plain.' }, { id: 'critic', name: 'Critic', icon: 'target', details: 'Hard.' }] }", h.context);
  for (const [mode, persona, label, name, tip] of [['along', 'critic', 'Reads along', 'Felix as Critic', /^Felix reads changes/], ['ask', 'reader', 'When you ask', 'Felix as Reader', /^Select a passage/]]) {
    Object.assign(h.state, { mode, persona });
    h.context.marginControl();
    assert.equal(els['#margin-mode-label'].textContent, label);
    assert.equal(els['#margin-persona-label'].textContent, name);
    assert.match(els['#margin-mode'].title, tip);
  }
  assert.match(els['#margin-persona-menu'].innerHTML, /Instructions for this note…/);
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  assert.ok(core.indexOf('id="margin-persona"') < core.indexOf('id="margin-mode"'));
  assert.ok(core.indexOf('<div class="note-meta">') < core.indexOf('<h1 id="title"'));
  assert.ok(core.indexOf('<h1 id="title"') < core.indexOf('<div class="note-originals">'));
  assert.match(core, /<span id="margin-persona-label">Felix as Reader<\/span>/);
  assert.match(core, /<span id="margin-mode-label">When you ask<\/span>/);
});

test('note instructions show the saved text, save once on Enter and say so', async () => {
  const h = harness();
  const on = {};
  const input = { value: '', addEventListener: (kind, fn) => { on[kind] = fn; }, focus() { h.context.document.activeElement = input; }, blur() { h.context.document.activeElement = null; on.blur(); } };
  const box = { hidden: true, dataset: {} };
  h.context.$ = (sel) => (sel === '#margin-instructions' ? box : sel === 'input' ? input : null);
  const sent = [];
  h.context.api = async (url, options) => { sent.push(options.body); return { instructions: options.body.instructions }; };
  h.state.instructions = 'Check sources.';
  h.context.marginInstructions();
  assert.equal(box.hidden, false);
  assert.equal(input.value, 'Check sources.');
  h.context.marginInstructions(true);
  input.value = '  Keep it short.  ';
  on.keydown({ key: 'Enter', preventDefault() {}, stopPropagation() {} });
  await new Promise(setImmediate);
  assert.equal(JSON.stringify(sent), '[{"instructions":"Keep it short."}]');
  assert.equal(h.state.instructions, 'Keep it short.');
  assert.equal(h.state.status, 'Instructions saved.');
  input.value = '';
  on.blur();
  await new Promise(setImmediate);
  assert.equal(box.hidden, true);
  assert.equal(h.state.status, 'Instructions removed.');
});

test('a writer failure shows the server sentence and a Set up writing AI link in the status line', async () => {
  const sentence = "leo can't write yet: no writing AI is ready (Ollama isn't running).";
  const h = harness();
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  h.settle().reject(Object.assign(new Error(sentence), { status: 503, data: { why: 'no-ai' } }));
  await pending;
  assert.equal(h.state.status, sentence);
  const status = { textContent: '', html: '', insertAdjacentHTML(where, html) { this.html = html; } };
  h.context.$ = (sel) => (sel === '#margin-status' ? status : null);
  h.context.marginSetStatus(sentence);
  assert.equal(status.textContent, sentence);
  assert.match(status.html, /<a href="#\/settings\/writing">Set up writing AI<\/a>/);
  status.html = '';
  h.context.marginSetStatus('Felix reads again in a moment');
  assert.equal(status.html, '');
});

test('read through shows the server sentence for a missing writing AI and passes other errors through unchanged', async () => {
  const sentence = "leo can't write yet: no writing AI is chosen.";
  for (const [message, say] of [[sentence, sentence], ['private provider detail', FAILED]]) {
    const h = harness();
    h.context.marginThroughControl = () => {};
    h.context.marginSetStatus = (text) => { h.state.status = text; };
    const run = { started: Date.now(), stop: false, added: [], abort: new AbortController() };
    const part = { blocks: ['A changed paragraph long enough to read.'], from: 0, to: 1 };
    h.state.s.doc.glow = () => {};
    h.state.s.doc.source = () => 'A changed paragraph long enough to read.';
    const pending = h.context.marginThroughPart(h.state.s, run, part, 'Reading through…');
    await new Promise(setImmediate);
    h.settle().reject(Object.assign(new Error(message), { status: 503, data: { why: 'no-ai' } }));
    assert.equal((await pending).say, say);
  }
});

test('pin and resolve change the margin at once and come back when the save fails', async () => {
  const h = harness();
  const t = { id: 't1', quote: 'q', pinned: false };
  h.state.threads = [t];
  const pin = h.context.marginPin(t);
  assert.equal(h.context.marginThread('t1').pinned, true);
  h.settle().reject(new Error('save failed'));
  await assert.rejects(pin, /save failed/);
  assert.equal(h.context.marginThread('t1').pinned, false);
  const resolve = h.context.marginStatus(t, 'resolved');
  assert.deepEqual([...h.state.threads.map((x) => x.id)], []);
  assert.deepEqual([...h.state.resolved.map((x) => x.id)], ['t1']);
  h.settle().reject(new Error('save failed'));
  await assert.rejects(resolve, /save failed/);
  assert.deepEqual([...h.state.threads.map((x) => x.id)], ['t1']);
  assert.equal(h.state.resolved.length, 0);
});
