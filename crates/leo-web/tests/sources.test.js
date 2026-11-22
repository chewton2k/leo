'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
require('../src/web/recording.js');

const app = path.join(__dirname, '../src/web/app');
const read = (file) => fs.readFileSync(path.join(app, file), 'utf8').replace(/\r\n/g, '\n');
const escape = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const unescape = (s) => s.replace(/&quot;/g, '"').replace(/&gt;/g, '>').replace(/&lt;/g, '<').replace(/&amp;/g, '&');

function recordings() {
  return [
    {
      id: 'rec-aaaaaaaa',
      started: '2026-11-01T09:00:00Z',
      revision: 1,
      wants: 'keep it short',
      passages: [1, 2, 3, 4, 5].map((seq) => [seq, { start_secs: seq * 10, end_secs: seq * 10 + 9, text: `first recording line ${seq}`, speaker: '' }]),
      points: [[7, { at_secs: 15, text: 'typed point seven' }], [9, { at_secs: 35, text: 'typed point nine' }]],
    },
    {
      id: 'rec-bbbbbbbb',
      started: '2026-11-02T09:00:00Z',
      revision: 1,
      wants: '',
      from: 'Other note',
      passages: [1, 2, 3].map((seq) => [seq, { start_secs: seq * 5, end_secs: seq * 5 + 4, text: seq === 3 ? 'the osmosis passage' : `second recording line ${seq}`, speaker: '' }]),
      points: [],
    },
  ];
}

function engine(rows, size = 2) {
  const log = [];
  const hooks = { failPut: null };
  const clone = (v) => JSON.parse(JSON.stringify(v));
  const head = (r, trace) => ({
    id: r.id, started: r.started, context: '', wants: r.wants, warnings: [], speakers: r.speakers || [], passages: r.passages.length, points: r.points.length, revision: r.revision,
    ...(r.from ? { from: r.from } : {}), ...(trace ? { trace: [] } : {}),
  });
  const take = (list, from) => {
    if (from === null) return { rows: [], next: null };
    const start = from === 0 ? 0 : list.findIndex(([seq]) => seq === from);
    if (start < 0) return { rows: [], next: null };
    const rest = list.slice(start);
    return { rows: rest.slice(0, size), next: rest.length > size ? rest[size][0] : null };
  };
  const cursor = (v) => (v === null ? 0 : v === 'end' ? null : Number(v));
  async function api(url, { method = 'GET', body } = {}) {
    log.push({ url, method, body: body && clone(body) });
    const u = new URL(url, 'http://leo');
    if (method === 'PUT') {
      if (hooks.failPut && hooks.failPut(body)) throw Object.assign(new Error(hooks.failPutSays || 'leo answered 500.'), { status: hooks.failPutStatus || 500 });
      const r = rows.find((x) => x.id === body.source);
      if (body.base !== r.revision) throw Object.assign(new Error('Could not save original notes'), { status: 409 });
      if (body.rename) {
        for (const [, passage] of r.passages) if (passage.speaker === body.rename.from) passage.speaker = body.rename.to;
        r.speakers = r.speakers.map((name) => (name === body.rename.from ? body.rename.to : name));
      }
      for (const c of body.changes || []) {
        const row = (c.kind === 'passage' ? r.passages : r.points).find(([seq]) => seq === c.seq);
        if (!row) throw Object.assign(new Error('Could not save original notes'), { status: 400 });
        row[1].text = c.text;
      }
      r.revision += 1;
      return { saved: true, revision: r.revision };
    }
    const q = (u.searchParams.get('q') || '').toLowerCase();
    const at = rows.find((r) => r.id === u.searchParams.get('at')) || rows[0];
    const p = take(at.passages, cursor(u.searchParams.get('passages')));
    const t = take(at.points, cursor(u.searchParams.get('points')));
    const matches = [];
    if (q) for (const r of rows) for (const [seq, passage] of r.passages) if (passage.text.toLowerCase().includes(q)) matches.push({ source: r.id, seq, score: 1, passage });
    return clone({
      list: rows.map((r) => head(r, false)),
      wants: rows.map((r) => r.wants).filter(Boolean).join('\n'),
      page: {
        head: head(at, true),
        passages: p.rows,
        points: t.rows,
        at: { passages: cursor(u.searchParams.get('passages')), points: cursor(u.searchParams.get('points')) },
        next: p.next === null && t.next === null ? null : { passages: p.next, points: t.next },
      },
      matches: matches.slice(0, 10),
    });
  }
  return { api, log, rows, hooks };
}

function screen(eng) {
  const ui = { open: false, areas: [], rows: [], toasts: [], timers: [], sheets: 0, facts: '' };
  const field = (value) => ({ value, listeners: {}, addEventListener(type, fn) { this.listeners[type] = fn; } });
  const layout = { dataset: {}, classes: new Set(), classList: { add(c) { layout.classes.add(c); }, remove(c) { layout.classes.delete(c); }, contains: (c) => layout.classes.has(c) } };
  const draw = (html) => {
    ui.open = true;
    ui.sheets += 1;
    ui.html = html;
    ui.areas = [...html.matchAll(/<span class="source-text" contenteditable="plaintext-only" spellcheck="true" data-(passage|point)="(\d+)">([\s\S]*?)<\/span>/g)].map((m) => ({
      kind: m[1], seq: Number(m[2]), textContent: unescape(m[3]), dataset: { [m[1]]: m[2] }, scrolled: 0,
      addEventListener() {}, scrollIntoView() { this.scrolled += 1; },
    }));
    ui.rows = [...html.matchAll(/<div class="source-row (?:turn|minute)" data-text="([^"]*)">/g)].map((m) => ({ dataset: { text: unescape(m[1]) }, hidden: false }));
    ui.query = field(unescape((html.match(/<input id="source-query" value="([^"]*)"/) || [, ''])[1]));
    const pick = html.includes('id="source-pick"');
    ui.pick = pick ? field((html.match(/<option value="([^"]*)" selected>/) || [, ''])[1]) : null;
    ui.wants = field(unescape((html.match(/<textarea id="regenerate-wants"[^>]*>([\s\S]*?)<\/textarea>/) || [, ''])[1]));
    const from = html.indexOf('<div id="source-found">') + 23;
    ui.found = { innerHTML: html.slice(from, html.indexOf('\n', from)).replace(/<\/div>$/, '') };
    ui.passages = { scrollIntoView() {} };
    ui.none = { hidden: true };
  };
  const aside = {
    set innerHTML(html) { if (html) draw(html); else ui.open = false; },
    querySelectorAll(selector) {
      return selector === '[data-passage],[data-point]' ? ui.areas : selector === '.source-row' ? ui.rows : [];
    },
    addEventListener() {},
  };
  const facts = { set innerHTML(html) { ui.facts = html; } };
  const find = (selector) => {
    if (selector === '.note-layout') return layout;
    if (selector === '.note-layout.aside-open') return layout.classes.has('aside-open') ? layout : null;
    if (selector === '#note-aside') return aside;
    if (selector === '#note-aside-facts') return facts;
    if (!ui.open) return null;
    const map = { '#source-query': ui.query, '#source-pick': ui.pick, '#source-found': ui.found, '#source-passages': ui.passages, '#regenerate-wants': ui.wants, '.source-none': ui.none };
    return map[selector] || null;
  };
  const document = {
    activeElement: null,
    getElementById: () => null,
    querySelector(selector) {
      const m = selector.match(/^\[data-(passage|point)="(\d+)"\]$/);
      return m ? ui.areas.find((a) => a.kind === m[1] && a.seq === Number(m[2])) || null : find(selector);
    },
  };
  const context = {
    seq: 1, document, window: { leoRecording: globalThis.leoRecording, scrollTo() {} }, esc: escape, enc: encodeURIComponent, ICON: { chevron: '', search: '' },
    plural: (n, word) => `${n} ${n === 1 ? word : `${word}s`}`,
    $: (selector) => find(selector),
    api: eng.api,
    ready: async () => ({ note: { id: 'n1', title: 'Lecture' } }),
    toast: (message, options = {}) => ui.toasts.push({ message, bad: Boolean(options.bad) }),
    fail: (e) => ui.toasts.push({ message: e.message, bad: true }),
    arriveOnPage() {},
    sheet: () => ({ querySelector: () => null }),
    sheetButtons: () => '',
    settingsSwitch: () => '',
    closeSheet() {},
    showNote: async () => {},
    md: { render: String },
    setTimeout: (fn) => ui.timers.push({ fn, live: true }),
    clearTimeout: (id) => { if (ui.timers[id - 1]) ui.timers[id - 1].live = false; },
    TextEncoder,
  };
  vm.createContext(context);
  vm.runInContext(`${read('sources.js')}\nthis.page = () => sourcePage;`, context);
  ui.context = context;
  ui.type = async (text) => {
    ui.query.value = text;
    ui.query.listeners.input();
    const live = ui.timers.filter((t) => t.live);
    for (const t of live) {
      t.live = false;
      await t.fn();
    }
  };
  ui.edit = (kind, seq, text) => {
    ui.areas.find((a) => a.kind === kind && a.seq === seq).textContent = text;
  };
  ui.shown = (kind) => ui.areas.filter((a) => a.kind === kind).map((a) => [a.seq, a.textContent]);
  return ui;
}

async function opened(size) {
  const eng = engine(recordings(), size);
  const ui = screen(eng);
  await ui.context.showRecordingSources();
  return { eng, ui };
}

test('opening a note with several recordings lists them and shows the first page of the first', async () => {
  const { eng, ui } = await opened();
  assert.equal(eng.log.length, 1);
  assert.equal(eng.log[0].url, '/api/notes/n1/recording?q=');
  assert.ok(ui.html.includes('id="source-pick"'));
  assert.equal((ui.html.match(/<option /g) || []).length, 2);
  assert.ok(ui.html.includes('<option value="rec-aaaaaaaa" selected>'));
  assert.deepEqual(ui.shown('passage'), [[1, 'first recording line 1'], [2, 'first recording line 2']]);
  assert.deepEqual(ui.shown('point'), [[7, 'typed point seven'], [9, 'typed point nine']]);
  assert.ok(ui.html.includes('data-action="source-later"'));
  assert.ok(!ui.html.includes('data-action="source-earlier"'));
  assert.equal(ui.wants.value, 'keep it short');
});

test('choosing another recording shows the first page of that one', async () => {
  const { eng, ui } = await opened();
  await ui.context.sourcePick('rec-bbbbbbbb');
  assert.equal(eng.log[1].url, '/api/notes/n1/recording?q=&at=rec-bbbbbbbb&passages=0&points=0');
  assert.deepEqual(ui.shown('passage'), [[1, 'second recording line 1'], [2, 'second recording line 2']]);
  assert.deepEqual(ui.shown('point'), []);
  assert.ok(ui.html.includes('<option value="rec-bbbbbbbb" selected>'));
  assert.ok(ui.html.includes('Recording 2 · from “Other note”'));
});

test('Later and Earlier ask for the neighbouring page with the cursor the engine gave', async () => {
  const { eng, ui } = await opened();
  await ui.context.sourceLater();
  assert.equal(eng.log[1].url, '/api/notes/n1/recording?q=&at=rec-aaaaaaaa&passages=3&points=end');
  assert.deepEqual(ui.shown('passage'), [[3, 'first recording line 3'], [4, 'first recording line 4']]);
  assert.deepEqual(ui.shown('point'), []);
  assert.ok(ui.html.includes('data-action="source-earlier"') && ui.html.includes('data-action="source-later"'));
  await ui.context.sourceLater();
  assert.equal(eng.log[2].url, '/api/notes/n1/recording?q=&at=rec-aaaaaaaa&passages=5&points=end');
  assert.deepEqual(ui.shown('passage'), [[5, 'first recording line 5']]);
  assert.ok(!ui.html.includes('data-action="source-later"'), 'the last page has no Later');
  await ui.context.sourceEarlier();
  assert.equal(eng.log[3].url, '/api/notes/n1/recording?q=&at=rec-aaaaaaaa&passages=3&points=end');
  assert.deepEqual(ui.shown('passage'), [[3, 'first recording line 3'], [4, 'first recording line 4']]);
  await ui.context.sourceEarlier();
  assert.equal(eng.log[4].url, '/api/notes/n1/recording?q=&at=rec-aaaaaaaa&passages=0&points=0');
  assert.deepEqual(ui.shown('point'), [[7, 'typed point seven'], [9, 'typed point nine']]);
  assert.ok(!ui.html.includes('data-action="source-earlier"'), 'the first page has no Earlier');
});

test('a search shows its matches across recordings and a match opens the page that holds it', async () => {
  const { eng, ui } = await opened();
  await ui.type('osmosis');
  assert.equal(eng.log[1].url, '/api/notes/n1/recording?q=osmosis&at=rec-aaaaaaaa&passages=0&points=0');
  assert.equal((ui.found.innerHTML.match(/class="btn plain source-match"/g) || []).length, 1);
  assert.ok(ui.found.innerHTML.includes('data-source="rec-bbbbbbbb" data-at="3"'));
  assert.ok(ui.found.innerHTML.includes('2 · '), 'the match names its recording');
  assert.ok(ui.found.innerHTML.includes('the osmosis passage'));
  await ui.context.sourceMatch('rec-bbbbbbbb', '3');
  assert.equal(eng.log[2].url, '/api/notes/n1/recording?q=osmosis&at=rec-bbbbbbbb&passages=3&points=0');
  assert.deepEqual(ui.shown('passage'), [[3, 'the osmosis passage']]);
  assert.ok(ui.html.includes('<option value="rec-bbbbbbbb" selected>'));
  assert.equal(ui.areas[0].scrolled, 1, 'the matching row is scrolled into view');
  assert.ok(ui.found.innerHTML.includes('data-at="3"'), 'the matches stay listed');
});

test('after jumping to a match in the middle of a recording the earlier lines are still reachable', async () => {
  const { eng, ui } = await opened();
  await ui.context.sourceMatch('rec-aaaaaaaa', '4');
  assert.deepEqual(ui.shown('passage'), [[4, 'first recording line 4'], [5, 'first recording line 5']]);
  assert.ok(ui.html.includes('data-action="source-earlier"'));
  await ui.context.sourceEarlier();
  assert.equal(eng.log.at(-1).url, '/api/notes/n1/recording?q=&at=rec-aaaaaaaa&passages=0&points=0');
  assert.deepEqual(ui.shown('passage'), [[1, 'first recording line 1'], [2, 'first recording line 2']]);
});

test('fixing two lines sends only those two lines with the revision and reloads the same page', async () => {
  const { eng, ui } = await opened();
  await ui.context.sourceLater();
  ui.edit('passage', 4, 'fixed four');
  ui.edit('passage', 3, 'fixed three');
  const before = eng.log.length;
  await ui.context.saveSource();
  const put = eng.log[before];
  assert.equal(put.method, 'PUT');
  assert.equal(put.url, '/api/notes/n1/recording');
  assert.deepEqual(put.body, { source: 'rec-aaaaaaaa', base: 1, changes: [{ kind: 'passage', seq: 3, text: 'fixed three' }, { kind: 'passage', seq: 4, text: 'fixed four' }] });
  assert.equal(eng.log.length, before + 2, 'one save and one reload of the same page');
  assert.equal(eng.log.at(-1).url, '/api/notes/n1/recording?q=&at=rec-aaaaaaaa&passages=3&points=end');
  assert.deepEqual(ui.shown('passage'), [[3, 'fixed three'], [4, 'fixed four']]);
  assert.equal(ui.context.page().page.head.revision, 2);
  assert.deepEqual(ui.toasts.at(-1), { message: 'Saved your fixes to “Lecture”', bad: false });
  assert.ok(ui.html.includes('data-action="source-earlier"'), 'the page is still the second page');
});

test('a fixed point is sent as a point, and nothing is sent when nothing changed', async () => {
  const { eng, ui } = await opened();
  await ui.context.saveSource();
  assert.deepEqual(ui.toasts.at(-1), { message: 'Nothing has changed.', bad: false });
  assert.equal(eng.log.length, 1);
  ui.edit('point', 9, 'typed point nine, fixed');
  await ui.context.saveSource();
  assert.deepEqual(eng.log[1].body.changes, [{ kind: 'point', seq: 9, text: 'typed point nine, fixed' }]);
});

test('a save refused because the revision moved keeps the fixes on the page and says the recording changed', async () => {
  const { eng, ui } = await opened();
  eng.rows[0].revision = 5;
  ui.edit('passage', 2, 'my fix');
  await ui.context.saveSource().catch(ui.context.fail);
  const said = ui.toasts.at(-1);
  assert.equal(said.bad, true);
  assert.match(said.message, /changed/);
  assert.match(said.message, /still/);
  assert.equal(ui.shown('passage')[1][1], 'my fix', 'the person\'s fix is still on the page');
  assert.equal(ui.sheets, 1, 'the transcript was not redrawn');
  assert.equal(eng.log.filter((c) => c.method === 'GET').length, 1, 'nothing was reloaded over the fix');
});

test('a save that fails part way can be tried again with the revision the saved part returned', async () => {
  const { eng, ui } = await opened();
  const big = 'x'.repeat(600000);
  ui.edit('passage', 1, big);
  ui.edit('passage', 2, `${big}!`);
  let puts = 0;
  eng.hooks.failPut = () => ++puts === 2;
  await ui.context.saveSource().catch(ui.context.fail);
  assert.equal(ui.toasts.at(-1).bad, true);
  assert.equal(eng.rows[0].revision, 2, 'the first batch was saved');
  eng.hooks.failPut = null;
  await ui.context.saveSource();
  const sent = eng.log.filter((c) => c.method === 'PUT');
  assert.deepEqual(sent.map((c) => c.body.base), [1, 2, 2, 3], 'the retry names the revision the first batch made');
  assert.deepEqual(ui.toasts.at(-1), { message: 'Saved your fixes to “Lecture”', bad: false });
  assert.equal(eng.rows[0].passages[1][1].text, `${big}!`);
});

test('switching recording, page or match with unsaved fixes keeps the fixes and the chosen recording', async () => {
  const { eng, ui } = await opened();
  ui.edit('passage', 1, 'my fix');
  const before = eng.log.length;
  ui.pick.value = 'rec-bbbbbbbb';
  await ui.context.sourcePick('rec-bbbbbbbb');
  assert.equal(ui.pick.value, 'rec-aaaaaaaa', 'the list goes back to the recording that is shown');
  assert.deepEqual(ui.toasts.at(-1), { message: 'Save your fixes first.', bad: false });
  await ui.context.sourceLater();
  await ui.context.sourceEarlier();
  await ui.context.sourceMatch('rec-bbbbbbbb', '3');
  assert.equal(eng.log.length, before, 'nothing was asked of the engine');
  assert.equal(ui.shown('passage')[0][1], 'my fix');
  assert.equal(ui.sheets, 1);
});

test('the sheet\'s buttons run the paging functions', () => {
  const actions = read('actions.js');
  for (const [name, call] of [['source-save', 'saveSource'], ['source-earlier', 'sourceEarlier'], ['source-later', 'sourceLater'], ['source-match', 'sourceMatch']]) {
    assert.ok(new RegExp(`'${name}': \\([a-z]*\\) => ${call}\\(`).test(actions), name);
  }
  assert.ok(/'source-match': \(el\) => sourceMatch\(el\.dataset\.source, el\.dataset\.at\)/.test(actions));
});

function meeting() {
  return [{
    id: 'rec-cccccccc',
    started: '2026-11-03T09:00:00Z',
    revision: 1,
    wants: '',
    speakers: ['Speaker 1', 'Speaker 2'],
    passages: [
      [1, { start_secs: 0, end_secs: 4, text: 'hello all', speaker: 'Speaker 1' }],
      [2, { start_secs: 5, end_secs: 9, text: 'next item', speaker: 'Speaker 1' }],
      [3, { start_secs: 10, end_secs: 14, text: 'sure', speaker: 'Speaker 2' }],
      [4, { start_secs: 15, end_secs: 19, text: 'sure', speaker: 'Speaker 1', echo: true }],
      [5, { start_secs: 16, end_secs: 19, text: 'it echoed', speaker: 'Speaker 1', echo: true }],
      [6, { start_secs: 20, end_secs: 29, text: 'back to me', speaker: 'Speaker 1' }],
    ],
    points: [[1, { at_secs: 6, text: 'my point' }]],
  }];
}

test('a meeting shows one turn per speaker with a tone dot, the typed point at its time and echo lines folded', async () => {
  const eng = engine(meeting(), 50);
  const ui = screen(eng);
  await ui.context.showRecordingSources();
  assert.equal(ui.rows.length, 3, 'three turns: Speaker 1, Speaker 2 with its echo, Speaker 1');
  const who = [...ui.html.matchAll(/<span class="turn-name">([^<]*)<\/span>/g)].map((m) => m[1]);
  assert.deepEqual(who, ['Speaker 1', 'Speaker 2', 'Speaker 1']);
  assert.ok(ui.html.includes('--tone: var(--hue-0)') && ui.html.includes('--tone: var(--hue-1)'));
  const first = ui.html.indexOf('hello all');
  const point = ui.html.indexOf('Your point · ');
  assert.ok(first < point && point < ui.html.indexOf('>sure<'), 'the point sits inside the first turn, after the line before it');
  assert.ok(ui.html.includes('Hidden: 2 echo lines · '));
  assert.ok(ui.html.includes('aria-label="Rename Speaker 2"'));
  assert.match(ui.facts, /^Transcript · 0:29 · /);
});

test('renaming a speaker sends the rename with the revision and reloads the same page', async () => {
  const eng = engine(meeting(), 50);
  const ui = screen(eng);
  await ui.context.showRecordingSources();
  const before = eng.log.length;
  await ui.context.renameSpeaker('Speaker 2', 'Sam');
  assert.deepEqual(eng.log[before].body, { source: 'rec-cccccccc', base: 1, rename: { from: 'Speaker 2', to: 'Sam' } });
  assert.ok(ui.html.includes('<span class="turn-name">Sam</span>'));
  assert.ok(ui.html.includes('aria-label="Rename Sam"'));
  assert.equal(ui.context.page().page.head.revision, 2);
  assert.deepEqual(ui.toasts.at(-1), { message: 'Speaker 2 is now Sam in this recording.', bad: false });
});

test('a lecture shows a paragraph a minute with its time and no speaker row', async () => {
  const rows = [{ id: 'rec-dddddddd', started: '2026-11-04T09:00:00Z', revision: 1, wants: '', speakers: ['Microphone'], points: [],
    passages: [[1, { start_secs: 2, end_secs: 30, text: 'first', speaker: 'Microphone' }], [2, { start_secs: 40, end_secs: 59, text: 'second', speaker: 'Microphone' }], [3, { start_secs: 61, end_secs: 80, text: 'third', speaker: 'Microphone' }]] }];
  const ui = screen(engine(rows, 50));
  await ui.context.showRecordingSources();
  assert.equal(ui.rows.length, 2);
  assert.ok(!ui.html.includes('speaker-row'));
  assert.ok(!ui.html.includes('turn-name'));
  assert.match(ui.html, /<span class="rec-at">0:02<\/span><div class="minute-said"><p class="said">[^]*first[^]*second[^]*<\/p><\/div>/);
  assert.match(ui.html, /<span class="rec-at">1:01<\/span>/);
});
