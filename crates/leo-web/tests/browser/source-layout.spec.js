const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss } = require('../page-source');

test('documents split notes and keep Felix at the right without losing drafts', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const style = pageCss();
  const calendar = fs.readFileSync(path.join(web, 'app/calendar.js'), 'utf8');
  const uploads = fs.readFileSync(path.join(web, 'app/uploads.js'), 'utf8');
  const core = fs.readFileSync(path.join(web, 'app/core.js'), 'utf8');
  const scrollbar = core.slice(core.indexOf('function pageBarRight()'), core.indexOf('function drawPageBar()'));
  const source = calendar.slice(calendar.indexOf('const source ='));
  const viewer = uploads.slice(uploads.indexOf('function viewKind'));
  const fixture = `const state = {view: 'note', session: {note: {id: 'sample'}}};
    const enc = encodeURIComponent;
    const esc = text => String(text).replace(/[&<>"']/g, ch => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const ICON = {close: '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M4 4l8 8M4 12l8-8" stroke="currentColor"/></svg>'};
    const md = {render: text => '<p>' + esc(text) + '</p>'};
    class Locked extends Error {}
    let pending = null;
    async function api() { if (pending) await pending; return [{name:'paper.pdf'}, {name:'notes.txt'}, {name:'board.png'}]; }
    function sheet(contents) { const el = document.createElement('div'); el.className = 'scrim'; el.innerHTML = '<section class="sheet viewer">' + contents + '</section>'; document.body.append(el); return el; }
    const $ = selector => document.querySelector(selector);
    const chat = leoChat.create({render: md.render, escape: esc, icons: ICON});
    ${source}
    ${viewer}
    ${scrollbar}
    document.addEventListener('click', event => {
      const el = event.target.closest('[data-action]'); if (!el) return;
      event.preventDefault();
      if (el.dataset.action === 'view-original') { el.focus({preventScroll:true}); viewOriginal('sample', 'paper.pdf'); }
      if (el.dataset.action === 'source-close') closeSource(el);
      if (el.dataset.action === 'source-file') openSource('sample', el.dataset.name);
      if (el.dataset.action === 'close') el.closest('.scrim').remove();
    });
    window.audit = {scrollbarRight: pageBarRight, open: name => viewOriginal('sample', name), close: closeSource, source,
      chat(open) { chat.toggle(open); },
      wait() { pending = new Promise(resolve => window.release = () => { pending = null; resolve(); }); }};`;
  const server = http.createServer((request, response) => {
    if (request.url === '/fixture.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fixture); }
    if (request.url === '/pane-resize.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fs.readFileSync(path.join(web, 'pane-resize.js'))); }
    if (request.url === '/chat.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fs.readFileSync(path.join(web, 'chat.js'))); }
    if (request.url.startsWith('/api/') && !request.url.includes('/originals/')) { response.setHeader('Content-Type', 'application/json'); return response.end('[]'); }
    if (request.url === '/style.css') { response.setHeader('Content-Type', 'text/css; charset=utf-8'); return response.end(style); }
    if (request.url.includes('/originals/notes.txt')) return response.end('Original text <must stay escaped>.');
    if (request.url.includes('/originals/')) return response.end('Document fixture');
    response.setHeader('Content-Type', 'text/html; charset=utf-8');
    response.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="stylesheet" href="/style.css"></head><body><aside class="side">Notes</aside><header class="bar"><div class="bar-row">Lecture</div></header><main id="app"><h1>Lecture notes</h1><a href="/paper.pdf" data-action="view-original">paper.pdf</a><textarea id="note-draft">My note draft</textarea><p>' + 'Notes stay readable. '.repeat(300) + '</p></main><div class="dock"><div class="activity">Background activity</div></div><script src="/pane-resize.js"></script><script src="/chat.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.setViewportSize({width: 1280, height: 900});
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.evaluate(() => audit.chat(true));
    await page.locator('#chat-input').fill('Help me understand this');
    await page.locator('#chat-input').focus();
    await page.locator('#chat-input').evaluate(el => el.setSelectionRange(5, 9));
    await page.evaluate(() => audit.open('paper.pdf'));
    await expect(page.locator('#source-pane')).toBeVisible();
    await expect(page.locator('.scrim')).toHaveCount(0);
    await expect(page.locator('.source-frame')).toHaveAttribute('src', /paper\.pdf\?view=1$/);
    await expect(page.locator('#chat-input')).toBeFocused();
    expect(await page.locator('#chat-input').evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([5, 9]);
    for (const theme of ['light', 'dark']) {
      for (const width of [1000, 1280, 1440, 1920]) {
        await page.setViewportSize({width, height: 900});
        await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
        await expect.poll(async () => (await page.locator('#chat-input').boundingBox()).width).toBeGreaterThan(150);
        const boxes = await page.evaluate(() => {
          const rect = selector => { const r = document.querySelector(selector).getBoundingClientRect(); return {left:r.left, right:r.right, width:r.width}; };
          return {note:rect('main'), source:rect('#source-pane'), chat:rect('.chat'), input:rect('#chat-input'), dock:rect('.dock'), viewport:document.documentElement.clientWidth, scroll:document.documentElement.scrollWidth};
        });
        expect(boxes.note.right).toBeLessThanOrEqual(boxes.source.left + 2);
        expect(boxes.dock.right).toBeLessThanOrEqual(boxes.source.left);
        expect(boxes.source.right).toBeLessThanOrEqual(boxes.chat.left + 2);
        expect(boxes.chat.right).toBeLessThanOrEqual(boxes.viewport + 1);
        expect(boxes.note.width).toBeGreaterThan(175);
        expect(boxes.source.width).toBeGreaterThanOrEqual(250);
        expect(boxes.chat.width).toBeGreaterThanOrEqual(290);
        expect(boxes.input.width).toBeGreaterThan(150);
        expect(boxes.scroll).toBeLessThanOrEqual(boxes.viewport + 1);
      }
    }
    await page.setViewportSize({width: 1280, height: 900});
    await expect(page.locator('.chat-history')).toBeHidden();
    await page.locator('.chat-history-toggle').click();
    await expect(page.locator('.chat-history')).toBeVisible();
    await expect(page.locator('.chat-history-back')).toBeVisible();
    await page.locator('.chat-history-back').click();
    await expect(page.locator('#chat-input')).toHaveValue('Help me understand this');
    await page.setViewportSize({width: 1920, height: 900});
    const grip = page.locator('.source-grip');
    const before = (await page.locator('#source-pane').boundingBox()).width;
    await grip.focus();
    await page.keyboard.press('ArrowRight');
    await expect.poll(async () => (await page.locator('#source-pane').boundingBox()).width).toBeLessThan(before - 20);
    await page.evaluate(() => audit.chat(false));
    expect(await page.evaluate(() => audit.scrollbarRight())).toBeGreaterThan(250);
    await page.evaluate(() => audit.chat(true));
    await expect(page.locator('.source-frame')).toHaveCount(1);
    await page.evaluate(() => audit.open('notes.txt'));
    await expect(page.locator('.source-text')).toHaveText('Original text <must stay escaped>.');
    await expect(page.locator('.source-text must')).toHaveCount(0);
    const divider = await page.locator('.source-grip').boundingBox();
    const startWidth = (await page.locator('#source-pane').boundingBox()).width;
    await page.mouse.move(divider.x + 4, divider.y + 200);
    await page.mouse.down();
    await page.mouse.move(divider.x + 36, divider.y + 200);
    await expect.poll(async () => (await page.locator('#source-pane').boundingBox()).width).toBeLessThan(startWidth - 20);
    await page.evaluate(() => audit.close());
    await expect(page.locator('body')).not.toHaveClass(/source-resizing/);
    await page.mouse.up();
    await page.evaluate(() => { audit.wait(); audit.open('paper.pdf'); audit.close(); release(); });
    await expect(page.locator('#source-pane')).toBeHidden();
    await expect(page.locator('#source-pane iframe')).toHaveCount(0);
    await page.locator('#app > a').click();
    await expect(page.locator('#source-pane')).toBeVisible();
    await page.locator('[data-action="source-close"]').click();
    await expect(page.locator('#source-pane')).toBeHidden();
    await expect(page.locator('#app > a')).toBeFocused();
    await page.setViewportSize({width: 390, height: 844});
    await page.evaluate(() => audit.open('notes.txt'));
    await expect(page.locator('.sheet.viewer')).toBeVisible();
    await expect(page.locator('.viewer-text')).toHaveText('Original text <must stay escaped>.');
    expect(await page.locator('.sheet.viewer').evaluate(el => document.elementFromPoint(el.getBoundingClientRect().left + 20, el.getBoundingClientRect().top + 20)?.closest('.viewer') !== null)).toBe(true);
    await page.locator('.viewer [data-action="close"]').click();
    await expect(page.locator('#chat-input')).toHaveValue('Help me understand this');
    await expect(page.locator('#note-draft')).toHaveValue('My note draft');
    expect(errors).toEqual([]);
  } finally {
    await context.close();
    await new Promise(resolve => server.close(resolve));
  }
});

test('note headings navigate without losing edits and adapt beside Felix in both themes', async ({ browser }, testInfo) => {
  const web = path.resolve(__dirname, '../../src/web');
  const style = pageCss();
  const core = fs.readFileSync(path.join(web, 'app/core.js'), 'utf8');
  const icons = core.slice(core.indexOf('const svg ='), core.indexOf('const recorder ='));
  const show = core.slice(core.indexOf('async function showNote('), core.indexOf('function newNote('));
  const body = '# Introduction to planetary environments\n\nFirst paragraph.\n\n' + 'Reading material.\n\n'.repeat(80) + '## **Sounds**\n\nCompare vowels.\n\n### Vowel examples\n\n' + 'Examples to compare.\n\n'.repeat(30) + '```md\n# Ignore code\n```\n\n## Sounds\n\nLast paragraph.';
  const fixture = `const md = window.leoMarkdown, leoDoc = window.leoDoc;
    const $ = (q, root = document) => root.querySelector(q), app = $('#app');
    const esc = md.escape, enc = encodeURIComponent;
    let seq = 0, state = {}, side = {}, saved = '', citedLines = null;
    const noteHash = id => '#/n/' + encodeURIComponent(id);
    ${core.slice(core.indexOf('const SEARCH_HIGHLIGHT_MS'), core.indexOf('async function go('))}
    const source = {}, chat = {setContext() {}};
    const saving = {open(note) {return {note, edit: {...note, tags: []}}}};
    const chrome = () => {}, arriveOnPage = () => {}, drawOriginals = () => {}, hideAskFloat = () => {}, restoreReading = () => {}, drawRelated = () => {}, placeAskFloat = () => {};
    const swap = (_key, draw) => draw(), scrollToTop = () => window.scrollTo(0, 0);
    const marginLoadPrefs = async () => {}, marginMount = async () => {}, marginEdited = () => {}, fail = error => { throw error; };
    const cleanTitle = text => text.trim(), folderLabel = () => 'Top level', rel = () => 'now';
    function changed(s) {saved = s.doc.source();}
    ${icons}
    ${show}
    const note = ${JSON.stringify({id:'outline-note', title:'Heading navigation', body, directory:'', updated_at:'2026-10-25'})};
    window.audit = {saved: () => saved, remove: () => {state.session.doc.destroy(); app.innerHTML = '';}};
    showNote(note.id, {fresh:note});`;
  const server = http.createServer((req, res) => {
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(fixture); }
    const file = {'/markdown.js':'markdown.js', '/editing.js':'editing.js', '/doc.js':'doc.js'}[req.url];
    if (file) { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(fs.readFileSync(path.join(web, file))); }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end(`<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><style>${style}</style></head><body class="on-note"><header id="bar" class="bar"><div class="bar-row">Leo</div></header><main id="app"></main><aside class="chat" hidden><textarea id="chat-input" aria-label="Felix message"></textarea></aside><script src="/markdown.js"></script><script src="/editing.js"></script><script src="/doc.js"></script><script src="/fixture.js"></script></body></html>`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({reducedMotion:'reduce'});
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.setViewportSize({width:1600, height:900});
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const nav = page.locator('.note-outline nav');
    await expect(nav.locator('button[data-line]')).toHaveCount(4);
    await expect(nav.getByRole('button', {name:'Ignore code'})).toHaveCount(0);
    await expect(page.locator('.note-outline details')).not.toHaveAttribute('open', '');
    const facts = page.locator('#title + #note-facts');
    await expect(facts).toHaveText('Edited now · 2 min read');
    await expect(page.locator('#note-edited')).toBeVisible();
    await expect(page.locator('.note-meta #save-state')).toHaveCount(0);
    expect(await facts.evaluate(el => getComputedStyle(el).fontVariantNumeric)).toBe('tabular-nums');
    await page.emulateMedia({media:'print'});
    await expect(facts).toBeHidden();
    await page.emulateMedia({media:'screen'});
    const rail = await page.locator('.note-outline').evaluate(el => {
      const ticks = el.querySelector('.outline-ticks').getBoundingClientRect(), note = document.querySelector('.note').getBoundingClientRect(), columns = document.querySelector('.note-columns').getBoundingClientRect();
      return {ticks:note.left - ticks.right, centre:(note.left - columns.left) - (columns.right - note.right), count:el.querySelectorAll('.outline-ticks span').length};
    });
    expect(rail.ticks).toBeGreaterThanOrEqual(0);
    expect(Math.abs(rail.centre)).toBeLessThanOrEqual(2);
    expect(rail.count).toBe(4);
    expect(await nav.evaluate(el => getComputedStyle(el).opacity)).toBe('0');
    expect(await nav.evaluate(el => getComputedStyle(el).scrollbarWidth)).toBe('none');
    await page.locator('.note-outline summary').click();
    await expect.poll(() => nav.evaluate(el => getComputedStyle(el).opacity)).toBe('1');
    await nav.evaluate(el => { el.style.maxHeight = '64px'; });
    expect(await nav.evaluate(el => el.scrollHeight > el.clientHeight)).toBe(true);
    await nav.hover();
    await page.mouse.wheel(0, 100);
    await expect.poll(() => nav.evaluate(el => el.scrollTop)).toBeGreaterThan(0);
    await nav.evaluate(el => { el.style.removeProperty('max-height'); el.scrollTop = 0; });
    const heading = nav.locator('button[data-line]').first();
    expect(await heading.evaluate(el => getComputedStyle(el).whiteSpace)).toBe('nowrap');
    expect((await heading.boundingBox()).height).toBeLessThan(40);
    const screenshot = testInfo.outputPath('outline-desktop.png');
    await page.screenshot({path:screenshot});
    await testInfo.attach('outline-desktop', {path:screenshot, contentType:'image/png'});
    const parent = nav.getByRole('button', {name:'Subheadings of Introduction to planetary environments', exact:true});
    await parent.click();
    await expect(parent).toHaveAttribute('aria-expanded', 'false');
    await expect(nav.getByRole('button', {name:'Sounds', exact:true})).toHaveCount(0);
    await expect(heading).toBeVisible();
    await expect(parent).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(parent).toHaveAttribute('aria-expanded', 'true');
    const nested = nav.getByRole('button', {name:'Subheadings of Sounds', exact:true});
    await nested.click();
    await expect(nested).toHaveAttribute('aria-expanded', 'false');
    await expect(nav.getByRole('button', {name:'Vowel examples', exact:true})).toHaveCount(0);
    await page.locator('#doc .blk').filter({hasText:'Vowel examples'}).evaluate(el => window.scrollTo(0, window.scrollY + el.getBoundingClientRect().top - 80));
    await expect(nav.getByRole('button', {name:'Sounds', exact:true}).first()).toHaveAttribute('aria-current', 'location');



    await nav.getByRole('button', {name:'Sounds', exact:true}).nth(1).click();
    await expect(page.locator('#doc .blk:focus')).toContainText('Sounds');
    await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(1000);
    await expect(nav.locator('[aria-current]')).toHaveAttribute('data-line', String(body.split('\n').lastIndexOf('## Sounds')));
    await page.evaluate(() => window.scrollTo(0, 0));
    await page.locator('#doc .blk').filter({hasText:'First paragraph.'}).click();
    const editor = page.locator('#doc textarea');
    await editor.fill('## Newly typed heading\n' + 'w '.repeat(300));
    await expect(facts).toHaveText('Edited now · 3 min read');
    await page.locator('.note-outline summary').hover();
    await expect(nav.getByRole('button', {name:'Newly typed heading'})).toBeVisible();
    await expect(editor).toBeFocused();
    await expect(nested).toHaveAttribute('aria-expanded', 'false');
    expect(await page.evaluate(() => audit.saved())).toContain('## Newly typed heading');
    await expect(facts).not.toContainText('words');
    for (const theme of ['light','dark']) {
      for (const width of [1600, 1100, 390]) {
        await page.setViewportSize({width, height:900});
        await page.evaluate(theme => {
          document.documentElement.dataset.theme = theme;
          document.body.classList.toggle('chat-open', innerWidth >= 1000);
          document.querySelector('.chat').hidden = innerWidth < 1000;
          document.querySelector('.chat').classList.toggle('open', innerWidth >= 1000);
          document.body.style.setProperty('--chat-w', '400px');
        }, theme);
        await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
        if (width >= 1200) {
          const edges = await page.evaluate(() => ({note:document.querySelector('.note').getBoundingClientRect().right, chat:document.querySelector('.chat').getBoundingClientRect().left}));
          expect(edges.note).toBeLessThanOrEqual(edges.chat + 1);
        }
        if (width === 390) await expect(page.locator('.note-outline details')).not.toHaveAttribute('open', '');
        const contrast = await page.locator('.note-outline summary').evaluate(el => {
          const rgb = value => value.match(/[\d.]+/g).slice(0,3).map(Number).map(n => {n /= 255; return n <= .04045 ? n/12.92 : ((n+.055)/1.055)**2.4;}).reduce((n,c,i) => n+c*[.2126,.7152,.0722][i],0);
          const a = rgb(getComputedStyle(el).color), b = rgb(getComputedStyle(document.body).backgroundColor);
          return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);
        });
        expect(contrast).toBeGreaterThanOrEqual(3);
      }
    }
    await page.locator('.note-outline summary').click();
    await expect(nav.getByRole('button', {name:'Newly typed heading'})).toBeVisible();
    await nav.getByRole('button', {name:'Newly typed heading'}).focus();
    await page.keyboard.press('Enter');
    await expect(page.locator('#doc .blk:focus')).toContainText('Newly typed heading');
    await page.emulateMedia({media:'print'});
    await expect(page.locator('.note-outline')).toBeHidden();
    await page.emulateMedia({media:'screen'});
    await page.evaluate(() => audit.remove());
    await page.evaluate(() => window.dispatchEvent(new Event('scroll')));
    expect(errors).toEqual([]);
  } finally {
    await context.close();
    await new Promise(resolve => server.close(resolve));
  }
});
