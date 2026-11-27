'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { paneHelpers } = require('./page-source');
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
    const folds = new Map();
    const foldOf = (at) => {
      const opened = html.lastIndexOf('<p class="spoken echo-lines" hidden>', at);
      if (opened < 0 || html.lastIndexOf('</p>', at) > opened) return null;
      if (!folds.has(opened)) {
        const lines = { hidden: true, previousElementSibling: { classList: { contains: (c) => c === 'echo-fold' }, remove() { lines.unfolded = true; } } };
        folds.set(opened, lines);
      }
      return folds.get(opened);
    };
    ui.folds = folds;
    ui.areas = [...html.matchAll(/<span class="source-text" data-(passage|point)="(\d+)">([\s\S]*?)<\/span>/g)].map((m) => {
      const fold = foldOf(m.index);
      return {
        kind: m[1], seq: Number(m[2]), textContent: unescape(m[3]), dataset: { [m[1]]: m[2] }, scrolled: 0,
        addEventListener() {},
        closest: (selector) => (selector === '.echo-lines' ? fold : null),
        scrollIntoView() { this.scrolled += 1; this.shownWhenScrolled = !fold || !fold.hidden; },
      };
    });
    ui.rows = [...html.matchAll(/<div class="source-row (?:turn|minute)" data-text="([^"]*)">/g)].map((m) => ({ dataset: { text: unescape(m[1]) }, hidden: false }));
    ui.query = field(unescape((html.match(/<input id="source-query" value="([^"]*)"/) || [, ''])[1]));
    const pick = html.includes('id="source-pick"');
    ui.pick = pick ? field((html.match(/<option value="([^"]*)" selected>/) || [, ''])[1]) : null;
    const from = html.indexOf('<div id="source-found">') + 23;
    ui.found = { innerHTML: html.slice(from, html.indexOf('\n', from)).replace(/<\/div>$/, '') };
    ui.passages = { scrollIntoView(where) { ui.scrolledTo = where; } };
    ui.none = { hidden: true };
  };
  ui.layout = layout;
  const aside = {
    set innerHTML(html) { if (html) draw(html); else ui.open = false; },
    querySelectorAll(selector) {
      return selector === '[data-passage],[data-point]' ? ui.areas : selector === '.source-row' ? ui.rows : [];
    },
    querySelector: (selector) => ({ selector }),
    addEventListener() {},
  };
  const facts = { set innerHTML(html) { ui.facts = html; } };
  const find = (selector) => {
    if (selector === '.note-layout') return layout;
    if (selector === '.note-layout.aside-open') return layout.classes.has('aside-open') ? layout : null;
    if (selector === '#note-aside') return aside;
    if (selector === '#note-aside-facts') return facts;
    if (!ui.open) return null;
    const map = { '#source-query': ui.query, '#source-pick': ui.pick, '#source-found': ui.found, '#source-passages': ui.passages, '.source-none': ui.none };
    return map[selector] || null;
  };
  ui.chips = [];
  const document = {
    activeElement: null,
    getElementById: () => null,
    querySelectorAll: (selector) => (selector === '[data-action="view-original"]' ? ui.chips : []),
    querySelector(selector) {
      const m = selector.match(/^\[data-(passage|point)="(\d+)"\]$/);
      return m ? ui.areas.find((a) => a.kind === m[1] && a.seq === Number(m[2])) || null : find(selector);
    },
  };
  const context = {
    seq: 1, document, window: { leoRecording: globalThis.leoRecording, scrollY: 0, scrollTo(x, y) { ui.scrolled = [x, y]; } }, esc: escape, enc: encodeURIComponent, ICON: { chevron: '', search: '' },
    plural: (n, word) => `${n} ${n === 1 ? word : `${word}s`}`,
    $: (selector) => find(selector),
    api: eng.api,
    ready: async () => ({ note: { id: 'n1', title: 'Lecture' } }),
    toast: (message, options = {}) => ui.toasts.push({ message, bad: Boolean(options.bad) }),
    fail: (e) => ui.toasts.push({ message: e.message, bad: true }),
    arriveOnPage() {},
    sheet: () => ({ querySelector: () => null }),
    confirmSheet: (asked) => { ui.asked = asked; },
    sheetButtons: () => '',
    settingsSwitch: () => '',
    closeSheet() {},
    showNote: async () => {},
    md: { render: String },
    setTimeout: (fn) => ui.timers.push({ fn, live: true }),
    clearTimeout: (id) => { if (ui.timers[id - 1]) ui.timers[id - 1].live = false; },
    fillTextFile: async (box) => { ui.filled = box; },
    state: { view: 'note' }, app: {},
  };
  const uploads = read('uploads.js');
  const fileAside = uploads.slice(uploads.indexOf('async function openFileAside('), uploads.indexOf('async function fillTextFile('));
  vm.createContext(context);
  vm.runInContext(paneHelpers(), context);
  vm.runInContext(`${read('sources.js')}\n${fileAside}\nthis.page = () => sourcePage;`, context);
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
  assert.doesNotMatch(ui.html, /contenteditable|regenerate-wants|source-save|source-regenerate/, 'the transcript only shows: no editable lines, no wants box, no Save fixes or Write the note again');
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

test('the sheet\'s buttons run the paging functions', () => {
  const actions = read('actions.js');
  for (const [name, call] of [['source-earlier', 'sourceEarlier'], ['source-later', 'sourceLater'], ['source-match', 'sourceMatch']]) {
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
  assert.ok(ui.html.includes('<p class="spoken echo-lines" hidden><span class="typed-label">Speaker 1 · echo · </span>'), 'shown echo is labelled as the microphone side, not the turn it sits in');
  assert.ok(ui.html.includes('aria-label="Rename Speaker 2"'));
  assert.match(ui.facts, /^Transcript · 0:29 · /);
});

test('a match inside folded echo lines opens the fold before it is scrolled to', async () => {
  const eng = engine(meeting(), 50);
  const ui = screen(eng);
  await ui.context.showRecordingSources();
  await ui.context.sourceMatch('rec-cccccccc', '5');
  const row = ui.areas.find((a) => a.kind === 'passage' && a.seq === 5);
  assert.equal(row.scrolled, 1);
  assert.equal(row.shownWhenScrolled, true);
  const fold = row.closest('.echo-lines');
  assert.equal(fold.hidden, false);
  assert.equal(fold.unfolded, true);
});

test('Back to note puts the page where it was and focus on the original, for a PDF, a picture and a text file, and after a redraw', async () => {
  const chip = (name) => ({ isConnected: true, dataset: { action: 'view-original', name }, focused: 0, focus() { this.focused += 1; } });
  for (const [kind, name] of [['pdf', 'slides.pdf'], ['image', 'board.png'], ['text', 'notes.md']]) {
    const ui = screen(engine(recordings(), 50));
    const opener = chip(name);
    ui.context.document.activeElement = opener;
    ui.context.window.scrollY = 640;
    await ui.context.openFileAside(`/api/notes/n1/originals/${name}`, kind, name, null);
    assert.ok(ui.layout.classes.has('aside-open'), kind);
    assert.deepEqual(ui.scrolled, [0, 0], kind);
    ui.context.closeAside();
    assert.ok(!ui.layout.classes.has('aside-open'), kind);
    assert.deepEqual(ui.scrolled, [0, 640], kind);
    assert.equal(opener.focused, 1, kind);

    ui.context.document.activeElement = opener;
    await ui.context.openFileAside(`/api/notes/n1/originals/${name}`, kind, name, null);
    const held = ui.context.holdAside('n1');
    opener.isConnected = false;
    const drawn = chip(name);
    ui.chips = [chip('other.pdf'), drawn];
    ui.context.restoreAside(held);
    ui.context.closeAside();
    assert.deepEqual(ui.scrolled, [0, 640], `${kind} after a redraw`);
    assert.equal(drawn.focused, 1, `${kind} after a redraw`);
    assert.equal(opener.focused, 1, `${kind} after a redraw`);
  }
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

test('renaming a voice to a name it already shares asks to merge and sends the merge only when confirmed', async () => {
  const rows = meeting();
  rows[0].speakers = ['Rowan Ellery', 'Others 2'];
  for (const [, p] of rows[0].passages) p.speaker = p.speaker === 'Speaker 1' ? 'Rowan Ellery' : 'Others 2';
  const eng = engine(rows, 50);
  const ui = screen(eng);
  await ui.context.showRecordingSources();
  const before = eng.log.length;
  await ui.context.renameSpeaker('Others 2', 'rowan ellery');
  assert.equal(eng.log.length, before, 'nothing is sent before the merge is confirmed');
  assert.equal(ui.asked.title, 'Merge Others 2 into Rowan Ellery?');
  assert.equal(ui.asked.action, 'speaker-merge');
  assert.ok(ui.asked.data.includes('data-from="Others 2"') && ui.asked.data.includes('data-into="Rowan Ellery"'));
  await ui.context.mergeSpeaker('Others 2', 'Rowan Ellery');
  assert.deepEqual(eng.log[before].body, { source: 'rec-cccccccc', base: 1, rename: { from: 'Others 2', to: 'Rowan Ellery', merge: true } });
  assert.deepEqual(ui.toasts.at(-1), { message: 'Others 2 is now part of Rowan Ellery in this recording.', bad: false });
});

test('a lecture shows a paragraph a minute with its time and no speaker row', async () => {
  const rows = [{ id: 'rec-dddddddd', started: '2026-11-04T09:00:00Z', revision: 1, wants: '', speakers: ['Microphone'], points: [],
    passages: [[1, { start_secs: 2, end_secs: 30, text: 'first', speaker: 'Microphone' }], [2, { start_secs: 40, end_secs: 59, text: 'second', speaker: 'Microphone' }], [3, { start_secs: 61, end_secs: 80, text: 'third', speaker: 'Microphone' }]] }];
  const ui = screen(engine(rows, 50));
  await ui.context.showRecordingSources();
  assert.equal(ui.rows.length, 2);
  assert.ok(!ui.html.includes('speaker-row'));
  assert.ok(!ui.html.includes('turn-name'));
  assert.match(ui.html, /<span class="rec-at">0:02<\/span><div class="minute-said"><p class="spoken">[^]*first[^]*second[^]*<\/p><\/div>/);
  assert.match(ui.html, /<span class="rec-at">1:01<\/span>/);
});

test('Later scrolls the list to its start so a tall list still moves', async () => {
  const { ui } = await opened();
  await ui.context.sourceLater();
  assert.equal(ui.scrolledTo.block, 'start');
});

test('a redraw of the same note puts the transcript back', async () => {
  const { ui } = await opened();
  const held = ui.context.holdAside('n1');
  assert.equal(held.kind, 'transcript');
  ui.layout.classes.clear();
  ui.open = false;
  ui.context.restoreAside(held);
  assert.equal(ui.open, true);
  assert.equal(ui.context.holdAside('other'), null, 'another note\'s page is never held');
});
