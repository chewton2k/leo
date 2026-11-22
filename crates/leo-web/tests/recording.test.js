'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/recording.js');

const L = globalThis.leoRecording;

test('background recording levels stay bounded without a mounted waveform', () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/recording.js'), 'utf8');
  const start = source.indexOf('    function feedFromServer(view)');
  const end = source.indexOf('    function advance(now)', start);
  const state = { queue: [], lastFed: 0, fedUpTo: 0 };
  const feed = require('node:vm').runInNewContext(`(function(){${source.slice(start, end)} return feedFromServer;})()`, {
    s: state, freshLevels: L.freshLevels, loudness: L.loudness, spread: L.spread, SERVER_STEP_MS: 250, WAVE_STEP_MS: 60, MOST_LEVEL_QUEUE: 40,
  });
  for (let poll = 0; poll < 1000; poll++) {
    feed({ levels: [0.1, 0.2, 0.3, 0.4], levels_start: poll * 4 });
    assert.ok(state.queue.length <= 40);
  }
  assert.equal(state.queue.length, 40);
  assert.equal(state.fedUpTo, 4000);
});

test('the clock reads like a timer', () => {
  assert.equal(L.clock(0), '0:00');
  assert.equal(L.clock(65.9), '1:05');
  assert.equal(L.clock(3723), '1:02:03');
});

test('the state line says what is happening, with steps while writing', () => {
  assert.equal(L.stateWord({ state: 'recording' }), 'Recording');
  assert.equal(L.stateWord({ state: 'writing', step: 'Writing the notes', steps: [1, 3] }), 'Writing the notes 1/3');
  assert.equal(L.stateWord({ state: 'writing', step: 'Transcribing the recording', steps: null }), 'Transcribing the recording');
});

test('loudness follows how loud it sounds, not raw amplitude', () => {
  assert.equal(L.loudness(0), 0);
  assert.equal(L.loudness(-1), 0);
  assert.equal(L.loudness(Number.NaN), 0);
  assert.equal(L.loudness(1), 1);
  const whisper = L.loudness(0.005);
  const speech = L.loudness(0.1);
  assert.ok(whisper > 0 && whisper < L.QUIET, `a whisper of noise reads as quiet: ${whisper}`);
  assert.ok(speech > 0.5, `normal speech fills most of the wave: ${speech}`);
});

test('the wave says when sound is heard, and what to check when it is not', () => {
  const loud = Array(10).fill(0.6);
  assert.equal(L.hearing([], 80), 'waiting');
  assert.equal(L.hearing(loud, 80), 'sound');
  assert.equal(L.hearing([...loud, ...Array(10).fill(0)], 80), 'sound', 'a short pause between words still counts');
  assert.equal(L.hearing([...loud, ...Array(30).fill(0)], 80), 'quiet');
  assert.equal(L.hearing(Array(30).fill(0), 250), 'silent');
  assert.equal(L.hearingWords('sound', 'browser', false), 'Hearing sound');
  assert.match(L.hearingWords('silent', 'browser', false), /microphone/);
  assert.match(L.hearingWords('silent', 'screen', false), /playing on the computer/);
  assert.match(L.hearingWords('sound', 'browser', true), /Paused/);
});

test('the wave rises quickly with sound and falls back gently', () => {
  let level = 0;
  level = L.ease(level, 1);
  assert.ok(level > 0.5, `a sound shows at once: ${level}`);
  const peak = level;
  level = L.ease(level, 0);
  assert.ok(level > peak * 0.8, `and fades slowly: ${level}`);
  let settled = 1;
  for (let i = 0; i < 60; i++) settled = L.ease(settled, 0);
  assert.ok(settled < 0.01);
});

test('levels from the computer are spread into smooth steps and never repeated', () => {
  assert.deepEqual(L.spread(0, 1, 4), [0.25, 0.5, 0.75, 1]);
  assert.deepEqual(L.freshLevels([0.1, 0.2, 0.3], 10, 0), { fresh: [0.1, 0.2, 0.3], fedUpTo: 13 });
  assert.deepEqual(L.freshLevels([0.2, 0.3, 0.4], 11, 13), { fresh: [0.4], fedUpTo: 14 });
  assert.deepEqual(L.freshLevels([0.2, 0.3, 0.4], 11, 14), { fresh: [], fedUpTo: 14 });
  assert.deepEqual(L.freshLevels([0.5, 0.6], 50, 14), { fresh: [0.5, 0.6], fedUpTo: 52 }, 'after a gap, only what is still there');
});

test('two choices: the microphone here, or the screen, wherever that can work', () => {
  assert.equal(L.sourceFor('microphone', { local: true, canShare: true }), 'microphone');
  assert.equal(L.sourceFor('microphone', { local: false, canShare: false }), 'microphone');
  assert.equal(L.sourceFor('screen', { local: true, canShare: false }), 'screen', 'on the computer: everything it plays');
  assert.equal(L.sourceFor('screen', { local: false, canShare: true }), 'screen', 'elsewhere: share a tab or screen');
  assert.equal(L.sourceFor('screen', { local: false, canShare: false }), 'screen', 'a phone cannot record its screen');
});

test('empty-call failures have a calm explanation and keep technical details separate', () => {
  const silent = 'No sound was recorded. Check that the browser may use the microphone.';
  const failure = L.failureOf({ error: `Others: ${silent}; You: ${silent}`, warnings: ['Recovered audio from the last interrupted recording.'] });
  assert.equal(failure.title, 'No audio was captured');
  assert.match(failure.message, /stopped before any sound/);
  assert.doesNotMatch(failure.message, /Others:|You:|Recovered/);
  assert.equal(failure.details.length, 2);
  assert.equal(L.failureOf({ error: 'Provider failed', warnings: [] }).title, 'Your note could not be saved');
});

test('failed recordings offer deletion and dismissal on their own page', () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/recording.js'), 'utf8');
  const start = source.indexOf('    function drawFailed()');
  const end = source.indexOf('    let folders', start);
  const render = (overview, error) => require('node:vm').runInNewContext(`(function(){${source.slice(start, end)} return drawFailed;})()`, {
    s: { overview, view: { id: 'failed-01', session: 'sess-1', source: 'nativecall', secs: 0, error, warnings: [] } },
    failureOf: L.failureOf, clock: L.clock, esc: (s) => String(s).replaceAll('<', '&lt;'),
    felix: { felix: () => '' }, icons: { mic: '', trash: '' },
  })();
  const html = render({ pending: [{ id: 'sess-1' }] }, '<unsafe>');
  assert.match(html, /data-action="rec-discard" data-id="failed-01"/);
  assert.match(html, /data-action="rec-dismiss"/);
  assert.match(html, /<details[^>]*.*Error details/);
  assert.ok(!html.includes('<unsafe>'));
  assert.match(render({ pending: [{ id: 'sess-1' }] }, 'Provider failed'), /data-action="rec-recover" data-id="sess-1"[^>]*>Recover recording<.*>Close</s);
  assert.doesNotMatch(render({ pending: [] }, 'Provider failed'), /rec-recover/);
  assert.doesNotMatch(render({ pending: [{ id: 'sess-1' }] }, 'No sound was recorded.'), /rec-recover/);
});

test('an interrupted recording says when it started, what it was and where it goes', () => {
  const started = new Date();
  started.setHours(9, 2, 0, 0);
  assert.match(L.pendingLine({ started: started.toISOString(), source: 'microphone', directory: 'cs162' }), /^Today .+ · Microphone · cs162$/);
  assert.equal(L.pendingLine({ source: 'microphone', directory: 'cs162' }), 'Microphone · cs162');
  assert.equal(L.pendingLine({ source: 'nativecall', directory: '' }), 'Call · Top level');
});

test('a recording saved without written notes says so where the user lands', () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/recording.js'), 'utf8');
  const start = source.indexOf('    function accept(view)');
  const end = source.indexOf('    async function start(', start);
  const finish = (view) => {
    const went = [];
    const toasts = [];
    const accept = require('node:vm').runInNewContext(`(function(){${source.slice(start, end)} return accept;})()`, {
      s: { view: { id: 'r1', state: 'writing', warnings: [] }, earlier: [], container: { isConnected: true } },
      api: () => Promise.resolve(), feedFromServer: () => {}, draw: () => {}, pill: () => {}, noteReady: null,
      go: (hash) => went.push(hash), noteHash: (id) => `#/n/${id}`, toast: (text, opts) => toasts.push({ text, ...opts }), root: {},
    });
    accept({ id: 'r1', state: 'done', note: 'n1', warnings: [], ...view });
    return { went, toasts };
  };
  const failed = finish({ writer_failed: true });
  assert.deepEqual(failed.went, ['#/n/n1']);
  assert.equal(failed.toasts.length, 1);
  assert.equal(failed.toasts[0].text, 'Saved the transcript. The notes were not written.');
  assert.equal(failed.toasts[0].action, 'Set up writing AI');
  failed.toasts[0].run();
  assert.equal(failed.went.at(-1), '#/settings/writing');
  assert.equal(finish({}).toasts[0].text, 'Your recording is now a note.');
});

test('a note whose recording was not written up offers both ways forward', () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/app/core.js'), 'utf8');
  const start = source.indexOf('${note.writeup_pending');
  const end = source.indexOf(" : ''}", start) + 6;
  const notice = (note) => require('node:vm').runInNewContext(`\`${source.slice(start, end)}\``, { note });
  const html = notice({ writeup_pending: true });
  assert.match(html, /<div class="set-card writeup-pending"><h3>These notes are not written up yet<\/h3><p class="set-note">No writing AI was ready, so leo kept the transcript as it is\.<\/p>/);
  assert.match(html, /<a class="btn primary sm" href="#\/settings\/writing">Set up writing AI<\/a>/);
  assert.match(html, /<button type="button" class="btn plain sm" data-action="note-sources">Write the notes<\/button>/);
  assert.equal(notice({}), '');
});

const recordingSource = require('node:fs').readFileSync(require.resolve('../src/web/recording.js'), 'utf8');
const piece = (from, to, name, scope) => {
  const start = recordingSource.indexOf(`    function ${from}(`);
  const end = recordingSource.indexOf(`    function ${to}(`, start);
  return require('node:vm').runInNewContext(`(function(){${recordingSource.slice(start, end)} return ${name};})()`, scope);
};
const esc = (text) => String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');
const icons = new Proxy({}, { get: () => '' });

function idle(checks, platform, denied = false) {
  return piece('chip', 'drawLive', 'drawIdle', {
    s: { overview: { available: true, checks }, calendar: null, event: null },
    esc, icons, whenOf: L.whenOf, pendingLine: L.pendingLine, folderOptions: () => '', kindChoices: () => '', speakersRow: () => '',
    askChips: L.askChips, preflight: L.preflight, tipWords: L.tipWords, HEADSET: L.HEADSET, desktopNow: () => ({ platform }), micDenied: () => denied,
  })([], '');
}

test('each missing piece is said above Start in a quiet line, and Start stays enabled', () => {
  const html = idle({ speech: false, writing: false, headset: true }, 'macos');
  assert.match(html, /<p class="set-note">No speech model yet\. The audio is kept and written up once you choose one\. <a href="#\/settings\/models">Choose a speech model<\/a><\/p>/);
  assert.match(html, /<p class="set-note">No writing AI is ready, so you.ll get a transcript\. <a href="#\/settings\/writing">Set up writing AI<\/a><\/p>/);
  assert.match(html, /<p class="set-note" id="rec-headset" hidden>Your sound output is a headset, so leo can.t record only what this computer plays\. Switch to other speakers or headphones first\.<\/p>/);
  assert.ok(html.indexOf('set-note') < html.indexOf('rec-start'));
  assert.doesNotMatch(html, /warn/);
  assert.doesNotMatch(html, /rec-go"[^>]*disabled/);
  const ready = idle({ speech: true, writing: true, headset: false }, 'macos');
  assert.doesNotMatch(ready, /set-note|rec-headset/);
});

test('a denied microphone is said above Start with a way to the system settings, and Start stays enabled', () => {
  const html = idle({}, 'macos', true);
  assert.match(html, /<p class="set-note">leo can’t hear: microphone access is off\. <button class="btn plain sm" data-action="app-command" data-app-action="microphone">Open System Settings<\/button><\/p>/);
  assert.ok(html.indexOf('microphone access is off') < html.indexOf('rec-start'));
  assert.doesNotMatch(html, /rec-go"[^>]*disabled/);
  assert.doesNotMatch(idle({}, 'macos'), /microphone access is off/);
});

test('the idle tip says system tray on Windows', () => {
  assert.match(idle({}, 'windows'), /Open leo from the system tray to return\./);
  assert.match(idle({}, 'macos'), /Open leo from the menu bar to return\./);
  assert.match(idle({}, undefined), /menu bar/);
});

test('a denied microphone is said plainly with a way to the system settings', () => {
  assert.equal(L.hearingWords('sound', 'microphone', false, true), 'leo can’t hear: microphone access is off.');
  assert.equal(L.hearingWords('sound', 'screen', false, true), 'Hearing sound');
  assert.equal(L.hearingWords('sound', 'microphone', false, false), 'Hearing sound');
  const live = (denied) => piece('chip', 'drawWriting', 'drawLive', {
    s: { view: { state: 'recording', source: 'microphone', points: [], warnings: [], secs: 3, transcript: '' }, earlier: [], wave: [], tab: 'points', wantsDraft: null, wantsState: '' },
    esc, icons, whenOf: L.whenOf, askChips: L.askChips, clock: L.clock, earlierFirst: L.earlierFirst, placed: () => [], stateWord: L.stateWord, hearing: L.hearing, hearingWords: L.hearingWords, WAVE_STEP_MS: 60, micDenied: () => denied, desktopNow: () => ({ platform: 'macos' }),
  })();
  const html = live(true);
  assert.match(html, /leo can’t hear: microphone access is off\./);
  assert.match(html, /<button class="btn plain sm" data-action="app-command" data-app-action="microphone">Open System Settings<\/button>/);
  assert.doesNotMatch(live(false), /Open System Settings/);
});

test('choosing Call fills an empty want and never overwrites typed text', () => {
  const pick = (kind, typed) => {
    const wants = { value: typed };
    const headset = { hidden: true };
    const box = { querySelector: (at) => ({ 'input[name="rec-kind"]:checked': { value: kind }, '#rec-headset': headset, '#rec-wants': wants })[at] };
    piece('pickKind', 'idleForm', 'pickKind', { MEETING_WANTS: L.MEETING_WANTS })(box, true);
    return { wants: wants.value, headset: headset.hidden };
  };
  assert.deepEqual(pick('call', ''), { wants: L.MEETING_WANTS, headset: false });
  assert.deepEqual(pick('call', '  '), { wants: L.MEETING_WANTS, headset: false });
  assert.equal(pick('call', 'only the exam').wants, 'only the exam');
  assert.deepEqual(pick('microphone', ''), { wants: '', headset: true });
});

test('a meeting prompt preselects Call and its event and fills the meeting wording without starting', () => {
  const ask = (asked, typed = '', s = { calendar: { events: [{ id: 'standup@1', title: 'Standup', start: '2026-11-09T09:00:00Z', end: '2026-11-09T09:15:00Z' }, { id: 'sync@2', title: 'Design sync', start: '2026-11-09T15:00:00Z', end: '2026-11-09T16:00:00Z' }] }, event: null, eventChosen: false }) => {
    let picked = 'microphone';
    const radios = ['microphone', 'screen', 'call'].map((value) => ({ value, disabled: false, get checked() { return picked === value; }, set checked(on) { if (on) picked = value; } }));
    const fields = { '#rec-title': { value: '' }, '#rec-wants': { value: typed }, '#rec-headset': { hidden: true }, '.rec-idle': {} };
    const box = { querySelector: (at) => at === 'input[name="rec-kind"]:checked' ? radios.find((r) => r.checked) : fields[at] || radios.find((r) => at === `input[name="rec-kind"][value="${r.value}"]`) || null };
    piece('chooseEvent', 'idleForm', 'preselect', { s, MEETING_WANTS: L.MEETING_WANTS, eventNow: () => -1 })(box, asked);
    return { kind: radios.find((r) => r.checked).value, event: s.event, chosen: s.eventChosen, title: fields['#rec-title'].value, wants: fields['#rec-wants'].value };
  };
  assert.deepEqual(ask({ event: 'sync@2', kind: 'call' }), { kind: 'call', event: 1, chosen: true, title: 'Design sync', wants: L.MEETING_WANTS });
  assert.equal(ask({ event: 'sync@2', kind: 'call' }, 'only the decisions').wants, 'only the decisions');
  assert.deepEqual(ask({ event: 'gone@3', kind: 'call' }), { kind: 'call', event: null, chosen: false, title: '', wants: L.MEETING_WANTS });
  assert.equal(ask({ event: 'sync@2', kind: 'call' }, '', { calendar: { events: [{ id: 'sync@2', title: 'Design sync' }] }, event: null, eventChosen: true }).event, 0, 'the prompt wins over an earlier choice');
  assert.deepEqual(ask({ event: 'sync@2', kind: 'microphone' }), { kind: 'microphone', event: 1, chosen: true, title: 'Design sync', wants: '' });
  assert.equal(ask({ kind: '"] , x' }).kind, 'microphone');
  assert.equal(ask({}).kind, 'microphone');
  const body = recordingSource.slice(recordingSource.indexOf('    function preselect('), recordingSource.indexOf('    function idleForm('));
  assert.doesNotMatch(body, /begin|start\(|rec-start|click/, 'the prompt never presses Start');
});

test('a hidden window pauses the speaker-model poll', async () => {
  const timers = [];
  const asked = [];
  const box = { isConnected: true, querySelector: () => ({}) };
  const scope = {
    s: {}, setTimeout: (fn, ms) => { timers.push({ fn, ms }); return timers.length; }, clearTimeout() {}, redrawSpeakers() {}, onScreen: () => false,
    api: async (url) => { asked.push(url); return { speakers: { progress: { state: 'downloading', bytes: 1, total: 2 } } }; },
  };
  const speakerModel = require('node:vm').runInNewContext(`(function(){${recordingSource.slice(recordingSource.indexOf('    async function speakerModel('), recordingSource.indexOf('    function redrawSpeakers('))} return speakerModel;})()`, scope);
  await speakerModel(box);
  const first = asked.length;
  for (let i = 0; i < 4; i++) {
    for (const t of timers.splice(0)) t.fn();
    for (let n = 0; n < 5; n++) await Promise.resolve();
  }
  assert.equal(asked.length - first, 0);
});

test('older recordings are appended once, newest page first', () => {
  const first = { pending: [{ id: 'c' }, { id: 'b' }], more: true, after: '20260102-090000', checks: { speech: true } };
  const second = { pending: [{ id: 'b' }, { id: 'a' }], more: false, after: null, checks: { speech: true } };
  const merged = L.mergePages(first, second);
  assert.deepEqual(merged.pending.map((p) => p.id), ['c', 'b', 'a']);
  assert.equal(merged.more, false);
  assert.equal(merged.after, null);
  assert.deepEqual(first.pending.map((p) => p.id), ['c', 'b']);
});

test('the quit warning says when there are more than one page', () => {
  assert.equal(L.unsavedLine(1, false), "1 recording wasn't saved");
  assert.equal(L.unsavedLine(50, false), "50 recordings weren't saved");
  assert.equal(L.unsavedLine(50, true), "50 or more recordings weren't saved");
});

test('earlier points go before the shown ones, once', () => {
  const shown = [[50, 300, 'same words'], [51, 310, 'newest']];
  const earlier = [[48, 280, 'same words'], [49, 290, 'older'], [50, 300, 'same words']];
  assert.deepEqual(L.earlierFirst(earlier, shown), [
    [48, 280, 'same words'],
    [49, 290, 'older'],
    [50, 300, 'same words'],
    [51, 310, 'newest'],
  ]);
  assert.deepEqual(L.earlierFirst([], shown), shown);
});

test('a live recording off the Record page is announced for the activity panel, and not on it', () => {
  const heard = [];
  const from = recordingSource.indexOf('    function pill(');
  const pill = (s) => require('node:vm').runInNewContext(`(function(){${recordingSource.slice(from, recordingSource.indexOf('    async function refresh(', from))} return pill;})()`, {
    s, live: (view) => ['starting', 'recording', 'paused'].includes(view.state), clock: L.clock,
    root: { CustomEvent: class { constructor(name, init) { this.name = name; this.detail = init.detail; } }, dispatchEvent: (event) => heard.push(event) },
  });
  const page = { view: null, container: null };
  const announce = (view, container) => {
    Object.assign(page, { view, container });
    pill(page)();
  };
  announce(null, null);
  announce({ state: 'recording', secs: 75 }, null);
  announce({ state: 'recording', secs: 75 }, null);
  announce({ state: 'paused', secs: 3605 }, { isConnected: false });
  announce({ state: 'recording', secs: 5 }, { isConnected: true });
  announce({ state: 'recording', secs: 6 }, { isConnected: true });
  announce({ state: 'done', secs: 5 }, null);
  announce(null, null);
  assert.deepEqual(JSON.parse(JSON.stringify(heard.map((event) => [event.name, event.detail]))), [
    ['leo-recording', { paused: false, label: 'Recording · 1:15' }],
    ['leo-recording', { paused: true, label: 'Paused · 1:00:05' }],
    ['leo-recording', null],
  ]);
});

test('a new transcript keeps the shown words and replaces only the changed tail', () => {
  const merged = (shown, next) => {
    const m = L.tailMerge(shown, next);
    assert.equal(shown.slice(m.drop, m.drop + m.keep) + m.tail, next);
    return m;
  };
  assert.deepEqual(merged('', 'Hello there'), { drop: 0, keep: 0, tail: 'Hello there' });
  assert.deepEqual(merged('Today we cover', 'Today we cover graphs'), { drop: 0, keep: 14, tail: ' graphs' });
  assert.deepEqual(merged('Today we cover grafs and', 'Today we cover graphs and trees'), { drop: 0, keep: 15, tail: 'graphs and trees' });
  assert.deepEqual(merged('Today we cover graphs', 'Today we cover'), { drop: 0, keep: 14, tail: '' });
  assert.deepEqual(merged('same', 'same'), { drop: 0, keep: 4, tail: '' });
  const long = Array.from({ length: 40 }, (_, i) => `word${i}`).join(' ');
  const trimmed = merged(long, `${long.slice(30)} more`);
  assert.equal(trimmed.drop, 30);
  assert.equal(trimmed.keep, long.length - 30);
  const both = merged(`${long} tale`, `${long.slice(50)} tail end`);
  assert.equal(both.drop, 50);
  assert.equal(both.tail, 'tail end');
  assert.deepEqual(merged('nothing alike', 'Something else entirely'), { drop: 0, keep: 0, tail: 'Something else entirely' });
  assert.equal(merged('we laughed \u{1F600}', 'we laughed \u{1F601}').keep, 11);
  assert.equal(merged('a'.repeat(100), 'a'.repeat(100) + 'b').keep, 100 - 32);
});

function fakeTranscript() {
  const text = { kids: [], scrollTop: 0, scrollHeight: 0, clientHeight: 0, wired: 0 };
  const own = (node) => Object.assign(node, { remove() { text.kids.splice(text.kids.indexOf(node), 1); } });
  const textNode = (data) => own({ nodeType: 3, firstChild: null, data, get textContent() { return this.data; }, deleteData(at, count) { this.data = this.data.slice(0, at) + this.data.slice(at + count); } });
  Object.defineProperties(text, {
    firstChild: { get: () => text.kids[0] || null },
    lastChild: { get: () => text.kids[text.kids.length - 1] || null },
    textContent: { get: () => text.kids.map((k) => k.textContent).join(''), set: (v) => { text.kids = v ? [textNode(v)] : []; } },
    innerHTML: { set: (v) => { text.kids = [own({ hint: v, textContent: '' })]; } },
  });
  text.append = (node) => text.kids.push(node);
  text.addEventListener = () => text.wired++;
  text.querySelectorAll = (at) => text.kids.filter((k) => at === '.rec-new' && k.className === 'rec-new');
  text.normalize = () => {
    for (let i = text.kids.length - 1; i > 0; i--) {
      if (text.kids[i].nodeType === 3 && text.kids[i - 1].nodeType === 3) text.kids[i - 1].data += text.kids.splice(i, 1)[0].data;
    }
  };
  const document = {
    createTextNode: textNode,
    createElement: () => own({
      firstChild: null,
      get childNodes() { return [this.firstChild]; },
      get textContent() { return this.firstChild.data; },
      set textContent(v) { this.firstChild = textNode(v); },
      getAnimations() { return this.fading ? [{}] : []; },
      replaceWith(...nodes) { text.kids.splice(text.kids.indexOf(this), 1, ...nodes); },
    }),
  };
  const heard = { addEventListener: () => text.wired++ };
  const latest = { hidden: true, addEventListener: () => text.wired++ };
  const box = { querySelector: (at) => ({ '#rec-transcript': text, '#rec-heard': heard, '#rec-latest': latest })[at] };
  return { text, latest, box, document };
}

test('the live transcript is patched in place: kept words stay the same node and only new words fade in', () => {
  const { text, latest, box, document } = fakeTranscript();
  const s = { view: { state: 'recording', transcript: '' }, follow: true };
  const said = 'We started with the outline of the course and what the exam will ask about.';
  const selection = { isCollapsed: true, containsNode: () => true };
  const show = piece('showTranscript', 'draw', 'showTranscript', { s, root: { document, getSelection: () => selection }, tailMerge: L.tailMerge });
  show(box);
  assert.ok(text.kids[0].hint.includes('Listening'));
  s.view.transcript = `${said} Today we cover graphs`;
  show(box);
  assert.equal(text.kids[0].className, 'rec-new', 'the first words after Listening fade in');
  const first = text.kids[0].firstChild;
  s.view.transcript = `${said} Today we cover graphs and treez`;
  show(box);
  s.view.transcript = `${said} Today we cover graphs and trees today`;
  show(box);
  assert.equal(text.textContent, s.view.transcript);
  assert.equal(text.kids[0], first, 'faded words are folded back into the kept text node');
  assert.deepEqual(text.kids.map((k) => [k.className, k.textContent]), [[undefined, `${said} Today we cover graphs and `], ['rec-new', 'trees today']]);
  s.view.transcript = s.view.transcript.slice(30) + ' more';
  show(box);
  assert.equal(text.textContent, s.view.transcript, 'a trimmed head is dropped from the front');
  assert.equal(text.kids[0], first);
  assert.equal(text.wired, 2, 'listeners are added once, when the transcript is made');
  assert.equal(latest.hidden, true);
  s.follow = false;
  s.view.transcript += ' again';
  show(box);
  assert.equal(latest.hidden, false, 'a reader scrolled up is offered Back to latest');
  const again = fakeTranscript();
  piece('showTranscript', 'draw', 'showTranscript', { s: { view: { state: 'recording', transcript: 'Said before' } }, root: { document: again.document }, tailMerge: L.tailMerge })(again.box);
  assert.equal(again.text.kids[0].nodeType, 3, 'what was already said arrives without a fade');
  assert.equal(again.text.textContent, 'Said before');
});

test('a long recording keeps a handful of transcript nodes, and never folds under a selection or a running fade', () => {
  const { text, box, document } = fakeTranscript();
  const s = { view: { state: 'recording', transcript: 'Start.' } };
  const selection = { isCollapsed: true, containsNode: () => true };
  const show = piece('showTranscript', 'draw', 'showTranscript', { s, root: { document, getSelection: () => selection }, tailMerge: L.tailMerge });
  show(box);
  for (let poll = 0; poll < 3000; poll++) {
    s.view.transcript += ` word${poll}`;
    show(box);
    assert.ok(text.kids.length <= 2);
  }
  assert.equal(text.textContent, s.view.transcript);
  text.kids[1].fading = true;
  s.view.transcript += ' later';
  show(box);
  assert.equal(text.kids.length, 3, 'a fade still running is left alone');
  selection.isCollapsed = false;
  for (let poll = 0; poll < 3; poll++) {
    s.view.transcript += ` held${poll}`;
    show(box);
  }
  assert.equal(text.kids.length, 6, 'a selection in the transcript is never collapsed by folding');
  selection.isCollapsed = true;
  text.kids.forEach((k) => { k.fading = false; });
  s.view.transcript += ' free';
  show(box);
  assert.equal(text.kids.length, 2);
  assert.equal(text.textContent, s.view.transcript);
});

test('a live redraw replaces the cards around the transcript and Felix\'s questions, at any depth, and never those two', () => {
  const el = (id, children = [], className = '') => {
    const node = { id, className, children };
    node.contains = (other) => node === other || node.children.some((child) => child.contains(other));
    node.querySelector = () => null;
    children.forEach((child) => {
      child.replaceWith = (next) => { if (['rec-heard', 'rec-ask'].includes(child.id)) throw new Error(`${child.id} was replaced`); node.children[node.children.indexOf(child)] = next; };
    });
    return node;
  };
  const page = (paused) => [el('card'), el('pane', [el('side'), el('rec-heard')], paused ? 'pane paused' : 'pane'), el('pad'), el('rec-ask'), el('wants')];
  const shown = page(false);
  const heard = shown[1].children[1];
  const ask = shown[3];
  const box = el('box', [el('', shown, 'rec rec-live')]);
  box.querySelectorAll = (at) => (at === '[data-keep]' ? [heard, ask] : []);
  Object.defineProperty(box, 'innerHTML', { set() { throw new Error('the whole page was rewritten'); } });
  const fresh = el('', page(true), 'rec rec-live paused');
  const document = { createElement: () => el('made', [fresh]) };
  piece('graft', 'showTranscript', 'paintLive', { root: { document }, drawLive: () => '' })(box);
  const shell = box.children[0];
  assert.equal(shell.className, 'rec rec-live paused');
  assert.equal(shell.children[1].className, 'pane paused');
  assert.equal(shell.children[1].children[1], heard);
  assert.equal(shell.children[3], ask);
  assert.deepEqual([shell.children[0], shell.children[1].children[0], shell.children[2], shell.children[4]], [fresh.children[0], fresh.children[1].children[0], fresh.children[2], fresh.children[4]]);
  const other = el('box', [el('', [el('card')], 'rec rec-idle')]);
  let rewritten = '';
  other.querySelectorAll = () => [];
  Object.defineProperty(other, 'innerHTML', { set(v) { rewritten = v; } });
  piece('graft', 'showTranscript', 'paintLive', { root: { document: { createElement: () => el('made', []) } }, drawLive: () => '<live>' })(other);
  assert.equal(rewritten, '<live>', 'without a transcript yet, the live page is drawn whole');
});

test('Felix\'s questions follow the stage: before Start, while recording, and on a recorded note', () => {
  assert.deepEqual(L.FELIX_ASKS.before, ['What should I listen for?', 'What should I ask?']);
  assert.deepEqual(L.FELIX_ASKS.live, ['What did I just miss?', 'Sum up so far', 'What should I ask next?']);
  assert.equal((L.askChips('live', esc).match(/class="starter" data-rec-ask=/g) || []).length, 3);
  const chips = idle({ speech: true, writing: true }, 'macos');
  assert.match(chips, /<div class="rec-ask" id="rec-ask" hidden>.*What should I listen for\?/s, 'hidden until an event is chosen or a want is typed');
  const s = { event: null };
  const ask = { hidden: false };
  const wants = { value: '' };
  const sync = piece('syncAsk', 'askFelix', 'syncAsk', { s });
  const box = { querySelector: (at) => ({ '.rec-idle #rec-ask': ask, '#rec-wants': wants })[at] };
  sync(box);
  assert.equal(ask.hidden, true);
  wants.value = 'the exam topics';
  sync(box);
  assert.equal(ask.hidden, false);
  wants.value = '';
  s.event = 0;
  sync(box);
  assert.equal(ask.hidden, false);
  const live = piece('chip', 'drawWriting', 'drawLive', {
    s: { view: { state: 'recording', source: 'microphone', points: [], warnings: [], secs: 3, transcript: '' }, earlier: [], wave: [], tab: 'points', wantsDraft: null, wantsState: '' },
    esc, icons, whenOf: L.whenOf, askChips: L.askChips, clock: L.clock, earlierFirst: L.earlierFirst, placed: () => [], stateWord: L.stateWord, hearing: L.hearing, hearingWords: L.hearingWords, WAVE_STEP_MS: 60, micDenied: () => false, desktopNow: () => ({ platform: 'macos' }),
  })();
  assert.match(live, /rec-notepad.*rec-wants-card.*<aside class="set-card rec-felix rec-ask" id="rec-ask" data-keep aria-label="Ask Felix"><header><h3>Felix<\/h3><\/header>.*What did I just miss\?/s, 'the questions sit in Felix\'s own column beside the tabs');
  assert.match(live, /id="rec-heard" role="tabpanel" data-keep/);
  assert.match(live, /id="rec-tab-points"/);
  assert.match(live, /id="rec-points-panel" role="tabpanel" aria-labelledby="rec-tab-points"/);
  assert.match(live, /id="rec-heard" role="tabpanel" data-keep aria-labelledby="rec-tab-heard"/);
  assert.match(live, /id="rec-wants-panel" role="tabpanel" aria-labelledby="rec-tab-wants"/);
  assert.match(live, /id="rec-pause" data-keep data-action="rec-pause"/);
  assert.match(live, /id="rec-stop" data-keep data-action="rec-stop"/);
  assert.match(live, /<textarea id="rec-wants-live"[^>]*data-keep/);
  assert.match(live, /data-rec-tab="points"[^>]*aria-selected="true"[^>]*>Your notes <span class="badge" id="rec-count">0<\/span><\/button>.*data-rec-tab="heard"[^>]*>Transcript<\/button>.*data-rec-tab="wants"[^>]*>What you want<\/button>/s, 'Your notes is the first tab and open by default');
  assert.match(live, /<div class="rec-bar"[^>]*><span class="rec-dot"><\/span>.*id="rec-time">0:03<\/span>.*id="rec-wave".*data-action="rec-pause".*data-action="rec-stop"/s, 'one recorder bar: dot, clock, wave, Pause, Stop');
});

test('a question asked while recording sends the newest transcript and the typed points within 8,000 characters', () => {
  const said = 'x'.repeat(250000) + ' the newest words';
  const text = L.liveAsk(['first point', 'second point'], said);
  assert.ok(text.length <= L.MOST_ASK, `${text.length}`);
  assert.ok(Array.from(text).length <= 8000);
  assert.match(text, /^My points so far:\n- first point\n- second point\n\nThe newest part of the transcript:\n/);
  assert.ok(text.endsWith(' the newest words'), 'the tail is sent, not the head');
  const many = L.liveAsk(Array.from({ length: 2000 }, (_, i) => `point ${i}`), said);
  assert.ok(many.length <= L.MOST_ASK);
  assert.match(many, /- point 1999\n\nThe newest part/);
  const emoji = L.liveAsk([], '\u{1F600}'.repeat(5000));
  assert.ok(emoji.length <= L.MOST_ASK);
  assert.doesNotMatch(emoji, /\n[\uDC00-\uDFFF]/, 'never starts inside a surrogate pair');
  assert.match(L.liveAsk([], ''), /nothing heard yet/);
  const brief = L.briefAsk({ title: 'Design sync', start: '2026-11-21T15:00:00Z', end: '2026-11-21T16:00:00Z', context: 'Agenda: launch' }, 'decisions only');
  assert.match(brief, /^Calendar event: Design sync \(.*\)\nAgenda: launch\n\nWhat I want from the notes: decisions only$/);
  assert.equal(L.briefAsk(null, 'only wants'), 'What I want from the notes: only wants');
});

test('the live page keeps one column unless its container is wide enough for Felix\'s column', () => {
  const css = require('node:fs').readFileSync(require.resolve('../src/web/styles/13-setup-misc.css'), 'utf8');
  const columns = css.split('\n').filter((line) => /\.rec-body[^{]*\{[^}]*grid-template-columns/.test(line));
  assert.equal(columns.length, 1);
  assert.match(columns[0], /^@container recroom \(min-width: \d+px\) \{ \.rec-body:has\(> \.rec-felix\)/, 'only inside a container query, and only while Felix\'s column is there');
});

test('after Stop the page keeps its header and tabs, with one progress line and Finish now', () => {
  const writing = piece('chip', 'drawFailed', 'drawLive', {
    s: { view: { id: 'r', state: 'writing', step: 'Writing the notes', steps: [2, 5], source: 'microphone', title: 'Capstone sync', directory: 'Work', points: [[30, 'a point']], points_total: 1, warnings: [], secs: 90, transcript: 'said' }, earlier: [], wave: [], tab: 'wants', wantsDraft: null, wantsState: '' },
    esc, icons, whenOf: L.whenOf, askChips: L.askChips, clock: L.clock, earlierFirst: L.earlierFirst, placed: (v) => v.points.map(([at, text], k) => [k, at, text]), stateWord: L.stateWord, hearing: L.hearing, hearingWords: L.hearingWords, WAVE_STEP_MS: 60, micDenied: () => false, desktopNow: () => ({ platform: 'macos' }), Date,
  })();
  assert.match(writing, /<h1>Capstone sync<\/h1>/);
  assert.match(writing, /<use href="#leo-mark"\/><\/svg><span class="rec-progress-step">Writing the notes · 2\/5<\/span><div class="upload-bar"/);
  assert.match(writing, /data-action="rec-finish"[^>]*>Finish now<\/button>/);
  assert.match(writing, /aria-disabled="true"[^>]*disabled>Notes<\/button>.*data-rec-tab="points"[^>]*aria-selected="true".*data-rec-tab="heard"/s, 'Notes waits; Your notes is open since What you want is gone');
  assert.doesNotMatch(writing, /rec-bar|rec-wants-live|rec-point-text|rec-felix/);
  assert.match(writing, /a point/);
  assert.match(writing, /id="rec-heard" role="tabpanel" data-keep/);
});

test('speakers take tones in the order they were first heard, You keeps ink and source labels are not speakers', () => {
  const speakers = ['You', 'Others', 'Sam'];
  assert.equal(L.toneOf(speakers, 'You'), 'var(--text)');
  assert.equal(L.toneOf(speakers, 'Others'), 'var(--hue-0)');
  assert.equal(L.toneOf(speakers, 'Sam'), 'var(--hue-1)');
  assert.equal(L.toneOf(['Speaker 1', 'Speaker 2', 'Speaker 3'], 'Speaker 3'), 'var(--hue-4)');
  assert.deepEqual(L.speakersOf(['Microphone', 'Computer audio', '']), []);
  assert.deepEqual(L.speakersOf(['Microphone', 'Speaker 1']), ['Speaker 1']);
});

test('a meeting groups lines into turns and folds echo lines without starting a turn', () => {
  const p = (seq, at, speaker, text, echo) => [seq, { start_secs: at, end_secs: at + 4, text, speaker, ...(echo ? { echo: true } : {}) }];
  const flow = L.transcriptFlow(
    [p(1, 0, 'You', 'a'), p(2, 5, 'You', 'b'), p(3, 9, 'Others', 'c'), p(4, 10, 'You', 'c', true), p(5, 11, 'You', 'd', true), p(6, 14, 'Others', 'e'), p(7, 20, 'You', 'f')],
    [[1, { at_secs: 5, text: 'typed' }]],
    ['You', 'Others'],
  );
  assert.equal(flow.meeting, true);
  assert.deepEqual(flow.groups.map((g) => [g.speaker, g.at]), [['You', 0], ['Others', 9], ['You', 20]]);
  assert.deepEqual(flow.groups[0].items.map((i) => Object.keys(i)[0]), ['passage', 'passage', 'point'], 'a point comes after the line said at the same second');
  assert.deepEqual(flow.groups[1].items.map((i) => (i.echo ? i.echo.map(([seq]) => seq) : i.passage[0])), [3, [4, 5], 6]);
});

test('a lecture makes one paragraph a minute and a missing echo field counts as not echo', () => {
  const p = (seq, at) => [seq, { start_secs: at, end_secs: at + 5, text: String(seq), speaker: 'Microphone' }];
  const flow = L.transcriptFlow([p(1, 3), p(2, 50), p(3, 60), p(4, 179)], [], ['Microphone']);
  assert.equal(flow.meeting, false);
  assert.deepEqual(flow.groups.map((g) => [g.at, g.items.map((i) => i.passage[0])]), [[3, [1, 2]], [60, [3]], [179, [4]]]);
});

test('a lecture draws a typed point under its own minute, not under the minute before', () => {
  const p = (seq, at) => [seq, { start_secs: at, end_secs: at + 5, text: String(seq), speaker: 'Microphone' }];
  const flow = L.transcriptFlow([p(1, 3), p(2, 50), p(3, 130)], [[9, { at_secs: 95, text: 'point' }]], ['Microphone']);
  assert.deepEqual(flow.groups.map((g) => [g.at, g.items.map((i) => (i.passage || i.point)[0])]), [[3, [1, 2]], [95, [9]], [130, [3]]]);
});

test('a live redraw keeps Pause, Stop and the wants box and changes Pause in place', () => {
  const node = (id, extra = {}) => {
    const attrs = { 'aria-label': 'Pause', title: 'Pause' };
    return { id, children: [], className: '', innerHTML: 'pause', value: '', setAttribute: (k, v) => { attrs[k] = v; }, getAttribute: (k) => attrs[k], attrs, contains(other) { return this === other || this.children.some((c) => c.contains(other)); }, ...extra };
  };
  const shown = [node('rec-pause'), node('rec-stop'), node('rec-wants-live', { value: 'typed' })];
  const shell = node('', { children: shown, className: 'rec rec-live' });
  const resumed = node('rec-pause', { innerHTML: 'resume' });
  resumed.attrs['aria-label'] = 'Resume';
  resumed.attrs.title = 'Resume';
  const freshShell = node('', { children: [resumed, node('rec-stop'), node('rec-wants-live', { value: 'server' })], className: 'rec rec-live paused' });
  const box = node('box', { children: [shell] });
  box.querySelectorAll = () => shown;
  box.querySelector = (at) => shown.find((n) => `#${n.id}` === at) || null;
  const made = node('made', { children: [freshShell] });
  made.querySelector = (at) => freshShell.children.find((n) => `#${n.id}` === at) || null;
  piece('graft', 'showTranscript', 'paintLive', { root: { document: { createElement: () => made, activeElement: shown[2] } }, drawLive: () => '' })(box);
  assert.deepEqual(shell.children, shown);
  assert.equal(shell.className, 'rec rec-live paused');
  assert.equal(shown[0].innerHTML, 'resume');
  assert.equal(shown[0].attrs['aria-label'], 'Resume');
  assert.equal(shown[2].value, 'typed', 'a focused wants box keeps what is being typed');
});
