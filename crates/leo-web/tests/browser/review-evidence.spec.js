const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');
const { token } = require('./sign-in');

const EVIDENCE_ROOT = process.env.LEO_EVIDENCE_DIR || path.join(__dirname, 'test-results', 'review-evidence');
let OUT = EVIDENCE_ROOT;
let MANIFEST = path.join(OUT, 'review-evidence.json');
const WIDTHS = [[1440, 900], [900, 800], [390, 844]];
const SCHEMES = ['light', 'dark'];
const QUOTE = 'just checks if it matches';
const ANSWER = 'In plain text? If this table leaks, every account goes.';
const SEEDED = `Signup flow notes.\n\nThe login screen asks for an email and a password.\n\nThen login ${QUOTE}, I think.`;
const EDIT = ' Probably fine.';

let counter = 0;
const unique = () => `${Date.now().toString(36)}${counter++}`;

function manifest() {
  try {
    return JSON.parse(fs.readFileSync(MANIFEST, 'utf8'));
  } catch (e) {
    return { entries: {}, files: {} };
  }
}

function record(key, value) {
  fs.mkdirSync(OUT, { recursive: true });
  const all = manifest();
  all.updated = new Date().toISOString();
  all.entries[key] = value;
  fs.writeFileSync(MANIFEST, JSON.stringify(all, null, 2));
}

function remember(name, file) {
  const all = manifest();
  all.files[name] = file;
  fs.mkdirSync(OUT, { recursive: true });
  fs.writeFileSync(MANIFEST, JSON.stringify(all, null, 2));
  return file;
}

async function still(page, name, options = {}) {
  fs.mkdirSync(OUT, { recursive: true });
  const file = path.join(OUT, `${name}.png`);
  await page.screenshot({ path: file, scale: 'css', ...options });
  return remember(name, file);
}

const env = (page, extra = {}) => ({
  browserName: test.info().project.name,
  browserVersion: page.context().browser().version(),
  viewport: page.viewportSize(),
  ...extra,
});

async function settle(page) {
  await page.evaluate(() => Promise.race([
    Promise.allSettled(document.getAnimations().map((run) => run.finished)),
    new Promise((done) => setTimeout(done, 2500)),
  ]));
}

const frame = (page) => page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));

async function signedIn(browser, baseURL, options = {}) {
  const context = await browser.newContext({ baseURL, ...options });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(String(error)));
  await page.goto(`/?token=${token()}`);
  await expect(page.locator('#app')).not.toBeEmpty();
  return { context, page, errors };
}

async function createNote(page, title, body, directory = '') {
  const response = await page.request.post('/api/notes', { data: { title, body, directory } });
  expect(response.ok()).toBe(true);
  return response.json();
}

test.beforeEach(async ({ page }, testInfo) => {
  OUT = path.join(EVIDENCE_ROOT, testInfo.project.name);
  MANIFEST = path.join(OUT, 'review-evidence.json');
  await page.goto(`/?token=${token()}`);
  await expect(page.locator('#app')).not.toBeEmpty();
});

const positions = (page) => page.evaluate(() => {
  const list = document.querySelector('#chat-pick .chat-pick-list');
  const body = document.querySelector('#chat .chat-body');
  const input = document.querySelector('#chat-input');
  const tree = document.querySelector('#side .side-tree');
  return {
    page: document.scrollingElement.scrollTop,
    chat: body ? body.scrollTop : null,
    list: list ? list.scrollTop : null,
    tree: tree ? tree.scrollTop : null,
    focus: document.activeElement && document.activeElement.id,
    caret: [input.selectionStart, input.selectionEnd],
    zoom: window.visualViewport.scale,
    width: innerWidth,
  };
});

test('R1 the real Felix @ picker scrolls on its own at both ends and in the middle', async ({ page }, testInfo) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1280, height: 760 });
  const tag = unique();
  for (let i = 0; i < 14; i++) await createNote(page, `Evidence pick ${tag} ${String(i).padStart(2, '0')}`, 'Picker fixture.');
  const long = await createNote(page, `Evidence long ${tag}`, Array.from({ length: 140 }, (_, i) => `Reading paragraph ${i} for the picker evidence.`).join('\n\n'));
  const sent = [];
  const answer = Array.from({ length: 40 }, (_, i) => `Line ${i} of a long answer that makes the transcript scroll.`).join('\n\n');
  await page.route('**/api/chat', (route) => {
    if (route.request().method() !== 'POST') return route.continue();
    sent.push(route.request().postDataJSON());
    return route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { t: answer }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' });
  });
  await page.goto(`/#/n/${long.id}`);
  await expect(page.locator('#title')).toHaveText(long.title);
  await page.locator('#chat-toggle').click();
  const input = page.locator('#chat-input');
  await input.fill('First question');
  await input.press('Enter');
  await expect(page.locator('#chat .msg.leo').last()).toContainText('Line 39', { timeout: 15000 });
  await input.fill('Second question');
  await input.press('Enter');
  await expect.poll(() => sent.length).toBe(2);
  await expect(page.locator('#chat .msg.leo')).toHaveCount(2, { timeout: 15000 });
  const posted = sent.length;
  await page.evaluate(() => scrollTo(0, 900));
  await input.click();
  await input.pressSequentially('Compare @');
  const rows = page.locator('#chat-pick .chat-pick-row');
  await expect.poll(() => rows.count(), { timeout: 10000 }).toBeGreaterThanOrEqual(6);
  const geometry = await page.evaluate(() => {
    const list = document.querySelector('#chat-pick .chat-pick-list');
    const body = document.querySelector('#chat .chat-body');
    body.scrollTop = Math.floor((body.scrollHeight - body.clientHeight) / 2);
    return { overflow: list.scrollHeight - list.clientHeight, chatOverflow: body.scrollHeight - body.clientHeight, pageOverflow: document.scrollingElement.scrollHeight - innerHeight, pageTop: document.scrollingElement.scrollTop };
  });
  expect(geometry.overflow, 'fixture: the picker list must overflow').toBeGreaterThan(40);
  expect(geometry.chatOverflow, 'fixture: the transcript must scroll').toBeGreaterThan(100);
  expect(geometry.pageTop, 'fixture: the note must be scrolled partway').toBeGreaterThan(300);
  const list = page.locator('#chat-pick .chat-pick-list');
  const box = await list.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const log = {};
  const same = (a, b, keys) => keys.every((key) => JSON.stringify(a[key]) === JSON.stringify(b[key]));
  const outside = ['page', 'chat', 'tree', 'focus', 'caret', 'zoom', 'width'];
  await still(page, 'r1-picker-before');

  await list.evaluate((el) => { el.scrollTop = 0; });
  let before = await positions(page);
  await page.mouse.wheel(0, 70);
  await page.waitForTimeout(250);
  let after = await positions(page);
  log.middle = { before, after };
  expect(after.list, 'wheel in the middle moves the picker').toBeGreaterThan(before.list);
  expect(same(before, after, outside), `middle wheel left the note, transcript, focus and caret alone: ${JSON.stringify({ before, after })}`).toBe(true);

  await list.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  before = await positions(page);
  for (let i = 0; i < 3; i++) await page.mouse.wheel(0, 320);
  await page.waitForTimeout(250);
  after = await positions(page);
  log.bottom = { before, after };
  expect(same(before, after, [...outside, 'list']), `bottom boundary wheel moved something: ${JSON.stringify({ before, after })}`).toBe(true);

  await list.evaluate((el) => { el.scrollTop = 0; });
  before = await positions(page);
  for (let i = 0; i < 3; i++) await page.mouse.wheel(0, -320);
  await page.waitForTimeout(250);
  after = await positions(page);
  log.top = { before, after };
  expect(same(before, after, [...outside, 'list']), `top boundary wheel moved something: ${JSON.stringify({ before, after })}`).toBe(true);

  const zoom = await page.evaluate(() => {
    const list = document.querySelector('#chat-pick .chat-pick-list');
    const verdict = {};
    for (const [name, top] of [['top', 0], ['bottom', list.scrollHeight]]) {
      list.scrollTop = top;
      const event = new WheelEvent('wheel', { deltaY: name === 'top' ? -200 : 200, ctrlKey: true, bubbles: true, cancelable: true });
      list.dispatchEvent(event);
      verdict[name] = event.defaultPrevented;
    }
    return verdict;
  });
  log.ctrlWheelPrevented = zoom;
  expect(zoom, 'ctrl+wheel (zoom) is never cancelled by the picker').toEqual({ top: false, bottom: false });

  await list.evaluate((el) => { el.scrollTop = 0; });
  await input.focus();
  before = await positions(page);
  const count = await rows.count();
  for (let i = 0; i < count - 1; i++) await page.keyboard.press('ArrowDown');
  await expect(rows.last()).toHaveClass(/\bon\b/);
  await frame(page);
  after = await positions(page);
  const active = await page.evaluate(() => {
    const list = document.querySelector('#chat-pick .chat-pick-list');
    const rows = [...list.querySelectorAll('.chat-pick-row')];
    const on = list.querySelector('.chat-pick-row.on');
    const a = on.getBoundingClientRect(), b = list.getBoundingClientRect();
    return { index: rows.indexOf(on), count: rows.length, inside: a.top >= b.top - 1 && a.bottom <= b.bottom + 1 };
  });
  log.keyboard = { before, after, active };
  expect(active.index).toBe(count - 1);
  expect(active.inside, 'the highlighted row is scrolled into the list').toBe(true);
  expect(after.list).toBeGreaterThan(0);
  expect(same(before, after, outside), `keyboard selection left the note, transcript, focus and caret alone: ${JSON.stringify({ before, after })}`).toBe(true);
  await still(page, 'r1-picker-keyboard-end');

  const title = await page.locator('#chat-pick .chat-pick-row.on b').innerText();
  await page.keyboard.press('Enter');
  await expect(page.locator('#chat-refs .note-card')).toContainText(title);
  await expect(page.locator('#chat-pick')).toBeHidden();
  expect(sent.length, 'choosing a note does not send the message').toBe(posted);
  log.chosen = title;
  record('R1', { ...env(page), log, artifacts: ['r1-picker-before', 'r1-picker-keyboard-end'] });
});

async function prepareAlong(page, title) {
  const note = await createNote(page, title, SEEDED);
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#title')).toHaveText(title);
  await page.locator('#margin-mode').click();
  await page.getByRole('menuitemradio', { name: 'Reads along' }).click();
  await expect(page.locator('#margin-mode')).toHaveText('Reads along');
  await page.reload();
  await expect(page.locator('#margin-mode')).toHaveText('Reads along');
  return note;
}

async function startRead(page, note) {
  const timeline = { marks: {} };
  const mark = (name) => { timeline.marks[name] = Date.now(); };
  let release;
  const held = new Promise((done) => { release = done; });
  let requested = null;
  await page.route(`**/api/notes/${note.id}/margin/read`, async (route) => {
    if (route.request().method() !== 'POST') return route.continue();
    requested = route.request().postDataJSON();
    mark('requested');
    await held;
    return route.continue();
  });
  const block = page.locator('#doc .blk').last();
  await block.click();
  const area = page.locator('#doc textarea.line-edit');
  await area.press('End');
  mark('editStart');
  await area.pressSequentially(EDIT, { delay: 30 });
  mark('typed');
  const caret = await area.evaluate((el) => [el.selectionStart, el.selectionEnd]);
  timeline.caret = caret;
  return { timeline, mark, release: () => { timeline.marks.released = Date.now(); release(); }, requested: () => requested, area };
}

async function installSampler(page) {
  await page.evaluate(() => {
    window.__ink = [];
    window.__inkOn = true;
    const t0 = performance.now();
    const inside = (target) => target && target.closest && target.closest('.margin-ink, .margin-note, .margin-layer');
    const tick = () => {
      const marks = [...document.querySelectorAll('.margin-ink .squiggle')];
      const animations = document.getAnimations().filter((a) => a.effect && inside(a.effect.target));
      window.__ink.push({
        t: performance.now() - t0,
        at: Date.now(),
        marks: marks.length,
        rects: marks.map((el) => { const r = el.getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; }),
        clip: marks.map((el) => getComputedStyle(el).clipPath),
        dash: marks.map((el) => getComputedStyle(el).strokeDasharray),
        notes: document.querySelectorAll('.margin-note').length,
        words: document.querySelectorAll('.margin-note .ink-word').length,
        status: (document.querySelector('#margin-status') || {}).textContent || '',
        waiting: Boolean(document.querySelector('.margin-waiting')),
        animations: animations.map((a) => {
          const target = a.effect.target;
          const klass = target.className && target.className.baseVal !== undefined ? target.className.baseVal : String(target.className);
          return { name: a.animationName || a.transitionProperty || '', time: a.currentTime, duration: a.effect.getTiming().duration, delay: a.effect.getTiming().delay, state: a.playState, klass, ink: Boolean(target.closest?.('.margin-ink')) };
        }),
      });
      if (window.__inkOn) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

const stopSampler = (page) => page.evaluate(() => { window.__inkOn = false; return window.__ink; });

async function noMascot(page) {
  for (const selector of ['.margin-layer .felix', '.margin-note .felix', '.margin-mark .felix', '.margin-reading .felix', '#doc .felix', '.margin-note svg.felix']) {
    expect(await page.locator(selector).count(), `no Felix mascot at ${selector}`).toBe(0);
  }
}

test('R2/R14 a real edit in Reads along pauses, shows Reading, then reveals the underline and answer', async ({ browser, baseURL }, testInfo) => {
  test.setTimeout(120000);
  fs.mkdirSync(path.join(OUT, 'video'), { recursive: true });
  const { context, page, errors } = await signedIn(browser, baseURL, { viewport: { width: 1600, height: 900 }, recordVideo: { dir: path.join(OUT, 'video'), size: { width: 1600, height: 900 } } });
  const note = await prepareAlong(page, `Evidence along ${unique()}`);
  await still(page, 'r2-before-edit');
  const read = await startRead(page, note);
  const scrollBefore = await page.evaluate(() => scrollY);
  await page.waitForTimeout(1500);
  expect(read.requested(), 'no read request during the pause').toBeNull();
  await still(page, 'r2-pause');
  const statusDuringPause = await page.locator('#margin-status').innerText();
  await expect.poll(() => read.requested(), { timeout: 12000, message: 'the debounced read request reached the engine route' }).not.toBeNull();
  const pauseMs = read.timeline.marks.requested - read.timeline.marks.typed;
  expect(pauseMs, `the read waited out a real pause (${pauseMs} ms), not an immediate Ask`).toBeGreaterThan(2500);
  expect(pauseMs).toBeLessThan(10500);
  expect(read.requested().blocks.join(' ')).toContain(QUOTE);
  await expect(page.locator('.margin-waiting')).toBeVisible({ timeout: 5000 });
  read.mark('reading');
  await still(page, 'r2-reading');
  await installSampler(page);
  await expect.poll(() => page.evaluate(() => window.__ink.some((sample) => sample.waiting)), { message: 'the sampler saw the reading dots' }).toBe(true);
  const clip = await page.locator('#doc .blk').last().boundingBox();
  read.release();
  const frames = [];
  const burstEnd = Date.now() + 9000;
  let appeared = null;
  while (Date.now() < burstEnd) {
    const file = path.join(OUT, `r2-frame-${String(frames.length).padStart(2, '0')}.png`);
    const top = Math.max(0, Math.min(700, clip.y - 30));
    await page.screenshot({ path: file, scale: 'css', clip: { x: 0, y: top, width: 1600, height: Math.min(900 - top, clip.height + 150) } });
    frames.push(remember(`r2-frame-${frames.length}`, file));
    if (appeared === null && await page.locator('.margin-note').count()) appeared = Date.now();
    if (appeared !== null && Date.now() - appeared > 900) break;
  }
  read.mark('settled');
  await expect(page.locator('.margin-note .margin-felix')).toContainText(ANSWER);
  const samples = await stopSampler(page);
  await still(page, 'r2-answer-settled');
  await noMascot(page);
  expect(await page.evaluate(() => scrollY), 'automatic arrival does not scroll the page').toBe(scrollBefore);
  const caretAfter = await read.area.evaluate((el) => [document.activeElement === el, el.selectionStart, el.selectionEnd]);
  expect(caretAfter, 'editor focus and caret survive the arrival').toEqual([true, ...read.timeline.caret]);

  const withInk = samples.filter((s) => s.marks > 0);
  expect(withInk.length, 'the underline was sampled').toBeGreaterThan(3);
  const first = withInk[0].rects[0];
  const drift = Math.max(...withInk.map((s) => Math.max(...s.rects[0].map((v, i) => Math.abs(v - first[i])))));
  expect(drift, 'dotted positions are fixed during the reveal').toBeLessThan(1);
  const inkAnimations = samples.flatMap((s) => s.animations).filter((a) => a.ink);
  expect(inkAnimations.length, 'the reveal is observable as an animation on the ink').toBeGreaterThan(0);
  const length = Math.max(...inkAnimations.map((a) => a.duration + (a.delay || 0)));
  expect(length, `reveal lasts about 500 ms (${length})`).toBeGreaterThanOrEqual(400);
  expect(length).toBeLessThanOrEqual(700);
  const readingAt = samples.find((s) => s.waiting);
  const inkAt = withInk[0].at;
  expect(readingAt, 'the reading dots were sampled before release').toBeTruthy();
  expect(inkAt, 'the underline starts only after the read is released').toBeGreaterThan(read.timeline.marks.released - 50);

  await page.setViewportSize({ width: 1480, height: 900 });
  await page.waitForTimeout(700);
  const replay = await page.evaluate(() => document.getAnimations().filter((a) => a.effect && a.effect.target && a.effect.target.closest && a.effect.target.closest('.margin-ink, .margin-note') && a.playState === 'running').length);
  await still(page, 'r2-after-resize');
  expect(replay, 'a resize redraw does not replay the arrival').toBe(0);
  expect(await page.locator('.margin-ink .squiggle').count()).toBeGreaterThan(0);
  await page.setViewportSize({ width: 1600, height: 900 });
  expect(errors, 'no page errors').toEqual([]);

  const environment = env(page, { viewport: { width: 1600, height: 900 } });
  await context.close();
  const video = await page.video().path();
  remember('r2-video', video);
  record('R2-live-read-along', {
    ...environment,
    note: note.id,
    request: { blocks: read.requested().blocks },
    statusDuringPause,
    timelineMs: Object.fromEntries(Object.entries(read.timeline.marks).map(([k, v]) => [k, v - read.timeline.marks.editStart])),
    pauseMs,
    revealMs: length,
    drift,
    sampledFrames: samples.length,
    stills: frames.length,
    video,
    errors,
  });
});

test('R2 slowed arrival: the dotted underline reveals left to right in place', async ({ browser, baseURL }, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'needs the Chromium animation protocol');
  test.setTimeout(150000);
  const { context, page, errors } = await signedIn(browser, baseURL, { viewport: { width: 1600, height: 900 } });
  const note = await prepareAlong(page, `Evidence slow ${unique()}`);
  const read = await startRead(page, note);
  await expect.poll(() => read.requested(), { timeout: 12000 }).not.toBeNull();
  await expect(page.locator('.margin-waiting')).toBeVisible({ timeout: 5000 });
  await page.addStyleTag({ content: '.line-edit { caret-color: transparent !important; } .margin-ink path.pencil { display: none !important; }' });
  const block = await page.locator('#doc .blk').last().boundingBox();
  const clip = { x: Math.max(0, block.x - 4), y: Math.max(0, block.y - 4), width: Math.min(1600 - Math.max(0, block.x - 4), block.width + 8), height: Math.min(890 - block.y, block.height + 14) };
  const base = (await page.screenshot({ scale: 'css', clip })).toString('base64');
  const cdp = await context.newCDPSession(page);
  await cdp.send('Animation.enable');
  await cdp.send('Animation.setPlaybackRate', { playbackRate: 0.1 });
  read.release();
  const frames = [];
  const end = Date.now() + 12000;
  let stable = 0;
  while (Date.now() < end) {
    frames.push((await page.screenshot({ scale: 'css', clip })).toString('base64'));
    if (frames.length > 4 && frames.at(-1) === frames.at(-2) && await page.locator('.margin-ink .squiggle').count()) { if (++stable >= 2) break; } else stable = 0;
  }
  await cdp.send('Animation.setPlaybackRate', { playbackRate: 1 });
  const columns = await page.evaluate(async ({ base, frames }) => {
    const pixels = async (b64) => {
      const bin = atob(b64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
      const canvas = document.createElement('canvas');
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const context = canvas.getContext('2d');
      context.drawImage(bitmap, 0, 0);
      return { w: bitmap.width, h: bitmap.height, d: context.getImageData(0, 0, bitmap.width, bitmap.height).data };
    };
    const reference = await pixels(base);
    const out = [];
    for (const frame of frames) {
      const p = await pixels(frame);
      let left = -1, right = -1, count = 0;
      for (let x = 0; x < Math.min(p.w, reference.w); x++) {
        let hit = false;
        for (let y = 0; y < Math.min(p.h, reference.h) && !hit; y++) {
          const i = (y * p.w + x) * 4, j = (y * reference.w + x) * 4;
          hit = Math.abs(p.d[i] - reference.d[j]) + Math.abs(p.d[i + 1] - reference.d[j + 1]) + Math.abs(p.d[i + 2] - reference.d[j + 2]) > 48;
        }
        if (hit) { if (left < 0) left = x; right = x; count++; }
      }
      out.push({ left, right, count });
    }
    return out;
  }, { base, frames });
  fs.mkdirSync(OUT, { recursive: true });
  const kept = [0, Math.floor(frames.length / 3), Math.floor((2 * frames.length) / 3), frames.length - 1].map((i, n) => remember(`r2-slow-${n}`, (() => { const f = path.join(OUT, `r2-slow-${n}.png`); fs.writeFileSync(f, Buffer.from(frames[i], 'base64')); return f; })()));
  const changed = columns.filter((c) => c.count > 0);
  expect(changed.length, 'the underline appeared in the sampled frames').toBeGreaterThan(3);
  const final = changed.at(-1);
  const extent = final.right - final.left;
  expect(extent, 'the settled underline has width').toBeGreaterThan(40);
  const lefts = changed.map((c) => c.left);
  expect(Math.max(...lefts) - Math.min(...lefts), 'the reveal starts at one fixed left edge').toBeLessThanOrEqual(4);
  let regress = 0;
  for (let i = 1; i < changed.length; i++) regress = Math.max(regress, changed[i - 1].right - changed[i].right);
  expect(regress, 'the visible right edge only advances').toBeLessThanOrEqual(3);
  const partial = new Set(changed.filter((c) => (c.right - c.left) / extent > 0.12 && (c.right - c.left) / extent < 0.88).map((c) => Math.round(c.right / 4)));
  expect(partial.size, `at least two distinct partial reveal frames (${JSON.stringify(changed.map((c) => c.right))})`).toBeGreaterThanOrEqual(2);
  expect(errors).toEqual([]);
  const environment = env(page);
  await context.close();
  record('R2-reveal-slowed', { ...environment, note: note.id, playbackRate: 0.1, frames: columns.length, extentPx: extent, columns, kept });
});

test('R2 an interrupted read never draws on another note and returns to its own', async ({ browser, baseURL }) => {
  test.setTimeout(120000);
  const { context, page, errors } = await signedIn(browser, baseURL, { viewport: { width: 1600, height: 900 } });
  const a = await prepareAlong(page, `Evidence interrupt A ${unique()}`);
  const b = await createNote(page, `Evidence interrupt B ${unique()}`, 'Another note that nobody reads along.');
  const read = await startRead(page, a);
  await expect.poll(() => read.requested(), { timeout: 12000 }).not.toBeNull();
  await expect(page.locator('.margin-waiting')).toBeVisible({ timeout: 5000 });
  await page.evaluate((id) => { location.hash = `#/n/${id}`; }, b.id);
  await expect(page.locator('#title')).toHaveText(b.title);
  await expect(page.locator('.margin-waiting')).toHaveCount(0);
  await still(page, 'r2-interrupt-other-note');
  read.release();
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${a.id}/margin`)).json()).threads.length, { timeout: 15000 }).toBe(1);
  await page.waitForTimeout(1200);
  expect(await page.locator('.margin-note').count(), 'the late answer is not drawn on the note that is open now').toBe(0);
  expect(await page.locator('.margin-ink .squiggle').count()).toBe(0);
  await expect(page.locator('.margin-waiting')).toHaveCount(0);
  await page.evaluate((id) => { location.hash = `#/n/${id}`; }, a.id);
  await expect(page.locator('.margin-note .margin-felix')).toContainText(ANSWER, { timeout: 10000 });
  await still(page, 'r2-interrupt-back');
  expect(errors).toEqual([]);
  record('R2-interrupted-read', { ...env(page), notes: { a: a.id, b: b.id }, errors });
  await context.close();
});

for (const interaction of ['edit', 'chat']) test(`R2 a pending read survives ${interaction} with the right note and status`, async ({ browser, baseURL }) => {
  test.setTimeout(45000);
  const { context, page, errors } = await signedIn(browser, baseURL, { viewport: { width: 1440, height: 900 } });
  try {
    const note = await prepareAlong(page, `Evidence ${interaction} ${unique()}`);
    const read = await startRead(page, note);
    await expect.poll(() => read.requested(), { timeout: 12000 }).not.toBeNull();
    await expect(page.locator('.margin-waiting')).toBeVisible();
    if (interaction === 'edit') {
      await read.area.pressSequentially(' Revised again.');
      await expect(page.locator('#save-state')).toHaveText('Saved');
      const response = page.waitForResponse(response => response.url().endsWith(`/api/notes/${note.id}/margin/read`));
      read.release();
      expect((await response).status()).toBe(409);
      await expect(page.locator('#margin-status')).toContainText('changed');
      await expect(read.area).toHaveValue(/Revised again\./);
      expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toContain('Revised again.');
      expect((await (await page.request.get(`/api/notes/${note.id}/margin`)).json()).threads).toHaveLength(0);
    } else {
      await page.locator('#chat-toggle').click();
      await expect(page.locator('#chat')).toBeVisible();
      await expect(page.locator('.note-columns')).toHaveAttribute('data-margin', 'marks');
      read.release();
      await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}/margin`)).json()).threads.length, { timeout: 10000 }).toBe(1);
      await expect(page.locator('.margin-mark')).toBeVisible();
      await page.locator('.margin-mark').click();
      await expect(page.locator('.margin-felix')).toContainText(ANSWER);
      await noMascot(page);
    }
    await still(page, `r2-pending-${interaction}`);
    expect(errors).toEqual([]);
    record(`R2-pending-${interaction}`, { ...env(page), note: note.id, errors });
  } finally { await context.close(); }
});

test('R2/R14 Reduced Motion and forced colors keep the underline and answer complete without motion', async ({ browser, baseURL }) => {
  test.setTimeout(120000);
  const { context, page, errors } = await signedIn(browser, baseURL, { viewport: { width: 1600, height: 900 }, reducedMotion: 'reduce', forcedColors: 'active' });
  const note = await prepareAlong(page, `Evidence reduced ${unique()}`);
  const read = await startRead(page, note);
  await expect.poll(() => read.requested(), { timeout: 12000 }).not.toBeNull();
  await expect(page.locator('.margin-waiting')).toBeVisible({ timeout: 5000 });
  await installSampler(page);
  read.release();
  await expect(page.locator('.margin-note .margin-felix')).toContainText(ANSWER, { timeout: 12000 });
  await page.waitForTimeout(500);
  const samples = await stopSampler(page);
  await still(page, 'r2-reduced-forced-colors');
  await noMascot(page);
  const moving = samples.flatMap((s) => s.animations).filter((a) => /squiggle|margin-ink|ink-word|margin-note/.test(a.klass) && (a.duration + (a.delay || 0)) > 60);
  expect(moving, 'no decorative animation longer than a frame under Reduced Motion').toEqual([]);
  const stroke = await page.locator('.margin-ink .squiggle').first().evaluate((el) => getComputedStyle(el).stroke);
  expect(stroke, 'the underline is visible in forced colors').not.toBe('none');
  expect(errors).toEqual([]);
  record('R2-reduced-motion-forced-colors', { ...env(page), note: note.id, stroke, longAnimations: moving.length });
  await context.close();
});

test('R15 the mode dropdown opens and closes with an interruptible animation and snaps under Reduced Motion', async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1440, height: 900 });
  const note = await createNote(page, `Evidence menu ${unique()}`, 'A passage for the menu.');
  await page.goto(`/#/n/${note.id}`);
  const button = page.locator('#margin-mode');
  await expect(button).toBeEnabled();
  const watch = () => page.evaluate(() => {
    window.__menu = [];
    window.__menuOn = true;
    const menu = document.querySelector('#margin-mode-menu');
    const t0 = performance.now();
    const tick = () => {
      const cs = getComputedStyle(menu);
      window.__menu.push({ t: performance.now() - t0, hidden: menu.hidden, shown: cs.display !== 'none' && cs.visibility !== 'hidden', opacity: parseFloat(cs.opacity), transform: cs.transform, animations: menu.getAnimations({ subtree: true }).length });
      if (window.__menuOn) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  const read = () => page.evaluate(() => { window.__menuOn = false; return window.__menu; });
  const distinct = (frames) => new Set(frames.filter((f) => f.shown).map((f) => `${f.opacity.toFixed(2)}|${f.transform}`)).size;

  await watch();
  await button.click();
  await settle(page);
  await frame(page);
  const opening = await read();
  await still(page, 'r15-menu-open');
  expect(distinct(opening), 'opening passes through animated frames').toBeGreaterThanOrEqual(3);
  expect(opening.at(-1).opacity).toBeGreaterThan(0.99);

  await watch();
  await page.keyboard.press('Escape');
  await expect(page.locator('#margin-mode-menu')).toBeHidden();
  await settle(page);
  await frame(page);
  const closing = await read();
  const afterClose = closing.filter((f) => f.shown);
  expect(afterClose.length, 'the menu stays drawn for at least two frames while it closes').toBeGreaterThanOrEqual(2);
  expect(distinct(closing)).toBeGreaterThanOrEqual(3);
  expect(closing.at(-1).hidden || !closing.at(-1).shown, 'and is gone afterwards').toBe(true);
  await expect(button).toBeFocused();

  const reversals = await button.evaluate(button => {
    const menu = document.querySelector('#margin-mode-menu');
    const animate = menu.animate;
    let run, play;
    menu.animate = function (...args) {
      run = animate.apply(this, args);
      play = run.play;
      run.play = function () { this.pause(); };
      return run;
    };
    button.click();
    const first = run;
    const duration = first.effect.getTiming().duration;
    const frame = () => {
      const current = menu.getAnimations()[0];
      const style = getComputedStyle(menu);
      return { same: current === first, time: current?.currentTime, rate: current?.playbackRate, state: current?.playState, hidden: menu.hidden, opacity: Number(style.opacity), transform: style.transform, clip: style.clipPath };
    };
    first.currentTime = duration * 0.4;
    const opening = frame();
    button.click();
    const reversedClosed = frame();
    first.currentTime = duration * 0.2;
    const closing = frame();
    button.click();
    const reversedOpen = frame();
    first.currentTime = duration * 0.8;
    const continued = frame();
    menu.animate = animate;
    run.play = play;
    run.play();
    return { duration, opening, reversedClosed, closing, reversedOpen, continued };
  });
  const pairs = [[reversals.opening, reversals.reversedClosed, -1.25], [reversals.closing, reversals.reversedOpen, 1]];
  let jump = 0;
  for (const [before, after, rate] of pairs) {
    expect(after.same, 'reversal keeps the same native animation').toBe(true);
    expect(after.state, 'the probe controls the animation clock').toBe('paused');
    expect(after.time, 'reversal preserves current time').toBeCloseTo(before.time, 2);
    expect(after.rate, 'reversal changes direction').toBe(rate);
    expect(after.hidden, 'reversal keeps the menu rendered').toBe(false);
    expect(after.transform, 'reversal preserves the current position').toBe(before.transform);
    expect(after.clip, 'reversal preserves the current reveal').toBe(before.clip);
    jump = Math.max(jump, Math.abs(after.opacity - before.opacity));
  }
  expect(jump, 'reversal preserves opacity at the same native animation time').toBeLessThan(0.005);
  expect(reversals.closing.opacity, 'closing advances towards hidden').toBeLessThan(reversals.reversedClosed.opacity);
  expect(reversals.continued.opacity, 'reopening advances towards visible').toBeGreaterThan(reversals.reversedOpen.opacity);
  await expect.poll(() => page.locator('#margin-mode-menu').evaluate(menu => menu.getAnimations().length)).toBe(0);
  await expect(page.locator('#margin-mode-menu')).toBeVisible();
  await expect(button).toHaveAttribute('aria-expanded', 'true');
  expect(await page.getByRole('menu', { name: 'Note reading mode' }).count()).toBe(1);
  await page.keyboard.press('Escape');
  await expect(page.locator('#margin-mode-menu')).toBeHidden();
  await settle(page);

  await page.emulateMedia({ reducedMotion: 'reduce' });
  await watch();
  await button.click();
  await expect(page.locator('#margin-mode-menu')).toBeVisible();
  await settle(page);
  await frame(page);
  const reduced = await read();
  const between = reduced.filter((f) => f.shown && f.opacity > 0.01 && f.opacity < 0.99);
  expect(between, 'Reduced Motion shows the menu at once').toEqual([]);
  await still(page, 'r15-menu-reduced-motion');
  await page.keyboard.press('Escape');
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  for (const [width, height] of WIDTHS) for (const scheme of SCHEMES) {
    await page.setViewportSize({ width, height });
    await page.emulateMedia({ colorScheme: scheme });
    await button.click();
    await settle(page);
    await still(page, `r15-menu-${width}-${scheme}`);
    await page.keyboard.press('Escape');
    await expect(page.locator('#margin-mode-menu')).toBeHidden();
    await settle(page);
  }
  await page.emulateMedia({ colorScheme: 'light' });
  record('R15', { ...env(page), opening: distinct(opening), closing: distinct(closing), maxOpacityJump: jump, reversals, reducedIntermediateFrames: between.length });
});

test('Settings chooses how often Felix reads and says so in one sentence', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/#/settings/felix');
  const rhythm = page.locator('#felix-rhythm');
  await expect(rhythm).toContainText('(longer mid-sentence), at most once every 90 sec.');
  await expect(rhythm).not.toContainText('today');
  const reads = page.locator('.felix-rhythm header #felix-reads');
  await expect(reads).toHaveText(/^\d+\/40 today$/);
  const clipped = await page.locator('.felix-rhythm .segment span').evaluateAll(spans => spans.filter(span => span.scrollWidth > span.clientWidth).length);
  expect(clipped).toBe(0);
  await page.locator('.segmented[aria-label="Time between reads"]').getByRole('radio', { name: '15 sec' }).click();
  await expect(rhythm).toContainText('at most once every 15 sec');
  await page.locator('.segmented[aria-label="Reads a day"]').getByRole('radio', { name: 'No limit' }).click();
  await expect(reads).toHaveText(/^\d+\/∞ today$/);
  await expect(reads).toHaveAttribute('aria-label', /reads today, no daily limit/);
  await page.reload();
  await expect(page.locator('.segmented[aria-label="Reads a day"]').getByRole('radio', { name: 'No limit' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.segmented[aria-label="Time between reads"]').getByRole('radio', { name: '15 sec' })).toHaveAttribute('aria-checked', 'true');
  await page.request.post('/api/margin', { data: { cap: 40, gap: 90 } });
});

test('R16 the When you ask copy in Settings describes the real activation', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/#/settings/felix');
  const card = page.locator('[data-find="felix-new-notes"]');
  await expect(card).toContainText('When you ask');
  const copy = await card.innerText();
  await expect(card.locator('.choice-pick'), 'two choices').toHaveCount(2);
  await expect(card.locator('.choice-about'), 'each with one line').toHaveCount(2);
  expect(copy, 'names the passage selection').toMatch(/select(ing)? (a |the )?(passage|words|text)/i);
  expect(copy, 'names Ask Felix').toMatch(/Ask Felix/);
  expect(copy, 'names the shortcut').toMatch(/(⌘|Cmd|Ctrl)\s*\+?\s*(⇧|Shift)\s*\+?\s*E/i);
  await expect(card.locator('p.hint'), 'one privacy sentence').toHaveCount(1);
  expect(copy, 'says what Reads along sends and to which AI').toMatch(/Reads along sends the note’s title and the text you change to \S/);
  record('R16', { ...env(page), copy });
});

test('R4 Settings stays centered and is captured in both themes at every width', async ({ page }) => {
  test.setTimeout(120000);
  const gaps = [];
  for (const [width, height] of WIDTHS) for (const scheme of SCHEMES) {
    await page.setViewportSize({ width, height });
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto('/#/settings/felix');
    await expect(page.locator('.settings-section')).toBeVisible();
    await settle(page);
    if (width >= 900) {
      const [start, end] = await page.evaluate(() => {
        const main = document.querySelector('main').getBoundingClientRect();
        return [document.querySelector('.settings-nav').getBoundingClientRect().left - main.left, main.right - document.querySelector('.settings-section').getBoundingClientRect().right];
      });
      gaps.push({ width, scheme, start, end });
      expect(Math.abs(start - end), `nav and section centered together at ${width}px ${scheme}`).toBeLessThan(2);
    }
    await still(page, `r4-settings-${width}-${scheme}`);
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ forcedColors: 'active' });
  await still(page, 'r4-settings-390-forced-colors');
  await page.emulateMedia({ forcedColors: 'none', reducedMotion: 'reduce' });
  await still(page, 'r4-settings-390-reduced-motion');
  await page.emulateMedia({ reducedMotion: 'no-preference', colorScheme: 'light' });
  record('R4', { ...env(page), gaps });
});

const PLANS = () => [
  { provider: 'codex', name: 'Codex', installed: true, signed_in: true, signin: 'codex login', limits: { five_hour: { used: 0.39, resets_at: Math.floor(Date.now() / 1000) + 3 * 3600 + 300 }, seven_day: { used: 0.06, resets_at: Math.floor(Date.now() / 1000) + 4 * 86400 }, seen_at: new Date(Date.now() - 60000).toISOString() } },
  { provider: 'claude_code', name: 'Claude Code', installed: true, signed_in: true, signin: 'claude', limits: { five_hour: { used: 0.95, resets_at: Math.floor(Date.now() / 1000) + 2 * 3600 }, seven_day: { used: 0.4, resets_at: Math.floor(Date.now() / 1000) + 5 * 86400 }, seen_at: new Date(Date.now() - 60000).toISOString() } },
];

test('R3/R12 Usage keeps its layout and returns to the pre-redesign provider colors in both themes', async ({ page }) => {
  test.setTimeout(120000);
  const real = await (await page.request.get('/api/settings')).json();
  const view = () => ({ ...real, plans: PLANS() });
  await page.route('**/api/settings', (route) => (route.request().method() === 'GET' ? route.fulfill({ json: view() }) : route.continue()));
  await page.route('**/api/settings/usage', (route) => route.fulfill({ json: view() }));
  const tones = [];
  for (const [width, height] of WIDTHS) for (const scheme of SCHEMES) {
    await page.setViewportSize({ width, height });
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto('/#/settings');
    await expect(page.locator('#app')).not.toBeEmpty();
    await page.locator('.settings-nav [data-action="usage"]').click();
    await expect(page.locator('.usage-sheet .coding-plan')).toHaveCount(2);
    await settle(page);
    const colors = await page.evaluate(() => {
      const pixel = (value) => {
        const canvas = document.createElement('canvas');
        canvas.width = canvas.height = 1;
        const context = canvas.getContext('2d');
        context.fillStyle = '#ffffff';
        context.fillStyle = value;
        context.fillRect(0, 0, 1, 1);
        return [...context.getImageData(0, 0, 1, 1).data].slice(0, 3);
      };
      const read = (provider) => {
        const fill = document.querySelectorAll(`.usage-sheet .coding-plan[data-provider="${provider}"] .plan-meter i`)[1];
        const meter = fill.parentElement;
        return { fill: pixel(getComputedStyle(fill).backgroundColor), height: meter.getBoundingClientRect().height, tone: getComputedStyle(fill.closest('.coding-plan')).getPropertyValue('--plan-tone').trim() };
      };
      return { codex: read('codex'), claude: read('claude_code') };
    });
    const chroma = ([r, g, b]) => Math.max(r, g, b) - Math.min(r, g, b);
    tones.push({ width, scheme, colors });
    expect(colors.claude.fill[0] - colors.claude.fill[2], `Claude meter is orange-toned, not grayscale (${width} ${scheme}: ${colors.claude.fill})`).toBeGreaterThanOrEqual(25);
    expect(chroma(colors.codex.fill), `Codex meter keeps the slate tone (${width} ${scheme}: ${colors.codex.fill})`).toBeLessThan(30);
    expect(colors.claude.fill, 'the two providers differ').not.toEqual(colors.codex.fill);
    expect(colors.claude.tone, 'the provider tone token is applied').not.toBe('');
    expect(colors.codex.height, 'the Usage meter geometry is unchanged').toBe(width <= 560 ? 26 : 32);
    await still(page, `r12-usage-${width}-${scheme}`);
    await page.locator('.usage-sheet [data-action="close"]').click();
    await expect(page.locator('.usage-sheet')).toHaveCount(0);
  }
  await page.emulateMedia({ colorScheme: 'light' });
  record('R12', { ...env(page), fixture: 'real page and CSS; /api/settings plans and /api/settings/usage answered with deterministic readings (Codex 61/94%, Claude 5/60%)', tones });
});

test('R11/R13 the explorer has no Collapse all and the rail has no Felix, while the toolbar chat opens at every width', async ({ page }) => {
  test.setTimeout(150000);
  const tag = unique();
  const dir = `Evidence folder ${tag}`;
  expect((await page.request.post('/api/dirs', { data: { path: dir } })).ok()).toBe(true);
  const inside = await createNote(page, `Evidence inside ${tag}`, 'Inside a folder.', dir);
  await createNote(page, `Evidence root ${tag}`, 'At the top.');
  const named = (el) => /collapse all/i.test(`${el.innerText} ${[...el.querySelectorAll('[aria-label],[title]')].map((x) => `${x.getAttribute('aria-label') || ''} ${x.getAttribute('title') || ''}`).join(' ')}`);
  for (const [width, height] of WIDTHS) for (const scheme of SCHEMES) {
    await page.setViewportSize({ width, height });
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto(`/#/n/${inside.id}`);
    await expect(page.locator('#title')).toHaveText(inside.title);
    if (width >= 600) {
      await expect(page.locator('#side')).toBeVisible();
      expect(await page.locator('#side .side-group').evaluate(named), 'no Collapse all in the Folders heading').toBe(false);
      expect(await page.locator('#side .side-rail').evaluate((el) => /felix/i.test(`${el.innerText} ${[...el.querySelectorAll('[aria-label],[title]')].map((x) => `${x.getAttribute('aria-label')} ${x.getAttribute('title')}`).join(' ')}`)), 'no Felix in the rail').toBe(false);
      expect(await page.locator('#side .side-rail [data-action="chat"]').count()).toBe(0);
    } else {
      await page.locator('#menu').click();
      await page.locator(':is(.sheet, .popup-menu) [data-action="folders"]').click();
      await expect(page.locator('#folders-sheet')).toBeVisible();
      expect(await page.locator('#folders-sheet').evaluate(named), 'no Collapse all in the compact Folders sheet').toBe(false);
    }
    await settle(page);
    await still(page, `r11-explorer-${width}-${scheme}`);
    if (width < 600) await page.locator('#folders-sheet [data-action="close"]').click();
    const toggle = page.locator('#chat-toggle');
    await expect(toggle, `toolbar chat access at ${width}`).toBeVisible();
    await toggle.click();
    await expect(page.locator('#chat')).toBeVisible();
    await page.locator('#chat [data-chat="close"]').click();
    await expect(page.locator('#chat')).toBeHidden();
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ forcedColors: 'active' });
  await page.goto(`/#/n/${inside.id}`);
  await still(page, 'r11-explorer-390-forced-colors');
  await page.emulateMedia({ forcedColors: 'none', reducedMotion: 'reduce' });
  await still(page, 'r11-explorer-390-reduced-motion');
  await page.emulateMedia({ reducedMotion: 'no-preference', colorScheme: 'light' });
  record('R11-R13', { ...env(page), folder: dir });
});

test('R17 the loaded explorer keeps its rows and never flashes Opening notes during Settings interaction or refresh', async ({ page }) => {
  test.setTimeout(120000);
  await page.setViewportSize({ width: 1440, height: 900 });
  const tag = unique();
  const dir = `Evidence cache ${tag}`;
  expect((await page.request.post('/api/dirs', { data: { path: dir } })).ok()).toBe(true);
  const made = [];
  for (let i = 0; i < 6; i++) made.push(await createNote(page, `Cached row ${tag} ${i}`, 'A row.', dir));
  await page.goto(`/#/n/${made[0].id}`);
  const folder = page.locator(`#side .side-row[data-action="open-folder"][data-dir="${dir}"]`);
  await expect(folder).toBeVisible();
  if (await folder.getAttribute('aria-expanded') === 'true') await folder.click();
  await expect(folder).toHaveAttribute('aria-expanded', 'false');
  await folder.click();
  await expect(page.locator(`#side .side-note[data-id="${made[1].id}"]`)).toBeVisible();
  await page.evaluate((dir) => {
    const side = document.querySelector('#side');
    const count = () => [...side.querySelectorAll('.side-note[data-id]')].filter((el) => el.dataset.from === dir).length;
    window.__flash = { on: true, opening: 0, frames: 0, start: count(), min: count() };
    const tick = () => {
      const f = window.__flash;
      f.frames++;
      f.min = Math.min(f.min, count());
      if (/Opening notes/.test(side.textContent)) f.opening++;
      if (f.on) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }, dir);
  await page.goto('/#/settings/general');
  await expect(page.locator('.settings-nav')).toBeVisible();
  for (const name of ['Felix', 'General']) {
    await page.locator(`.settings-nav a[href="#/settings/${name.toLowerCase()}"]`).click();
    await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
  }
  const theme = page.locator('[data-action="theme-pick"]');
  await theme.nth(1).click();
  await expect(theme.nth(1)).toHaveAttribute('aria-pressed', 'true');
  await theme.nth(0).click();
  await expect(theme.nth(0)).toHaveAttribute('aria-pressed', 'true');
  await createNote(page, `Cached row late ${tag}`, 'Arrives while Settings is open.', dir);
  await page.evaluate(() => { window.dispatchEvent(new Event('focus')); window.dispatchEvent(new Event('leo-visibility')); document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForTimeout(2500);
  const flash = await page.evaluate(() => { window.__flash.on = false; return window.__flash; });
  await still(page, 'r17-explorer-after-settings');
  expect(flash.opening, `Opening notes appeared in ${flash.opening} frames`).toBe(0);
  expect(flash.min, 'cached rows were never dropped during the refresh').toBeGreaterThanOrEqual(flash.start);
  record('R17', { ...env(page), flash });
});

test('R18 Chats history has no New chat, a smaller title and an animated X to Delete', async ({ page }) => {
  test.setTimeout(120000);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.route('**/api/chat', (route) => (route.request().method() === 'POST'
    ? route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { t: 'A short stored answer.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' })
    : route.continue()));
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  const chat = page.locator('#chat');
  await chat.locator('#chat-input').fill('Evidence history question');
  await chat.locator('#chat-input').press('Enter');
  await expect(chat.locator('.msg.leo').last()).toContainText('A short stored answer.');
  await expect.poll(async () => (await (await page.request.get('/api/chats')).json()).length, { timeout: 10000 }).toBeGreaterThan(0);
  expect(await page.locator('#chat .chat-head [data-chat="new"]').count(), 'the chat toolbar keeps its New chat action').toBeGreaterThan(0);
  if (!(await page.locator('#chat-history').isVisible())) await chat.locator('.chat-history-toggle').click();
  await expect(page.locator('#chat-history')).toBeVisible();
  const head = page.locator('#chat-history .chat-history-head');
  await expect(head.locator('[data-chat="new"]'), 'New chat moves to the Chats heading while it is open').toBeVisible();
  await expect(page.locator('#chat .chat-head [data-chat="new"]')).toBeHidden();
  const size = await page.locator('#chat-history .chat-history-title').first().evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  expect(size, `history title is smaller than the old 16px (${size})`).toBeLessThan(16);
  await still(page, 'r18-history');
  const row = page.locator('#chat-history .chat-history-row').first();
  const x = row.locator('.chat-history-x');
  const watch = () => x.evaluate((el) => new Promise((done) => {
    el.click();
    el.getBoundingClientRect();
    const moving = el.getAnimations().map((run) => run.transitionProperty);
    setTimeout(() => done({ moving, end: el.getBoundingClientRect().width }), 700);
  }));
  await row.hover();
  const resting = (await x.boundingBox()).width;
  const { moving, end } = await watch();
  await expect(x).toHaveClass(/sure/);
  expect(end, 'Delete is wider than X').toBeGreaterThan(resting + 4);
  expect(moving, 'the width animates').toContain('width');
  await expect(x.locator('.chat-delete-label')).toBeVisible();
  await still(page, 'r18-history-delete-confirm');
  await page.locator('#chat-history-search').click();
  await expect(x).not.toHaveClass(/sure/);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await row.hover();
  const snapped = await watch();
  expect(snapped.moving, 'Reduced Motion snaps the width').not.toContain('width');
  await page.locator('#chat-history-search').click();
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  record('R18', { ...env(page), titlePx: size, restingWidth: resting, endWidth: end, animated: moving, reduced: snapped.moving });
});
