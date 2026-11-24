const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss } = require('../page-source');

const web = path.resolve(__dirname, '../../src/web');
const SAVED = [['You', 'var(--text)', '0:02', 'Shall we start?'], ['Others', 'var(--hue-0)', '0:06', 'Yes.']];
const savedRows = SAVED.map(([name, tone, at, text]) => `<div class="source-row turn"><div class="turn-who"><span class="tone-dot" style="--tone: ${tone}"></span><span class="turn-name">${name}</span><span class="rec-at">${at}</span></div><p class="spoken"><span class="source-text">${text}</span></p></div>`).join('');

const spoken = (n, speaker, text) => ({ n, speaker, text });
const START = [spoken(0, 'You', 'Shall we start?'), spoken(1, 'Others', 'Yes. The parser shipped'), spoken(2, 'You', 'Good. What is next')];

async function fixture(browser, run, { turns = START, transcript, context = {}, state = 'recording', source = 'nativecall' } = {}) {
  const script = `
    const view = { id: 'rec-1', session: 'rec-1', source: ${JSON.stringify(source)}, title: 'Weekly call', directory: '', state: ${JSON.stringify(state)}, secs: 42, step: '', steps: null, warnings: [], points: [], points_total: 0, wants: '', levels: [], levels_start: 0, note: null, error: null };
    window.say = (turns, over = {}) => { Object.assign(view, { turns, transcript: turns.map((t) => t.speaker + ': ' + t.text).join('\\n') }, over); if (!turns.length) delete view.turns; };
    window.polls = 0;
    window.asked = [];
    const api = async (url) => { if (url === '/api/record/rec-1') window.polls++; return structuredClone(url === '/api/record' ? { available: true, local: true, job: view, pending: [] } : url === '/api/record/rec-1' ? view : {}); };
    const esc = (t) => String(t).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
    say(${JSON.stringify(turns)}, ${JSON.stringify(transcript === undefined ? {} : { transcript })});
    const recorder = leoRecording.create({ api, esc, toast() {}, go() {}, noteHash: (id) => id, felix: { felix: () => '' }, icons: new Proxy({}, { get: () => '<svg></svg>' }), encodePicture() {}, choiceCards: () => '', syncChoiceCards() {}, bytes: String, chat: () => ({ isBusy: () => false, attachSelection: (a) => window.asked.push(a.text), askNew() {} }) });
    recorder.show(document.querySelector('#rec-box'), { dir: '', folders: [] }).then(() => document.querySelector('[data-rec-tab="heard"]').click());`;
  const server = http.createServer((req, res) => {
    const js = { '/fixture.js': () => script, '/recording.js': () => fs.readFileSync(path.join(web, 'recording.js'), 'utf8') }[req.url];
    if (js) { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(js()); }
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); return res.end(pageCss()); }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end(`<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="stylesheet" href="/style.css"></head><body><main style="padding: 24px"><div id="rec-box"></div><div id="saved" class="note-transcript"><div class="source-list transcript-flow">${savedRows}</div></div></main><script src="/recording.js"></script><script src="/fixture.js"></script></body></html>`);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const browserContext = await browser.newContext({ viewport: { width: 1100, height: 900 }, ...context });
  try {
    const page = await browserContext.newPage();
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await expect(page.locator('#rec-transcript')).toBeVisible();
    await run(page);
    expect(errors).toEqual([]);
  } finally {
    await browserContext.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

async function hear(page, turns, over) {
  await page.evaluate(([t, o]) => window.say(t, o), [turns, over]);
  const polls = await page.evaluate(() => window.polls);
  await page.waitForFunction((before) => window.polls >= before + 2, polls);
}

const shown = (page) => page.locator('#rec-transcript > .turn').evaluateAll((rows) => rows.map((row) => `${row.querySelector('.turn-name').textContent}: ${row.querySelector('.spoken').textContent}`));

const look = (row) => {
  const of = (at, keys) => { const css = getComputedStyle(row.querySelector(at)); return keys.map((key) => css[key]); };
  const at = (sel) => row.querySelector(sel).getBoundingClientRect();
  return {
    dot: of('.tone-dot', ['width', 'height', 'borderRadius', 'backgroundColor']),
    name: of('.turn-name', ['fontSize', 'fontWeight', 'color']),
    words: of('.spoken', ['fontSize', 'lineHeight', 'color', 'marginTop', 'marginLeft']),
    nameIndent: Math.round(at('.turn-name').left - at('.tone-dot').left),
    wordsIndent: Math.round(at('.spoken').left - at('.tone-dot').left),
    whoHeight: Math.round(at('.turn-who').height),
  };
};

for (const colorScheme of ['light', 'dark']) {
  test(`a live call shows who spoke as the saved transcript does, ${colorScheme}`, async ({ browser }) => {
    await fixture(browser, async (page) => {
      expect(await shown(page)).toEqual(['You: Shall we start?', 'Others: Yes. The parser shipped', 'You: Good. What is next']);
      const live = await page.locator('#rec-transcript > .turn').evaluateAll((rows, fn) => rows.map(new Function(`return ${fn}`)()), look.toString());
      const saved = await page.locator('#saved .turn').evaluateAll((rows, fn) => rows.map(new Function(`return ${fn}`)()), look.toString());
      expect(live[0]).toEqual(saved[0]);
      expect(live[1]).toEqual(saved[1]);
      expect(live[0].dot).not.toEqual(live[1].dot);
      const gaps = await page.evaluate(() => ['#rec-transcript', '#saved .source-list'].map((at) => { const [a, b] = document.querySelectorAll(`${at} > .turn`); return Math.round(b.getBoundingClientRect().top - a.getBoundingClientRect().bottom); }));
      expect(gaps[0]).toBe(gaps[1]);
      await expect(page.locator('#rec-transcript .rec-new'), 'what was said before the page opened arrives plain').toHaveCount(0);
      await expect(page.locator('#rec-transcript')).toHaveAttribute('aria-live', 'off');
      const copied = await page.evaluate(() => { getSelection().selectAllChildren(document.querySelector('#rec-transcript')); return getSelection().toString(); });
      expect(copied.split('\n').map((line) => line.trim()).filter(Boolean)).toEqual(['You', 'Shall we start?', 'Others', 'Yes. The parser shipped', 'You', 'Good. What is next']);
      await page.evaluate(() => getSelection().removeAllRanges());
      await page.locator('[data-rec-ask="What did I just miss?"]').click();
      expect(await page.evaluate(() => window.asked[0])).toContain('You: Shall we start?\nOthers: Yes. The parser shipped\nYou: Good. What is next');
      await hear(page, [START[0], START[1], spoken(2, 'You', 'Good. What is next for us?')], { state: 'writing', step: 'Transcribing the recording' });
      expect(await shown(page)).toEqual(['You: Shall we start?', 'Others: Yes. The parser shipped', 'You: Good. What is next for us?']);
    }, { context: { colorScheme } });
  });
}

test('a live call streams under a selection and only the changed words are replaced', async ({ browser }) => {
  await fixture(browser, async (page) => {
    await page.evaluate(() => {
      const words = document.querySelectorAll('#rec-transcript .spoken');
      window.kept = [...document.querySelectorAll('#rec-transcript > .turn')];
      window.node = words[1].firstChild;
      getSelection().setBaseAndExtent(words[0].firstChild, 6, words[1].firstChild, 4);
      window.seen = [];
      new MutationObserver((all) => all.forEach((m) => m.addedNodes.forEach((n) => window.seen.push([m.target.className, n.className || '', n.textContent])))).observe(document.querySelector('#rec-transcript'), { childList: true, subtree: true });
    });
    const picked = () => page.evaluate(() => getSelection().toString());
    const before = await picked();
    const next = [START[0], spoken(1, 'Others', 'Yes. The parser shipped on Monday.'), spoken(2, 'You', 'Good. What comes after?'), spoken(3, 'Others', 'Docs')];
    await hear(page, next);
    expect(await shown(page)).toEqual(next.map((t) => `${t.speaker}: ${t.text}`));
    expect(await picked()).toBe(before);
    expect(await page.evaluate(() => window.kept.every((row, i) => row === document.querySelectorAll('#rec-transcript > .turn')[i]) && window.node.isConnected && window.node.data)).toBe('Yes. The parser shipped');
    expect(await page.evaluate(() => window.seen.map((change) => change.slice(1)))).toEqual([['rec-new', ' on Monday.'], ['rec-new', 'comes after?'], ['turn', 'OthersDocs'], ['rec-new', 'Docs']]);
    await hear(page, [...next.slice(0, 3), spoken(3, 'Others', 'Docs and then')]);
    await page.evaluate(() => getSelection().removeAllRanges());
    await page.waitForTimeout(700);
    await hear(page, [...next.slice(0, 3), spoken(3, 'Others', 'Docs and then tests')]);
    await expect(page.locator('#rec-transcript .rec-new'), 'faded words fold back').toHaveCount(0);
    expect(await page.evaluate(() => [...document.querySelectorAll('#rec-transcript .spoken')].map((p) => p.childNodes.length))).toEqual([1, 1, 1, 1]);
    expect(await page.evaluate(() => window.kept.every((row) => row.isConnected))).toBe(true);
  });
});

test('a reader scrolled up stays put while a call goes on, the head is trimmed, and Back to latest follows again', async ({ browser }) => {
  const long = (n) => spoken(n, n % 2 ? 'Others' : 'You', `Turn ${n} says ${'a few more words '.repeat(12)}and stops.`);
  const turns = Array.from({ length: 30 }, (_, n) => long(n));
  await fixture(browser, async (page) => {
    const heard = page.locator('#rec-transcript');
    const bottom = () => heard.evaluate((el) => el.scrollTop + el.clientHeight >= el.scrollHeight - 2);
    expect(await bottom()).toBe(true);
    const more = [...turns, long(30)];
    await hear(page, more);
    expect(await bottom(), 'the view follows').toBe(true);
    await heard.evaluate((el) => { el.scrollTop = el.scrollHeight / 2; });
    await expect(page.locator('#rec-latest')).toBeVisible();
    const top = () => page.evaluate(() => { const row = [...document.querySelectorAll('#rec-transcript > .turn')].find((r) => r.textContent.includes('Turn 18 ')); return Math.round(row.getBoundingClientRect().top); });
    const before = await top();
    const trimmed = [spoken(3, 'Others', turns[3].text.slice(90)), ...more.slice(4), long(31)];
    await hear(page, trimmed);
    expect(await shown(page)).toEqual(trimmed.map((t) => `${t.speaker}: ${t.text}`));
    expect(Math.abs((await top()) - before)).toBeLessThanOrEqual(1);
    await page.locator('#rec-latest').click();
    await expect(page.locator('#rec-latest')).toBeHidden();
    expect(await bottom()).toBe(true);
    await expect(heard).toBeFocused();
    await hear(page, [...trimmed, long(32)]);
    expect(await bottom()).toBe(true);
    await expect(heard).toBeFocused();
    expect(await heard.evaluate((el) => el.querySelectorAll('*').length)).toBeLessThan(trimmed.length * 6 + 12);
  }, { turns });
});

test('new words do not fade under reduced motion, and names stay in forced colors and narrow windows', async ({ browser }) => {
  for (const [reducedMotion, fades] of [['reduce', 'none'], ['no-preference', 'fade']]) {
    await fixture(browser, async (page) => {
      await hear(page, [...START, spoken(3, 'Others', 'Docs')]);
      const name = await page.evaluate(() => { const probe = document.createElement('span'); probe.className = 'rec-new'; document.querySelector('#rec-transcript .spoken').append(probe); return getComputedStyle(probe).animationName; });
      expect(name).toBe(fades);
      await expect(page.locator('#rec-transcript .turn-name')).toHaveText(['You', 'Others', 'You', 'Others']);
      expect(await page.locator('#rec-transcript').evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    }, { context: { reducedMotion, forcedColors: 'active', viewport: { width: 380, height: 800 } } });
  }
});

test('a microphone recording and a call nobody has spoken in look as before', async ({ browser }) => {
  await fixture(browser, async (page) => {
    await expect(page.locator('#rec-transcript')).toHaveText('You: is only a word here.');
    await expect(page.locator('#rec-transcript')).toHaveAttribute('class', 'rec-transcript');
    await expect(page.locator('#rec-transcript .turn')).toHaveCount(0);
  }, { turns: [], transcript: 'You: is only a word here.', source: 'microphone' });
  await fixture(browser, async (page) => {
    await expect(page.locator('#rec-transcript')).toHaveText('Listening… words appear here after a few seconds.');
    await hear(page, [START[0]]);
    expect(await shown(page)).toEqual(['You: Shall we start?']);
    await expect(page.locator('#rec-transcript .hint')).toHaveCount(0);
  }, { turns: [] });
});
