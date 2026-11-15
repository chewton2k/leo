const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss, pageHtml } = require('../page-source');

const web = path.resolve(__dirname, '../../src/web');
const read = (name) => fs.readFileSync(path.join(web, name), 'utf8');
const css = () => pageCss();

const WORDS = Array.from({ length: 60 }, (_, i) => `word${i}`).join(' ');
const SHORT = 'The password is stored as plain text.';
const SOURCE = `Opening line of the note.\n\nBefore it, ${SHORT} After it we keep writing.\n\nA long paragraph begins here and ${WORDS} and then it ends.\n\nClosing paragraph of the note.`;
const THREADS = {
  short: { id: 't1', quote: SHORT, prefix: 'Before it, ', suffix: ' After it we keep', status: 'open', messages: [{ from: 'felix', text: 'Hash it before you store it.' }] },
  long: { id: 't2', quote: WORDS, prefix: 'begins here and ', suffix: ' and then it ends.', status: 'open', messages: [{ from: 'felix', text: 'This runs long. Split it.' }] },
};

const MODE_MARKUP = `<div class="margin-mode-wrap"><button type="button" id="margin-mode" class="margin-mode" aria-haspopup="menu" aria-expanded="false" aria-controls="margin-mode-menu" disabled><span id="margin-mode-icon" class="margin-mode-icon" aria-hidden="true">{{pen}}</span><span id="margin-mode-label">When you ask</span>{{chevron}}</button><div id="margin-mode-menu" class="margin-mode-menu" role="menu" aria-label="Note reading mode" hidden><button type="button" role="menuitemradio" aria-checked="false" data-margin-mode="along">Reads along<small>Reads changes after a pause.</small></button><button type="button" role="menuitemradio" aria-checked="true" data-margin-mode="ask">When you ask<small>Reads nothing until you ask.</small></button></div></div>`;

async function fixture(browser, run, { width = 1280, height = 800, layout = 'right', mode = 'ask', threads = [THREADS.short, THREADS.long], context = {}, source = SOURCE } = {}) {
  const core = read('app/core.js');
  const icons = core.slice(core.indexOf('const svg ='), core.indexOf('const recorder ='));
  const motion = core.slice(core.indexOf('const LEAVE_MS'), core.indexOf('function rel('));
  const shortcuts = core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('async function dictateHint'));
  const fixtureScript = `const $=(selector,root=document)=>root.querySelector(selector);
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const enc=encodeURIComponent;
    class Locked extends Error {}
    ${icons}
    ${motion}
    ${shortcuts}
    let seq=0;
    const state={view:'note',session:null};
    window.errors=[];
    const fail=error=>window.errors.push(error.message||String(error));
    const toast=()=>{};
    const noAi=()=>false;
    const cleanTitle=text=>text;
    const requestId=()=>'r1';
    const felix={felix:(size,cls)=>'<span class="felix '+cls+'" data-mascot></span>'};
    const chat={isOpen:()=>false,isBusy:()=>false,attachSelection(){}};
    const saving={unsaved:()=>false};
    const flush=async()=>{};
    const showNote=async()=>{};
    window.leoTheme={noteWidth:()=>'wide'};
    window.fake={mode:${JSON.stringify(mode)},layout:${JSON.stringify(layout)},threads:${JSON.stringify(threads)},failMode:false,posts:[]};
    async function api(url,{method='GET',body}={}){
      await new Promise(r=>setTimeout(r,fake.delay||2));
      if(url==='/api/margin')return {layout:fake.layout,pace:'regular',default_mode:'ask'};
      if(url.endsWith('/margin')&&method==='GET')return {mode:fake.mode,threads:structuredClone(fake.threads)};
      if(url.endsWith('/margin/threads')&&method==='POST'){fake.posts.push(body);return {id:'t9',quote:body.quote,prefix:body.prefix,suffix:body.suffix,status:'open',source:'ask',messages:[{from:'felix',text:'A fake answer.'}]};}
      if(method==='PATCH'&&url.includes('/margin/threads/')){fake.posts.push(body);const t=fake.threads.find(x=>url.endsWith('/'+x.id));t.pinned=body.pinned;return structuredClone(t);}
      if(url.endsWith('/margin/read')){fake.posts.push(body);return new Promise(r=>{fake.answer=r;});}
      if(url.endsWith('/margin')&&method==='POST'){fake.posts.push(body);if(fake.failMode){const e=new Error('Try again.');e.status=503;throw e;}fake.mode=body.mode;return {mode:body.mode};}
      throw new Error('No fake for '+method+' '+url);
    }
    ${read('app/margin-notes.js')}
    window.start=async()=>{
      app.innerHTML='<div class="note-layout"><div class="note-columns"><article class="note"><header class="note-head"><div class="note-context"><span id="margin-status" role="status" class="hint"></span></div><h1 id="title">Test</h1><div class="note-meta">${MODE_MARKUP}</div></header><div class="prose doc" id="doc"></div></article></div></div>'.replace(/\\{\\{(\\w+)\\}\\}/g,(m,k)=>ICON[k]);
      const s={note:{id:'n1',version:'v1',title:'Test'},title:$('#title'),edit:{}};
      s.doc=leoDoc.mount($('#doc'),{source:${JSON.stringify(source)},onChange(){},placeholder:''});
      state.session=s;window.session=s;
      await marginMount(s,seq);
    };
    window.rectsOf=id=>{const t=marginPage.threads.find(x=>x.id===id);const at=leoMargin.find(session.doc.source(),t);return session.doc.spanRects(at.from,at.to).map(r=>({left:r.left,right:r.right,top:r.top,bottom:r.bottom}));};
    window.arrive=thread=>{marginPage.threads.push(thread);marginPage.arriving.add(thread.id);marginRedraw();};
    window.dots=id=>[...document.querySelectorAll('.margin-ink path.squiggle[data-id="'+id+'"]')].sort((a,b)=>a.dataset.line-b.dataset.line).map(p=>{const cs=getComputedStyle(p);const m=cs.clipPath.match(/inset\\(([^)]*)\\)/);let progress=1;if(m){const right=m[1].trim().split(/\\s+/)[1];progress=right.endsWith('%')?1-parseFloat(right)/100:1;}const dash=cs.strokeDasharray.split(/[ ,]+/).map(parseFloat);const n=Math.floor(p.getTotalLength()/4)+1;const pts=[];for(let k=0;k<n&&pts.length<400;k++){const pt=p.getPointAtLength(Math.min(k*4,p.getTotalLength()));pts.push([+pt.x.toFixed(2),+pt.y.toFixed(2)]);}return {progress,dash,d:p.getAttribute('d'),pts,clip:cs.clipPath,anim:cs.animationName};});
    window.start().catch(fail);`;
  const server = http.createServer((req, res) => {
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(fixtureScript); }
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); return res.end(css()); }
    const lib = req.url.match(/^\/lib\/(.+)$/);
    if (lib) { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(read(lib[1])); }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="stylesheet" href="/style.css"></head><body class="on-note"><header class="bar" id="bar"><div class="bar-row">Leo</div></header><main id="app"></main><script src="/lib/markdown.js"></script><script src="/lib/editing.js"></script><script src="/lib/doc.js"></script><script src="/lib/margin.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const browserContext = await browser.newContext({ viewport: { width, height }, ...context });
  try {
    const page = await browserContext.newPage();
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await expect(page.locator('#margin-mode')).toBeEnabled();
    await page.waitForFunction(() => document.querySelector('.note-columns').dataset.margin);
    if (threads.length) await page.waitForFunction(() => document.querySelector('.margin-ink path.squiggle'));
    await run(page);
    expect(errors).toEqual([]);
    expect(await page.evaluate(() => window.errors)).toEqual([]);
  } finally {
    await browserContext.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

function frames(page, ms, probe, between = []) {
  return page.evaluate(([span, source, steps]) => new Promise((resolve) => {
    const read = new Function(`return (${source})`)();
    const out = [];
    const start = performance.now();
    const todo = steps.map(([at, code]) => [at, new Function(code)]);
    (function tick() {
      const t = performance.now() - start;
      while (todo.length && todo[0][0] <= t) todo.shift()[1]();
      out.push({ t, ...read() });
      if (t < span) requestAnimationFrame(tick);
      else resolve(out);
    })();
  }), [ms, probe.toString(), between]);
}

function wipeFrames(page, thread) {
  return page.evaluate(async (arriving) => {
    window.arrive(arriving);
    await new Promise((found) => {
      const look = () => (document.querySelector(`.margin-ink path.squiggle.arriving[data-id="${arriving.id}"]`) ? found() : requestAnimationFrame(look));
      look();
    });
    const runs = document.getAnimations().filter((run) => run.animationName === 'margin-wipe');
    for (const run of runs) run.pause();
    const out = [];
    for (let t = 0; t <= WIPE_MS + 100; t += 10) {
      for (const run of runs) run.currentTime = t;
      out.push({ t, dots: window.dots(arriving.id) });
    }
    return out;
  }, thread);
}

const stateOf = (id) => {
  const get = (selector) => document.querySelector(selector);
  const solid = get(`.margin-ink path.solid[data-id="${id}"]`);
  const wash = get(`.margin-ink path.wash[data-id="${id}"]`);
  const dotted = get(`.margin-ink path.squiggle[data-id="${id}"]`);
  const number = (el, prop) => (el ? parseFloat(getComputedStyle(el)[prop]) : NaN);
  return { solid: number(solid, 'opacity'), wash: number(wash, 'opacity'), dash: dotted ? getComputedStyle(dotted).strokeDasharray : '', solidDash: solid ? getComputedStyle(solid).strokeDasharray : '' };
};

test.describe('arriving underline', () => {
  test('wide Marks only preserves the outline gutter and centers the note', async ({ browser }) => {
    await fixture(browser, async page => {
      const note = await page.locator('article.note').boundingBox();
      const columns = await page.locator('.note-columns').boundingBox();
      expect(Math.abs((note.x - columns.x) - (columns.x + columns.width - note.x - note.width))).toBeLessThanOrEqual(2);
    }, { width: 1440, layout: 'marks', threads: [] });
  });
  test('first compact comment reserves its touch gutter before arrival and never reflows the editor', async ({ browser }) => {
    await fixture(browser, async page => {
      const before = await page.locator('#doc').boundingBox();
      await page.evaluate(thread => window.arrive(thread), THREADS.short);
      await expect(page.locator('.margin-mark')).toBeVisible();
      const after = await page.locator('#doc').boundingBox();
      expect(after.x).toBeCloseTo(before.x, 1);
      expect(after.width).toBeCloseTo(before.width, 1);
      expect(after.height).toBeCloseTo(before.height, 1);
    }, { width: 390, height: 844, layout: 'marks', threads: [], context: { hasTouch: true, isMobile: true } });
  });

  for (const [name, options] of [['wide', {}], ['compact', { width: 390, height: 844, layout: 'marks', context: { hasTouch: true, isMobile: true } }]]) {
    test(`${name}: each measured line is revealed left to right over about 500ms while the dots never move`, async ({ browser }) => {
      await fixture(browser, async (page) => {
        const long = { ...THREADS.long, id: 't3', quote: WORDS.slice(0, 200), suffix: WORDS.slice(200, 224) };
        const after = await wipeFrames(page, long);
        const seen = after.filter((f) => f.dots.length);
        expect(seen.length).toBeGreaterThan(10);
        const count = seen[seen.length - 1].dots.length;
        expect(count).toBeGreaterThanOrEqual(2);
        const final = seen[seen.length - 1].dots;
        for (const frame of seen) {
          expect(frame.dots).toHaveLength(count);
          frame.dots.forEach((line, i) => {
            expect(line.dash).toEqual([1, 3]);
            expect(line.d).toBe(final[i].d);
            expect(line.pts).toEqual(final[i].pts);
          });
        }
        for (let i = 0; i < count; i++) {
          let last = 0;
          for (const frame of seen) {
            const p = frame.dots[i].progress;
            expect(p).toBeGreaterThanOrEqual(last - 0.001);
            last = p;
          }
          expect(seen[0].dots[i].progress).toBeLessThan(0.3);
          expect(last).toBeCloseTo(1, 2);
        }
        expect(seen.filter((f) => f.dots.some((l) => l.progress > 0.05 && l.progress < 0.95)).length).toBeGreaterThanOrEqual(3);
        for (const frame of seen) {
          for (let i = 1; i < count; i++) if (frame.dots[i].progress > 0.02) expect(frame.dots[i - 1].progress).toBeGreaterThan(0.97);
        }
        const began = seen.find((f) => f.dots.some((l) => l.progress > 0.02));
        const done = seen.find((f) => f.dots.every((l) => l.progress > 0.98));
        const took = done.t - began.t;
        expect(took).toBeGreaterThan(350);
        expect(took).toBeLessThan(700);
        await expect.poll(() => page.evaluate(() => window.dots('t3').map((l) => l.clip).every((c) => c === 'none'))).toBe(true);
      }, options);
    });
  }

  test('an interrupted redraw and a reflow neither restart nor rewind the reveal', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const long = { ...THREADS.long, id: 't3', quote: WORDS.slice(0, 200), suffix: WORDS.slice(200, 224) };
      const timeline = await frames(page, 1500, () => ({ dots: window.dots('t3') }), [
        [40, `window.arrive(${JSON.stringify(long)})`],
        [200, 'marginRedraw()'],
        [260, 'marginRedraw()'],
        [320, 'document.querySelector(".note-columns").style.paddingRight="40px"'],
      ]);
      const seen = timeline.filter((f) => f.dots.length);
      const count = seen[seen.length - 1].dots.length;
      for (let i = 0; i < count; i++) {
        let last = 0;
        for (const frame of seen) {
          const p = frame.dots[i].progress;
          expect(p).toBeGreaterThanOrEqual(last - 0.001);
          last = p;
          expect(frame.dots[i].dash).toEqual([1, 3]);
        }
        expect(last).toBeCloseTo(1, 2);
      }
    });
  });

  test('reduced motion shows the finished dotted line at once', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const long = { ...THREADS.long, id: 't3', quote: WORDS.slice(0, 200), suffix: WORDS.slice(200, 224) };
      const timeline = await frames(page, 300, () => ({ dots: window.dots('t3') }), [[20, `window.arrive(${JSON.stringify(long)})`]]);
      const seen = timeline.filter((f) => f.dots.length);
      expect(seen.length).toBeGreaterThan(3);
      for (const frame of seen) for (const line of frame.dots) {
        expect(line.clip).toBe('none');
        expect(line.anim).toBe('none');
        expect(line.dash).toEqual([1, 3]);
      }
    }, { context: { reducedMotion: 'reduce' } });
  });

  test('forced colors keep the underline visible in system colors', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const ink = await page.evaluate(() => {
        const dotted = getComputedStyle(document.querySelector('.margin-ink path.squiggle'));
        return { stroke: dotted.stroke, dash: dotted.strokeDasharray };
      });
      expect(ink.stroke).not.toBe('none');
      expect(ink.stroke).not.toMatch(/rgba\(0, 0, 0, 0\)/);
      expect(ink.dash).toMatch(/^1(px)?[ ,]+3/);
    }, { context: { forcedColors: 'active' } });
  });
});

test.describe('reading pencil', () => {
  const startRead = (page, marker) => page.evaluate((m) => {
    marginPage.baseline = leoMargin.blocksOf(session.doc.source()).filter((block) => !block.includes(m));
    marginRead(session).catch(fail);
  }, marker);
  const shapeOf = (page, marker) => page.evaluate((m) => {
    const source = session.doc.source();
    const block = leoMargin.blocksOf(source).find((b) => b.includes(m));
    const at = source.indexOf(block);
    const rects = session.doc.spanRects(at, at + block.length);
    const box = (el) => { const r = el.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height }; };
    return {
      bottoms: [...new Set(rects.map((r) => Math.round(r.bottom)))].sort((a, b) => a - b),
      left: Math.min(...rects.map((r) => r.left)),
      paths: [...document.querySelectorAll('.margin-ink path.pencil')].map(box),
      draws: document.getAnimations().filter((run) => run.animationName === 'pencil-draw').map((run) => run.effect.getComputedTiming().endTime),
    };
  }, marker);
  const pencil = (page) => page.locator('.margin-ink path.pencil');

  test('a short passage gets one pencil line under each line, drawn at a pencil pace, and the mark breathes; silence clears both', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await startRead(page, 'plain text');
      await expect(pencil(page).first()).toBeAttached();
      await expect(page.locator('.margin-waiting svg.margin-drop use[href="#leo-mark"]')).toHaveCount(1);
      expect(await page.locator('.margin-waiting .margin-drop').evaluate((el) => getComputedStyle(el).animationName)).toBe('margin-breathe');
      const shape = await shapeOf(page, 'plain text');
      expect(shape.paths.length).toBe(shape.bottoms.length);
      expect(shape.paths.length).toBeLessThan(4);
      shape.paths.forEach((line, i) => expect(line.top).toBeGreaterThanOrEqual(shape.bottoms[i]));
      expect(Math.max(...shape.draws)).toBeGreaterThanOrEqual(650);
      await page.evaluate(() => fake.answer({ note: null }));
      await expect(pencil(page)).toHaveCount(0);
      await expect(page.locator('.margin-waiting')).toHaveCount(0);
    }, { mode: 'along', threads: [] });
  });

  test('four or more lines get one side line left of the text, and a note or a failure removes it at once', async ({ browser }) => {
    for (const answer of [{ note: { id: 't5', quote: 'A long paragraph begins here', prefix: '\n\n', suffix: ' and', status: 'open', messages: [{ from: 'felix', text: 'Long.' }] } }, { note: null, failed: true }]) {
      await fixture(browser, async (page) => {
        await startRead(page, 'word0');
        await expect(pencil(page)).toHaveCount(1);
        const shape = await shapeOf(page, 'word0');
        expect(shape.bottoms.length).toBeGreaterThanOrEqual(4);
        const [side] = shape.paths;
        expect(side.height).toBeGreaterThan(side.width * 4);
        expect(side.right).toBeLessThanOrEqual(shape.left);
        await page.evaluate((got) => fake.answer(got), answer);
        await expect(pencil(page)).toHaveCount(0);
        await expect(page.locator('.margin-waiting')).toHaveCount(0);
      }, { mode: 'along', threads: [] });
    }
  });

  test('a heading with a line under it gets one stroke per line and nothing in the gap between them', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await startRead(page, 'A heading here');
      await expect(pencil(page).first()).toBeAttached();
      const shape = await shapeOf(page, 'A heading here');
      expect(shape.bottoms.length).toBe(2);
      expect(shape.paths.length).toBe(2);
      shape.paths.forEach((line, i) => expect(line.top).toBeGreaterThanOrEqual(shape.bottoms[i]));
      await page.evaluate(() => fake.answer({ note: null }));
      await expect(pencil(page)).toHaveCount(0);
    }, { mode: 'along', threads: [], source: 'Opening line of the note.\n\n## A heading here\nBody line under the heading.\n\nClosing paragraph of the note.' });
  });

  test('a block open in its textarea is read like the rendered one: one line under one line, one side line for a long one', async ({ browser }) => {
    for (const [marker, count] of [['plain text', 1], ['word0', 1]]) {
      await fixture(browser, async (page) => {
        await page.locator('#doc .blk', { hasText: marker }).click();
        await expect(page.locator('#doc textarea')).toHaveCount(1);
        await startRead(page, marker);
        await expect(pencil(page)).toHaveCount(count);
        const [line] = (await shapeOf(page, marker)).paths;
        if (marker === 'word0') expect(line.height).toBeGreaterThan(line.width * 4);
        else expect(line.width).toBeGreaterThan(line.height * 4);
        await page.evaluate(() => fake.answer({ note: null }));
        await expect(pencil(page)).toHaveCount(0);
      }, { mode: 'along', threads: [] });
    }
  });

  test('editing during a read takes the pencil and mark away and the late answer brings no stray line', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await startRead(page, 'plain text');
      await expect(pencil(page).first()).toBeAttached();
      await page.evaluate(() => marginEdited(session));
      await expect(pencil(page)).toHaveCount(0);
      await expect(page.locator('.margin-waiting')).toHaveCount(0);
      await page.evaluate(() => fake.answer({ note: null }));
      await page.waitForTimeout(300);
      await expect(pencil(page)).toHaveCount(0);
      await expect(page.locator('.margin-waiting')).toHaveCount(0);
    }, { mode: 'along', threads: [] });
  });

  test('reduced motion draws the pencil and the mark still', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await startRead(page, 'plain text');
      await expect(pencil(page).first()).toBeAttached();
      await expect(page.locator('.margin-waiting')).toHaveCount(1);
      expect(await page.evaluate(() => document.getAnimations().filter((run) => run.playState === 'running').length)).toBe(0);
      await page.evaluate(() => fake.answer({ note: null }));
      await expect(pencil(page)).toHaveCount(0);
    }, { mode: 'along', threads: [], context: { reducedMotion: 'reduce' } });
  });
});

test.describe('whole notes and pins', () => {
  const LONG = { ...THREADS.short, messages: [{ from: 'felix', text: 'Hash it before you store it. Use a slow hash such as Argon2id with a unique salt for each user, and verify at login by hashing the attempt the same way.' }] };
  test('a closed note shows its whole text and dims; opening shows the conversation right of a thin rule, and a pin stays undimmed', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const note = page.locator('.margin-note[data-id="t1"]');
      const said = note.locator('.margin-said');
      const line = await note.locator('.margin-felix').evaluate((el) => parseFloat(getComputedStyle(el).lineHeight));
      const height = () => said.evaluate((el) => el.getBoundingClientRect().height);
      const opacity = () => note.evaluate((el) => Number(getComputedStyle(el).opacity));
      await expect.poll(height).toBeGreaterThan(line * 2);
      expect(await note.locator('.margin-felix').evaluate((el) => el.scrollHeight <= el.clientHeight + 1)).toBe(true);
      await expect(note.locator('.margin-cut')).toHaveCount(0);
      await expect.poll(opacity).toBeCloseTo(0.7, 2);
      await note.locator('.margin-toggle').click();
      await expect(note).toHaveAttribute('data-open', '');
      expect(await height()).toBeGreaterThan(line * 2);
      expect(await said.evaluate((el) => [getComputedStyle(el).borderLeftWidth, getComputedStyle(el).borderLeftStyle])).toEqual(['1px', 'solid']);
      await note.getByRole('button', { name: 'Pin open' }).click();
      const pin = note.getByRole('button', { name: 'Unpin' });
      await expect(pin).toHaveAttribute('aria-pressed', 'true');
      expect(await pin.evaluate((el) => getComputedStyle(el.querySelectorAll('svg path')[1]).fill)).not.toBe('none');
      expect(await pin.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe('rgba(0, 0, 0, 0)');
      await expect.poll(() => page.evaluate(() => fake.posts.at(-1))).toEqual({ pinned: true });
      await page.mouse.click(5, 5);
      await expect(note).not.toHaveAttribute('data-open', '');
      await expect(note).toHaveAttribute('data-pinned', '');
      expect(await height()).toBeGreaterThan(line * 2);
      await expect.poll(opacity).toBe(1);
    }, { threads: [LONG] });
  });

  test('a long closed note stops at twelve lines with a fade, and a figure always shows whole', async ({ browser }) => {
    const words = Array.from({ length: 400 }, (_, i) => `w${i}`).join(' ');
    const drawn = `${words}\n\n\`\`\`svg\n<svg viewBox="0 0 40 400" xmlns="http://www.w3.org/2000/svg"><rect width="40" height="400"/></svg>\n\`\`\``;
    await fixture(browser, async (page) => {
      const note = page.locator('.margin-note[data-id="t1"]');
      const text = note.locator('.margin-felix > p').first();
      await expect(text).toHaveClass(/margin-cut/);
      const line = await note.locator('.margin-felix').evaluate((el) => parseFloat(getComputedStyle(el).lineHeight));
      expect((await text.boundingBox()).height).toBeLessThanOrEqual(line * 12 + 1);
      const figure = note.locator('.margin-felix figure').first();
      expect(await figure.evaluate((el) => el.scrollHeight <= el.clientHeight + 1 && getComputedStyle(el).maxHeight === 'none')).toBe(true);
      await page.evaluate(() => marginOpen('t1', false));
      await expect(note.locator('.margin-cut')).toHaveCount(0);
    }, { threads: [{ ...THREADS.short, messages: [{ from: 'felix', text: drawn }] }] });
  });

  test('twelve lines cap the closed note\'s text as a whole across paragraphs, and the figure after them stays whole', async ({ browser }) => {
    const para = (tag) => Array.from({ length: 70 }, (_, i) => `${tag}${i}`).join(' ');
    const drawn = `${para('a')}\n\n${para('b')}\n\n${para('c')}\n\n\`\`\`svg\n<svg viewBox="0 0 40 200" xmlns="http://www.w3.org/2000/svg"><rect width="40" height="200"/></svg>\n\`\`\``;
    await fixture(browser, async (page) => {
      const note = page.locator('.margin-note[data-id="t1"]');
      const line = await note.locator('.margin-felix').evaluate((el) => parseFloat(getComputedStyle(el).lineHeight));
      const visible = await note.locator('.margin-felix > p').evaluateAll((els) => els.map((el) => el.getBoundingClientRect().height));
      expect(visible[0] + visible[1] + visible[2]).toBeLessThanOrEqual(line * 12 + 1);
      expect(visible[0]).toBeGreaterThan(line * 3);
      expect(visible[2]).toBe(0);
      await expect(note.locator('.margin-felix > p').nth(1)).toHaveClass(/margin-cut/);
      const figure = note.locator('.margin-felix figure').first();
      expect(await figure.evaluate((el) => el.scrollHeight <= el.clientHeight + 1 && el.getBoundingClientRect().height > 0)).toBe(true);
    }, { threads: [{ ...THREADS.short, messages: [{ from: 'felix', text: drawn }] }] });
  });

  test('at 1600 px with the comfortable column an open note is wider than 300 px, has no close cross and keeps every action on one row', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const note = page.locator('.margin-note[data-id="t1"]');
      await page.evaluate(() => { document.documentElement.dataset.noteWidth = 'comfortable'; marginRedraw(); });
      await expect.poll(async () => (await note.boundingBox()).width).toBeGreaterThan(300);
      await page.evaluate(() => marginOpen('t1', true));
      await expect(note).toHaveAttribute('data-open', '');
      await expect(note.locator('.margin-toggle svg')).toHaveCount(0);
      const tops = await note.locator('.margin-acts button:visible').evaluateAll((els) => els.map((el) => el.getBoundingClientRect().top));
      expect(tops.length).toBe(5);
      expect(Math.max(...tops) - Math.min(...tops)).toBeLessThanOrEqual(2);
      await page.keyboard.press('Escape');
      await expect(note).not.toHaveAttribute('data-open', '');
    }, { width: 1600, height: 900 });
  });

  test('in a narrow margin only pin and chat wrap, to the end of a second row', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const note = page.locator('.margin-note[data-id="t1"]');
      expect((await note.boundingBox()).width).toBeLessThan(290);
      await page.evaluate(() => marginOpen('t1', false));
      await expect(note).toHaveAttribute('data-open', '');
      const box = await note.locator('.margin-acts').evaluate((acts) => {
        const at = (sel) => acts.querySelector(sel).getBoundingClientRect();
        return { resolve: at('[data-margin-act="resolve"]'), add: at('.margin-add'), pin: at('.margin-pin'), chat: at('.margin-chat'), acts: acts.getBoundingClientRect() };
      });
      expect(Math.abs(box.resolve.top - box.add.top)).toBeLessThanOrEqual(2);
      expect(box.pin.top).toBeGreaterThan(box.add.bottom - 2);
      expect(Math.abs(box.pin.top - box.chat.top)).toBeLessThanOrEqual(2);
      expect(box.chat.right).toBeGreaterThan(box.acts.right - 4);
    });
  });

  test('an open note closes with Escape while focus is on the page, and with an outside click even when a reply is typed, keeping the text', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const note = page.locator('.margin-note[data-id="t1"]');
      await page.evaluate(() => marginOpen('t1', false));
      await expect(note).toHaveAttribute('data-open', '');
      await page.evaluate(() => document.activeElement.blur());
      expect(await page.evaluate(() => document.activeElement === document.body)).toBe(true);
      await page.keyboard.press('Escape');
      await expect(note).not.toHaveAttribute('data-open', '');
      await page.evaluate(() => marginOpen('t1', true));
      await expect(note).toHaveAttribute('data-open', '');
      await note.locator('textarea').fill('half a thought');
      await page.mouse.click(5, 5);
      await expect(note).not.toHaveAttribute('data-open', '');
      await page.evaluate(() => marginOpen('t1', false));
      await expect(note).toHaveAttribute('data-open', '');
      await expect(note.locator('textarea')).toHaveValue('half a thought');
    });
  });
});

test.describe('passage states', () => {
  test('a lit note draws a thin tether from the end of its passage to the note, never over the note', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const tether = page.locator('.margin-ink path.tether[data-id="t1"]');
      await expect(tether).toHaveCount(0);
      await page.locator('.margin-note[data-id="t1"]').hover();
      await expect(tether).toHaveCount(1);
      await expect.poll(() => tether.evaluate((p) => Number(getComputedStyle(p).opacity))).toBeGreaterThan(0.3);
      const line = await tether.boundingBox();
      const end = await page.evaluate((quote) => {
        const walk = document.createTreeWalker(document.querySelector('#doc'), NodeFilter.SHOW_TEXT);
        for (let node = walk.nextNode(); node; node = walk.nextNode()) {
          const at = node.data.indexOf(quote);
          if (at < 0) continue;
          const range = document.createRange();
          range.setStart(node, at);
          range.setEnd(node, at + quote.length);
          const rects = [...range.getClientRects()];
          return rects[rects.length - 1].right;
        }
        return null;
      }, THREADS.short.quote);
      const note = await page.locator('.margin-note[data-id="t1"]').boundingBox();
      expect(end).not.toBeNull();
      expect(line.x).toBeGreaterThanOrEqual(end - 1);
      expect(line.x).toBeLessThanOrEqual(end + 10);
      expect(line.x + line.width).toBeLessThanOrEqual(note.x);
      await page.mouse.move(5, 5);
      await expect(tether).toHaveCount(0);
    }, { threads: [THREADS.short] });
  });

  test('a pinned note keeps a faint tether at rest that brightens on hover', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const tether = page.locator('.margin-ink path.tether[data-id="t1"]');
      await expect(tether).toHaveCount(1);
      await expect.poll(() => tether.evaluate((p) => Number(getComputedStyle(p).opacity))).toBeLessThan(0.3);
      await page.locator('.margin-note[data-id="t1"]').hover();
      await expect.poll(() => tether.evaluate((p) => Number(getComputedStyle(p).opacity))).toBeGreaterThan(0.3);
      await page.mouse.move(5, 5);
      await expect(tether).toHaveCount(1);
    }, { threads: [{ ...THREADS.short, pinned: true }] });
  });

  test('quiet dotted at rest, solid underline and a subtle wash when open, hovered or focused', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const rest = await page.evaluate(stateOf, 't1');
      expect(rest.solid).toBeLessThan(0.05);
      expect(rest.wash).toBeLessThan(0.02);
      expect(rest.dash).toMatch(/^1(px)?[ ,]+3/);
      const box = (await page.evaluate(() => rectsOf('t1')))[0];
      await page.mouse.move(box.left + 20, (box.top + box.bottom) / 2);
      await expect.poll(async () => (await page.evaluate(stateOf, 't1')).solid).toBeGreaterThan(0.95);
      const hovered = await page.evaluate(stateOf, 't1');
      expect(hovered.wash).toBeGreaterThan(0.02);
      expect(hovered.wash).toBeLessThan(0.25);
      expect(hovered.solidDash).toBe('none');
      expect(hovered.dash).toMatch(/^1(px)?[ ,]+3/);
      await page.mouse.move(5, 5);
      await expect.poll(async () => (await page.evaluate(stateOf, 't1')).solid).toBeLessThan(0.05);
      await page.locator('.margin-note[data-id="t1"] .margin-toggle').focus();
      await page.keyboard.press('ArrowRight');
      await expect.poll(async () => (await page.evaluate(stateOf, 't1')).solid).toBeGreaterThan(0.95);
      const outline = await page.locator('.margin-note[data-id="t1"] .margin-toggle').evaluate((el) => getComputedStyle(el).outlineStyle);
      expect(outline).not.toBe('none');
      await page.locator('.margin-note[data-id="t1"] .margin-toggle').click();
      await expect(page.locator('.margin-note[data-id="t1"]')).toHaveAttribute('data-open', '');
      await expect(page.locator('.margin-note[data-id="t1"] textarea')).toBeFocused();
      await page.mouse.move(5, 5);
      await expect.poll(async () => (await page.evaluate(stateOf, 't1')).solid).toBeGreaterThan(0.95);
      expect((await page.evaluate(stateOf, 't1')).wash).toBeGreaterThan(0.02);
      await page.keyboard.press('Escape');
      await expect(page.locator('.margin-note[data-id="t1"]')).not.toHaveAttribute('data-open', '');
      await page.locator('body').click({ position: { x: 3, y: 3 } });
      await expect.poll(async () => (await page.evaluate(stateOf, 't1')).solid).toBeLessThan(0.05);
    });
  });

  test('answer textarea focus marks its passage active even when it was not opened by a click', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await page.evaluate(() => { marginOpen('t1', false); });
      await page.mouse.move(5, 5);
      await page.locator('.margin-note[data-id="t1"] textarea').focus();
      await expect.poll(async () => (await page.evaluate(stateOf, 't1')).solid).toBeGreaterThan(0.95);
      expect((await page.evaluate(stateOf, 't2')).solid).toBeLessThan(0.05);
    });
  });

  test('forced colors state uses system highlight colors', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await page.locator('.margin-note[data-id="t1"] .margin-toggle').click();
      await expect.poll(async () => (await page.evaluate(stateOf, 't1')).solid).toBeGreaterThan(0.95);
      const stroke = await page.evaluate(() => getComputedStyle(document.querySelector('.margin-ink path.solid[data-id="t1"]')).stroke);
      expect(stroke).not.toMatch(/rgba\(0, 0, 0, 0\)/);
    }, { context: { forcedColors: 'active' } });
  });
});

test.describe('tether geometry', () => {
  const LIST = 'Opening line of the note.\n\n- a mesh office chair\n- a nice unique and short couch\n- a low futon bed\n\nClosing words that sit in a plain paragraph of the note.';
  const thread = (id, quote, prefix, suffix) => ({ id, quote, prefix, suffix, status: 'open', messages: [{ from: 'felix', text: 'A note.' }] });
  const SPAN = thread('t2', 'office chair\n- a nice unique and short couch\n- a low futon bed', '- a mesh ', '\n\nClosing words');
  const ITEM = thread('t1', 'a nice unique and short couch', 'chair\n- ', '\n- a low');
  const MIDDLE = thread('t3', 'sit in a plain paragraph', 'Closing words that ', ' of the note.');
  const ends = (page, id) => page.evaluate((key) => {
    const svg = document.querySelector('.margin-ink').getBoundingClientRect();
    const at = (p, n) => { const pt = p.getPointAtLength(n * p.getTotalLength()); return { x: pt.x + svg.left, y: pt.y + svg.top }; };
    const tie = document.querySelector(`.margin-ink path.tie[data-id="${key}"]`);
    const spine = document.querySelector(`.margin-ink path.spine[data-id="${key}"]`);
    return { tie: tie && [at(tie, 0), at(tie, 1)], spine: spine && [at(spine, 0), at(spine, 1)] };
  }, id);
  const items = (page) => page.evaluate(() => [...document.querySelectorAll('#doc li')].map((li) => {
    const range = document.createRange();
    range.selectNodeContents(li);
    const text = range.getBoundingClientRect();
    return { left: text.left, right: text.right, top: text.top, bottom: text.bottom, size: parseFloat(getComputedStyle(li).fontSize) };
  }));

  test('a list passage washes only its text lines and ties to the note from a spine at its middle', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const rects = await page.evaluate(() => rectsOf('t2'));
      const lines = await items(page);
      expect(rects).toHaveLength(3);
      for (const [i, r] of rects.entries()) {
        expect(r.left).toBeGreaterThanOrEqual(lines[i].left - 1);
        expect(r.right).toBeLessThanOrEqual(lines[i].right + 4);
      }
      const wash = await page.locator('.margin-ink path.wash[data-id="t2"]').boundingBox();
      expect(wash.width).toBeLessThanOrEqual(Math.max(...lines.map((l) => l.right - l.left)) + 8);
      await page.locator('.margin-note[data-id="t2"]').hover();
      await expect(page.locator('.margin-ink path.spine[data-id="t2"]')).toHaveCount(1);
      await page.waitForTimeout(900);
      const { tie, spine } = await ends(page, 't2');
      const middle = (rects[0].top + rects[2].bottom) / 2;
      const column = await page.locator('article.note').boundingBox();
      expect(Math.abs(tie[0].y - middle)).toBeLessThanOrEqual(2);
      expect(Math.abs(tie[1].y - middle)).toBeLessThanOrEqual(2);
      expect(Math.abs(tie[0].x - (column.x + column.width + 8))).toBeLessThanOrEqual(1);
      expect(Math.abs(spine[0].x - tie[0].x)).toBeLessThanOrEqual(1);
      expect(Math.abs(spine[0].y - rects[0].top)).toBeLessThanOrEqual(1);
      expect(Math.abs(spine[1].y - rects[2].bottom)).toBeLessThanOrEqual(1);
    }, { source: LIST, threads: [SPAN] });
  });

  test('the underline and spine wobble slightly, the same on every repaint, and keep their ends', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await page.locator('.margin-note[data-id="t2"]').hover();
      await expect(page.locator('.margin-ink path.spine[data-id="t2"]')).toHaveCount(1);
      const shape = () => page.evaluate(() => ['solid', 'spine'].map((cls) => document.querySelector(`.margin-ink path.${cls}[data-id="t2"]`).getAttribute('d')));
      const [solid, spine] = await shape();
      expect(solid).toMatch(/q/);
      expect(spine).toMatch(/q/);
      await page.evaluate(() => { marginRedraw(); window.scrollBy(0, 1); });
      await page.waitForTimeout(100);
      expect(await shape()).toEqual([solid, spine]);
      const rects = await page.evaluate(() => rectsOf('t2'));
      const run = await page.evaluate(() => {
        const svg = document.querySelector('.margin-ink').getBoundingClientRect();
        const p = document.querySelector('.margin-ink path.solid[data-id="t2"]');
        const first = p.getPointAtLength(0);
        return { x: first.x + svg.left, y: first.y + svg.top };
      });
      expect(Math.abs(run.x - rects[0].left)).toBeLessThanOrEqual(1);
      expect(Math.abs(run.y - (rects[0].bottom + 4))).toBeLessThanOrEqual(1);
    }, { source: LIST, threads: [SPAN] });
  });

  test('a one-line passage has a tie and no spine, starting just past its end', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const [r] = await page.evaluate(() => rectsOf('t1'));
      await page.locator('.margin-note[data-id="t1"]').hover();
      await expect(page.locator('.margin-ink path.tie[data-id="t1"]')).toHaveCount(1);
      await expect(page.locator('.margin-ink path.spine')).toHaveCount(0);
      const { tie } = await ends(page, 't1');
      expect(Math.abs(tie[0].x - (r.right + 8))).toBeLessThanOrEqual(1);
      expect(Math.abs(tie[0].y - (r.top + r.bottom) / 2)).toBeLessThanOrEqual(2);
    }, { source: LIST, threads: [ITEM] });
  });

  const markerLeft = (page, index) => page.evaluate((i) => {
    const li = document.querySelectorAll('#doc li')[i];
    const list = li.parentElement.cloneNode(false);
    const item = li.cloneNode(false);
    const probe = document.createElement('span');
    probe.style.cssText = 'display: inline-block; width: 0; height: 0';
    item.style.cssText = 'margin: 0; padding: 0; list-style-position: inside';
    list.style.cssText = 'position: absolute; visibility: hidden; margin: 0; padding: 0';
    item.append(probe);
    list.append(item);
    li.parentElement.parentElement.append(list);
    const width = probe.getBoundingClientRect().left - item.getBoundingClientRect().left;
    list.remove();
    return li.getBoundingClientRect().left - width;
  }, index);

  test('on the left a list tie ends at the measured bullet and a mid-line tie stops outside the text', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await page.evaluate(() => { marginPage.sides = { t1: 'left', t3: 'left' }; marginRedraw(); });
      await expect.poll(async () => (await page.locator('.margin-note[data-id="t1"]').boundingBox()).x).toBeLessThan((await page.locator('#doc').boundingBox()).x);
      const line = (await items(page))[1];
      await page.evaluate(() => { marginPage.hover = 't1'; marginPage.focus = 't3'; marginPaint(); });
      const item = (await ends(page, 't1')).tie[0];
      const bullet = await markerLeft(page, 1);
      expect(bullet).toBeLessThan(line.left - 4);
      expect(Math.abs(item.x - (bullet - 3))).toBeLessThanOrEqual(1);
      expect(Math.abs(item.y - (line.top + line.bottom) / 2)).toBeLessThanOrEqual(2);
      const middle = (await ends(page, 't3')).tie[0];
      const block = await page.locator('#doc .blk').last().boundingBox();
      expect(middle.x).toBeLessThan(block.x - 4);
    }, { width: 1600, height: 900, layout: 'both', source: LIST, threads: [ITEM, MIDDLE] });
  });

  test('on the left a numbered item ties to its measured number', async ({ browser }) => {
    const NUMBERED = '1. first thing here\n2. second thing here\n3. third thing here';
    await fixture(browser, async (page) => {
      await page.evaluate(() => { marginPage.sides = { t5: 'left' }; marginRedraw(); });
      await expect.poll(async () => (await page.locator('.margin-note[data-id="t5"]').boundingBox()).x).toBeLessThan((await page.locator('#doc').boundingBox()).x);
      await page.evaluate(() => { marginPage.hover = 't5'; marginPaint(); });
      await expect(page.locator('.margin-ink path.tie[data-id="t5"]')).toHaveCount(1);
      const { tie } = await ends(page, 't5');
      expect(Math.abs(tie[0].x - ((await markerLeft(page, 1)) - 3))).toBeLessThanOrEqual(1);
    }, { width: 1600, height: 900, layout: 'both', source: NUMBERED, threads: [thread('t5', 'second thing', '2. ', ' here\n3.')] });
  });

  test('a passage that starts in another block and runs into a list ends its tie at the bullets', async ({ browser }) => {
    const QUOTED = '> A quoted line before the list.\n\n- a mesh office chair\n- a nice unique and short couch\n\nClosing words.';
    const ACROSS = thread('t4', 'before the list.\n\n- a mesh office chair\n- a nice unique', 'A quoted line ', ' and short couch');
    await fixture(browser, async (page) => {
      await page.evaluate(() => { marginPage.sides = { t4: 'left' }; marginRedraw(); });
      await expect.poll(async () => (await page.locator('.margin-note[data-id="t4"]').boundingBox()).x).toBeLessThan((await page.locator('#doc').boundingBox()).x);
      await page.evaluate(() => { marginPage.hover = 't4'; marginPaint(); });
      await expect(page.locator('.margin-ink path.spine[data-id="t4"]')).toHaveCount(1);
      const { tie, spine } = await ends(page, 't4');
      const bullet = await markerLeft(page, 0);
      expect(Math.abs(spine[0].x - (bullet - 3))).toBeLessThanOrEqual(1);
      expect(Math.abs(tie[0].x - spine[0].x)).toBeLessThanOrEqual(1);
    }, { width: 1600, height: 900, layout: 'both', source: QUOTED, threads: [ACROSS] });
  });

  test('a lit tether draws once, tie then spine, and a repaint never restarts it', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await page.locator('.margin-note[data-id="t2"]').hover();
      const tie = page.locator('.margin-ink path.tie[data-id="t2"]');
      const spine = page.locator('.margin-ink path.spine[data-id="t2"]');
      await expect(tie).toHaveClass(/sketching/);
      await expect(spine).toHaveClass(/sketching/);
      expect(await tie.evaluate((p) => [p.getAttribute('pathLength'), getComputedStyle(p).animationName, getComputedStyle(p).animationDuration])).toEqual(['1', 'pencil-draw', '0.45s']);
      expect(await spine.evaluate((p) => [getComputedStyle(p).animationDuration, getComputedStyle(p).animationDelay])).toEqual(['0.35s', '0.45s']);
      await tie.evaluate((p) => { p.dataset.probe = 'kept'; });
      const before = await tie.evaluate((p) => p.getAnimations()[0].currentTime);
      await page.evaluate(() => { marginRedraw(); window.scrollBy(0, 1); });
      await page.waitForTimeout(100);
      await expect(tie).toHaveAttribute('data-probe', 'kept');
      expect(await tie.evaluate((p) => p.getAnimations().length ? p.getAnimations()[0].currentTime : Infinity)).toBeGreaterThan(before);
      await page.mouse.move(5, 5);
      await expect(tie).toHaveCount(0);
      await page.locator('.margin-note[data-id="t2"]').hover();
      await expect(tie).toHaveClass(/sketching/);
      await expect(tie).not.toHaveAttribute('data-probe', 'kept');
    }, { source: LIST, threads: [SPAN] });
    await fixture(browser, async (page) => {
      await page.locator('.margin-note[data-id="t2"]').hover();
      await expect(page.locator('.margin-ink path.tie[data-id="t2"]')).toHaveCount(1);
      await expect(page.locator('.margin-ink path.tether.sketching')).toHaveCount(0);
    }, { source: LIST, threads: [SPAN], context: { reducedMotion: 'reduce' } });
  });
});

test.describe('compact gutter anchor', () => {
  for (const [name, context] of [['touch', { hasTouch: true, isMobile: true }], ['touch and reduced motion', { hasTouch: true, isMobile: true, reducedMotion: 'reduce' }], ['touch and forced colors', { hasTouch: true, isMobile: true, forcedColors: 'active' }]]) {
    test(`390px ${name}: the 44px target fits the screen, the dot sits outside the text at the passage centre`, async ({ browser }) => {
      await fixture(browser, async (page) => {
        expect(await page.evaluate(() => matchMedia('(pointer: coarse)').matches)).toBe(true);
        for (const id of ['t1', 't2']) {
          const mark = page.locator(`.margin-mark[data-id="${id}"]`);
          await expect(mark).toBeVisible();
          const hit = await mark.boundingBox();
          const dot = await mark.locator('.margin-anchor').boundingBox();
          const rects = await page.evaluate((key) => rectsOf(key), id);
          const text = await page.locator('#doc .blk').nth(id === 't1' ? 1 : 2).boundingBox();
          expect(hit.width).toBeGreaterThanOrEqual(44);
          expect(hit.height).toBeGreaterThanOrEqual(44);
          expect(hit.x).toBeGreaterThanOrEqual(0);
          expect(hit.x + hit.width).toBeLessThanOrEqual(390);
          expect(hit.x + hit.width).toBeLessThanOrEqual(text.x + 0.5);
          expect(dot.x + dot.width).toBeLessThanOrEqual(text.x);
          const top = Math.min(...rects.map((r) => r.top));
          const bottom = Math.max(...rects.map((r) => r.bottom));
          const middle = dot.y + dot.height / 2;
          expect(middle).toBeGreaterThanOrEqual(top - 2);
          expect(middle).toBeLessThanOrEqual(bottom + 2);
          if (rects.length === 1) expect(Math.abs(middle - (top + bottom) / 2)).toBeLessThan(10);
        }
        const overlap = await page.evaluate(() => {
          const [a, b] = ['t1', 't2'].map((id) => document.querySelector(`.margin-mark[data-id="${id}"]`).getBoundingClientRect());
          return a.bottom > b.top + 0.5 && b.bottom > a.top + 0.5;
        });
        expect(overlap).toBe(false);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
        await page.locator('.margin-mark[data-id="t1"]').tap();
        await expect(page.locator('.margin-note[data-id="t1"] textarea')).toBeFocused();
        const note = await page.locator('.margin-note[data-id="t1"]').boundingBox();
        expect(note.x).toBeGreaterThanOrEqual(0);
        expect(note.x + note.width).toBeLessThanOrEqual(390);
      }, { width: 390, height: 844, layout: 'marks', context });
    });
  }

  test('desktop marks keep a 24px target inside the window and outside the text', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const mark = await page.locator('.margin-mark[data-id="t1"]').boundingBox();
      const text = await page.locator('#doc .blk').nth(1).boundingBox();
      expect(mark.x).toBeGreaterThanOrEqual(0);
      expect(mark.x + mark.width).toBeLessThanOrEqual(text.x + 0.5);
    }, { width: 600, height: 800, layout: 'marks' });
  });
});

test.describe('margin notes carry no mascot', () => {
  test('neither the closed, open nor inline note draws a Felix face', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await expect(page.locator('.margin-note')).toHaveCount(2);
      expect(await page.locator('.margin-note [data-mascot], .margin-note .felix, .margin-head').count()).toBe(0);
      await page.locator('.margin-note[data-id="t1"] .margin-toggle').click();
      expect(await page.locator('.margin-note [data-mascot], .margin-note .felix, .margin-head').count()).toBe(0);
      await expect(page.locator('.margin-note[data-id="t1"] .margin-felix')).toContainText('Hash it');
      await expect(page.locator('.margin-note[data-id="t1"] textarea')).toBeVisible();
      await expect(page.getByRole('button', { name: 'Add to note' }).first()).toBeVisible();
    });
    await fixture(browser, async (page) => {
      await page.locator('.margin-mark[data-id="t1"]').click();
      expect(await page.locator('.margin-note [data-mascot], .margin-note .felix, .margin-head').count()).toBe(0);
    }, { width: 390, height: 844, layout: 'marks', context: { hasTouch: true, isMobile: true } });
  });
});

test.describe('reading-mode menu', () => {
  test('chevron points down closed and up open, and the mode icon follows the mode', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const turn = () => page.locator('#margin-mode > svg').evaluate((el) => {
        const m = new DOMMatrix(getComputedStyle(el).transform);
        return Math.round(Math.atan2(m.b, m.a) * 180 / Math.PI);
      });
      expect(await turn()).toBe(90);
      await page.locator('#margin-mode').click();
      await expect.poll(turn).toBe(-90);
      await page.keyboard.press('Escape');
      await expect.poll(turn).toBe(90);
      const icon = () => page.locator('#margin-mode-icon').evaluate((el) => ({ html: el.innerHTML, turned: el.querySelector('svg') ? getComputedStyle(el.querySelector('svg')).transform : 'none' }));
      const asking = await icon();
      expect(asking.turned).toBe('none');
      expect(await page.locator('#margin-mode').getAttribute('data-mode')).toBe('ask');
      await page.locator('#margin-mode').click();
      await page.getByRole('menuitemradio', { name: 'Reads along' }).click();
      await expect(page.locator('#margin-mode')).toHaveAttribute('data-mode', 'along');
      const reading = await icon();
      expect(reading.html).not.toBe(asking.html);
      expect(reading.html).toContain('#leo-mark');
      expect(reading.turned).toBe('none');
    });
  });

  test('Tab dismisses the menu without trapping focus; arrows, Home, End, Enter and Escape still work', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const button = page.locator('#margin-mode');
      const menu = page.getByRole('menu', { name: 'Note reading mode' });
      await button.click();
      await expect(menu).toBeVisible();
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('Home');
      await expect(page.getByRole('menuitemradio', { name: /Reads along/ })).toBeFocused();
      await page.keyboard.press('End');
      await expect(page.getByRole('menuitemradio', { name: /When you ask/ })).toBeFocused();
      await page.keyboard.press('ArrowUp');
      await expect(page.getByRole('menuitemradio', { name: /Reads along/ })).toBeFocused();
      await page.keyboard.press('Tab');
      await expect(menu).toBeHidden();
      await expect(button).toHaveAttribute('aria-expanded', 'false');
      expect(await page.evaluate(() => Boolean(document.activeElement?.closest('#margin-mode-menu')))).toBe(false);
      const after = await page.evaluate(() => document.activeElement?.id || document.activeElement?.tagName);
      expect(after).not.toBe('BODY');
      await page.keyboard.press('Tab');
      expect(await page.evaluate(() => Boolean(document.activeElement?.closest('.margin-mode-wrap')))).toBe(false);
      await button.focus();
      await page.keyboard.press('ArrowDown');
      await expect(menu).toBeVisible();
      await page.keyboard.press('End');
      await page.keyboard.press('Enter');
      await expect(button).toHaveText(/When you ask/);
      await expect(button).toBeFocused();
      await expect(menu).toBeHidden();
    });
  });

  test('a failed save keeps the confirmed mode and closes the menu', async ({ browser }) => {
    await fixture(browser, async (page) => {
      await page.evaluate(() => { window.fake.failMode = true; });
      await page.locator('#margin-mode').click();
      await page.getByRole('menuitemradio', { name: /Reads along/ }).click();
      await expect(page.locator('#margin-status')).toHaveText('Could not save this mode. Try again.');
      await expect(page.locator('#margin-mode')).toHaveText(/When you ask/);
      await expect(page.locator('#margin-mode')).toHaveAttribute('data-mode', 'ask');
      await expect(page.getByRole('menu', { name: 'Note reading mode' })).toBeHidden();
    });
  });

  test('opening and closing animate, reverse mid-flight, and snap with reduced motion', async ({ browser }) => {
    await fixture(browser, async (page) => {
      const menu = page.locator('#margin-mode-menu');
      const pressAndSeek = () => page.evaluate(() => {
        document.querySelector('#margin-mode').click();
        const el = document.querySelector('#margin-mode-menu');
        const [run] = el.getAnimations();
        run.pause();
        run.currentTime = 30;
        const bottom = parseFloat(getComputedStyle(el).clipPath.match(/inset\(([^)]*)\)/)[1].trim().split(/\s+/)[2]);
        run.play();
        return { hidden: el.hidden, bottom };
      });
      const settledMenu = () => menu.evaluate((el) => el.getAnimations().length);
      const opening = await pressAndSeek();
      expect(opening.hidden).toBe(false);
      expect(opening.bottom).toBeGreaterThan(5);
      expect(opening.bottom).toBeLessThan(95);
      await expect.poll(settledMenu).toBe(0);
      await expect(menu).toBeVisible();
      const closing = await pressAndSeek();
      expect(closing.hidden).toBe(false);
      expect(closing.bottom).toBeGreaterThan(5);
      expect(closing.bottom).toBeLessThan(95);
      await expect(menu).toBeHidden();
      await page.evaluate(() => {
        const button = document.querySelector('#margin-mode');
        button.click();
        setTimeout(() => button.click(), 60);
        setTimeout(() => button.click(), 120);
      });
      await expect.poll(() => page.evaluate(() => document.querySelector('#margin-mode-menu').getAnimations().length)).toBe(0);
      const settled = await page.evaluate(() => ({ hidden: document.querySelector('#margin-mode-menu').hidden, expanded: document.querySelector('#margin-mode').getAttribute('aria-expanded') }));
      expect(settled.hidden).toBe(settled.expanded !== 'true');
    });
    await fixture(browser, async (page) => {
      await page.locator('#margin-mode').click();
      expect(await page.evaluate(() => ({ hidden: document.querySelector('#margin-mode-menu').hidden, running: document.querySelector('#margin-mode-menu').getAnimations().length }))).toEqual({ hidden: false, running: 0 });
      await page.keyboard.press('Escape');
      expect(await page.evaluate(() => document.querySelector('#margin-mode-menu').hidden)).toBe(true);
    }, { context: { reducedMotion: 'reduce' } });
  });
});

async function plain(browser, body, run, { width = 1280, height = 800, context = {} } = {}) {
  const server = http.createServer((req, res) => {
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); return res.end(css()); }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end(`<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body>${body}</body></html>`);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const browserContext = await browser.newContext({ viewport: { width, height }, ...context });
  try {
    const page = await browserContext.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await run(page);
  } finally {
    await browserContext.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

const HISTORY = '<div class="chat with-history" style="display:flex;width:320px"><div class="chat-history" style="display:flex;width:320px"><div class="chat-history-head"><b>Chats</b></div><div class="chat-history-list"><div class="chat-history-row" id="row"><button class="chat-history-item" title="A fairly long chat title that should keep most of its words"><span class="chat-history-title">A fairly long chat title that should keep most of its words</span></button><button class="chat-history-x" id="x" aria-label="Delete chat"><span class="chat-delete-x" aria-hidden="true">×</span><span class="chat-delete-label" aria-hidden="true">Delete</span></button></div></div></div></div>';

test.describe('chat history list', () => {
  test('the heading no longer has a New chat button and titles read smaller', async ({ browser }) => {
    await plain(browser, HISTORY, async (page) => {
      const title = await page.locator('.chat-history-title').evaluate((el) => ({ size: getComputedStyle(el).fontSize, weight: Number(getComputedStyle(el).fontWeight) }));
      expect(title.size).toBe('13px');
      expect(title.weight).toBeGreaterThanOrEqual(450);
      expect(title.weight).toBeLessThanOrEqual(560);
    });
    expect(pageHtml()).toContain('.chat-history-row');
    expect(pageHtml()).not.toContain('.chat-history-new');
  });

  test('Delete grows from the cross in width while the labels cross-fade, reversing cleanly', async ({ browser }) => {
    await plain(browser, HISTORY, async (page) => {
      await page.locator('#row').hover();
      const rest = await page.locator('#x').boundingBox();
      expect(rest.width).toBeLessThan(40);
      const timeline = await frames(page, 400, () => {
        const x = document.querySelector('#x');
        return { moving: x.getAnimations().length > 0, width: x.getBoundingClientRect().width, cross: parseFloat(getComputedStyle(document.querySelector('.chat-delete-x')).opacity), label: parseFloat(getComputedStyle(document.querySelector('.chat-delete-label')).opacity) };
      }, [[30, 'document.querySelector("#x").classList.add("sure")']]);
      const last = timeline[timeline.length - 1];
      expect(last.width).toBeGreaterThan(rest.width + 20);
      expect(last.label).toBeGreaterThan(0.95);
      expect(last.cross).toBeLessThan(0.05);
      expect(timeline.some((f) => f.moving)).toBe(true);
      expect(timeline.some((f) => f.cross > 0.1 && f.cross < 0.9 || f.label > 0.1 && f.label < 0.9)).toBe(true);
      let before = 0;
      for (const frame of timeline) {
        expect(frame.width).toBeGreaterThanOrEqual(before - 0.5);
        before = frame.width;
      }
      const back = await frames(page, 400, () => ({ width: document.querySelector('#x').getBoundingClientRect().width }), [[20, 'document.querySelector("#x").classList.remove("sure")']]);
      expect(back[back.length - 1].width).toBeCloseTo(rest.width, 0);
      expect(back.some((f) => f.width > rest.width + 3 && f.width < last.width - 3)).toBe(true);
    });
  });

  test('reduced motion snaps and focus stays visible', async ({ browser }) => {
    await plain(browser, HISTORY, async (page) => {
      await page.locator('#x').evaluate((el) => el.classList.add('sure'));
      const duration = await page.locator('#x').evaluate((el) => getComputedStyle(el).transitionDuration);
      expect(duration.split(',').every((d) => parseFloat(d) === 0)).toBe(true);
      await page.locator('#x').focus();
      await page.keyboard.press('ArrowRight');
      expect(await page.locator('#x').evaluate((el) => getComputedStyle(el).outlineStyle)).not.toBe('none');
    }, { context: { reducedMotion: 'reduce' } });
  });

  test('forced colors keep the confirm state distinct', async ({ browser }) => {
    await plain(browser, HISTORY, async (page) => {
      await page.locator('#x').evaluate((el) => el.classList.add('sure'));
      await expect.poll(() => page.locator('#x').evaluate((el) => getComputedStyle(el).backgroundColor)).not.toBe('rgba(0, 0, 0, 0)');
    }, { context: { forcedColors: 'active' } });
  });
});

const PLANS = '<div class="coding-plans">' + ['codex', 'claude_code'].map((provider) => `<section class="coding-plan" data-provider="${provider}"><div class="plan-window"><div class="plan-metric"><span>Session</span><strong>70% <small>left</small></strong></div><div class="plan-track"><div class="plan-meter"><i style="width:70%"></i></div><span class="hint">Resets in 2h</span></div></div><div class="plan-window"><div class="plan-metric"><span>Weekly</span><strong>40% <small>left</small></strong></div><div class="plan-track"><div class="plan-meter"><i style="width:40%"></i></div><span class="hint">Resets in 3d</span></div></div></section>`).join('') + '</div>';

test.describe('usage tones', () => {
  for (const scheme of ['light', 'dark']) {
    test(`${scheme}: Codex and Claude keep their own tones and the second row has no rule`, async ({ browser }) => {
      await plain(browser, `<div class="scrim"><div class="sheet usage-sheet">${PLANS}</div></div>`, async (page) => {
        const tone = (provider) => page.locator(`[data-provider="${provider}"] .plan-meter i`).first().evaluate((el) => {
          const pixel = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
          pixel.canvas.width = pixel.canvas.height = 1;
          pixel.fillStyle = getComputedStyle(el).backgroundColor;
          pixel.fillRect(0, 0, 1, 1);
          const [r, g, b] = pixel.getImageData(0, 0, 1, 1).data;
          return { r, g, b, spread: Math.max(r, g, b) - Math.min(r, g, b) };
        });
        const claude = await tone('claude_code');
        const codex = await tone('codex');
        expect(claude.r).toBeGreaterThan(claude.b + 8);
        expect(claude.spread).toBeGreaterThan(8);
        const text = await page.locator('.usage-sheet').evaluate((el) => getComputedStyle(el).color);
        const fill = await page.locator('[data-provider="codex"] .plan-meter i').first().evaluate((el) => getComputedStyle(el).backgroundColor);
        expect(fill).not.toBe(text);
        expect(Math.abs(claude.r - codex.r) + Math.abs(claude.b - codex.b)).toBeGreaterThan(12);
        const hatch = await page.locator('[data-provider="claude_code"] .plan-meter').first().evaluate((el) => getComputedStyle(el).backgroundImage);
        expect(hatch).toContain('gradient');
        const second = await page.locator('[data-provider="codex"] .plan-window').nth(1).evaluate((el) => getComputedStyle(el).borderTopWidth);
        expect(second).toBe('0px');
      }, { context: { colorScheme: scheme } });
    });
  }
});

test.describe('settings layout surface', () => {
  test('the centred settings grid paints no leftover pseudo band', async ({ browser }) => {
    await plain(browser, '<main style="padding:0"><div class="settings-layout"><nav class="settings-nav">Nav</nav><div class="settings-section">Body</div></div></main>', async (page) => {
      const before = await page.locator('.settings-layout').evaluate((el) => getComputedStyle(el, '::before').content);
      expect(before).toBe('none');
    });
  });
});

test('margin replies expand as borderless retained controls and collapse out of the tab order', async ({ browser }) => {
  await fixture(browser, async page => {
    const note = page.locator('.margin-note[data-id="t1"]');
    const more = note.locator('.margin-more');
    await expect(more).toHaveAttribute('aria-hidden', 'true');
    const values = await frames(page, 500, () => {
      const el = document.querySelector('.margin-note[data-id="t1"] .margin-more');
      return { height: el.getBoundingClientRect().height, opacity: Number(getComputedStyle(el).opacity) };
    }, [[30, "marginOpen('t1', true)"]]);
    expect(values.some(frame => frame.opacity > .01 && frame.opacity < .99)).toBe(true);
    expect(values.at(-1).height).toBeGreaterThan(35);
    await expect(note.locator('textarea')).toBeFocused();
    expect(await note.locator('textarea').evaluate(el => getComputedStyle(el).outlineStyle)).toBe('none');
    expect(await note.locator('.margin-surface').count()).toBe(0);
    await expect(more).toHaveAttribute('aria-hidden', 'false');
    await expect.poll(() => more.evaluate((el) => el.getAnimations().length)).toBe(0);
    const closing = await frames(page, 500, () => {
      const el = document.querySelector('.margin-note[data-id="t1"] .margin-more');
      return { height: el.getBoundingClientRect().height, opacity: Number(getComputedStyle(el).opacity) };
    }, [[30, 'marginClose()']]);
    expect(closing.some(frame => frame.height > 1 && frame.opacity > .01 && frame.opacity < .99)).toBe(true);
    expect(closing.at(-1).height).toBe(0);
    expect(await more.evaluate(el => el.inert)).toBe(true);
    await expect(more).toHaveAttribute('aria-hidden', 'true');
  }, { threads: [THREADS.short], context: { recordVideo: { dir: test.info().outputPath('video'), size: { width: 1280, height: 800 } } } });
});

test('margin diagram answers reuse escaped Markdown figures instead of displaying source or active HTML', async ({ browser }) => {
  const text = 'The two organelles do different jobs.\n\n```mermaid\nflowchart TD\n Sun-->Sugar\n Sugar-->Energy\n```\n\n<script>window.bad=true</script>';
  const thread = { ...THREADS.short, messages: [{ from: 'felix', text }] };
  await fixture(browser, async page => {
    const answer = page.locator('.margin-note[data-id="t1"] .margin-felix');
    await expect(answer.locator('figure.diagram .diagram-src code')).toContainText('Sun-->Sugar');
    expect(await answer.locator('script').count()).toBe(0);
    expect(await page.evaluate(() => window.bad)).toBeUndefined();
    await page.evaluate(() => marginOpen('t1', true));
    await expect(answer.locator('figure.diagram')).toHaveCount(1);
    expect(await answer.locator('script').count()).toBe(0);
    await page.evaluate(() => { window.leoTheme.dark = () => false; window.leoTheme.onChange = () => {}; });
    await page.addScriptTag({ content: read('vendor/mermaid.min.js') });
    await page.addScriptTag({ content: read('app/diagrams.js') });
    await page.evaluate(() => watchDiagrams());
    await expect(answer.locator('.diagram-art svg')).toHaveCount(1);
    await expect(answer.locator('.diagram-art')).toContainText('Sugar');
    expect(await answer.locator('.diagram-art .node rect').first().evaluate(el => getComputedStyle(el).fill)).toBe('rgba(0, 0, 0, 0)');
    expect(await answer.locator('.diagram-art .nodeLabel').first().evaluate(el => getComputedStyle(el).fontFamily)).toMatch(/Georgia/);
  }, { threads: [thread] });
});

test('a fresh margin reply with a sketch draws it instead of showing its source', async ({ browser }) => {
  const sketch = 'Both make energy.\n\n```svg\n<svg viewBox="0 0 40 20" xmlns="http://www.w3.org/2000/svg"><title>Two shapes</title><circle cx="10" cy="10" r="8"/></svg>\n```';
  await fixture(browser, async page => {
    await page.evaluate(([t, reply]) => {
      api = async () => ({ ...t, messages: [...t.messages, { from: 'you', text: 'Sketch it.' }, { from: 'felix', text: reply }] });
      marginOpen('t1', true);
    }, [THREADS.short, sketch]);
    const box = page.locator('.margin-note[data-id="t1"] textarea');
    await box.fill('Sketch it.');
    await box.press('Enter');
    const answer = page.locator('.margin-note[data-id="t1"] .margin-felix').last();
    await expect(answer.locator('figure.drawing img')).toHaveCount(1);
    await expect(answer).not.toContainText('```');
  });
});

test('every open thread offers the chat fallback, pin and chat close the action row, and an ask answers in place', async ({ browser }) => {
  await fixture(browser, async page => {
    await page.evaluate(() => marginOpen('t1', true));
    const note = page.locator('.margin-note[data-id="t1"]');
    await expect(note.getByRole('button', { name: 'Ask in chat' })).toBeVisible();
    await expect(note.locator('.margin-acts button:visible')).toHaveText(['Resolve', 'Dismiss', 'Add to note', '', '']);
    await expect(note.locator('.margin-acts > .margin-tools > button:visible')).toHaveCount(2);
    await expect(note.locator('.margin-tools > :last-child')).toHaveAttribute('aria-label', 'Ask in chat');
    await page.evaluate(() => {
      fake.delay = 300;
      const text = session.doc.source().slice(0, 12);
      marginAsk(session, { text, from: 0, to: 12 });
      window.draftEl = document.querySelector('.margin-note[data-id="draft"]');
    });
    const draft = page.locator('.margin-note[data-id="draft"]');
    await expect(draft.getByRole('button', { name: 'Ask in chat' })).toBeVisible();
    await expect(draft.locator('.margin-pending .margin-thinking svg.margin-drop use[href="#leo-mark"]')).toHaveCount(1);
    await expect(draft.getByRole('status', { name: 'Felix is thinking' })).toBeVisible();
    await expect(draft.getByRole('button', { name: 'Cancel' })).toBeHidden();
    const answered = page.locator('.margin-note[data-id="t9"]');
    await expect(answered.locator('.margin-felix')).toHaveText('A fake answer.');
    await expect(answered.locator('.margin-pending')).toBeHidden();
    expect(await page.evaluate(() => window.draftEl === document.querySelector('.margin-note[data-id="t9"]'))).toBe(true);
    expect(await page.evaluate(() => fake.posts.at(-1).question)).toBe('');
    await expect(answered.getByRole('button', { name: 'Ask in chat' })).toBeVisible();
  }, { threads: [THREADS.short] });
});

test('a closed thread with replies says how many more and marks-only shows a ring', async ({ browser }) => {
  const two = { ...THREADS.short, messages: [{ from: 'felix', text: 'Hash it.' }, { from: 'you', text: 'Why?' }, { from: 'felix', text: 'Salt it too.' }] };
  await fixture(browser, async page => {
    await expect(page.locator('.margin-note[data-id="t1"] .margin-foot')).toHaveText('2 more');
    await expect(page.locator('.margin-note[data-id="t1"] .margin-felix')).toHaveText('Salt it too.');
  }, { threads: [two] });
  await fixture(browser, async page => {
    await expect(page.locator('.margin-mark[data-id="t1"]')).toHaveAttribute('data-more', '');
  }, { threads: [two], layout: 'marks' });
});
