const { test, expect } = require('@playwright/test');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { signIn } = require('./sign-in');

const BYTES = Number(process.env.VERIFY_NOTE_BYTES || 200000);
const MARGIN_NOTES = Number(process.env.VERIFY_MARGIN_NOTES || 0);
const KEYS = 10;
const GROUPS = Number(process.env.VERIFY_UNDO_GROUPS || 200);

const median = (xs) => {
  const s = xs.filter((x) => Number.isFinite(x)).sort((a, b) => a - b);
  if (!s.length) return null;
  return +(s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2).toFixed(3);
};
const row = (browser, label, value) => console.log(`[typing-cost] ${JSON.stringify({ browser, bytes: BYTES, margin_notes: MARGIN_NOTES, label, value })}`);

const WORDS = 'the of and to in is that it for as with was on be by this are from or at an which not have has but they can one all their more when there been its into what some time other these than then two may only use each way such first well how over also'.split(' ');
function words(n, seed) {
  let x = (seed * 2654435761) % 4294967296 || 1;
  const out = [];
  for (let i = 0; i < n; i++) { x = (x * 1103515245 + 12345) % 2147483648; out.push(WORDS[x % WORDS.length]); }
  return out.join(' ');
}
function noteText(bytes) {
  const count = Math.ceil(bytes / 250);
  const blocks = [];
  for (let i = 0; i < count; i++) blocks.push(`${words(36, i + 1)} Unique passage number ${i}.`.slice(0, 248));
  return blocks.join('\n\n');
}
const passage = (i) => `Unique passage number ${i}.`;

function putNoteOnDisk(id, title, body) {
  const dir = path.join(process.env.LEO_BROWSER_HOME, 'notes');
  fs.mkdirSync(dir, { recursive: true });
  const at = '2026-01-01T00:00:00Z';
  fs.writeFileSync(path.join(dir, `${title}.md`), `---\nid: ${id}\ntitle: ${title}\ncreated_at: ${at}\nupdated_at: ${at}\n---\n\n${body}`);
}

async function lookAtDisk(page, id) {
  const poke = await page.request.post('/api/notes', { data: { title: 'Typing cost nudge', body: 'x' } });
  if (poke.status() !== 201) throw new Error(`POST /api/notes ${poke.status()}`);
  const r = await page.request.get(`/api/notes/${id}`);
  if (r.status() !== 200) throw new Error(`the note put on disk is not served: GET /api/notes/${id} ${r.status()}`);
  return r.json();
}

const SERVED = [
  ['/doc.js', [
    ['function changed() {', 'function changed() { return __mark("doc.changed", () => __changed()); }\n    function __changed() {'],
    ['function lineStart(line) {', 'function lineStart(line) { return __mark("lineStart", () => __lineStart(line)); }\n    function __lineStart(line) {'],
    ['function lineOf(at) {', 'function lineOf(at) { return __mark("lineOf", () => __lineOf(at)); }\n    function __lineOf(at) {'],
    ['function outlineDraw(force = false) {', 'function outlineDraw(force = false) { return __mark("outlineDraw", () => __outlineDraw(force)); }\n    function __outlineDraw(force = false) {'],
    ['ed.listPrefix(editing.prefix + area.value, lines.slice(0, start))', '__mark("listPrefix with the lines above", () => ed.listPrefix(editing.prefix + area.value, lines.slice(0, start)))'],
  ]],
  ['/app.js', [
    ['function snapshot(s) {', 'function snapshot(s) { return __mark("snapshot", () => __snapshot(s)); }\nfunction __snapshot(s) {'],
    ['function changed(s) { saving.changed(s, snapshot(s)); }', 'function changed(s) { __mark("app changed (snapshot, saving.changed)", () => saving.changed(s, snapshot(s))); }'],
    ['function marginDraw() {', 'function marginDraw() { return __mark("marginDraw", () => __marginDraw()); }\nfunction __marginDraw() {'],
    ['function readSoon(text) {', 'function readSoon(text) { return __mark("readSoon", () => __readSoon(text)); }\nfunction __readSoon(text) {'],
  ]],
];

async function instrument(page) {
  for (const [url, edits] of SERVED) {
    await page.route(`**${url}`, async (route) => {
      const response = await route.fetch();
      let body = await response.text();
      for (const [from, to] of edits) {
        if (!body.includes(from)) throw new Error(`${url} no longer has: ${from}`);
        body = body.replace(from, to);
      }
      await route.fulfill({ response, body });
    });
  }
  await page.addInitScript(() => {
    window.__phase = null;
    window.__marks = {};
    window.__long = [];
    window.__mark = (name, fn) => {
      const phase = window.__phase;
      if (!phase) return fn();
      const t = performance.now();
      try { return fn(); } finally {
        const k = `${phase}: ${name}`;
        (window.__marks[k] = window.__marks[k] || []).push(performance.now() - t);
      }
    };
    const hook = (name, wrap) => {
      let value;
      Object.defineProperty(window, name, { configurable: true, get: () => value, set: (v) => { value = v; wrap(v); } });
    };
    hook('leoMarkdown', (md) => { const f = md.readTime; md.readTime = (...a) => window.__mark('md.readTime (reading-time line)', () => f.apply(md, a)); });
    hook('leoEditing', (ed) => {
      for (const n of ['headings', 'headingTree']) { const f = ed[n]; ed[n] = (...a) => window.__mark(`ed.${n} (outline)`, () => f.apply(ed, a)); }
    });
    hook('leoSaving', (kit) => {
      const create = kit.create;
      kit.create = (...a) => {
        const m = create.apply(kit, a);
        for (const n of ['changed', 'flush']) { const f = m[n]; m[n] = (...b) => window.__mark(`saving.${n}`, () => f.apply(m, b)); }
        return m;
      };
    });
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = function (k, v) {
      return window.__mark(`localStorage.setItem ${String(v).length} chars`, () => set.call(this, k, v));
    };
    const stringify = JSON.stringify;
    JSON.stringify = function (...a) { return window.__mark('JSON.stringify', () => stringify.apply(this, a)); };
    const parse = JSON.parse;
    JSON.parse = function (...a) { return window.__mark('JSON.parse', () => parse.apply(this, a)); };
    new PerformanceObserver((l) => { for (const e of l.getEntries()) window.__long.push({ phase: window.__phase, ms: e.duration }); }).observe({ entryTypes: ['longtask'] });
  });
}

async function makeMarginNotes(page, id, want, version) {
  let made = 0;
  const ids = [];
  const make = async (count, v) => {
    for (let i = 0; i < count; i++) {
      const r = await page.request.post(`/api/notes/${id}/margin/threads`, { data: { version: v, quote: passage(Math.floor(((made + 0.5) * Math.ceil(BYTES / 250)) / 48)), question: 'Why?' } });
      if (!r.ok()) throw new Error(`thread ${r.status()} ${await r.text()}`);
      ids.push((await r.json()).id);
      made++;
    }
  };
  await make(Math.min(want, 24), version);
  if (want > 24) {
    for (const t of ids.splice(0)) {
      const r = await page.request.post(`/api/notes/${id}/margin/threads/${t}`, { data: { status: 'resolved' } });
      if (!r.ok()) throw new Error(`resolve ${r.status()}`);
    }
    await make(want - 24, (await (await page.request.get(`/api/notes/${id}`)).json()).version);
  }
}

test('typing cost C4: every step of a keystroke in a large note, then what runs after typing stops', async ({ page, browserName }) => {
  test.setTimeout(3600000);
  const id = crypto.randomUUID();
  const title = `Typing cost ${BYTES}`;
  await instrument(page);
  await signIn(page);
  await page.setViewportSize({ width: 1600, height: 900 });
  putNoteOnDisk(id, title, noteText(BYTES));
  const note = await lookAtDisk(page, id);
  row(browserName, 'note on disk', { bytes: note.body.length });
  if (MARGIN_NOTES) await makeMarginNotes(page, id, MARGIN_NOTES, note.version);

  const patches = [];
  page.on('requestfinished', async (req) => {
    if (req.method() !== 'PATCH' || !req.url().includes(`/api/notes/${id}`)) return;
    const res = await req.response();
    patches.push({ status: res ? res.status() : null, ms: +(req.timing().responseEnd).toFixed(1), body_chars: (req.postData() || '').length });
  });

  await page.goto(`/#/n/${id}`);
  await expect(page.locator('#doc .blk').first()).toBeVisible({ timeout: 120000 });
  if (MARGIN_NOTES) await expect(page.locator('.margin-note')).toHaveCount(Math.min(MARGIN_NOTES, 24), { timeout: 60000 });
  await page.waitForTimeout(1500);
  await page.locator('#doc .blk').last().click();
  await page.keyboard.press('End');
  await page.evaluate(() => {
    window.__idle = () => new Promise((done) => { const c = new MessageChannel(); c.port1.onmessage = () => done(performance.now()); c.port2.postMessage(0); });
  });
  const cdp = browserName === 'chromium' ? await page.context().newCDPSession(page) : null;
  if (cdp) await cdp.send('Performance.enable');
  const metrics = async () => (cdp ? Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value])) : null);

  const keyRuns = [];
  for (let r = 0; r < KEYS + 3; r++) {
    await page.evaluate(() => { window.__marks = {}; window.__phase = null; });
    const wait = page.evaluate(() => new Promise((done) => {
      window.addEventListener('keydown', (e) => {
        const start = e.timeStamp;
        window.__phase = 'key';
        requestAnimationFrame(() => {
          const raf = performance.now();
          const c = new MessageChannel();
          c.port1.onmessage = () => { const end = performance.now(); window.__idle().then((idle) => { window.__phase = 'after'; done({ toRaf: raf - start, toFrameEnd: end - start, toIdle: idle - start }); }); };
          c.port2.postMessage(0);
        });
      }, { once: true, capture: true });
    }));
    await page.waitForTimeout(30);
    const before = await metrics();
    await page.keyboard.press('x');
    const t = await wait;
    await page.waitForTimeout(650);
    const after = await metrics();
    const marks = await page.evaluate(() => { const m = window.__marks; window.__phase = null; return Object.fromEntries(Object.entries(m).map(([k, v]) => [k, { calls: v.length, ms: v.reduce((a, b) => a + b, 0) }])); });
    const long = await page.evaluate(() => { const l = window.__long; window.__long = []; return l; });
    const cost = before && after ? { script_ms: (after.ScriptDuration - before.ScriptDuration) * 1000, layout_ms: (after.LayoutDuration - before.LayoutDuration) * 1000, layouts: after.LayoutCount - before.LayoutCount, styles: after.RecalcStyleCount - before.RecalcStyleCount } : null;
    if (r >= 3) keyRuns.push({ ...t, marks, long, cost });
    await page.waitForTimeout(250);
  }
  const names = [...new Set(keyRuns.flatMap((x) => Object.keys(x.marks)))].sort();
  for (const name of names) row(browserName, `step ${name}`, { median_ms: median(keyRuns.map((x) => x.marks[name]?.ms ?? 0)), median_calls: median(keyRuns.map((x) => x.marks[name]?.calls ?? 0)) });
  row(browserName, 'keydown to next frame (rAF)', { runs: keyRuns.map((x) => +x.toRaf.toFixed(1)), median: median(keyRuns.map((x) => x.toRaf)) });
  row(browserName, 'keydown to frame end', { runs: keyRuns.map((x) => +x.toFrameEnd.toFixed(1)), median: median(keyRuns.map((x) => x.toFrameEnd)) });
  row(browserName, 'keydown to main thread idle', { runs: keyRuns.map((x) => +x.toIdle.toFixed(1)), median: median(keyRuns.map((x) => x.toIdle)) });
  const long = keyRuns.flatMap((x) => x.long);
  row(browserName, 'long tasks (50 ms or more)', { supported: await page.evaluate(() => PerformanceObserver.supportedEntryTypes.includes('longtask')), count: long.length, in_key: long.filter((l) => l.phase === 'key').length, in_after: long.filter((l) => l.phase === 'after').length, longest_ms: long.length ? +Math.max(...long.map((l) => l.ms)).toFixed(1) : 0 });
  if (cdp) row(browserName, 'browser metrics per keystroke and the 650 ms after it', { script_ms: median(keyRuns.map((x) => x.cost.script_ms)), layout_ms: median(keyRuns.map((x) => x.cost.layout_ms)), layouts: median(keyRuns.map((x) => x.cost.layouts)), styles: median(keyRuns.map((x) => x.cost.styles)) });
  await page.waitForTimeout(3000);
  row(browserName, 'autosave PATCH round trips (413 until the body limit is removed)', patches);
});

test('typing cost heap: whole-copy undo after typing groups separated by pauses', async ({ page, browserName }) => {
  test.setTimeout(3600000);
  const id = crypto.randomUUID();
  await page.addInitScript(() => {
    window.__skew = 0;
    const now = Date.now.bind(Date);
    Date.now = () => now() + window.__skew;
  });
  await signIn(page);
  await page.setViewportSize({ width: 1600, height: 900 });
  putNoteOnDisk(id, `Typing cost heap ${BYTES}`, noteText(BYTES));
  await lookAtDisk(page, id);
  await page.goto(`/#/n/${id}`);
  await expect(page.locator('#doc .blk').first()).toBeVisible({ timeout: 120000 });
  await page.waitForTimeout(1500);
  await page.locator('#doc .blk').last().click();
  await page.keyboard.press('End');
  const cdp = browserName === 'chromium' ? await page.context().newCDPSession(page) : null;
  const heap = async () => {
    if (!cdp) return null;
    await cdp.send('HeapProfiler.collectGarbage');
    return page.evaluate(() => performance.memory.usedJSHeapSize);
  };
  const before = await heap();
  for (let i = 0; i < GROUPS; i++) {
    await page.evaluate(() => { window.__skew += 1100; });
    await page.keyboard.press('x');
  }
  await page.waitForTimeout(1500);
  const after = await heap();
  row(browserName, 'undo heap', before === null
    ? 'owed by hand: WebKit has no performance.memory; take a Web Inspector heap snapshot before and after the same 200 groups'
    : { groups: GROUPS, before, after, grew_bytes: after - before, per_group_bytes: Math.round((after - before) / GROUPS), per_group_times_note: +((after - before) / GROUPS / BYTES).toFixed(2) });
});

test('typing cost storage: what the browser keeps for a draft', async ({ page, browserName }) => {
  test.setTimeout(900000);
  await signIn(page);
  const found = await page.evaluate(async () => {
    const accepts = (n) => {
      try { localStorage.setItem('typing-cost-probe', 'x'.repeat(n)); return true; } catch (_) { return false; } finally { localStorage.removeItem('typing-cost-probe'); }
    };
    const draftAccepts = (n) => {
      const body = 'x'.repeat(n);
      try { localStorage.setItem('typing-cost-probe', JSON.stringify({ note: { id: 'a', title: 'T', body, tags: [], directory: '', version: 'v' }, edit: { title: 'T', body, tags: [] } })); return true; } catch (_) { return false; } finally { localStorage.removeItem('typing-cost-probe'); }
    };
    const largest = (ok) => { let lo = 0; let hi = 64 * 1024 * 1024; while (lo < hi) { const mid = Math.ceil((lo + hi) / 2); if (ok(mid)) lo = mid; else hi = mid - 1; } return lo; };
    const out = { empty_origin_chars: largest(accepts), draft_body_chars_today: largest(draftAccepts) };
    for (let i = 0; i < 10; i++) localStorage.setItem(`leo-draft-v1:typing-cost-other-${i}`, 'y'.repeat(100000));
    out.ten_other_drafts_chars = largest(accepts);
    out.ten_other_drafts_body_chars_today = largest(draftAccepts);
    for (let i = 0; i < 10; i++) localStorage.removeItem(`leo-draft-v1:typing-cost-other-${i}`);
    out.throws = { two_mb: !accepts(2000000), twenty_mb: !accepts(20000000) };
    const idb = [];
    for (const n of [2000000, 20000000]) {
      const result = { chars: n };
      try {
        const db = await new Promise((ok, no) => { const q = indexedDB.open('typing-cost-probe', 1); q.onupgradeneeded = () => q.result.createObjectStore('drafts'); q.onsuccess = () => ok(q.result); q.onerror = () => no(q.error); });
        const body = 'x'.repeat(n);
        const tx = db.transaction('drafts', 'readwrite');
        const t0 = performance.now();
        tx.objectStore('drafts').put({ note: { id: 'a', title: 'T', body: '', tags: [], directory: '', version: 'v' }, edit: { title: 'T', body, tags: [] } }, 'k');
        result.put_call_ms = +(performance.now() - t0).toFixed(2);
        await new Promise((ok, no) => { tx.oncomplete = ok; tx.onerror = () => no(tx.error); tx.onabort = () => no(tx.error); });
        result.confirmed_ms = +(performance.now() - t0).toFixed(2);
        result.failed = false;
        db.close();
      } catch (error) {
        result.failed = true;
        result.error = String(error && error.name);
      }
      idb.push(result);
    }
    await new Promise((ok) => { const q = indexedDB.deleteDatabase('typing-cost-probe'); q.onsuccess = q.onerror = q.onblocked = () => ok(); });
    out.indexeddb = idb;
    return out;
  });
  row(browserName, 'storage', found);
  expect(found.empty_origin_chars).toBeGreaterThan(0);
});
