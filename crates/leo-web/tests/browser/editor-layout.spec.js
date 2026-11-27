const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss, paneHelpers } = require('../page-source');
const { signIn } = require('./sign-in');

const PICTURE_PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAABgAAAAQCAIAAACDRijCAAAClUlEQVR4nBXSERuGMBiF4XEyicfJJB4nk3icTOKHk0k8TibxOJnE42TS7/j63h9wX+c65xVC0Al6gRIMglFgBJNgFjjBIlgFCDbBLoiCQ3AKsuAS3IIqeASvQAhJJ+klSjJIRomRTJJZ4iSLZJUg2SS7JEoOySnJkktyS6rkkbzygxSdolcoxaAYFUYxKWaFUyyKVYFiU+yKqDgUpyIrLsWtqIpH8aoP0nSaXqM0g2bUGM2kmTVOs2hWDZpNs2ui5tCcmqy5NLemah7Nqz/I0Bl6gzIMhtFgDJNhNjjDYlgNGDbDboiGw3AasuEy3IZqeAyv+SBLZ+ktyjJYRouxTJbZ4iyLZbVg2Sy7JVoOy2nJlstyW6rlsbz2gxydo3cox+AYHcYxOWaHcyyO1YFjc+yO6DgcpyM7LsftqI7H8boP8nSe3qM8g2f0GM/kmT3Os3hWD57Ns3ui5/Ccnuy5PLeneh7P6z8IOuj/yzDA+C+ECeZ/DhZY+d8GO0Q44IQMF9xQ4YH3ezUR6AJ9QAWGwBgwgSkwB1xgCawBAltgD8TAETgDOXAF7kANPIE3fFCki/QRFRkiY8REpsgccZElssZ/ki2yR2LkiJyRHLkid6RGnsgbPyjRJfqESgyJMWESU2JOuMSSWBMktsSeiIkjcSZy4krciZp4Em/6oEyX6TMqM2TGjMlMmTnjMktmzf9KtsyeiZkjc2Zy5srcmZp5Mm/+oEJX6AuqMBTGgilMhbngCkthLVDYCnshFo7CWciFq3AXauEpvOWDKl2lr6jKUBkrpjJV5oqrLJW1/rfZKnslVo7KWcmVq3JXauWpvPWDGl2jb6jG0BgbpjE15oZrLI21QWNr7I3YOBpnIzeuxt2ojafxNn6Q3Alf0XLvqwAAAABJRU5ErkJggg==', 'base64');

async function editorFixture(browser, run, { remember = false, body = null, touch = false, related = false } = {}) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  const passages = read('app/passages.js');
  const source = body ?? (remember ? Array.from({ length: 160 }, (_, i) => `Paragraph number ${i + 1} about reading.`).join('\n\n') : 'An ordinary paragraph.');
  const fixture = `window.doc = leoDoc.mount(document.querySelector('#doc'), {
    source: new URLSearchParams(location.search).get('body') ?? ${JSON.stringify(source)}, onChange() {},
    dir: new URLSearchParams(location.search).get('dir') ?? '', onPictures: new URLSearchParams(location.search).has('pics') ? files => (window.pictured ??= []).push(files.length) : null, outline: document.querySelector('.note-outline'), noteTools: new URLSearchParams(location.search).has('tools')
  });
  window.audit = { pick: width => leoTheme.setNoteWidth(width), anchor: () => JSON.parse(localStorage.getItem('fixture-reading') || 'null') };
  ${related ? `const hideAskFloat = () => {};
  ${passages.slice(passages.indexOf('const KEEP_SELECTION'), passages.indexOf("document.addEventListener('keydown'"))}` : ''}
  ${remember ? `const $ = selector => document.querySelector(selector), enc = encodeURIComponent;
    const seq = 1, citedLines = null, app = document, session = {doc:window.doc}, state = {view:'note',session,main:session};
    ${paneHelpers()}
    const asideOpen = () => false;
    async function api(url, options = {}) {
      if (options.method === 'PUT') localStorage.setItem('fixture-reading', JSON.stringify(options.body.anchor));
      return {anchor:audit.anchor()};
    }
    ${passages.slice(passages.indexOf('const readingFresh ='))}
    restoreReading('fixture-note', seq);` : ''}`;
  const server = http.createServer((req, res) => {
    if (req.url === '/away') { res.setHeader('Content-Type', 'text/html'); return res.end('<p>Another page</p>'); }
    if (req.url.startsWith('/api/image')) { res.setHeader('Content-Type', 'image/png'); return res.end(PICTURE_PNG); }
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css'); return res.end(css); }
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript'); return res.end(fixture); }
    if (['/markdown.js', '/editing.js', '/doc.js', '/theme.js'].includes(req.url)) {
      res.setHeader('Content-Type', 'text/javascript'); return res.end(read(req.url.slice(1)));
    }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    const sidebar = remember ? '<aside class="side"><div class="side-head">Folders</div><div class="side-explorer"><nav class="side-tree">' + Array.from({length:100},(_,i)=>'<button class="side-row">Folder '+i+'</button>').join('') + '</nav></div></aside>' : '';
    res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><script src="/theme.js"></script><link rel="stylesheet" href="/style.css"></head><body class="on-note">'+sidebar+'<header id="bar" class="bar"><div class="bar-row">Leo</div></header><main><div class="note-layout"><div class="note-columns"><aside class="note-outline" hidden><details class="source-more"><summary aria-label="Headings in this note"></summary><nav aria-label="Headings"></nav></details></aside><article class="note"><h1>My note</h1><div id="doc" class="doc prose"></div>'+(related?'<section class="related" id="related"><h2 class="related-head">Related notes</h2><div class="related-list"><a class="list-row related-row" href="/away"><span class="related-title">Another note</span><span class="sub">Shares a topic</span></a></div></section>':'')+'</article></div></div></main><script src="/markdown.js"></script><script src="/editing.js"></script><script src="/doc.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({ hasTouch: touch });
  try { await run(context, 'http://127.0.0.1:' + server.address().port); }
  finally { await context.close(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}

for (const theme of ['light', 'dark']) {
  test('typing and removing every heading keeps the note, text and caret in place in ' + theme, async ({ browser }) => {
    await editorFixture(browser, async (context, url) => {
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      for (const width of [1600, 1200, 390]) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(url);
        await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
        await page.locator('#doc .blk').first().click();
        const editor = page.locator('#doc textarea');
        const before = await editor.boundingBox();
        for (let level = 1; level <= 6; level++) {
          const text = '#'.repeat(level) + ' Heading 文字 ' + level;
          if (level === 1) {
            await editor.fill('');
            await page.keyboard.type('#');
            await expect(editor).toHaveValue('#');
            await page.keyboard.insertText(text.slice(1));
          } else await editor.fill(text);
          await expect(page.locator('.note-outline button[data-line]')).toHaveText('Heading 文字 ' + level);
          const after = await editor.boundingBox();
          expect(Math.abs(after.x - before.x)).toBeLessThanOrEqual(1);
          expect(Math.abs(after.y - before.y)).toBeLessThanOrEqual(1);
          expect(Math.abs(after.width - before.width)).toBeLessThanOrEqual(1);
          await expect(editor).toBeFocused();
          expect(await editor.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([text.length, text.length]);
          await page.keyboard.type(' next');
          await expect(editor).toHaveValue(text + ' next');
        }
        await editor.fill('An ordinary paragraph.');
        await expect(page.locator('.note-outline')).toBeHidden();
        expect(await editor.boundingBox()).toEqual(before);
        await page.keyboard.press('Escape');
        await expect(page.locator('#doc .blk')).toHaveText('An ordinary paragraph.');
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      }
      expect(errors).toEqual([]);
      await page.close();
    });
  });
}

test('the note page marks links, quotes, tables and code in pencil, Copy copies only the code, and other editors keep plain markup', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text) => { window.copied = text; } } }));
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.setViewportSize({ width: 1200, height: 900 });
    const body = 'Read [the paper](https://example.org/p) first.\n\n> A chain has no memory.\n\n| From | To A |\n|---|---|\n| A | 0.20 |\n| B | 1,200 |\n\n```python\nprint(1)\n```\n\nAfter the code.';
    await page.goto(url + '?tools&body=' + encodeURIComponent(body));
    const doc = page.locator('#doc');
    const link = doc.locator('a');
    expect(await link.evaluate(el => [getComputedStyle(el).textDecorationLine, getComputedStyle(el).backgroundImage.includes('svg')])).toEqual(['none', true]);
    const quote = doc.locator('blockquote');
    expect(await quote.evaluate(el => [getComputedStyle(el).borderLeftWidth, getComputedStyle(el).paddingLeft, getComputedStyle(el, '::before').maskImage.includes('svg'), getComputedStyle(el).backgroundColor !== 'rgba(0, 0, 0, 0)'])).toEqual(['0px', '16px', true, false]);
    await expect(doc.locator('th.align-right')).toHaveText('To A');
    await expect(doc.locator('td.align-right')).toHaveText(['0.20', '1,200']);
    expect(await doc.locator('thead').evaluate(el => [getComputedStyle(el.querySelector('th')).backgroundColor, getComputedStyle(el.querySelector('th'), '::after').maskImage.includes('svg')])).toEqual(['rgba(0, 0, 0, 0)', true]);
    const code = doc.locator('.code[data-lang="python"]');
    const copy = code.locator('button.code-copy');
    await expect(copy).toHaveAttribute('aria-label', 'Copy code');
    expect(await code.evaluate(el => [el.textContent, getComputedStyle(el, '::before').content])).toEqual(['print(1)', '"python"']);
    expect(await copy.evaluate(el => getComputedStyle(el).opacity)).toBe('0');
    await code.hover();
    await expect.poll(() => copy.evaluate(el => getComputedStyle(el).opacity)).toBe('1');
    await copy.click();
    await expect(copy).toHaveAttribute('aria-label', 'Copied');
    await expect(copy.locator('.pencil-tick')).toHaveCount(1);
    await expect(page.locator('#code-said[role="status"]'), 'screen readers hear the copy').toHaveText('Copied');
    expect(await page.evaluate(() => window.copied)).toBe('print(1)');
    await expect(doc.locator('.editing')).toHaveCount(0);
    await expect(copy).toHaveAttribute('aria-label', 'Copy code', { timeout: 3000 });
    await expect(copy.locator('.pencil-tick')).toHaveCount(0);
    expect(await code.evaluate(el => {
      const button = el.querySelector('.code-copy').getBoundingClientRect();
      const range = document.createRange();
      range.selectNodeContents(el.querySelector('pre code'));
      return [range.getClientRects()[0].top - el.getBoundingClientRect().top < 24, range.getClientRects()[0].right <= button.left];
    }), 'the code starts at the top of the block and short code clears Copy').toEqual([true, true]);
    const source = await page.evaluate(() => window.doc.source());
    await copy.focus();
    await page.keyboard.press('Enter');
    await expect(copy).toHaveAttribute('aria-label', 'Copied');
    expect(await page.evaluate(() => window.doc.source()), 'Enter on Copy never types into the note').toBe(source);
    await expect(doc.locator('.editing')).toHaveCount(0);
    const quoted = await page.evaluate(() => {
      const range = document.createRange();
      range.setStart(document.querySelector('#doc p').firstChild, 5);
      range.setEnd([...document.querySelectorAll('#doc .blk p')].at(-1).firstChild, 9);
      getSelection().removeAllRanges();
      getSelection().addRange(range);
      const picked = window.doc.selection();
      getSelection().removeAllRanges();
      return picked;
    });
    expect(quoted.text).toContain('```python\nprint(1)\n```\n\nAfter the');
    await code.locator('pre').click({ position: { x: 40, y: 12 } });
    await expect(doc.locator('.blk-code.editing textarea')).toHaveValue('```python\nprint(1)\n```');
    await quote.click();
    expect(await doc.locator('.blk-quote.editing').evaluate(el => [getComputedStyle(el.querySelector('textarea')).borderLeftWidth, getComputedStyle(el, '::before').maskImage.includes('svg')])).toEqual(['0px', true]);
    await page.goto(url + '?body=' + encodeURIComponent(body));
    expect(await page.evaluate(() => {
      const range = document.createRange();
      range.setStart(document.querySelector('#doc p').firstChild, 5);
      range.setEnd([...document.querySelectorAll('#doc .blk p')].at(-1).firstChild, 9);
      getSelection().removeAllRanges();
      getSelection().addRange(range);
      return window.doc.selection();
    })).toEqual(quoted);
    expect(await page.evaluate(body => {
      const card = document.createElement('div');
      card.className = 'prose doc kanban-description';
      document.body.append(card);
      leoDoc.mount(card, { source: body, onChange() {} });
      const a = card.querySelector('a');
      return [card.querySelectorAll('.code, .code-copy, .align-right').length, getComputedStyle(a).textDecorationLine, getComputedStyle(card.querySelector('blockquote')).borderLeftWidth];
    }, body)).toEqual([0, 'underline', '3px']);
    expect(errors).toEqual([]);
  });
});

test('a style key with no selection opens a pair to type in, and leaves it or removes it', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    const editor = page.locator('#doc textarea');
    await page.locator('#doc .blk').first().click();
    await page.keyboard.type('plain ');
    await page.keyboard.press('ControlOrMeta+b');
    await expect(editor).toHaveValue('plain ****');
    await page.keyboard.type('bold');
    await page.keyboard.press('ControlOrMeta+b');
    await page.keyboard.type(' then ');
    await page.keyboard.press('ControlOrMeta+u');
    await page.keyboard.press('ControlOrMeta+u');
    await page.keyboard.press('ControlOrMeta+i');
    await page.keyboard.type('slanted');
    await expect(editor).toHaveValue('plain **bold** then *slanted*');
    expect(await page.evaluate(() => window.doc.source())).toBe('plain **bold** then *slanted*');
  }, { body: '' });
});

test('a slash opens a capped quick menu that writes Markdown, and Escape keeps what was typed', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    const editor = page.locator('#doc textarea');
    const menu = page.locator('.popup-menu.slash-menu');
    await page.locator('#doc .blk').first().click();
    await page.keyboard.type('/');
    await expect(menu).toBeVisible();
    expect(await menu.evaluate(el => el.scrollHeight > el.clientHeight && el.getBoundingClientRect().height <= 320)).toBe(true);
    await page.keyboard.type('check');
    await expect(menu.getByRole('option').first()).toHaveText('Checkbox');
    await page.keyboard.press('Enter');
    await expect(menu).toHaveCount(0);
    await page.keyboard.type('Milk /sta');
    await expect(menu.getByRole('option', { selected: true })).toContainText('Star icon');
    await menu.getByRole('option', { name: 'Star icon' }).click();
    await expect(editor).toBeFocused();
    await page.keyboard.type('a/b /hea');
    await expect(menu).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(editor).toBeFocused();
    await page.keyboard.type('d');
    await expect(menu).toHaveCount(0);
    expect(await page.evaluate(() => window.doc.source())).toBe('- [ ] Milk :star: a/b /head');
    await page.keyboard.press('Escape');
    await expect(page.locator('#doc .note-icon[data-tone="amber"]')).toBeVisible();
  }, { body: '' });
});

test('an active heading refits after pane resizing without clipping text or losing selection', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').first().click();
    const editor = page.locator('#doc textarea');
    const text = '### ' + 'Long heading 文字 😀 '.repeat(35);
    await editor.fill(text);
    await expect(page.locator('.note-outline')).toBeVisible();
    await editor.evaluate(el => el.setSelectionRange(8, 12));
    for (const width of [1200, 900, 390, 1600]) {
      await page.setViewportSize({ width, height: 900 });
      await expect.poll(() => editor.evaluate(el => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(1);
      await expect(editor).toBeFocused();
      await expect(editor).toHaveValue(text);
      expect(await editor.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([8, 12]);
      expect(await editor.evaluate(el => getComputedStyle(el).color === getComputedStyle(document.body).color)).toBe(true);
    }
    await page.keyboard.insertText('replacement');
    await expect(editor).toHaveValue(text.slice(0, 8) + 'replacement' + text.slice(12));
  });
});

test('switching editable blocks keeps unrelated rendered content and the viewport stable', async ({ browser }) => {
  const samples = ['Ordinary text', '## Heading', '- A list item', '- [ ] A task', '```js\nconst answer = 42;\n```', '| Name | Value |\n| --- | --- |\n| One | Two |', '> A quote\n> Another line', '> [!note] A callout\n> Kept open', '$$\nx = 2\n$$'];
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const width of [1400, 390]) {
      await page.setViewportSize({ width, height: 900 });
      for (const sample of samples) {
        await page.goto(url + '/?body=' + encodeURIComponent('First paragraph\n\n' + sample + '\n\nFollowing paragraph'));
        const target = page.locator('#doc .blk').nth(2);
        await target.scrollIntoViewIfNeeded();
        await page.evaluate(() => { window.keptBlock = Array.from(document.querySelectorAll('#doc .blk')).at(-1); });
        const before = await page.locator('#doc .blk').last().boundingBox();
        await target.click({ position: { x: 30, y: 10 } });
        await expect(page.locator('#doc textarea')).toBeFocused();
        expect(Math.abs((await page.locator('#doc .blk').last().boundingBox()).y - before.y), sample).toBeLessThanOrEqual(1);
        await page.locator('#doc .blk').first().click();
        expect(await page.evaluate(() => window.keptBlock.isConnected), sample).toBe(true);
        expect(await page.evaluate(() => doc.source())).toBe('First paragraph\n\n' + sample + '\n\nFollowing paragraph');
      }
    }
  });
});

test('focusing and dragging from a formatted line preserves document geometry in both directions', async ({ browser }) => {
  test.setTimeout(60000);
  const samples = ['## A heading', '- A list item', '  - A nested item', '- [ ] A task', '**Bold** text ' + 'wrapped words '.repeat(35), '```js\nconst answer = 42;\n```', '| Name | Value |\n| --- | --- |\n| One | Two |', '> A quote\n> Another line'];
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const width of [1400, 390]) {
      await page.setViewportSize({ width, height: 900 });
      for (const sample of samples) {
        for (const direction of ['up', 'down']) {
          const body = 'Before this line\n' + sample + '\nAfter this line\nLast line';
          await page.goto(url + '/?body=' + encodeURIComponent(body));
          const block = page.locator('#doc .blk').nth(1);
          const geometry = () => page.locator('#doc .blk').evaluateAll(els => els.map(el => ({ y: el.getBoundingClientRect().top + scrollY, height: el.getBoundingClientRect().height })));
          const before = await geometry();
          await block.click({ position: { x: 35, y: 10 } });
          const area = page.locator('#doc textarea');
          await expect(area).toBeFocused();
          const focused = await geometry();
          for (let i = 0; i < before.length; i++) {
            expect(Math.abs(focused[i].y - before[i].y), `${width} ${direction} ${sample}: focus block ${i}`).toBeLessThanOrEqual(1);
            expect(Math.abs(focused[i].height - before[i].height), `${width} ${sample}: focus height ${i}`).toBeLessThanOrEqual(1);
          }
          const box = await area.boundingBox();
          const target = page.locator('#doc .blk').nth(direction === 'up' ? 0 : 2);
          const end = await target.boundingBox();
          await page.mouse.move(box.x + 35, box.y + Math.min(10, box.height / 2));
          await page.mouse.down();
          await page.mouse.move(end.x + 35, end.y + end.height / 2, { steps: 8 });
          await page.mouse.up();
          const after = await geometry();
          for (let i = 0; i < before.length; i++) expect(Math.abs(after[i].y - before[i].y), `${width} ${direction} ${sample}: selection block ${i}`).toBeLessThanOrEqual(1);
          expect(await page.evaluate(() => getSelection().toString())).toContain(direction === 'up' ? 'this line' : 'After');
          expect(await page.evaluate(() => doc.source())).toBe(body);
        }
      }
    }
  });
});

test('focusing a partially visible Markdown line does not scroll the page', async ({ browser }) => {
  const sample = '**Bold words** and ordinary words. '.repeat(25);
  const body = 'Before\n\n'.repeat(30) + sample + '\n\n' + 'After\n\n'.repeat(30);
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 900 });
    await page.goto(url);
    const block = page.locator('#doc .blk').filter({ hasText: 'Bold words' });
    await block.evaluate(el => scrollTo(0, scrollY + el.getBoundingClientRect().top + 80));
    const box = await block.boundingBox();
    const before = await page.evaluate(() => scrollY);
    await page.mouse.click(box.x + 70, 100);
    await expect(page.locator('#doc textarea')).toBeFocused();
    expect(await page.evaluate(() => scrollY)).toBe(before);
  }, { body });
});

test('dragging after editing wrapped Markdown keeps the target under the pointer', async ({ browser }) => {
  const words = '**word 😀** '.repeat(25);
  const samples = [words, '## ' + words, '- ' + words, '- [ ] ' + words, '  - [x] ' + words, '```txt\n' + words + '\n```', '> ' + words, '| Name | Value |\n| --- | --- |\n| One | Two |', '', '- [ ] '];
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 900 });
    for (const sample of samples) for (const direction of ['up', 'down']) {
      await page.goto(url + '/?body=' + encodeURIComponent('Before this line\n' + sample + '\nAfter this line\nLast line'));
      await page.locator('#doc .blk').nth(1).click({ position: { x: 35, y: 10 } });
      const area = page.locator('#doc textarea');
      const value = await area.inputValue();
      const base = 'Before this line\n'.length + sample.length - value.length;
      await area.fill(sample === '' || sample === '- [ ] ' ? '' : value + (sample === words ? 'x\nPasted continuation' : 'x'));
      const source = await page.evaluate(() => doc.source());
      const box = await area.boundingBox();
      const target = page.locator('#doc .blk').nth(direction === 'up' ? 0 : 2);
      const before = await target.boundingBox();
      const end = { x: before.x + 35, y: before.y + before.height / 2 };
      const offset = await target.evaluate((el, point) => {
        const pos = document.caretPositionFromPoint?.(point.x, point.y);
        const hit = pos ? null : document.caretRangeFromPoint(point.x, point.y);
        const range = document.createRange();
        range.setStart(el, 0);
        range.setEnd(pos ? pos.offsetNode : hit.startContainer, pos ? pos.offset : hit.startOffset);
        return range.toString().length;
      }, end);
      await page.mouse.move(Math.max(2, box.x + 35), box.y + 10);
      await page.mouse.down();
      const anchor = base + await area.evaluate(el => el.selectionStart);
      await page.mouse.move(end.x, end.y, { steps: 8 });
      await page.mouse.up();
      expect(Math.abs((await target.boundingBox()).y - before.y), direction + sample).toBeLessThanOrEqual(1);
      expect(await page.evaluate(() => getSelection().focusOffset)).toBe(offset);
      const to = (direction === 'up' ? 0 : source.indexOf('After this line')) + offset;
      const expected = source.slice(Math.min(anchor, to), Math.max(anchor, to));
      const copied = await page.evaluate(() => {
        const data = new DataTransfer();
        document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
        return data.getData('text/plain');
      });
      expect(copied, direction + sample).toBe(expected);
      await page.keyboard.press('Backspace');
      expect(await page.evaluate(() => doc.source())).toBe(source.slice(0, Math.min(anchor, to)) + source.slice(Math.max(anchor, to)));
    }
  });
});

test('a dragged source line can be refocused without replacing its editor and returns to preview when cleared', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 900 });
    for (const sample of ['**word** '.repeat(25), '- [ ] ' + '**word** '.repeat(25), '```txt\nword\n```']) {
      await page.goto(url + '/?body=' + encodeURIComponent('Before\n' + sample + '\nAfter this line\nLast'));
      await page.locator('#doc .blk').nth(1).click({ position: { x: 35, y: 10 } });
      const area = page.locator('#doc textarea');
      await area.fill((await area.inputValue()) + 'x');
      await area.evaluate(el => { window.keptEditor = el; });
      const from = await area.boundingBox(), to = await page.locator('#doc .blk').nth(2).boundingBox();
      await page.mouse.move(from.x + 35, from.y + 10);
      await page.mouse.down();
      await page.mouse.move(to.x + 35, to.y + to.height / 2, { steps: 8 });
      await page.mouse.up();
      await expect(area).toHaveCount(0);
      const selected = page.locator('#doc .blk.editing .line-edit');
      const before = await selected.boundingBox();
      await page.mouse.move(before.x + 35, before.y + 10);
      await page.mouse.down();
      await page.mouse.up();
      await expect(area).toBeFocused();
      expect(await area.evaluate(el => el === window.keptEditor)).toBe(true);
      expect(await area.boundingBox()).toEqual(before);
      const value = await area.inputValue();
      const caret = await area.evaluate(el => el.selectionStart);
      await page.keyboard.type('Z');
      await expect(area).toHaveValue(value.slice(0, caret) + 'Z' + value.slice(caret));
      await page.locator('article.note > h1').click();
      await expect(page.locator('#doc .line-edit')).toHaveCount(0);
      expect(await page.evaluate(() => doc.source())).toContain('Z');
    }
  });
});

test('inserting a picture after a source-selection drag preserves the note and uses a live editor state', async ({ browser }) => {
  const body = 'Before\n' + '**word** '.repeat(25) + '\nAfter this line\nLast';
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').nth(1).click({ position: { x: 35, y: 10 } });
    const area = page.locator('#doc textarea');
    await area.fill((await area.inputValue()) + 'x');
    const source = await page.evaluate(() => doc.source());
    const from = await area.boundingBox(), to = await page.locator('#doc .blk').nth(2).boundingBox();
    await page.mouse.move(from.x + 35, from.y + 10);
    await page.mouse.down();
    await page.mouse.move(to.x + 35, to.y + to.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect(area).toHaveCount(0);
    await page.evaluate(() => doc.insert('![picture](attachment.png)'));
    expect(await page.evaluate(() => doc.source())).toBe(source + '\n\n![picture](attachment.png)');
  }, { body });
});

test('a picture inserted into a checkbox or bullet stays in the item, and Enter, after typing too, continues the list', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    const picture = '![p](/api/image?inline=1)';
    const items = () => page.evaluate(() => doc.source().split('\n').filter(line => line.startsWith('- ')).length);
    for (const item of ['- [ ] ', '- ', '- foo']) {
      for (const typed of ['', 'cat', ' cat']) {
        await page.goto(url + '/?body=' + encodeURIComponent(item));
        await page.locator('#doc .blk').first().click();
        await page.keyboard.press('End');
        await page.evaluate(src => doc.insert(src), picture);
        expect(await page.evaluate(() => doc.source())).toBe(item + picture);
        if (typed) {
          await page.keyboard.type(typed);
          expect(await page.evaluate(() => doc.source())).toBe(item + picture + typed);
        }
        await page.keyboard.press('Enter');
        expect(await items()).toBe(2);
        await page.keyboard.type('x');
        await page.keyboard.press('Enter');
        expect(await items()).toBe(3);
        await expect(page.locator('#doc img.note-img')).toHaveCount(1);
      }
    }
  });
});

test('a fold chevron sits level with its heading text, first heading or not', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url + '/?body=' + encodeURIComponent('## Key ideas\n\nText\n\n## Knapsack\n\nMore'));
    await expect(page.locator('#doc .fold-btn')).toHaveCount(2);
    const off = await page.evaluate(() => [...document.querySelectorAll('#doc .fold-btn')].map(b => {
      const h = b.parentElement;
      const range = document.createRange();
      range.selectNodeContents([...h.childNodes].find(n => n.nodeType === 3));
      const text = range.getBoundingClientRect();
      const chevron = b.getBoundingClientRect();
      return Math.abs(chevron.top + chevron.height / 2 - (text.top + text.height / 2));
    }));
    expect(off).toHaveLength(2);
    for (const gap of off) expect(gap).toBeLessThan(2);
  });
});

test('a drag from the text before a picture into the open text after it cuts exactly what it covers', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    const picture = '![p](/api/image?inline=1)';
    await page.goto(url + '/?body=' + encodeURIComponent(`Before ${picture} after`));
    await expect.poll(() => page.locator('#doc img.note-img').evaluate(img => img.naturalWidth)).toBeGreaterThan(0);
    const spot = (word, at) => page.locator('#doc .blk').first().evaluate((el, [word, at]) => {
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const i = node.data.indexOf(word);
        if (i < 0) continue;
        const range = document.createRange();
        range.setStart(node, i + at);
        range.setEnd(node, i + at + 1);
        const box = range.getBoundingClientRect();
        return { x: box.left + 1, y: box.top + box.height / 2 };
      }
    }, [word, at]);
    const end = await spot('after', 4);
    await page.mouse.click(end.x, end.y);
    await expect(page.locator('#doc textarea')).toHaveValue(' after');
    const from = await spot('Before', 2);
    const box = await page.locator('#doc textarea').boundingBox();
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width - 2, box.y + box.height / 2, { steps: 6 });
    await page.mouse.up();
    await page.keyboard.press('ControlOrMeta+x');
    expect(await page.evaluate(() => doc.source())).toBe('Be');
  }, { body: 'x' });
});

test('returning a drag to a list marker selects only the edited item text', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const item of ['- [ ] Task words', '- Item words', '12. Item words', '  - Nested words']) {
      const body = 'Before this line\n' + item + '\nAfter this line\nLast line';
      await page.goto(url + '/?body=' + encodeURIComponent(body));
      await page.locator('#doc .blk').nth(1).locator('li').click({ position: { x: 60, y: 10 } });
      const area = page.locator('#doc textarea');
      const value = await area.inputValue();
      const box = await area.boundingBox(), below = await page.locator('#doc .blk').nth(2).boundingBox();
      await page.mouse.move(box.x + 20, box.y + 10);
      await page.mouse.down();
      const anchor = await area.evaluate(el => el.selectionStart);
      await page.mouse.move(below.x + 20, below.y + 10, { steps: 5 });
      await page.mouse.move(box.x - 8, box.y + 10, { steps: 5 });
      await page.mouse.up();
      expect(await page.evaluate(() => doc.selection().text), item).toBe(value.slice(0, anchor));
      const copied = await page.evaluate(() => {
        const data = new DataTransfer();
        document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
        return data.getData('text/plain');
      });
      expect(copied, item).toBe(value.slice(0, anchor));
      await page.keyboard.press('Backspace');
      expect(await page.evaluate(() => doc.source()), item).toBe('Before this line\n' + item.slice(0, item.length - value.length) + value.slice(anchor) + '\nAfter this line\nLast line');
    }
  });
});

test('clicks and shift selections after emoji use the same raw caret offsets', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const sample of ['😀 target words', '**😀** target words', '- [ ] 😀 target words', '## 😀 target words']) {
      const body = sample + '\nAfter this line';
      await page.goto(url + '/?body=' + encodeURIComponent(body));
      const point = await page.locator('#doc .blk').first().evaluate(el => {
        const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
        let node;
        while ((node = walker.nextNode())) {
          const at = node.data.indexOf('target');
          if (at < 0) continue;
          const range = document.createRange();
          range.setStart(node, at);
          range.setEnd(node, at + 1);
          const rect = range.getBoundingClientRect();
          return { x: rect.left + .2, y: rect.top + rect.height / 2 };
        }
      });
      await page.mouse.click(point.x, point.y);
      const area = page.locator('#doc textarea');
      await expect(area).toBeFocused();
      expect(await area.evaluate(el => el.selectionStart), sample).toBe((await area.inputValue()).indexOf('target'));
      const end = await page.locator('#doc .blk').nth(1).evaluate(el => {
        const text = el.querySelector('p').firstChild;
        const range = document.createRange();
        range.setStart(text, 0);
        range.setEnd(text, 5);
        const rect = range.getBoundingClientRect();
        return { x: rect.right - .2, y: rect.top + rect.height / 2 };
      });
      await page.keyboard.down('Shift');
      await page.mouse.click(end.x, end.y);
      await page.keyboard.up('Shift');
      const copied = await page.evaluate(() => {
        const data = new DataTransfer();
        document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
        return data.getData('text/plain');
      });
      expect(copied, sample).toBe('target words\nAfter');
    }
  });
});

test('heading navigation scrolls smoothly and respects reduced motion', async ({ browser }) => {
  const body = '## First\n\n' + 'A paragraph.\n\n'.repeat(80) + '## Last\n\n' + 'More text.\n\n'.repeat(20);
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const reducedMotion of ['no-preference', 'reduce']) {
      await page.emulateMedia({ reducedMotion });
      await page.goto(url);
      const behavior = await page.evaluate(() => {
        const scroll = window.scrollTo.bind(window);
        let behavior;
        window.scrollTo = options => { behavior = options.behavior; scroll(options); };
        document.querySelector('.note-outline button[data-line="162"]').click();
        return behavior;
      });
      expect(behavior).toBe(reducedMotion === 'reduce' ? 'instant' : 'smooth');
      await expect.poll(() => page.locator('#doc .blk:focus').textContent()).toBe('Last');
      await expect.poll(() => page.locator('#doc .blk:focus').evaluate(el => el.getBoundingClientRect().top - document.querySelector('#bar').getBoundingClientRect().bottom)).toBeLessThanOrEqual(80);
      expect(await page.locator('#doc .blk:focus').evaluate(el => el.getBoundingClientRect().top >= document.querySelector('#bar').getBoundingClientRect().bottom)).toBe(true);
    }
  }, { body });
});

test('reading width choices cap the note column independently of its outline wrapper', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({width:2000,height:900});
    await page.goto(url);
    expect((await page.locator('.note').boundingBox()).width).toBeCloseTo(980,-1);
    for (const [choice, width] of [['comfortable',760],['wide',980],['full',null]]) {
      await page.evaluate(choice=>audit.pick(choice),choice);
      await page.reload();
      expect(await page.evaluate(()=>leoTheme.noteWidth())).toBe(choice);
      const measured=(await page.locator('.note').boundingBox()).width;
      if (width) expect(measured).toBeCloseTo(width,-1);
      else expect(measured).toBeGreaterThan(1200);
    }
  });
});

test('scrolling the note saves and restores reading while the explorer scrolls independently', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({width:1280,height:900});
    await page.goto(url);
    await expect(page.locator('#doc .blk').first()).toBeVisible();
    const tree=page.locator('.side-tree');
    await tree.hover();
    await page.mouse.wheel(0,600);
    await expect.poll(()=>tree.evaluate(el=>el.scrollTop)).toBeGreaterThan(0);
    expect(await page.evaluate(()=>scrollY)).toBe(0);
    expect(await page.evaluate(()=>audit.anchor())).toBeNull();
    await page.locator('#doc .blk').first().hover();
    await page.mouse.wheel(0,2400);
    await expect.poll(()=>page.evaluate(()=>scrollY)).toBeGreaterThan(1500);
    await expect.poll(()=>page.evaluate(()=>audit.anchor()),{timeout:5000}).not.toBeNull();
    await page.goto(url+'/away');
    await page.goto(url);
    await expect.poll(()=>page.evaluate(()=>scrollY)).toBeGreaterThan(1500);
  },{remember:true});
});

test('bottom checkboxes keep their position and focus when checked and unchecked', async ({ browser }) => {
  const body = '# A long note\n\n' + 'A paragraph above the checklist.\n\n'.repeat(140) + '- [ ] Pending task\n- [x] Completed task\n  - [ ] Nested task\n* [ ] Star task\n+ [ ] Plus task\n1. [ ] Numbered task\n- [ ]\n-   [ ] Wide spaces\n2) [ ] Parenthesis task';
  await editorFixture(browser, async (context,url)=>{
    const page=await context.newPage();
    await page.setViewportSize({width:1280,height:900});
    await page.goto(url);
    const boxes=page.locator('#doc input[data-box]');
    await expect(boxes).toHaveCount(9);
    for (let index=0; index<9; index++) {
      const box=boxes.nth(index);
      await box.scrollIntoViewIfNeeded();
      const before=await page.evaluate(()=>scrollY);
      expect(before).toBeGreaterThan(2000);
      const checked=await box.isChecked();
      await box.click();
      await expect(box).toBeChecked({checked:!checked});
      expect(Math.abs(await page.evaluate(()=>scrollY)-before)).toBeLessThanOrEqual(1);
      await expect(box).toBeFocused();
      await page.keyboard.press('Space');
      await expect(box).toBeChecked({checked});
      expect(Math.abs(await page.evaluate(()=>scrollY)-before)).toBeLessThanOrEqual(1);
      await expect(box).toBeFocused();
    }
    const box=boxes.nth(1), item=box.locator('..');
    const inputRect=await box.boundingBox(), labelRect=await item.boundingBox();
    await item.click({position:{x:inputRect.x+inputRect.width-labelRect.x+2,y:labelRect.height/2}});
    await expect(box).not.toBeChecked();
    await expect(box).toBeFocused();
    await box.click();
    await expect(box).toBeChecked();
    expect(await page.evaluate(()=>window.doc.source())).toBe(body);
  },{body});
});

test('toggling a bottom task after editing above it changes that task and keeps its position', async ({ browser }) => {
  const body = 'Earlier paragraph.\n\n'.repeat(120) + 'Editable line\n\n- [ ] Earlier task\n- [ ] Final task';
  await editorFixture(browser, async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    const editable=page.locator('#doc .blk').filter({hasText:'Editable line'});
    await editable.scrollIntoViewIfNeeded();
    await editable.click();
    await page.locator('#doc textarea').fill('Editable line\n- [ ] Newly typed task');
    const final=page.locator('#doc .task-item').filter({hasText:'Final task'}).locator('input');
    const before=(await final.boundingBox()).y;
    await final.click();
    await expect(final).toBeChecked();
    expect(Math.abs((await final.boundingBox()).y-before)).toBeLessThanOrEqual(1);
    await expect(final).toBeFocused();
    expect(await page.evaluate(()=>window.doc.source())).toContain('- [ ] Newly typed task');
    expect(await page.evaluate(()=>window.doc.source())).toContain('- [ ] Earlier task');
    expect(await page.evaluate(()=>window.doc.source())).toContain('- [x] Final task');
  },{body});
});

test('opening a heading, list or task line keeps its text and the lines below where they were drawn', async ({ browser }) => {
  const body = ['# Big heading', '- Bullet item', '- [ ] Task item', 'Plain line', 'Last line'].join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.goto(url);
    const firstText = (i) => page.evaluate((i) => {
      const blk = document.querySelectorAll('#doc .blk')[i];
      const walker = document.createTreeWalker(blk, NodeFilter.SHOW_TEXT);
      let node;
      while ((node = walker.nextNode())) {
        const at = node.data.search(/\S/);
        if (at < 0) continue;
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + 1);
        return range.getBoundingClientRect().left;
      }
      return null;
    }, i);
    for (const [i, marker] of [[0, '# '], [1, '- '], [2, '- [ ] ']]) {
      const drawn = await firstText(i);
      const below = await page.locator('#doc .blk').nth(4).boundingBox();
      await page.locator('#doc .blk').nth(i).locator('text=/item|heading/').click();
      const area = page.locator('#doc textarea');
      await expect(area).toBeFocused();
      const raw = await area.evaluate((el, marker) => {
        const style = getComputedStyle(el);
        const ctx = document.createElement('canvas').getContext('2d');
        ctx.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
        return { left: el.getBoundingClientRect().left + ctx.measureText(el.closest('li') ? '' : marker).width, size: style.fontSize };
      }, marker);
      if (i === 0) expect(raw.size).toBe('22px');
      else expect(Math.abs(raw.left - drawn)).toBeLessThanOrEqual(3);
      const after = await page.locator('#doc .blk').nth(4).boundingBox();
      expect(Math.abs(after.y - below.y)).toBeLessThanOrEqual(2);
      await page.keyboard.press('Escape');
      await expect(page.locator('#doc textarea')).toHaveCount(0);
    }
  }, { body });
});

test('a selection dragged out of the edited line keeps going across rendered lines, and copies as Markdown', async ({ browser }) => {
  const body = ['First line here', 'Second **bold** line', '- Third item', 'Fourth line'].join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').nth(0).click();
    const area = page.locator('#doc textarea');
    await expect(area).toBeFocused();
    const box = await area.boundingBox();
    const third = await page.locator('#doc .blk').nth(2).locator('li').boundingBox();
    const fourth = await page.locator('#doc .blk').nth(3).boundingBox();
    await page.mouse.move(box.x + 1, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 30, box.y + box.height / 2, { steps: 3 });
    await page.mouse.move(third.x + third.width - 2, third.y + third.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect(page.locator('#doc textarea')).toHaveCount(0);
    const picked = await page.evaluate(() => getSelection().toString());
    expect(picked).toContain('First line here');
    expect(picked).toContain('Third item');
    expect(picked).not.toContain('Fourth');
    expect(Math.abs((await page.locator('#doc .blk').nth(3).boundingBox()).y - fourth.y)).toBeLessThanOrEqual(30);
    const copied = await page.evaluate(() => {
      const data = new DataTransfer();
      document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
      return data.getData('text/plain');
    });
    expect(copied).toBe('First line here\nSecond **bold** line\n- Third item');
    await page.keyboard.press('Backspace');
    await expect.poll(() => page.evaluate(() => doc.source())).toBe('\nFourth line');
    await expect(page.locator('#doc textarea')).toBeFocused();
  }, { body });
});

test('the outline sits at the left edge as heading ticks, keeping the note centred, and opens its full outline on demand', async ({ browser }) => {
  const body = ['# One', 'Text', '## Two', 'More', '# Three'].join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    const outline = page.locator('.note-outline');
    await expect(outline).toBeVisible();
    await expect(outline.locator('.outline-ticks span')).toHaveCount(3);
    const note = await page.locator('.note').boundingBox();
    const main = await page.locator('.note-columns').boundingBox();
    expect(Math.abs((note.x - main.x) - (main.x + main.width - note.x - note.width))).toBeLessThanOrEqual(2);
    const ticks = await outline.locator('.outline-ticks').boundingBox();
    expect(ticks.x + ticks.width).toBeLessThanOrEqual(note.x + 1);
    const nav = page.locator('.note-outline nav');
    expect(await nav.evaluate((el) => getComputedStyle(el).opacity)).toBe('0');
    await outline.locator('summary').click();
    await expect.poll(() => nav.evaluate((el) => getComputedStyle(el).opacity)).toBe('1');
    await nav.getByRole('button', { name: 'Three', exact: true }).click();
    await expect(page.locator('#doc .blk:focus')).toContainText('Three');
    await outline.locator('.outline-ticks span').first().click();
    await expect(page.locator('#doc .blk:focus')).toContainText('One');
  }, { body });
});

test('list editing keeps rendered markers, indentation and wrapping while preserving Markdown operations', async ({ browser }) => {
  const items = ['- # Literal heading marker', '- Bullet 文字', '* Star item', '+ Plus item', '12. Number item', '3) Parenthesis item', '- [ ] Pending item', '* [x] Done item', '2. [ ] Ordered task', '  - Nested item', '    - [ ] Nested task', '- [ ]', '-   [ ] Wide spaces', '- ' + 'Wrapped text '.repeat(30)];
  const body = items.concat('Following paragraph').join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const width of [1400, 390]) {
      await page.setViewportSize({width, height:900});
      await page.goto(url);
      for (let i = 0; i < items.length; i++) {
        const block = page.locator('#doc .blk').nth(i);
        await block.scrollIntoViewIfNeeded();
        const before = await block.boundingBox();
        const below = await page.locator('#doc .blk').last().boundingBox();
        const boxCount = await block.locator('input[data-box]').count();
        const textLeft = await block.evaluate(el => {
          const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
          const node = walker.nextNode();
          if (!node) return null;
          const range = document.createRange();
          range.selectNodeContents(node);
          return range.getBoundingClientRect().left;
        });
        await block.locator('li').click({position:{x:Math.min(80, before.width / 2),y:10}});
        const area = block.locator('textarea');
        await expect(area).toBeFocused();
        if (textLeft !== null) expect(Math.abs((await area.boundingBox()).x - textLeft)).toBeLessThanOrEqual(1);
        await expect(block.locator('li')).toHaveCount(1);
        expect(await area.getAttribute('data-head')).toBeNull();
        await expect(block.locator('input[data-box]')).toHaveCount(boxCount);
        await expect(area).toHaveValue(items[i].replace(/^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\](?:\s+|$))?/, ''));
        expect(Math.abs((await block.boundingBox()).height - before.height), `${width}: ${items[i]}`).toBeLessThanOrEqual(1);
        expect(Math.abs((await page.locator('#doc .blk').last().boundingBox()).y - below.y)).toBeLessThanOrEqual(1);
        await page.keyboard.press('Escape');
        await expect(page.locator('#doc textarea')).toHaveCount(0);
        expect(await page.evaluate(() => doc.source())).toBe(body);
      }
    }
    await page.goto(url);
    await page.locator('#doc .task > span').first().click();
    await page.locator('#doc textarea').fill('Changed task');
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await expect(page.locator('#doc textarea')).toHaveValue('');
    expect(await page.evaluate(() => doc.source())).toContain('- [ ] Changed task\n- [ ] ');
    await page.keyboard.insertText('Next task');
    await page.keyboard.press('Tab');
    expect(await page.evaluate(() => doc.source())).toContain('  - [ ] Next task');
    await page.keyboard.press('Shift+Tab');
    expect(await page.evaluate(() => doc.source())).toContain('\n- [ ] Next task');
  }, {body});
});

test('application controls resist highlighting while note content and text fields remain selectable', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    await page.evaluate(() => {
      const controls = document.createElement('div');
      controls.innerHTML = '<button>Action label</button><label>Setting label <input value="Editable text"></label><details open><summary>Disclosure label</summary><p class="hint">Helpful explanation</p><p class="prose">Readable explanation</p></details><a href="#test">Readable link</a><img alt="Picture"><div draggable="true">Movable note</div>';
      document.body.appendChild(controls);
      window.controls = controls;
    });
    const selectable = selector => page.evaluate(selector => getComputedStyle(controls.querySelector(selector)).userSelect, selector);
    for (const selector of ['button', 'label', 'summary', '[draggable="true"]', 'input']) {
      expect(await selectable(selector), selector).toBe('none');
    }
    expect(await selectable('.prose')).toBe('text');
    expect(await page.locator('#doc p').evaluate(el => getComputedStyle(el).userSelect)).not.toBe('none');
    for (const selector of ['a', 'img']) {
      expect(await page.evaluate(selector => getComputedStyle(controls.querySelector(selector)).webkitUserDrag, selector)).toBe('none');
    }
    const paragraph = page.locator('#doc p');
    const box = await paragraph.boundingBox();
    await page.mouse.move(box.x + 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 140, box.y + box.height / 2, {steps:10});
    await page.mouse.up();
    expect(await page.evaluate(() => getSelection().toString())).not.toBe('');
    const hint = await page.locator('.hint').boundingBox();
    await page.mouse.move(hint.x + 1, hint.y + hint.height / 2);
    await page.mouse.down();
    await page.mouse.move(hint.x + hint.width - 1, hint.y + hint.height / 2, {steps:8});
    await page.mouse.up();
    expect(await page.evaluate(() => getSelection().toString()), 'a hint is chrome, so a drag across it selects nothing').toBe('');
    await page.locator('input').focus();
    expect(await selectable('input')).toBe('text');
    await page.locator('input').blur();
    expect(await selectable('input')).toBe('none');
  });
});

test('select all and undo keep checklists rendered and copy their complete Markdown', async ({ browser }) => {
  const body = '- [ ] First task\n2. Second item\n- [x] Last task';
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    await page.locator('#doc .task > span').first().click();
    const area = page.locator('#doc textarea');
    await area.evaluate(el => el.setSelectionRange(2, 7));
    expect(await page.evaluate(() => doc.selection())).toMatchObject({text:'rst t',from:1,to:1});
    await page.keyboard.press('ControlOrMeta+a');
    await expect(area).toHaveCount(0);
    await expect(page.locator('#doc input[data-box]')).toHaveCount(2);
    const copy = () => page.evaluate(() => {
      const data = new DataTransfer();
      document.dispatchEvent(new ClipboardEvent('copy', {clipboardData:data,bubbles:true,cancelable:true}));
      return data.getData('text/plain');
    });
    expect(await copy()).toBe(body);
    await page.keyboard.press('Backspace');
    expect(await page.evaluate(() => doc.source())).toBe('');
    await page.keyboard.press('ControlOrMeta+z');
    await expect(area).toHaveCount(0);
    await expect(page.locator('#doc input[data-box]')).toHaveCount(2);
    expect(await copy()).toBe(body);
    await page.keyboard.press('ControlOrMeta+Shift+z');
    expect(await page.evaluate(() => doc.source())).toBe('');
  }, {body});
});

test('typing a list or checklist marker renders it at once and keeps the caret in the line', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    await page.locator('#doc .blk').first().click();
    const area = page.locator('#doc textarea');
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await page.keyboard.type('- ');
    await expect(page.locator('#doc li')).toHaveCount(1);
    await expect(area).toBeFocused();
    await page.keyboard.type('[ ] ');
    await expect(page.locator('#doc .task-item input[data-box]')).toHaveCount(1);
    await page.keyboard.type('buy milk');
    await expect(area).toHaveValue('buy milk');
    expect(await page.evaluate(() => doc.source())).toBe('Start\n- [ ] buy milk');
    await area.evaluate(el => el.setSelectionRange(0, 0));
    await page.keyboard.press('Backspace');
    await expect(page.locator('#doc li')).toHaveCount(0);
    await expect(area).toHaveValue('buy milk');
  }, {body: 'Start'});
});

test('a list is written, nested and left with the keyboard alone', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    const source = () => page.evaluate(() => doc.source());
    const area = page.locator('#doc textarea');
    for (const [marker, second, third] of [['- ', '- ', '  - '], ['* ', '* ', '  * '], ['+ ', '+ ', '  + '], ['1. ', '2. ', '   1. '], ['1) ', '2) ', '   1) '], ['a. ', 'b. ', '   a. '], ['a) ', 'b) ', '   a) '], ['A) ', 'B) ', '   A) ']]) {
      await page.goto(url + '/?body=');
      await page.locator('#doc .blk').click();
      await page.keyboard.type(marker + 'one');
      await page.keyboard.press('Enter');
      expect(await source()).toBe(`${marker}one\n${second}`);
      await page.keyboard.press('Tab');
      expect(await source(), marker).toBe(`${marker}one\n${third}`);
      await page.keyboard.type('sub');
      await page.keyboard.press('Enter');
      await page.keyboard.press('Enter');
      expect(await source(), marker).toBe(`${marker}one\n${third}sub\n${second}`);
      await page.keyboard.press('Enter');
      expect(await source(), marker).toBe(`${marker}one\n${third}sub\n`);
      await page.keyboard.type('plain');
      await expect(area).toHaveValue('plain');
    }
    await page.goto(url + '/?body=');
    await page.locator('#doc .blk').click();
    await page.keyboard.type('- ');
    const marked = await page.evaluate(() => ({ item: document.querySelector('#doc li').getBoundingClientRect().height, area: document.querySelector('#doc textarea').getBoundingClientRect().height }));
    expect(marked.item).toBeLessThan(marked.area + 1);
    await page.keyboard.type('[ ] ');
    await expect(page.locator('#doc input[data-box]')).toHaveCount(1);
    await page.keyboard.type('box');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Tab');
    expect(await source()).toBe('- [ ] box\n  - [ ] ');
    await page.keyboard.press('Shift+Tab');
    expect(await source()).toBe('- [ ] box\n- [ ] ');
    await page.keyboard.press('Tab');
    await page.keyboard.type('deep');
    await area.evaluate(el => el.setSelectionRange(0, 0));
    await page.keyboard.press('Backspace');
    expect(await source()).toBe('- [ ] box\n- [ ] deep');
    await page.keyboard.press('Backspace');
    expect(await source()).toBe('- [ ] box\ndeep');
    await expect(area).toHaveValue('deep');
    await page.keyboard.press('ControlOrMeta+z');
    expect(await source()).toBe('- [ ] box\n- [ ] deep');
  });
});

test('nesting changes the marker like Google Docs, and only on the page', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    const source = () => page.evaluate(() => doc.source());
    const looks = () => page.evaluate(() => [...document.querySelectorAll('#doc ul, #doc ol')].map(list => getComputedStyle(list).listStyleType));
    for (const [marker, kinds, file] of [['1. ', ['decimal', 'lower-alpha', 'lower-roman', 'decimal'], '1. a\n   1. b\n      1. c\n         1. d'], ['- ', ['disc', 'circle', 'square', 'disc'], '- a\n  - b\n    - c\n      - d']]) {
      await page.goto(url + '/?body=');
      await page.locator('#doc .blk').click();
      await page.keyboard.type(marker + 'a');
      for (const text of ['b', 'c', 'd']) {
        await page.keyboard.press('Enter');
        await page.keyboard.press('Tab');
        await page.keyboard.type(text);
      }
      expect(await source()).toBe(file);
      await page.locator('h1').click();
      expect(await looks()).toEqual(kinds);
      await page.locator('#doc .blk').last().click();
      await page.keyboard.press('Shift+Tab');
      expect((await looks())[3]).toBe(kinds[2]);
      await page.keyboard.press('Tab');
      expect((await looks())[3]).toBe(kinds[3]);
    }
    await page.goto(url + '/?body=' + encodeURIComponent('1) a\n2) b'));
    expect(await looks()).toEqual(['paren', 'paren']);
    await page.locator('#doc .blk').last().click();
    await page.keyboard.press('Tab');
    expect(await looks()).toEqual(['paren', 'lower-alpha']);
    expect(await source()).toBe('1) a\n   1) b');
    await page.goto(url + '/?body=' + encodeURIComponent('1. a\n2. \n3. c'));
    await page.locator('#doc .blk').nth(1).click();
    await page.keyboard.press('Backspace');
    await page.keyboard.press('Backspace');
    expect(await source()).toBe('1. a\n2. c');
  });
});

test('a list pasted into an empty line leaves the caret at its end', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url + '/?body=');
    await page.locator('#doc .blk').click();
    await page.locator('#doc textarea').evaluate(el => {
      el.setRangeText('- one\n- two', 0, 0, 'end');
      el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertFromPaste' }));
    });
    await page.keyboard.type('!');
    expect(await page.evaluate(() => doc.source())).toBe('- one\n- two!');
  });
});

test('a checked task draws no strike line through its text', async ({ browser }) => {
  const long = 'wrapped words '.repeat(40).trim();
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({width: 900, height: 800});
    await page.goto(url);
    const spans = page.locator('#doc .task.done > span');
    await expect(spans).toHaveCount(3);
    for (let i = 0; i < 3; i++) {
      expect(await spans.nth(i).evaluate(el => getComputedStyle(el).textDecorationLine)).toBe('none');
    }
  }, {body: `- [x] Short\n- [x] ${long}\n- [x]`});
});

test('selecting text in a task line never toggles its checkbox, wherever the drag ends', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    const box = page.locator('#doc input[data-box]');
    const text = page.locator('#doc .task > span');
    for (const editing of [false, true]) {
      if (editing) await text.click();
      const t = await text.boundingBox();
      const b = await box.boundingBox();
      await page.mouse.move(t.x + t.width - 4, t.y + t.height / 2);
      await page.mouse.down();
      await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, {steps: 6});
      await page.mouse.up();
      await expect(box).not.toBeChecked();
      await page.mouse.move(t.x + t.width - 4, t.y + t.height / 2);
      await page.mouse.down();
      await page.mouse.move(b.x - 2, b.y + b.height / 2, {steps: 6});
      await page.mouse.up();
      await expect(box).not.toBeChecked();
    }
    expect(await page.evaluate(() => doc.source())).toBe('- [ ] Select across this task text');
    await box.click();
    await expect(box).toBeChecked();
  }, {body: '- [ ] Select across this task text'});
});

test('list text supports caret clicks, removing formatting, undo and navigation in both directions', async ({ browser }) => {
  const body = 'Before\n  - [x] Task text\n3. Number text\nAfter';
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    const task = page.locator('#doc .blk').nth(1);
    await task.locator('label > span').click();
    const area = page.locator('#doc textarea');
    await area.evaluate(el => el.setSelectionRange(el.value.length, el.value.length));
    await area.click({position:{x:1,y:10}});
    expect(await area.evaluate(el => el.selectionStart)).toBe(0);
    await page.keyboard.insertText('New ');
    expect(await page.evaluate(() => doc.source())).toContain('  - [x] New Task text');
    await area.evaluate(el => el.setSelectionRange(0, 0));
    await page.keyboard.press('Backspace');
    await expect(area).toHaveValue('New Task text');
    expect(await page.evaluate(() => doc.source())).toContain('\n- [x] New Task text\n');
    await page.keyboard.press('Backspace');
    await expect(area).toHaveValue('New Task text');
    expect(await page.evaluate(() => doc.source())).toContain('\nNew Task text\n');
    await page.keyboard.press('ControlOrMeta+z');
    await page.keyboard.press('ControlOrMeta+z');
    expect(await page.evaluate(() => doc.source())).toContain('  - [x] New Task text');
    await page.keyboard.press('Escape');
    await task.locator('label > span').click();
    await page.keyboard.press('ArrowDown');
    await expect(area).toHaveValue('Number text');
    await page.keyboard.press('ArrowUp');
    await expect(area).toHaveValue('New Task text');
    await page.keyboard.press('End');
    await page.keyboard.insertText('\nPasted continuation');
    expect(await page.evaluate(() => doc.source())).toContain('  - [x] New Task text\nPasted continuation');
  }, {body});
});

test('empty lists exit and a whole single list item can be deleted, cut or replaced', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const [source, left] of [['- [ ] ', ''], ['2. [ ] ', ''], ['-   [ ] ', ''], ['  - ', '- '], ['3) ', '']]) {
      await page.goto(url + '/?body=' + encodeURIComponent(source));
      await page.locator('#doc .blk').click({position:{x:80,y:10}});
      await expect(page.locator('#doc textarea')).toBeFocused();
      expect((await page.locator('#doc textarea').boundingBox()).height).toBeGreaterThan(20);
      await page.keyboard.press('Enter');
      expect(await page.evaluate(() => doc.source()), source).toBe(left);
    }
    for (const source of ['- [ ] Task', '12. Item', '* Bullet']) {
      for (const action of ['delete', 'cut', 'replace']) {
        await page.goto(url + '/?body=' + encodeURIComponent(source));
        await page.locator('#doc .blk').click({position:{x:80,y:10}});
        await page.keyboard.press('ControlOrMeta+a');
        if (action === 'cut') {
          const cut = await page.evaluate(() => {
            const data = new DataTransfer();
            document.dispatchEvent(new ClipboardEvent('cut', {clipboardData:data,bubbles:true,cancelable:true}));
            return data.getData('text/plain');
          });
          expect(cut).toBe(source);
        } else await page.keyboard.press(action === 'delete' ? 'Backspace' : 'X');
        expect(await page.evaluate(() => doc.source()), `${source}: ${action}`).toBe(action === 'replace' ? 'X' : '');
      }
    }
  });
});

test('formatted wrapped text and heading prefixes keep their position when focused', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({width:390,height:900});
    for (const line of ['- ' + '**word** '.repeat(25), '- [ ] Task ' + '[word](https://example.com) '.repeat(15), '**Bold text** '.repeat(30), ...Array.from({length:6}, (_, i) => '#'.repeat(i + 1) + ' A heading')]) {
      await page.goto(url + '/?body=' + encodeURIComponent(line + '\nFollowing paragraph'));
      const block = page.locator('#doc .blk').first();
      const before = await block.boundingBox();
      const below = await page.locator('#doc .blk').last().boundingBox();
      await block.click({position:{x:Math.min(35,before.width / 2),y:10}});
      const area = page.locator('#doc textarea');
      await expect(area).toBeFocused();
      expect(Math.abs((await block.boundingBox()).height - before.height), line).toBeLessThanOrEqual(1);
      expect(Math.abs((await page.locator('#doc .blk').last().boundingBox()).y - below.y), line).toBeLessThanOrEqual(1);
      if (line.startsWith('#')) {
        const rawTextLeft = await area.evaluate((el, prefix) => {
          const style = getComputedStyle(el);
          const canvas = document.createElement('canvas').getContext('2d');
          canvas.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
          return el.getBoundingClientRect().left + canvas.measureText(prefix).width;
        }, line.match(/^#+ /)[0]);
        expect(Math.abs(rawTextLeft - before.x)).toBeLessThanOrEqual(2);
      }
    }
  });
});

test('heading rail previews magnify nearby bars without opening the whole outline', async ({ browser }) => {
  const body = '# Overview\nA **clear** introduction.\n## Details 😀\nA <script>literal</script> preview.\n### Empty\n# Last\nFinal section.';
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 2000, height: 900 });
    await page.goto(url);
    const rail = page.getByRole('button', { name: /Jump to heading:/ });
    const preview = page.getByRole('tooltip');
    await rail.locator('[data-tick]').nth(1).hover();
    await expect(preview).toContainText('Details 😀');
    await expect(preview).toContainText('A <script>literal</script> preview.');
    await expect(preview.locator('script')).toHaveCount(0);
    await expect(rail.locator('[data-tick]').nth(1)).toHaveAttribute('data-distance', '0');
    await expect(rail.locator('[data-tick]').first()).toHaveAttribute('data-distance', '1');
    await expect(page.getByRole('navigation', { name: 'Headings' })).toBeHidden();
    await rail.locator('[data-tick]').nth(1).click();
    await expect(page.locator('#doc .blk:focus')).toContainText('Details 😀');
    await page.mouse.move(600, 500);
    await expect(preview).toBeHidden();
    await rail.focus();
    await rail.press('End');
    await expect(preview).toContainText('Last');
    await rail.press('ArrowUp');
    await expect(preview).toHaveText('Empty');
    await rail.press('Home');
    await expect(preview).toContainText('Overview');
    await rail.press('Enter');
    await expect(page.locator('#doc .blk:focus')).toContainText('Overview');
    await expect(preview).toBeHidden();
    await rail.focus();
    await rail.press('Escape');
    await expect(preview).toBeHidden();
    await page.emulateMedia({ reducedMotion: 'reduce' });
    expect(await rail.locator('[data-tick]').first().evaluate(el => getComputedStyle(el).transitionDuration)).toBe('0s');
    await page.setViewportSize({ width: 390, height: 900 });
    await expect(rail).toBeHidden();
    await expect(preview).toBeHidden();
  }, { body });
});

test('the heading rail keeps clear of the note text and its preview shows at every width', async ({ browser }) => {
  const body = ['# One', 'Text '.repeat(80), '## Two', 'More '.repeat(80), '# Three', 'End'].join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const width of [1280, 2000]) {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(url);
      const ticks = page.locator('.outline-ticks [data-tick]');
      await expect(ticks).toHaveCount(3);
      await expect(page.locator('.outline-page:visible'), 'a short outline has no section arrows').toHaveCount(0);
      for (let i = 0; i < 3; i++) {
        await ticks.nth(i).hover();
        await expect(ticks.nth(i)).toHaveAttribute('data-distance', '0');
        await expect(page.locator('.outline-preview')).toBeVisible();
        await expect.poll(() => page.evaluate(() => document.getAnimations().filter((run) => run.playState === 'running' && run.effect && run.effect.getComputedTiming().iterations !== Infinity).length)).toBe(0);
        const seen = await page.evaluate(() => {
          const text = document.querySelector('#doc').getBoundingClientRect();
          const bars = [...document.querySelectorAll('.outline-ticks [data-tick]')].map((span) => { const r = span.getBoundingClientRect(); return r.left + parseFloat(getComputedStyle(span, '::after').width) * (r.width / span.offsetWidth); });
          const tip = document.querySelector('.outline-preview');
          const box = tip && !tip.hidden ? tip.getBoundingClientRect() : null;
          return { left: text.left, right: text.right, bar: Math.max(...bars), tip: box && [box.left, box.right] };
        });
        expect(seen.bar, `ticks clear the text at ${width}px`).toBeLessThanOrEqual(seen.left - 6);
        expect(seen.tip, `the preview shows at ${width}px, over the text if it must`).not.toBeNull();
        expect(seen.tip[0]).toBeGreaterThanOrEqual(0);
        expect(seen.tip[1]).toBeLessThanOrEqual(width);
        expect(seen.tip[0], `the preview sits beside the ticks at ${width}px`).toBeLessThanOrEqual(seen.bar + 40);
      }
    }
  }, { body });
});

test('a long outline shows its ticks a section at a time, with arrows only where there is more', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    const shown = page.locator('.outline-ticks [data-tick]');
    const up = page.getByRole('button', { name: 'Earlier headings' });
    const down = page.getByRole('button', { name: 'Later headings' });
    await expect(shown).toHaveCount(24);
    await expect(up).toBeHidden();
    const rail = (await page.locator('.outline-ticks').boundingBox()).height;
    const arrow = await down.boundingBox();
    const at = [arrow.x + arrow.width / 2, arrow.y + arrow.height / 2];
    await page.mouse.click(...at);
    await page.mouse.click(...at);
    expect(await page.evaluate(([x, y]) => Boolean(document.elementFromPoint(x, y)?.closest('.outline-ticks')), at), 'a second press where the arrow was never lands on a tick').toBe(false);
    await expect(shown).toHaveCount(6);
    expect((await page.locator('.outline-ticks').boundingBox()).height, 'a shorter section keeps the rail its height').toBe(rail);
    await expect(shown.first()).toHaveAttribute('data-tick', '96');
    await expect(down).toBeHidden();
    const tick = await shown.first().boundingBox();
    await page.mouse.move(tick.x + 3, tick.y + tick.height / 2);
    await expect(page.locator('.outline-preview')).toContainText('Heading 25');
    const title = page.locator('.outline-preview strong');
    await page.mouse.move(1500, 850);
    await page.locator('.outline-ticks').focus();
    await expect(shown, 'focusing the ticks keeps the section the arrows chose').toHaveCount(6);
    await expect(title).toHaveText('Heading 25');
    await page.keyboard.press('Home');
    await expect(shown).toHaveCount(24);
    await expect(title).toHaveText('Heading 1');
    await page.keyboard.press('End');
    await expect(shown).toHaveCount(6);
    await expect(title).toHaveText('Heading 30');
    await page.locator('.outline-ticks').blur();
    const sections = page.evaluate(() => new Promise((done) => {
      const seen = new Set();
      const t0 = performance.now();
      const look = () => {
        seen.add(document.querySelectorAll('.outline-ticks [data-tick]').length);
        if (performance.now() - t0 < 1500) requestAnimationFrame(look);
        else done([...seen]);
      };
      look();
    }));
    await shown.nth(2).click();
    expect(await sections, 'jumping to a heading never flips the rail back while it scrolls').toEqual([6]);
    await up.click();
    await expect(shown).toHaveCount(24);
    await expect(up).toBeHidden();
    await expect(down).toBeVisible();
  }, { body: Array.from({ length: 30 }, (_, i) => `## Heading ${i + 1}\n\nText under heading ${i + 1}.`).join('\n\n') });
});

const manyHeadings = Array.from({ length: 3000 }, (_, i) => `## Heading ${i + 1}`).join('\n');

test('a note with 3,000 headings lists them all and the rail shows 24 at a time', async ({ browser }) => {
  test.setTimeout(120000);
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    await expect(page.locator('.note-outline nav button[data-line]')).toHaveCount(3000);
    const ticks = page.locator('.outline-ticks [data-tick]');
    await expect(ticks).toHaveCount(24);
    await expect(ticks.first()).toHaveAttribute('data-tick', '0');
    await page.locator('.outline-page[aria-label="Later headings"]').click();
    await expect(ticks).toHaveCount(24);
    await expect(ticks.first()).toHaveAttribute('data-tick', '24');
    await page.locator('.outline-page[aria-label="Earlier headings"]').click();
    await expect(ticks.first()).toHaveAttribute('data-tick', '0');
  }, { body: manyHeadings });
});

test('scrolling a note with 3,000 headings reads a few positions per frame', async ({ browser }) => {
  test.setTimeout(120000);
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    await expect(page.locator('.note-outline nav button[data-line]')).toHaveCount(3000);
    await page.evaluate(() => {
      window.blockReads = 0;
      const read = Element.prototype.getBoundingClientRect;
      Element.prototype.getBoundingClientRect = function () {
        if (this.classList && this.classList.contains('blk')) window.blockReads++;
        return read.call(this);
      };
    });
    const frame = () => page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done, 50)))));
    const most = [];
    for (const share of [0.1, 0.35, 0.6, 0.85]) {
      await page.evaluate(() => { window.blockReads = 0; });
      await page.evaluate((share) => window.scrollTo(0, (document.documentElement.scrollHeight - innerHeight) * share), share);
      await frame();
      most.push(await page.evaluate(() => window.blockReads));
    }
    expect(Math.max(...most), `block positions read per scroll frame: ${most}`).toBeLessThanOrEqual(20);
    await expect(page.locator('.note-outline nav button[aria-current="location"]')).toHaveCount(1);
    await expect(page.locator('.note-outline nav button[aria-current="location"]')).not.toHaveText('Heading 1');
  }, { body: manyHeadings });
});

test('the last heading is current at the end of the note', async ({ browser }) => {
  test.setTimeout(120000);
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    await expect(page.locator('.note-outline nav button[data-line]')).toHaveCount(3000);
    await expect(page.locator('.note-outline nav button[aria-current="location"]')).toHaveText('Heading 1');
    await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
    await expect(page.locator('.note-outline nav button[aria-current="location"]')).toHaveText('Heading 3000');
    await expect(page.locator('.note-outline nav button[aria-current="location"]')).toHaveCount(1);
    await expect(page.locator('.outline-ticks [data-tick].on')).toHaveCount(1);
    await expect(page.locator('.outline-ticks [data-tick].on')).toHaveAttribute('data-tick', '2999');
  }, { body: manyHeadings });
});

test('dragging down through the note never highlights the empty space or the page chrome under it', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1100, height: 800 });
    await page.goto(url);
    await expect(page.locator('#doc .blk').first()).toBeVisible();
    await expect(page.locator('.related-row')).toBeVisible();
    const first = await page.locator('#doc .blk').first().boundingBox();
    const row = await page.locator('.related-row').boundingBox();
    await page.mouse.move(first.x + 4, first.y + first.height / 2);
    await page.mouse.down();
    await page.mouse.move(row.x + row.width / 2, row.y + row.height / 2, { steps: 12 });
    await page.mouse.up();
    const picked = await page.evaluate(() => getSelection().toString());
    expect(picked).toContain('First paragraph');
    expect(picked).not.toMatch(/Related notes|Another note|Shares a topic/);
    const chrome = await page.evaluate(() => ['.related', '.doc-tail', '.blk:has(.blank)', '.related-head', '#bar'].map((selector) => {
      for (let el = document.querySelector(selector); el; el = el.parentElement) {
        const pick = getComputedStyle(el).webkitUserSelect;
        if (pick && pick !== 'auto') return pick;
      }
      return 'auto';
    }));
    expect(chrome.slice(0, 4)).toEqual(['none', 'none', 'none', 'none']);
    await page.locator('.related-head').click({ position: { x: 8, y: 4 } });
    expect(await page.evaluate(() => getSelection().toString())).toBe('');
    const again = await page.locator('#doc .blk').first().boundingBox();
    await page.mouse.move(again.x + 4, again.y + again.height / 2);
    await page.mouse.down();
    await page.mouse.move(again.x + 200, again.y + again.height / 2, { steps: 4 });
    await page.mouse.up();
    await expect.poll(() => page.evaluate(() => getSelection().toString())).not.toBe('');
    await page.locator('.related-head').click({ position: { x: 8, y: 4 } });
    expect(await page.evaluate(() => getSelection().toString())).toBe('');
  }, { body: 'First paragraph of the note.\n\nSecond paragraph of the note.\n\n\n', related: true });
});

test('heading previews preserve edits, refresh body text, and release their elements on destroy', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 2000, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').nth(1).click();
    const editor = page.locator('#doc textarea');
    await editor.fill('An updated **draft** 😀');
    await editor.evaluate(el => el.setSelectionRange(4, 9));
    const rail = page.locator('.outline-ticks');
    await rail.locator('[data-tick]').first().hover();
    await expect(page.getByRole('tooltip')).toContainText('An updated draft 😀');
    await expect(editor).toBeFocused();
    expect(await editor.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([4, 9]);
    await editor.fill('Another draft');
    await expect(page.getByRole('tooltip')).toContainText('Another draft');
    await page.evaluate(() => window.doc.destroy());
    await expect(rail).toHaveCount(0);
    await expect(page.locator('.outline-preview')).toHaveCount(0);
  }, { body: '# Start\nOriginal paragraph.\n## End\nLast paragraph.' });
});

test('heading rail keeps every marker reachable in a short viewport and tracks reading separately from previews', async ({ browser }) => {
  const body = Array.from({ length: 300 }, (_, i) => `## Section ${i + 1}\nSome reading material.`).join('\n\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 2000, height: 500 });
    await page.goto(url);
    const rail = page.locator('.outline-ticks');
    await expect(rail.locator('[data-tick]')).toHaveCount(24);
    await expect(rail.locator('.on')).toHaveAttribute('data-tick', '0');
    const box = await rail.boundingBox();
    expect(box.y + box.height).toBeLessThan(500);
    await rail.focus();
    await rail.press('End');
    await expect(page.getByRole('tooltip')).toContainText('Section 300');
    await expect(rail.locator('[data-tick]'), 'the rail turns to the last page of headings').toHaveCount(12);
    await expect(rail.locator('.on'), 'the reading place is not on this page').toHaveCount(0);
    await expect(page.locator('.note-outline nav button[aria-current]')).toHaveAttribute('data-line', '0');
    await rail.press('Space');
    await expect(page.locator('#doc .blk:focus')).toContainText('Section 300');
    await expect(rail.locator('.on')).toHaveAttribute('data-tick', '897');
    await rail.focus();
    await rail.press('Home');
    await expect(page.getByRole('tooltip')).toContainText('Section 1');
    const tip = await page.getByRole('tooltip').boundingBox();
    expect(tip.y).toBeGreaterThanOrEqual(0);
    expect(tip.y + tip.height).toBeLessThan(500);
    await rail.press('ArrowUp');
    await expect(page.getByRole('tooltip')).toContainText('Section 1');
  }, { body });
});

for (const interaction of ['touch', 'disclosure', 'escape']) {
  test('heading rail handles ' + interaction + ' without a prior keyboard selection', async ({ browser }) => {
    await editorFixture(browser, async (context, url) => {
      const page = await context.newPage();
      await page.setViewportSize({ width: 2000, height: 900 });
      await page.goto(url);
      if (interaction === 'touch') {
        await page.locator('.outline-ticks [data-tick]').nth(2).tap();
        await expect(page.locator('#doc .blk:focus')).toContainText('Three');
      } else if (interaction === 'disclosure') {
        await page.locator('.note-outline summary').tap();
        await expect(page.getByRole('navigation', { name: 'Headings' })).toBeVisible();
        await page.locator('.note-outline summary').tap();
        await expect(page.getByRole('navigation', { name: 'Headings' })).toBeHidden();
      } else {
        await page.locator('#doc .blk').nth(1).click();
        const editor = page.locator('#doc textarea');
        await page.locator('.outline-ticks [data-tick]').first().hover();
        await expect(page.getByRole('tooltip')).toBeVisible();
        await editor.press('Escape');
        await expect(page.getByRole('tooltip')).toBeHidden();
        await expect(editor).toBeFocused();
      }
    }, { body: '# One\nParagraph\n## Two\nMore\n# Three', touch: interaction !== 'escape' });
  });
}


test('heading rail stays vertically centred and its dropdown animates with reduced-motion support', async ({ browser }) => {
  const short='# One\n\nFirst paragraph.\n\n## Two\n\nSecond paragraph.\n\n### Three\n\nLast paragraph.';
  await editorFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    for(const body of [short,short+'\n\n'+'Long reading material.\n\n'.repeat(100)]) {
      await page.setViewportSize({width:1600,height:900});
      await page.goto(url+'?body='+encodeURIComponent(body));
      const outline=page.locator('.note-outline');
      await expect(outline).toBeVisible();
      for(const height of [900,640]) {
        await page.setViewportSize({width:1600,height});
        await expect.poll(async()=>{const box=await outline.boundingBox();return Math.abs(box.y+box.height/2-height/2);}).toBeLessThanOrEqual(2);
      }
      if(body!==short) {
        await page.evaluate(()=>scrollTo(0,document.documentElement.scrollHeight/2));
        await expect.poll(async()=>{const box=await outline.boundingBox();return Math.abs(box.y+box.height/2-320);}).toBeLessThanOrEqual(2);
      }
    }
    const nav=page.locator('.note-outline nav'), summary=page.locator('.note-outline summary'), details=page.locator('.note-outline details');
    await nav.evaluate(el=>{
      const animate=el.animate.bind(el);
      el.animate=(...args)=>{const run=animate(...args);run.pause();return run;};
    });
    await summary.focus(); await summary.press('Enter');
    expect(await nav.evaluate(el=>el.getAnimations().some(a=>a.playState==='paused'))).toBe(true);
    await nav.evaluate(el=>{for(const run of el.getAnimations())run.currentTime=90;});
    expect(await nav.evaluate(el=>getComputedStyle(el).clipPath)).not.toBe('none');
    await nav.evaluate(el=>{for(const run of el.getAnimations())run.finish();});
    await expect.poll(()=>nav.evaluate(el=>el.getAnimations().length)).toBe(0);
    await expect(details).toHaveAttribute('open','');
    const menu=await nav.boundingBox();
    expect(menu.y).toBeGreaterThanOrEqual(0);
    expect(menu.y+menu.height).toBeLessThanOrEqual(640);
    await summary.focus(); await summary.press('Enter');
    expect(await nav.evaluate(el=>el.getAnimations().some(a=>a.playState==='paused'))).toBe(true);
    await nav.evaluate(el=>{for(const run of el.getAnimations())run.finish();});
    await expect(details).not.toHaveAttribute('open','');
    await summary.evaluate(el=>{el.click();el.click();el.click();});
    await nav.evaluate(el=>{for(const run of el.getAnimations())run.finish();});
    await expect.poll(()=>nav.evaluate(el=>el.getAnimations().length)).toBe(0);
    await expect(details).toHaveAttribute('open','');
    await page.emulateMedia({reducedMotion:'reduce'});
    await summary.focus(); await summary.press('Enter');
    await expect(details).not.toHaveAttribute('open','');
    expect(await nav.evaluate(el=>el.getAnimations().length)).toBe(0);
    await page.setViewportSize({width:390,height:844});
    await page.evaluate(()=>scrollTo(0,0));
    await expect.poll(async()=>(await page.locator('.note-outline').boundingBox()).y,'the narrow layout applies after the resize').toBeLessThan(200);
  },{body:short});
});


test('Enter keeps every continued list item on its marker line and an empty item exits once', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const [source, next] of [['- kung fu hustle', '- '], ['* first', '* '], ['+ first', '+ '], ['2. first', '3. '], ['3) first', '4) '], ['  - nested', '  - '], ['- [x] first', '- [ ] '], ['  2. [ ] nested', '  3. [ ] ']]) {
      await page.goto(url + '/?body=' + encodeURIComponent(source));
      await page.locator('#doc .blk').first().click();
      const area = page.locator('#doc textarea');
      await area.press('End');
      await area.press('Enter');
      expect(await page.evaluate(() => doc.source()), source).toBe(source + '\n' + next);
      await expect(area).toBeFocused();
      const geometry = await area.evaluate(el => {
        const item = el.closest('li');
        const area = el.getBoundingClientRect();
        const box = item.getBoundingClientRect();
        return { top: area.top, itemTop: box.top, height: area.height, itemHeight: box.height, lineHeight: parseFloat(getComputedStyle(el).lineHeight), caret: el.selectionStart, value: el.value };
      });
      expect(geometry.top - geometry.itemTop, source).toBeLessThan(2);
      expect(geometry.itemHeight, source).toBeLessThanOrEqual(geometry.lineHeight + 2);
      expect(geometry.caret, source).toBe(0);
      expect(geometry.value, source).toBe('');
      await area.pressSequentially('Second item');
      expect(await page.evaluate(() => doc.source()), source).toBe(source + '\n' + next + 'Second item');
      await area.press('Enter');
      await area.press('Enter');
      if (source.startsWith(' ')) await area.press('Enter');
      expect(await page.evaluate(() => doc.source()), source).toBe(source + '\n' + next + 'Second item\n');
      await expect(area).toBeFocused();
      await expect(page.locator('#doc .blk').last().locator('li')).toHaveCount(0);
    }
  });
});


test('list Enter geometry is correct during keydown and its first frame for fresh and multiline editors', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const path of ['rendered', 'fresh', 'multiline']) {
      await page.goto(url + '/?body=' + encodeURIComponent(path === 'fresh' ? '' : '- kung fu hustle'));
      await page.locator('#doc .blk').first().click();
      const area = page.locator('#doc textarea');
      if (path === 'fresh') await area.fill('- kung fu hustle');
      if (path === 'multiline') await area.fill('First item\n- kung fu hustle');
      await area.press('ControlOrMeta+End');
      await page.evaluate(() => {
        window.enterFrames = [];
        document.addEventListener('keydown', e => {
          if (e.key !== 'Enter') return;
          const read = () => {
            const el = document.querySelector('#doc textarea');
            const item = el?.closest('li');
            const box = el?.getBoundingClientRect();
            const parent = item?.getBoundingClientRect();
            window.enterFrames.push({ value: el?.value, source: doc.source(), top: box?.top, height: box?.height, itemTop: parent?.top, itemHeight: parent?.height, focused: document.activeElement === el, caret: el?.selectionStart, lineHeight: el && parseFloat(getComputedStyle(el).lineHeight) });
          };
          read();
          requestAnimationFrame(read);
        }, { once: true });
      });
      await area.press('Enter');
      await page.waitForFunction(() => window.enterFrames.length === 2);
      const frames = await page.evaluate(() => window.enterFrames);
      for (const frame of frames) {
        expect(frame.source, path).toBe((path === 'multiline' ? '- First item\n' : '') + '- kung fu hustle\n- ');
        expect(frame.value, path).toBe('');
        expect(frame.caret, path).toBe(0);
        expect(frame.focused, path).toBe(true);
        expect(frame.top - frame.itemTop, path).toBeLessThan(2);
        expect(frame.height, path).toBeGreaterThanOrEqual(frame.lineHeight - 1);
        expect(frame.itemHeight, path).toBeLessThanOrEqual(frame.lineHeight + 2);
      }
    }
  });
});


test('heading disclosure closes on outside clicks while the destination keeps its click and focus', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const width of [1600, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(url);
      const details = page.locator('.note-outline details');
      const summary = details.locator('summary');
      await page.evaluate(() => {
        const button = document.createElement('button');
        button.id = 'outside-outline';
        button.textContent = 'Outside';
        button.onclick = () => { window.outsideClicks = (window.outsideClicks || 0) + 1; };
        document.querySelector('#bar .bar-row').append(button);
      });
      await summary.focus(); await summary.press('Enter');
      await expect(details).toHaveAttribute('open', '');
      await page.locator('#outside-outline').click();
      await expect(details).not.toHaveAttribute('open', '');
      expect(await page.evaluate(() => window.outsideClicks)).toBe(1);
      await page.locator('#outside-outline').focus();
      await expect(page.locator('#outside-outline')).toBeFocused();
      await summary.focus(); await summary.press('Enter');
      await expect(details).toHaveAttribute('open', '');
      await page.locator('#doc .blk').nth(1).click();
      await expect(details).not.toHaveAttribute('open', '');
      await expect(page.locator('#doc textarea')).toBeFocused();
      await page.keyboard.insertText('Kept ');
      expect(await page.evaluate(() => doc.source())).toContain('Kept ');
    }
  }, { body: '# Heading\nParagraph to edit.\n## Subheading\nAnother paragraph.' });
});

test('virtual Enter continues list items and exits empty markers without adding an unmarked line', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const inputType of ['insertLineBreak', 'insertParagraph']) for (const [source, expected] of [['- kung fu hustle', '- kung fu hustle\n- '], ['2. one', '2. one\n3. '], ['  - [x] nested', '  - [x] nested\n  - [ ] '], ['- first\n- ', '- first\n']]) {
      await page.goto(url + '/?body=' + encodeURIComponent(source));
      await page.locator('#doc .blk').last().click();
      const area = page.locator('#doc textarea');
      await area.evaluate((el, inputType) => {
        el.setSelectionRange(el.value.length, el.value.length);
        const event = new InputEvent('beforeinput', { inputType, bubbles: true, cancelable: true });
        if (el.dispatchEvent(event)) {
          el.setRangeText('\n', el.selectionStart, el.selectionEnd, 'end');
          el.dispatchEvent(new InputEvent('input', { inputType, bubbles: true }));
        }
      }, inputType);
      expect(await page.evaluate(() => doc.source()), `${inputType}: ${source}`).toBe(expected);
      await expect(area).toBeFocused();
      expect(await area.inputValue()).toBe('');
      if (expected.endsWith('- ') || expected.endsWith('3. ') || expected.endsWith('- [ ] ')) expect(await area.evaluate(el => Boolean(el.closest('li')))).toBe(true);
      else expect(await area.evaluate(el => Boolean(el.closest('li')))).toBe(false);
    }
  }, { touch: true });
});

test('soft Enter and composing input keep their native line-break behavior', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    await page.locator('#doc .blk').click();
    const area = page.locator('#doc textarea');
    await area.press('End');
    await area.press('Shift+Enter');
    expect(await page.evaluate(() => doc.source())).toBe('- first\n');
    expect(await area.inputValue()).toBe('first\n');
    await area.evaluate(el => {
      const event = new InputEvent('beforeinput', { inputType: 'insertLineBreak', isComposing: true, bubbles: true, cancelable: true });
      window.composingCancelled = !el.dispatchEvent(event);
    });
    expect(await page.evaluate(() => window.composingCancelled)).toBe(false);
    expect(await page.evaluate(() => doc.source())).toBe('- first\n');
    await area.evaluate(el => {
      const event = new InputEvent('beforeinput', { inputType: 'insertParagraph', bubbles: true, cancelable: false });
      window.nativeCancelled = !el.dispatchEvent(event);
    });
    expect(await page.evaluate(() => window.nativeCancelled)).toBe(false);
  }, { body: '- first' });
});

test('heading peaks are taller and preview motion opens and closes on a retained tooltip', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 2000, height: 900 });
    await page.goto(url);
    const preview = page.locator('.outline-preview');
    await preview.evaluate(el => {
      const animate = el.animate.bind(el);
      el.animate = (...args) => { const run = animate(...args); run.pause(); return run; };
    });
    const tick = page.locator('.outline-ticks [data-tick]').nth(1);
    await tick.hover();
    const duration = await preview.evaluate(el => el.getAnimations().map(run => run.effect.getTiming().duration));
    expect(duration).toEqual([240]);
    await expect(preview).toContainText('Second');
    await preview.evaluate(el => { for (const run of el.getAnimations()) run.currentTime = 120; });
    const between = await preview.evaluate(el => parseFloat(getComputedStyle(el).opacity));
    expect(between).toBeGreaterThan(0);
    expect(between).toBeLessThan(1);
    await preview.evaluate(el => { for (const run of el.getAnimations()) run.finish(); });
    await expect.poll(() => tick.evaluate(el => new DOMMatrix(getComputedStyle(el).transform).m11)).toBeCloseTo(1.8, 1);
    expect(await tick.evaluate(el => new DOMMatrix(getComputedStyle(el).transform).m22)).toBe(1);
    expect(await tick.evaluate(el => parseFloat(getComputedStyle(el).transitionDuration))).toBeGreaterThanOrEqual(0.24);
    await page.mouse.move(600, 500);
    expect(await preview.evaluate(el => el.getAnimations().map(run => run.effect.getTiming().duration))).toEqual([192]);
    await preview.evaluate(el => { for (const run of el.getAnimations()) run.finish(); });
    await expect(preview).toBeHidden();
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await tick.hover();
    await expect(preview).toBeVisible();
    expect(await preview.evaluate(el => el.getAnimations().length)).toBe(0);
    expect(await tick.evaluate(el => getComputedStyle(el).transitionDuration)).toBe('0s');
  }, { body: '# First\nA paragraph.\n## Second\nAnother paragraph.\n### Third\nLast paragraph.' });
});

test('typing a dash then text continues at the next marker through keyboard and text-input Enter', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const size of [16.5, 24, 32]) for (const enter of ['key', 'text-input']) {
      await page.goto(url);
      await page.locator('#doc').evaluate((el, size) => { el.style.fontSize = `${size}px`; el.style.fontFamily = size === 24 ? 'Georgia, serif' : 'sans-serif'; }, size);
      await page.locator('#doc .blk').click();
      const area = page.locator('#doc textarea');
      await area.pressSequentially('-');
      await area.pressSequentially(' ');
      await area.pressSequentially('kung fu hustle');
      if (enter === 'key') await area.press('Enter');
      else await page.keyboard.insertText('\n');
      expect(await page.evaluate(() => doc.source()), `${size}px ${enter}`).toBe('- kung fu hustle\n- ');
      await expect(area).toBeFocused();
      const geometry = await area.evaluate(el => ({ value: el.value, caret: el.selectionStart, top: el.getBoundingClientRect().top, itemTop: el.closest('li').getBoundingClientRect().top, itemHeight: el.closest('li').getBoundingClientRect().height, lineHeight: parseFloat(getComputedStyle(el).lineHeight) }));
      expect(geometry.value).toBe('');
      expect(geometry.caret).toBe(0);
      expect(geometry.top - geometry.itemTop).toBeLessThan(2);
      expect(geometry.itemHeight).toBeLessThanOrEqual(geometry.lineHeight + 2);
    }
  }, { body: '' });
});

 test('heading disclosure hover crosses into menu without taking editor focus', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').nth(1).click();
    const editor = page.locator('#doc textarea');
    const summary = page.locator('.note-outline summary');
    const nav = page.locator('.note-outline nav');
    await summary.hover();
    await expect(nav).toBeVisible();
    await expect(editor).toBeFocused();
    await nav.hover();
    await expect(nav).toBeVisible();
    await page.mouse.move(1000, 100);
    await expect(nav).toBeHidden();
    await summary.focus();
    await summary.press('Enter');
    await expect(nav).toBeVisible();
    await page.mouse.click(1000, 100);
    await expect(nav).toBeHidden();
  }, { body: '# One\nParagraph\n## Two\nMore' });
});

test('every kind of formatting is typed with the keyboard and stays plain Markdown', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url + '/?body=');
    await page.locator('#doc .blk').first().click();
    const keys = page.keyboard;
    const press = async (...names) => { for (const name of names) await keys.press(name); };
    const select = async (count) => { for (let i = 0; i < count; i++) await keys.press('Shift+ArrowLeft'); };
    await keys.type('a) one');
    await press('Enter');
    await keys.type('two');
    await press('Enter', 'Tab');
    await keys.type('sub');
    await press('Enter', 'Enter', 'Enter');
    await keys.type('A) cap');
    await press('Enter', 'Enter');
    await keys.type('1) num');
    await press('Enter', 'Enter');
    await keys.type('A. Smith');
    await press('Enter');
    await keys.type('# Head');
    await press('Enter');
    await keys.type('## ');
    await press('Backspace');
    await keys.type('***both*** snake_case hello word');
    await select(4);
    await press('ControlOrMeta+u');
    await keys.press('ArrowRight');
    await press('End', 'Enter');
    await keys.type('<u onclick=x><script>no</script></u>');
    await press('Enter');
    await keys.type('| L | C | R |');
    await press('Enter');
    await keys.type('|:--|:-:|--:|');
    await press('Enter');
    await keys.type('| 1 | 2 | 3 |');
    await press('Enter');
    await page.locator('#doc th').first().click();
    await press('Tab');
    await keys.type('Mid');
    await press('Tab', 'Tab', 'Tab', 'Tab', 'Tab');
    await keys.type('new');
    await page.locator('#doc h2').first().click();
    await page.evaluate(() => document.activeElement.blur());
    const source = await page.evaluate(() => window.doc.source());
    expect(source).toBe([
      'a) one',
      'b) two',
      '   a) sub',
      'A) cap',
      '1) num',
      'A. Smith',
      '# Head',
      '***both*** snake_case hello <u>word</u>',
      '<u onclick=x><script>no</script></u>',
      '| L | Mid | R |',
      '|:--|:-:|--:|',
      '| 1 | 2 | 3 |',
      '| new |  |  |',
      '',
    ].join('\n'));
    const doc = page.locator('#doc');
    await expect(doc.locator('ol.list-lower-paren li[value="2"]')).toHaveText('two');
    await expect(doc.locator('ol.list-upper-paren')).toHaveText('cap');
    await expect(doc.locator('ol.list-paren')).toHaveText('num');
    await expect(doc.locator('.blk p', { hasText: 'A. Smith' })).toHaveCount(1);
    await expect(doc.locator('h2')).toHaveText('Head');
    await expect(doc.locator('em strong')).toHaveText('both');
    await expect(doc.locator('u')).toHaveText('word');
    await expect(doc.locator('script, u[onclick]')).toHaveCount(0);
    await expect(doc.locator('th.align-center')).toHaveText('Mid');
    await expect(doc.locator('td.align-right').first()).toHaveText('3');
    await expect(doc.locator('tbody tr')).toHaveCount(2);
  }, { body: '' });
});

test('a new note focuses the title first, the reading controls come before it, and Tab leaves it for the body', async ({ page }) => {
  await signIn(page, '#/new');
  await expect(page.locator('#title')).toBeFocused();
  expect(await page.evaluate(() => ['#margin-persona', '#margin-mode'].map((id) => Boolean(document.querySelector(id).compareDocumentPosition(document.querySelector('#title')) & Node.DOCUMENT_POSITION_FOLLOWING)))).toEqual([true, true]);
  await page.keyboard.type('Reading order');
  await page.keyboard.press('Tab');
  await expect(page.locator('#doc .line-edit')).toBeFocused();
});

test('the title stays put when the original-file chips load below it', async ({ page }) => {
  await signIn(page);
  const note = await (await page.request.post('/api/notes', { data: { title: `Originals ${Date.now().toString(36)}`, body: 'x' } })).json();
  const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
  fs.mkdirSync(folder, { recursive: true });
  fs.writeFileSync(path.join(folder, 'notes.txt'), 'Plain text.');
  fs.writeFileSync(path.join(folder, 'slides.pptx'), 'pptx');
  const url = `**/api/notes/${note.id}/originals`;
  for (const width of [1280, 400]) {
    let release;
    const gate = new Promise((resolve) => { release = resolve; });
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/#/');
    await page.route(url, async (route) => { await gate; await route.continue(); });
    await page.goto(`/#/n/${note.id}`);
    const title = page.locator('#title');
    await expect(title).toBeVisible();
    await expect(page.locator('.chip.original')).toHaveCount(0);
    const before = (await title.boundingBox()).y;
    const doc = (await page.locator('#doc').boundingBox()).y;
    release();
    await expect(page.locator('.chip.original')).toHaveCount(3);
    expect((await title.boundingBox()).y, `${width}px: the title does not move`).toBe(before);
    expect((await page.locator('.chip.original').first().boundingBox()).y, `${width}px: the chips sit below the title`).toBeGreaterThan(before);
    expect((await page.locator('#doc').boundingBox()).y, `${width}px: the body makes room`).toBeGreaterThan(doc);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), `${width}px: no sideways scroll`).toBe(true);
    await page.unroute(url);
  }
  await page.request.delete(`/api/notes/${note.id}`);
});

test('a note ends with a short pencil stroke that is not part of its text, and an empty note or a printout has none', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1100, height: 800 });
    const stroke = () => page.evaluate(() => { const s = getComputedStyle(document.querySelector('#doc > .doc-tail'), '::before'); return { display: s.display, width: s.width, mask: s.maskImage || s.webkitMaskImage }; });
    await page.goto(url);
    await expect(page.locator('#doc .blk').first()).toBeVisible();
    const shown = await stroke();
    expect(shown.display).toBe('block');
    expect(shown.width).toBe('40px');
    expect(shown.mask).toContain('svg');
    expect(await page.evaluate(() => window.doc.source())).toBe('An ordinary paragraph.');
    await page.emulateMedia({ media: 'print' });
    expect(await page.locator('#doc > .doc-tail').evaluate((el) => getComputedStyle(el).display)).toBe('none');
    await page.emulateMedia({ media: 'screen' });
    await page.goto(url + '/?body=');
    await expect(page.locator('#doc .placeholder')).toBeVisible();
    expect((await stroke()).display).toBe('none');
    await page.locator('#doc .blk').first().click();
    await expect(page.locator('#doc .line-edit')).toBeFocused();
    expect((await stroke()).display, 'an empty note being written in still has no end mark').toBe('none');
    await page.keyboard.type('Begun');
    expect((await stroke()).display).toBe('block');
  });
});

test('Cmd or Ctrl+Enter on a selected line never splits or empties it; Enter still does', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    await page.locator('#doc .blk').first().click();
    const editor = page.locator('#doc textarea');
    for (const keys of ['Meta+Enter', 'Control+Enter']) {
      await editor.evaluate(el => el.setSelectionRange(0, el.value.length));
      await page.keyboard.press(keys);
      await expect(editor).toHaveValue('An ordinary paragraph.');
      await expect(page.locator('#doc .blk')).toHaveCount(1);
    }
    await editor.evaluate(el => el.setSelectionRange(el.value.length, el.value.length));
    await page.keyboard.press('Enter');
    await expect(page.locator('#doc .blk')).toHaveCount(2);
  });
});

const PICTURE_NOTE = 'Before the picture\n\n![shot](attachments/shot.png)\n\nAfter it';

async function pictureNote(page, url, query = '') {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(url + '/?body=' + encodeURIComponent(PICTURE_NOTE) + query);
  await page.waitForFunction(() => { const img = document.querySelector('#doc img.note-img'); return img && img.complete && img.naturalWidth; });
  return errors;
}

const ownHtml = (markdown, from = '') => `<div data-leo="${markdown}" data-leo-from="${from}" style="color:#000"><p>x</p></div>`;

function pasteInto(page, { html = '', text = '', files = 0, selector = '#doc img.note-img' }) {
  return page.evaluate(([html, text, files, selector]) => {
    const data = new DataTransfer();
    for (let n = 0; n < files; n++) data.items.add(new File([new Uint8Array([137, 80, 78, 71])], 'p' + n + '.png', { type: 'image/png' }));
    data.setData('text/plain', text);
    data.setData('text/html', html);
    const event = new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true });
    (document.querySelector(selector) || document.body).dispatchEvent(event);
    return event.defaultPrevented;
  }, [html, text, files, selector]);
}

test('copying pictures: a selection with a picture puts the markdown in data-leo and a picture in the html, and cut and paste give it back', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    const errors = await pictureNote(page, url);
    await page.evaluate(() => {
      Object.defineProperty(navigator.clipboard, 'write', { configurable: true, value: async ([item]) => {
        const read = async type => (await item.getType(type)).text();
        window.written = { types: item.types, text: await read('text/plain'), html: await read('text/html') };
      } });
    });
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.press('ControlOrMeta+c');
    await expect.poll(() => page.evaluate(() => window.written)).toBeTruthy();
    const written = await page.evaluate(() => window.written);
    expect([...written.types].sort()).toEqual(['image/png', 'text/html', 'text/plain']);
    expect(written.text).not.toContain('![');
    expect(written.text).toContain('Before the picture');
    expect(written.html.startsWith('<div data-leo="')).toBe(true);
    expect(written.html).toMatch(/<img src="data:image\/png;base64,/);
    const kept = await page.evaluate(html => new DOMParser().parseFromString(html, 'text/html').body.firstElementChild.dataset.leo, written.html);
    expect(kept).toBe(PICTURE_NOTE);
    await page.keyboard.press('ControlOrMeta+x');
    expect(await page.evaluate(() => doc.source())).toBe('');
    await page.keyboard.press('ControlOrMeta+v');
    expect(await page.evaluate(() => doc.source())).toBe(PICTURE_NOTE);
    expect(errors).toEqual([]);
  }, { body: PICTURE_NOTE });
});

test('copying pictures: a clicked picture goes to the clipboard as a png and html inside the copy event', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    const errors = await pictureNote(page, url);
    await page.evaluate(() => {
      window.writes = [];
      let inCopy = false;
      document.addEventListener('copy', () => { inCopy = true; }, true);
      document.addEventListener('copy', () => { inCopy = false; });
      Object.defineProperty(navigator.clipboard, 'write', { configurable: true, value: items => { window.writes.push({ items, inCopy }); return Promise.resolve(); } });
    });
    await page.locator('#doc img.note-img').click();
    await page.keyboard.press('ControlOrMeta+c');
    await expect.poll(() => page.evaluate(() => window.writes.length)).toBe(1);
    const sent = await page.evaluate(async () => {
      const { items, inCopy } = window.writes[0];
      const item = items[0];
      const png = await (await item.getType('image/png')).arrayBuffer();
      const html = await (await item.getType('text/html')).text();
      const img = document.querySelector('#doc img.note-img');
      return { inCopy, count: items.length, types: [...item.types].sort(), signature: [...new Uint8Array(png).slice(0, 8)], width: new DataView(png).getUint32(16), height: new DataView(png).getUint32(20), natural: [img.naturalWidth, img.naturalHeight], html };
    });
    expect(sent.inCopy).toBe(true);
    expect(sent.count).toBe(1);
    expect(sent.types).toEqual(['image/png', 'text/html']);
    expect(sent.signature).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
    expect([sent.width, sent.height]).toEqual(sent.natural);
    expect(sent.html.startsWith('<div data-leo="![shot](attachments/shot.png)"')).toBe(true);
    expect(await page.evaluate(() => doc.source())).toBe(PICTURE_NOTE);
    expect(errors).toEqual([]);
  }, { body: PICTURE_NOTE });
});

test('copying pictures: when the clipboard refuses the picture the note is unchanged and says so', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await pictureNote(page, url);
    await page.evaluate(() => {
      Object.defineProperty(navigator.clipboard, 'write', { configurable: true, value: () => Promise.reject(new Error('refused')) });
    });
    await page.locator('#doc img.note-img').click();
    await page.keyboard.press('ControlOrMeta+c');
    await expect(page.locator('#code-said')).toHaveText('Could not copy');
    expect(await page.evaluate(() => doc.source())).toBe(PICTURE_NOTE);
  }, { body: PICTURE_NOTE });
});

test('copying pictures: leo\'s own picture paste goes in as markdown from the same folder and as a new picture from another', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await pictureNote(page, url, '&pics');
    await page.locator('#doc img.note-img').click();
    const own = '![copy](attachments/shot.png)';
    expect(await pasteInto(page, { html: ownHtml(own), files: 1 })).toBe(true);
    expect(await page.evaluate(() => doc.source())).toBe('Before the picture\n\n![shot](attachments/shot.png)\n\n' + own + '\n\nAfter it');
    expect(await page.evaluate(() => window.pictured)).toBeUndefined();

    await pictureNote(page, url, '&pics');
    await page.locator('#doc img.note-img').click();
    expect(await pasteInto(page, { html: ownHtml(own, 'other'), files: 1 })).toBe(true);
    expect(await page.evaluate(() => doc.source())).toContain('\n\n' + own + '\n\nAfter it');
    expect(await page.evaluate(() => window.pictured)).toBeUndefined();

    await pictureNote(page, url, '&pics');
    await page.locator('#doc img.note-img').click();
    await pasteInto(page, { html: ownHtml('![far](far.png)', 'other'), files: 1 });
    expect(await page.evaluate(() => window.pictured)).toEqual([1]);
    expect(await page.evaluate(() => doc.source())).toBe(PICTURE_NOTE);
  }, { body: PICTURE_NOTE });
});

test('copying pictures: html that is not leo\'s own copy is ignored', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await pictureNote(page, url, '&pics');
    await page.locator('#doc img.note-img').click();
    expect(await pasteInto(page, { html: ownHtml('![web](https://example.com/web.png)'), files: 1 })).toBe(true);
    expect(await page.evaluate(() => window.pictured)).toEqual([1]);
    expect(await page.evaluate(() => doc.source())).toBe(PICTURE_NOTE);
    expect(await pasteInto(page, { html: ownHtml('Hello'), text: 'Goodbye' })).toBe(false);
    expect(await pasteInto(page, { html: ownHtml('![web](https://example.com/web.png)'), text: 'Hello' })).toBe(false);
    expect(await page.evaluate(() => doc.source())).toBe(PICTURE_NOTE);
    expect(await page.evaluate(() => window.pictured)).toEqual([1]);
  }, { body: PICTURE_NOTE });
});

test('copying pictures: after destroy a paste does nothing', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    const errors = await pictureNote(page, url, '&pics');
    await page.locator('#doc img.note-img').click();
    await page.evaluate(() => doc.destroy());
    const before = await page.evaluate(() => document.querySelector('#doc').innerHTML);
    expect(await pasteInto(page, { html: ownHtml('![copy](attachments/shot.png)'), files: 1, selector: 'body' })).toBe(false);
    expect(await page.evaluate(() => document.querySelector('#doc').innerHTML)).toBe(before);
    expect(await page.evaluate(() => window.pictured)).toBeUndefined();
    expect(errors).toEqual([]);
  }, { body: PICTURE_NOTE });
});

test('a narrow note keeps its outline as a small button on the chip row that opens the headings over the note', async ({ page }) => {
  await signIn(page);
  const note = await (await page.request.post('/api/notes', { data: { title: `Narrow outline ${Date.now().toString(36)}`, body: '## One\n\nText.\n\n## Two\n\nMore.' } })).json();
  await page.setViewportSize({ width: 700, height: 800 });
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('.note-layout')).not.toHaveAttribute('data-wide', '');
  const summary = page.locator('.note-outline summary');
  await expect(summary).toBeVisible();
  await expect(page.locator('.note-outline summary > svg')).toBeHidden();
  const button = await summary.boundingBox();
  expect([button.width, button.height]).toEqual([32, 32]);
  const article = await page.locator('article.note').boundingBox();
  const columns = await page.locator('.note-columns').boundingBox();
  const meta = await page.locator('.note-meta').boundingBox();
  expect(Math.abs(article.y - columns.y), 'no row above the note').toBeLessThanOrEqual(1);
  expect(Math.abs(button.x - article.x), 'the button lines up with the note').toBeLessThanOrEqual(1);
  expect(Math.abs(button.y + button.height / 2 - (meta.y + meta.height / 2)), 'the button sits on the chip row').toBeLessThanOrEqual(1);
  const persona = await page.locator('#margin-persona').boundingBox();
  expect(persona.x).toBeGreaterThanOrEqual(button.x + button.width);
  expect(await page.evaluate(([x, y]) => Boolean(document.elementFromPoint(x, y)?.closest('#margin-persona')), [persona.x + persona.width / 2, persona.y + persona.height / 2]), 'the chips stay clickable').toBe(true);
  await summary.click();
  const nav = page.locator('.note-outline nav');
  await expect(nav).toBeVisible();
  const panel = await nav.boundingBox();
  expect(await nav.evaluate((el) => getComputedStyle(el).position)).toBe('absolute');
  expect(Math.abs(panel.x - button.x)).toBeLessThanOrEqual(1);
  expect(panel.y).toBeGreaterThanOrEqual(button.y + button.height - 1);
  expect((await page.locator('article.note').boundingBox()).y, 'opening it moves nothing').toBe(article.y);
  await nav.getByRole('button', { name: 'Two', exact: true }).click();
  await expect(page.locator('#doc .blk:focus')).toContainText('Two');
  await expect(nav).toBeHidden();
  await page.request.delete(`/api/notes/${note.id}`);
});

test('text beside a drawn picture sits in its own element, so WebKit paints the selection on it', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    const picture = '![p](/api/image?inline=1)';
    await page.goto(url + '/?body=' + encodeURIComponent(`- [ ] ${picture} task words after\n\n${picture} paragraph words after`));
    await expect.poll(() => page.locator('#doc img.note-img').evaluateAll(imgs => imgs.length === 2 && imgs.every(img => img.naturalWidth > 0))).toBe(true);
    const bare = await page.locator('#doc').evaluate(doc => [...doc.querySelectorAll('img.note-img')].flatMap(img => [...(img.closest('.note-fig') || img).parentNode.childNodes].filter(n => n.nodeType === 3 && n.data.trim()).map(n => n.data)));
    expect(bare).toEqual([]);
    await expect(page.locator('#doc .task > span > span:not(.note-fig)')).toContainText('task words after');
    await page.getByText('paragraph words after').click();
    await expect(page.locator('#doc textarea')).toHaveValue(' paragraph words after');
    await expect(page.locator('#doc img.note-img')).toHaveCount(2);
  }, { body: 'x' });
});

test('an open line beside a picture is tall enough for all of its wrapped text', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 520, height: 900 });
    const words = 'less invasive to the note but the user should be able to exit the note that is split without having to scroll all the way up';
    await page.goto(url + '/?body=' + encodeURIComponent(`- [ ] ![p](/api/image?inline=1) ${words}`));
    await expect.poll(() => page.locator('#doc img.note-img').evaluate(img => img.naturalWidth)).toBeGreaterThan(0);
    await page.getByText('scroll all the way up').click({ position: { x: 4, y: 4 } });
    const area = page.locator('#doc textarea');
    await expect(area).toHaveValue(' ' + words);
    const fit = await area.evaluate(el => ({ lines: Math.round(el.scrollHeight / parseFloat(getComputedStyle(el).lineHeight)), scroll: el.scrollHeight, client: el.clientHeight }));
    expect(fit.lines).toBeGreaterThan(1);
    expect(fit.scroll).toBeLessThanOrEqual(fit.client + 1);
  }, { body: 'x' });
});
