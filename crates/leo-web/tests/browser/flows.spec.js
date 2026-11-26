const base = require('@playwright/test');
const { expect } = base;

const TECHNICAL = /Unexpected end of JSON|is not valid JSON|Failed to execute|Cannot read propert|is not a function|is not defined|\bundefined\b|\[object |\bNaN\b|SyntaxError|TypeError|ReferenceError/;
const test = base.test.extend({
  guard: [async ({ page }, use) => {
    const crashes = [];
    const shown = [];
    page.on('pageerror', (e) => crashes.push(e.message));
    await page.exposeFunction('__leoShown', (text) => { shown.push(text); });
    await page.addInitScript(() => {
      const seen = new WeakSet();
      const look = () => {
        for (const el of document.querySelectorAll('.toast, .msg-error, [role="alert"]')) {
          if (seen.has(el)) continue;
          seen.add(el);
          window.__leoShown(el.textContent || '');
        }
      };
      new MutationObserver(look).observe(document, { childList: true, subtree: true, characterData: true });
    });
    await use();
    expect(crashes, 'the page threw').toEqual([]);
    expect(shown.filter((t) => TECHNICAL.test(t)), 'a programmer error was shown to the user').toEqual([]);
  }, { auto: true }],
});
const fs = require('node:fs');
const path = require('node:path');
const { signIn, token } = require('./sign-in');

const FIXED_NOW = new Date(2026, 10, 3, 11, 40);
const FIXED_TODAY = '2026-11-03';
const FIXED_TOMORROW = '2026-11-04';

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

async function goPlace(page, action) {
  if (await page.locator('#side').isHidden()) {
    await page.locator('#menu').click();
    return page.locator(`.sheet [data-action="${action}"]`).click();
  }
  // Trash and Keeping live inside Settings on a wide screen.
  if (action === 'trash' || action === 'keeping') {
    await page.locator('#side [data-action="settings"]').click();
    return page.locator(`.settings-nav a[href="#/settings/${action}"]`).click();
  }
  await page.locator(`#side [data-action="${action}"]`).click();
}







function control(page, action) {
  return page.locator(`.new-card[data-action="${action}"]:visible, #side [data-action="${action}"]:visible`).first();
}

async function dragBetween(page, source, target, hold = '') {
  for (let tries = 0; ; tries++) {
    try {
      await source.scrollIntoViewIfNeeded();
      await source.evaluate(el => el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' }));
      break;
    } catch (e) {
      if (tries >= 2 || !/not attached/.test(String(e))) throw e;
      await expect(source).toBeVisible();
    }
  }
  const from = await source.boundingBox();
  expect(from).not.toBeNull();
  const x = from.x + Math.min(16, from.width / 2);
  const y = from.y + Math.min(20, from.height / 2);
  await page.mouse.move(x, y);
  await page.mouse.down();
  try {
    await page.mouse.move(x + 8, y + 8, { steps: 3 });
    await target.evaluate(el => el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' }));
    const into = await target.boundingBox();
    expect(into).not.toBeNull();
    const end = { x: into.x + into.width / 2, y: into.y + into.height / 2 };
    expect(await target.evaluate((el, point) => el.contains(document.elementFromPoint(point.x, point.y)), end), 'the drop target is visible above fixed controls').toBe(true);
    await page.mouse.move(end.x, end.y, { steps: 3 });
    await page.mouse.move(end.x, end.y);
    let nudge = 0;
    if (hold) await expect.poll(async () => {
      nudge = 1 - nudge;
      await page.mouse.move(end.x + nudge, end.y);
      return target.evaluate((el, name) => el.classList.contains(name), hold);
    }).toBe(true);
  } finally {
    await page.mouse.up();
  }
}

async function openSearchBox(page) {
  await page.goto('/#/search/');
  await expect(page.locator('#search-input')).toBeVisible();
}

async function dropFiles(page, selector, files) {
  const data = await page.evaluateHandle((list) => {
    const transfer = new DataTransfer();
    for (const f of list) transfer.items.add(new File([f.text], f.name, { type: f.type }));
    return transfer;
  }, files);
  await page.dispatchEvent(selector, 'dragover', { dataTransfer: data });
  await page.dispatchEvent(selector, 'drop', { dataTransfer: data });
}

async function fakeFileSends(page) {
  const puts = [];
  await page.route('**/api/import/files/**', async (route) => {
    const request = route.request();
    const parts = new URL(request.url()).pathname.split('/');
    puts.push({ method: request.method(), upload: parts.at(-2), item: parts.at(-1), name: new URL(request.url()).searchParams.get('name'), body: request.postDataBuffer() });
    await route.fulfill({ status: 204 });
  });
  return puts;
}

async function openNote(page, title) {
  const response = await page.request.post('/api/notes', { data: { title, body: 'Original sentence' } });
  expect(response.status()).toBe(201);
  const note = await response.json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#title')).toHaveText(title);
  await page.locator('.blk').first().click();
  return note;
}

test('retired practice state opens as Chat and old bookmarks return to the workspace', async ({ page }) => {
  const id = `retired-compat-${test.info().project.name}`;
  const messages = [{ role: 'assistant', text: 'An old saved explanation.', proposals: [{ kind: 'interview_test', state: 'applied', title: 'Old check' }] }];
  expect((await page.request.put(`/api/chats/${id}`, { data: { title: 'Saved conversation', mode: 'interview', refs: [], messages } })).ok()).toBe(true);
  for (const hash of ['#/interviews', '#/interviews/old-practice']) {
    await page.evaluate(({ id, messages, hash }) => {
      localStorage.setItem('leo-chat-v1', JSON.stringify({ id, mode: 'interview', messages, refs: [] }));
      localStorage.setItem('leo-last-place', hash);
      localStorage.setItem('leo-interview-drafts', 'preserve this stale state');
      sessionStorage.removeItem('leo-launched');
    }, { id, messages, hash });
    await page.goto('/');
    await expect(page).toHaveURL(/\/(?:#\/)?$/);
    await expect(page.locator('#app')).not.toBeEmpty();
    expect(await page.evaluate(() => localStorage.getItem('leo-last-place'))).toBe('#/');
    await page.goto(`/${hash}`);
    await expect(page).toHaveURL(/\/(?:#\/)?$/);
  }
  await page.locator('#chat-toggle').click();
  await expect(page.locator('#chat')).toContainText('An old saved explanation.');
  await expect(page.locator('#chat')).toContainText('Interview practice is no longer part of leo.');
  await expect(page.locator('[data-action="interviews"]')).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.getItem('leo-interview-drafts'))).toBe('preserve this stale state');
});

test('a failed save survives navigation and reload, then saves on reconnection', async ({ page }) => {
  const note = await openNote(page, 'Draft recovery');
  const refuseSaves = () => {
    const fetch = window.fetch.bind(window);
    window.__refusedSaves = 0;
    window.fetch = (input, options) => {
      const url = new URL(typeof input === 'string' ? input : input.url, location.href);
      if (!window.__allowNoteSaves && options && options.method === 'PATCH' && url.pathname.startsWith('/api/notes/')) {
        window.__refusedSaves++;
        return Promise.resolve(new Response(JSON.stringify({ error: 'The test server cannot save yet.' }), { status: 500, headers: { 'Content-Type': 'application/json' } }));
      }
      return fetch(input, options);
    };
  };
  await page.addInitScript(refuseSaves);
  await page.evaluate(refuseSaves);
  await page.locator('.line-edit').fill('Keep this phone draft');
  await expect(page.locator('#save-state')).toContainText('draft kept');
  const refused = await page.evaluate(() => window.__refusedSaves);
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press('Escape');
  await expect.poll(() => page.evaluate(() => window.__refusedSaves)).toBeGreaterThan(refused);
  await page.reload();
  await expect(page.locator('#save-state')).toContainText('draft kept');
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toContainText('Keep this phone draft');
  const original = await (await page.request.get(`/api/notes/${note.id}`)).json();
  expect(original.body).toBe('Original sentence');
  await page.evaluate(() => { window.__allowNoteSaves = true; window.dispatchEvent(new Event('online')); });
  await expect(page.locator('#save-state')).toHaveText('Saved');
  const saved = await (await page.request.get(`/api/notes/${note.id}`)).json();
  expect(saved.body).toBe('Keep this phone draft');
  expect(await page.evaluate(() => Object.keys(localStorage).filter((key) => key.startsWith('leo-draft-v1:')))).toEqual([]);
});

test('typing during an in-flight save keeps the latest edit', async ({ page }) => {
  const note = await openNote(page, 'Slow save');
  let release;
  const hold = new Promise((resolve) => { release = resolve; });
  let requests = 0;
  await page.route(`**/api/notes/${note.id}`, async (route) => {
    if (route.request().method() === 'PATCH' && ++requests === 1) await hold;
    await route.continue();
  });
  await page.locator('.line-edit').fill('First edit');
  await expect(page.locator('#save-state')).toHaveText('Saving…');
  await expect(page.locator('#save-state.save-wet svg.save-drop use')).toHaveAttribute('href', '#leo-mark');
  await page.locator('.line-edit').fill('Latest edit');
  release();
  await expect(page.locator('#save-state')).toHaveText('Saved');
  await expect(page.locator('#save-state.save-dry svg.save-drop')).toBeVisible();
  await expect.poll(() => page.locator('#save-state .save-drop').evaluate((el) => getComputedStyle(el).opacity)).toBe('0.45');
  const saved = await (await page.request.get(`/api/notes/${note.id}`)).json();
  expect(saved.body).toBe('Latest edit');
  expect(requests).toBe(2);
});

test('a phone edit preserves an external edit as a separate conflict copy', async ({ page }) => {
  const note = await openNote(page, 'Conflict example');
  const updated = await page.request.patch(`/api/notes/${note.id}`, { data: { body: 'Computer edit' } });
  expect(updated.status()).toBe(200);
  await page.locator('.line-edit').fill('Phone edit');
  await expect(page.locator('#save-state')).toHaveText('Saved');
  await expect(page.locator('#title')).toContainText('conflict from leo');
  const notes = await (await page.request.get('/api/notes')).json();
  expect(notes.find((n) => n.id === note.id).body).toBe('Computer edit');
  expect(notes.some((n) => n.title === 'Conflict example (conflict from leo)' && n.body === 'Phone edit')).toBe(true);
});

test('authenticated requests cannot create notes outside the notes folder', async ({ page }) => {
  for (const directory of ['../outside', '/tmp/outside', 'nested/../../outside', '..\\outside', 'C:\\outside', '.trash']) {
    const response = await page.request.post('/api/notes', { data: { title: 'Escape', directory } });
    expect(response.status()).toBe(400);
  }
  expect(fs.existsSync(path.join(process.env.LEO_BROWSER_HOME, 'outside'))).toBe(false);
});

test('the title placeholder sits behind the cursor and comes back when the title is cleared', async ({ page }) => {
  await page.goto('/#/new');
  const title = page.locator('#title');
  await expect(title).toHaveClass(/blank/);
  const before = await title.evaluate((el) => {
    const style = getComputedStyle(el, '::before');
    return { content: style.content, position: style.position };
  });
  expect(before).toEqual({ content: '"Title"', position: 'absolute' });
  await title.click();
  await page.locator('.blk').first().click();
  await title.click();
  expect(await title.evaluate(() => getSelection().anchorOffset)).toBe(0);
  await page.keyboard.type('Graphs');
  await expect(title).toHaveText('Graphs');
  await expect(title).not.toHaveClass(/blank/);
  for (let i = 0; i < 6; i++) await page.keyboard.press('Backspace');
  await expect(title).toHaveClass(/blank/);
  expect(await title.evaluate((el) => el.innerHTML)).toBe('');
});

test('a tap on any checkbox ticks it and saves without moving the note or opening the line', async ({ page }) => {
  const prefix = 'Reading above the checklist.\n\n'.repeat(100);
  const body = prefix + ['- [ ] dash', '* [ ] star', '+ [ ] plus', '1. [ ] numbered', '- [ ] parent', '  - [ ] nested', '- [x] done', '- [ ]', '-   [ ] wide', '', 'Plain line'].join('\n');
  const note = await (await page.request.post('/api/notes', { data: { title: `Ticks ${test.info().project.name}`, body } })).json();
  await page.goto(`/#/n/${note.id}`);
  const boxes = page.locator('.doc input[data-box]');
  await expect(boxes).toHaveCount(9);
  for (let i = 0; i < 9; i++) {
    const box = boxes.nth(i);
    const was = await box.isChecked();
    await box.scrollIntoViewIfNeeded();
    const before = await page.evaluate(() => scrollY);
    await box.click();
    await expect(boxes.nth(i), `box ${i + 1} toggles`).toBeChecked({ checked: !was });
    await expect(box).toBeFocused();
    expect(Math.abs(await page.evaluate(() => scrollY) - before)).toBeLessThanOrEqual(1);
    await expect(page.locator('.doc textarea')).toHaveCount(0);
  }
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body, { timeout: 5000 })
    .toBe(prefix + ['- [x] dash', '* [x] star', '+ [x] plus', '1. [x] numbered', '- [x] parent', '  - [x] nested', '- [ ] done', '- [x]', '-   [x] wide', '', 'Plain line'].join('\n'));
  await page.locator('.doc .task-item', { hasText: 'dash' }).locator('span').click();
  await expect(page.locator('.doc textarea')).toHaveCount(1);
});

test('a click 60 ms after a page change reaches the page, never the root element', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: `Quick ${test.info().project.name}`, body: '- [ ] first' } })).json();
  await page.goto('/#/');
  const card = page.locator(`.side-note[data-id="${note.id}"]`);
  await card.scrollIntoViewIfNeeded();
  await card.locator('.side-label').click();
  const seen = await page.evaluate(async () => {
    while (!document.querySelector('.doc input[data-box]')) await new Promise((resolve) => requestAnimationFrame(resolve));
    await new Promise((resolve) => setTimeout(resolve, 60));
    const box = document.querySelector('.doc input[data-box]').getBoundingClientRect();
    return { hit: document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2).tagName, root: document.documentElement.className };
  });
  expect(seen).toEqual({ hit: 'INPUT', root: '' });
  await page.locator('.doc input[data-box]').click({ timeout: 1000, trial: false });
  await expect(page.locator('.doc input[data-box]')).toBeChecked();
});

test.describe('forced colors', () => {
  test.use({ forcedColors: 'active' });
  test('surfaces keep an edge and every selected state differs from its neighbours', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Edge ${test.info().project.name}`, body: 'text' } })).json();
    await page.goto('/#/search/' + encodeURIComponent(note.title));
    await expect(page.locator(`.card[data-id="${note.id}"]`)).toBeVisible();
    const look = (selector) => page.locator(selector).first().evaluate((el) => { const cs = getComputedStyle(el); return { border: parseFloat(cs.borderTopWidth) + parseFloat(cs.borderLeftWidth), bg: cs.backgroundColor, color: cs.color }; });
    expect((await look('.card')).border, 'library cards have an edge').toBeGreaterThan(0);
    await page.goto('/#/settings/felix');
    await expect(page.locator('.segment.on').first()).toBeVisible();
    expect((await look('.set-card')).border, 'settings groups have an edge').toBeGreaterThan(0);
    expect((await look('.segment.on')).bg, 'a segmented choice shows which is selected').not.toBe((await look('.segment:not(.on)')).bg);
    expect((await look('.settings-link.on')).bg, 'the settings section list shows where you are').not.toBe((await look('.settings-link:not(.on)')).bg);
    for (const where of ['/#/settings/felix', '/#/settings/general']) {
      await page.goto(where);
      await expect(page.locator('.choice-pick.on').first()).toBeVisible();
      const chosen = await look('.choice-pick.on');
      expect(chosen.border + (await look('.choice-pick:not(.on)')).border, `choice cards have edges on ${where}`).toBeGreaterThan(0);
    expect(chosen.bg, `the chosen card differs from the others on ${where}`).not.toBe((await look('.choice-pick:not(.on)')).bg);
    }
  });
  test('the explorer shows the open place and the rail has edges', async ({ page }) => {
    await page.goto('/#/settings/general');
    await expect(page.locator('#side .side-rail .side-row.on')).toBeVisible();
    const row = await page.locator('#side .side-rail .side-row.on').evaluate((el) => { const cs = getComputedStyle(el); return { bg: cs.backgroundColor, border: parseFloat(cs.borderTopWidth) }; });
    const other = await page.locator('#side .side-rail .side-row:not(.on)').first().evaluate((el) => getComputedStyle(el).backgroundColor);
    expect(row.bg).not.toBe(other);
    expect(row.border).toBeGreaterThan(0);
  });
});

test('on a wide screen the sidebar reaches every place and can be narrowed', async ({ page }) => {
  const wide = test.info().project.name === 'desktop';
  await page.request.post('/api/dirs', { data: { path: 'side-course' } });
  await page.goto('/');
  const side = page.locator('#side');
  if (!wide) {
    await expect(side).toBeHidden();
    await expect(page.locator('#menu')).toBeVisible();
    return;
  }
  await expect(side).toBeVisible();
  await expect(page.locator('#menu')).toBeHidden();
  await expect(side.locator('.side-rail [data-action="home"]'), 'notes are opened from the explorer, not a rail button').toHaveCount(0);
  for (const [action, url] of [['settings', /#\/settings$/]]) {
    await side.locator(`[data-action="${action}"]`).click();
    await expect(page).toHaveURL(url);
    await expect(side.locator(`[data-action="${action}"]`)).toHaveAttribute('aria-current', 'page');
  }
  // Trash and Keeping moved into Settings' own sidebar; the rail keeps Settings selected there.
  await expect(side.locator('[data-action="trash"], [data-action="keeping"]')).toHaveCount(0);
  for (const section of ['trash', 'keeping']) {
    await page.locator(`.settings-nav a[href="#/settings/${section}"]`).click();
    await expect(page).toHaveURL(new RegExp(`#/settings/${section}$`));
    await expect(page.locator('.settings-nav a[aria-current="page"]')).toHaveAttribute('href', `#/settings/${section}`);
    await expect(side.locator('[data-action="settings"]')).toHaveAttribute('aria-current', 'page');
  }
  await side.locator('[data-action="settings"]').click();
  await expect(page).toHaveURL(/#\/settings$/);
  await side.locator('[data-action="open-folder"][data-dir="side-course"]').click();
  await expect(side.locator('[data-action="open-folder"][data-dir="side-course"]')).toHaveAttribute('aria-expanded', 'true');
  await expect(page).toHaveURL(/#\/settings$/);
  await expect(side.locator('[data-action="chat"]')).toHaveCount(0);
  await page.locator('#chat-toggle').click();
  await expect(page.locator('#chat')).toBeVisible();
  await page.locator('[data-chat="close"]').click();

  await page.keyboard.press('ControlOrMeta+k');
  await expect(page.locator('.scrim:not(.scrim-gone) #quick-input')).toBeFocused();
  const cameo = page.locator('.scrim:not(.scrim-gone) .quick-perch');
  await expect(cameo).toHaveAttribute('aria-hidden', 'true');
  await expect.poll(() => cameo.evaluate((el) => {
    const face = el.querySelector('svg.felix').getBoundingClientRect();
    const sheet = el.closest('.sheet').getBoundingClientRect();
    const box = el.querySelector('svg.felix').viewBox.baseVal;
    return { scale: face.width / box.width, above: face.top < sheet.top, pointer: getComputedStyle(el).pointerEvents };
  }), { message: 'Felix sits on the quick bar at whole pixels and takes no clicks' }).toEqual({ scale: 1, above: true, pointer: 'none' });
  await page.evaluate(() => window.addEventListener('keydown', (e) => { window.escaped = e.defaultPrevented; }, { once: true }));
  await page.keyboard.press('Escape');
  expect(await page.evaluate(() => window.escaped), 'Escape stays with the page, so it never leaves full screen').toBe(true);
  await expect(page.locator('.scrim:not(.scrim-gone)')).toHaveCount(0);
  await side.locator('.side-group [data-action="new-folder"]').click();
  await page.locator('#folder-name').fill('draft folder');
  await page.keyboard.press('ControlOrMeta+k');
  await expect(page.locator('.quick-leo'), 'an open sheet keeps what was typed in it').toHaveCount(0);
  await expect(page.locator('#folder-name')).toHaveValue('draft folder');
  await page.keyboard.press('Escape');
  await expect(page.locator('.scrim:not(.scrim-gone)')).toHaveCount(0);

  const full = (await side.boundingBox()).width;
  await side.locator('[data-action="side-fold"]').click();
  await expect(page.locator('body')).toHaveClass(/side-mini/);
  await expect(page.locator('body')).not.toHaveClass(/side-folding/);
  const narrow = (await side.boundingBox()).width;
  expect(narrow).toBeLessThan(80);
  expect((await page.locator('main').boundingBox()).x).toBeGreaterThanOrEqual(narrow);
  await expect(side.locator('.side-row .side-label').first()).toBeHidden();
  await page.reload();
  await expect(page.locator('body')).toHaveClass(/side-mini/);
  await side.locator('[data-action="side-fold"]').click();
  await expect(page.locator('body')).not.toHaveClass(/side-folding/);
  expect((await side.boundingBox()).width).toBe(full);
});

test('a board keeps its cards in the columns they were dragged to', async ({ page }) => {
  await goPlace(page, 'boards');
  await expect(page).toHaveURL(/#\/boards/);
  const name = `Semester ${test.info().project.name} ${Date.now()}`;
  await page.locator('[data-action="kanban-new-board"]').first().click();
  await page.locator('#kanban-board-name').fill(name);
  await page.locator('#kanban-board-make').click();
  await expect(page.locator('.kanban-title')).toHaveValue(name);
  const columns = page.locator('.kanban-col');
  await expect(columns).toHaveCount(3);
  expect(await columns.locator('.kanban-col-name').evaluateAll((inputs) => inputs.map((input) => input.value))).toEqual(['To do', 'In progress', 'Done']);

  await columns.nth(0).locator('[data-action="kanban-add-card"]').click();
  const composer = page.locator('.kanban-composer textarea');
  for (const title of ['Lab report', 'Problem set 5']) {
    await composer.fill(title);
    await composer.press('Enter');
  }
  await expect(columns.nth(0).locator('.kanban-card')).toHaveCount(2);
  await composer.press('Escape');

  await dragBetween(page, columns.nth(0).locator('.kanban-card', { hasText: 'Lab report' }), columns.nth(1));
  await expect(columns.nth(1).locator('.kanban-card')).toHaveText(['Lab report']);
  await expect(page.locator('.kanban-ghost')).toHaveCount(0);
  await expect(page.locator('.scrim')).toHaveCount(0);

  await columns.nth(0).locator('.kanban-card').click();
  await page.locator('#kanban-card-sub').fill('Chemistry');
  await page.locator('#kanban-card-description').click();
  await page.keyboard.type('Questions 1 to 8');
  await page.locator('#kanban-panel [data-kanban="close"]').click();
  await expect(page.locator('#kanban-panel')).toBeHidden();
  await expect(columns.nth(0).locator('.kanban-card-sub')).toHaveText('Chemistry');
  await expect(columns.nth(0).locator('.kanban-has-text')).toBeVisible();

  await page.reload();
  await expect(page.locator('.kanban-title')).toHaveValue(name);
  await expect(page.locator('.kanban-col').nth(1).locator('.kanban-card-title')).toHaveText(['Lab report']);
  await expect(page.locator('.kanban-col').nth(0).locator('.kanban-card-title')).toHaveText(['Problem set 5']);
  await page.locator('.kanban-col').nth(0).locator('.kanban-card').click();
  await expect(page.locator('#kanban-card-description')).toContainText('Questions 1 to 8');
  await page.keyboard.press('Escape');

  const board = (await (await page.request.get('/api/boards')).json()).boards.find((b) => b.name === name);
  expect(board.cards).toBe(2);
  expect((await page.request.delete(`/api/boards/${board.id}`)).status()).toBe(200);
});

test.describe('editing like Obsidian', () => {
  const body = ['First line here', 'Second **bold** line', '- Third item', 'Fourth line'].join('\n');
  async function open(page, tag) {
    const note = await (await page.request.post('/api/notes', { data: { title: `Select ${tag} ${test.info().project.name}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc .blk')).toHaveCount(4);
    return async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
  }
  async function select(page, from, to) {
    await page.evaluate(([a, b]) => {
      const text = (i) => {
        const walker = document.createTreeWalker(document.querySelectorAll('#doc .blk')[i], NodeFilter.SHOW_TEXT);
        return walker.nextNode();
      };
      const range = document.createRange();
      range.setStart(text(a[0]), a[1]);
      range.setEnd(text(b[0]), b[1]);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.activeElement.blur();
    }, [from, to]);
  }

  test('note arrival never moves text before its animation finishes', async ({ page }) => {
    await page.addInitScript(() => {
      const animate = Element.prototype.animate;
      Element.prototype.animate = function (frames, options) {
        const run = animate.call(this, frames, options);
        if (this.matches('#doc')) { run.pause(); run.currentTime = 0; }
        return run;
      };
    });
    for (const body of ['A paragraph', '## A heading', '- A list item', '- [ ] A task', '```js\nconst n = 1;\n```', '| A | B |\n| --- | --- |\n| One | Two |', '> A quote', '> [!note] A callout\n> Text', '$$\nx = 2\n$$']) {
      const note = await (await page.request.post('/api/notes', { data: { title: 'Arrival', body } })).json();
      await page.goto(`/#/n/${note.id}`);
      await expect(page.locator('#doc .blk').first()).toBeVisible();
      if (body.startsWith('$$')) await expect(page.locator('#doc .math')).toHaveAttribute('data-drawn', 'yes');
      const place = () => page.locator('#doc .blk').first().boundingBox();
      const before = await place();
      await page.locator('#doc').evaluate(el => { for (const run of el.getAnimations()) run.finish(); });
      await expect.poll(place, { message: body }).toEqual(before);
    }
  });

  test('a dragged selection across lines stays rendered in place, copies as Markdown, and Delete removes all of it', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    const saved = await open(page, 'drag');
    const first = await page.locator('#doc .blk').nth(0).locator('p').boundingBox();
    const third = await page.locator('#doc .blk').nth(2).locator('li').boundingBox();
    const fourth = await page.locator('#doc .blk').nth(3).boundingBox();
    await page.mouse.move(first.x + 2, first.y + first.height / 2);
    await page.mouse.down();
    await page.mouse.move(third.x + third.width - 2, third.y + third.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect(page.locator('#doc textarea.line-edit')).toHaveCount(0);
    expect(await page.evaluate(() => getSelection().toString())).toContain('Second bold line');
    expect(await page.locator('#doc .blk').nth(3).boundingBox()).toEqual(fourth);
    const copied = await page.evaluate(() => {
      const data = new DataTransfer();
      document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
      return data.getData('text/plain');
    });
    expect(copied).toContain('Second **bold** line\n- Third item');
    await page.keyboard.press('Backspace');
    await expect.poll(saved, { timeout: 5000 }).toBe('\nFourth line');
  });

  test('dragging out of the line being edited selects across the lines below', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    const saved = await open(page, 'drag-out');
    await page.locator('#doc .blk').nth(0).click();
    const area = page.locator('#doc textarea.line-edit');
    await expect(area).toBeFocused();
    const box = await area.boundingBox();
    const third = await page.locator('#doc .blk').nth(2).boundingBox();
    await page.mouse.move(box.x + 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 40, box.y + box.height / 2, { steps: 3 });
    await page.mouse.move(third.x + third.width - 2, third.y + third.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect(page.locator('#doc textarea.line-edit')).toHaveCount(0);
    const picked = await page.evaluate(() => getSelection().toString());
    expect(picked).toContain('First line here');
    expect(picked).toContain('Second bold line');
    await page.keyboard.press('Backspace');
    await expect.poll(saved, { timeout: 5000 }).toBe('\nFourth line');
  });

  test('typing over a selection replaces it, keeping the rest of both lines', async ({ page }) => {
    const saved = await open(page, 'type');
    await select(page, [0, 6], [2, 5]);
    await page.keyboard.press('X');
    await expect.poll(saved, { timeout: 5000 }).toBe(['First X item', 'Fourth line'].join('\n'));
    await expect(page.locator('#doc textarea.line-edit')).toBeFocused();
  });

  test('Shift+click from the caret selects every line between, and Cmd+A takes the whole note', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'Shift+click is for a mouse');
    const saved = await open(page, 'shift');
    await page.locator('#doc .blk').nth(1).click();
    const area = page.locator('#doc textarea.line-edit');
    await area.evaluate((a) => a.setSelectionRange(0, 0));
    const last = await page.locator('#doc .blk').nth(3).locator('p').boundingBox();
    await page.keyboard.down('Shift');
    await page.mouse.click(last.x + last.width - 1, last.y + last.height / 2);
    await page.keyboard.up('Shift');
    await expect(area).toHaveCount(0);
    const picked = await page.evaluate(() => getSelection().toString());
    expect(picked).toContain('Second bold line');
    expect(picked).toContain('Fourth line');
    expect(picked).not.toContain('First line here');
    await page.keyboard.press('Delete');
    await expect.poll(saved, { timeout: 5000 }).toBe('First line here\n');
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.press('Backspace');
    await expect.poll(saved, { timeout: 5000 }).toBe('');
  });

  test('one Cmd/Ctrl+A while editing a line takes the whole note', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'select all is a keyboard shortcut');
    const saved = await open(page, 'all');
    await page.locator('#doc .blk').nth(1).click();
    await expect(page.locator('#doc textarea.line-edit')).toBeFocused();
    await page.keyboard.press('ControlOrMeta+a');
    const area = page.locator('#doc textarea.line-edit');
    await expect(area).toHaveCount(0);
    expect(await page.evaluate(() => getSelection().toString())).toContain('Fourth line');
    expect(await page.evaluate(() => getSelection().toString())).toContain('First line here');
    await page.keyboard.press('Backspace');
    await expect.poll(saved, { timeout: 5000 }).toBe('');
  });

  test('Cmd/Ctrl+Z brings back a deleted section and Shift+Cmd+Z takes it away again', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'undo is a keyboard shortcut');
    const saved = await open(page, 'undo');
    await select(page, [0, 0], [2, 5]);
    await page.keyboard.press('Backspace');
    await expect.poll(saved, { timeout: 5000 }).toBe(' item\nFourth line');
    await page.keyboard.type('New start');
    await expect.poll(saved, { timeout: 5000 }).toBe('New start item\nFourth line');
    await page.keyboard.press('ControlOrMeta+z');
    await expect.poll(saved, { timeout: 5000 }).toBe(' item\nFourth line');
    await page.keyboard.press('ControlOrMeta+z');
    await expect.poll(saved, { timeout: 5000 }).toBe(body);
    const area = page.locator('#doc textarea.line-edit');
    await expect(area).toHaveCount(0);
    expect(await page.evaluate(() => getSelection().toString())).toContain('Second bold line');
    await page.keyboard.press('ControlOrMeta+Shift+z');
    await expect.poll(saved, { timeout: 5000 }).toBe(' item\nFourth line');
    await page.locator('#doc').click({ position: { x: 5, y: 5 } });
    await page.keyboard.press('Escape');
    await page.keyboard.press('ControlOrMeta+z');
    await expect.poll(saved, { timeout: 5000 }).toBe(body);
  });

  test('a selection that starts and ends outside the note leaves it alone', async ({ page }) => {
    const saved = await open(page, 'outside');
    await page.evaluate(() => {
      const range = document.createRange();
      range.setStartBefore(document.querySelector('#title'));
      range.setEndAfter(document.querySelector('#doc'));
      window.getSelection().removeAllRanges();
      window.getSelection().addRange(range);
    });
    await page.keyboard.press('Backspace');
    await page.waitForTimeout(900);
    expect(await saved()).toBe(body);
  });
});

test.describe('math', () => {
  test('LaTeX in a note and in Felix is drawn as math, with its fonts', async ({ page }) => {
    const fonts = [];
    page.on('response', (r) => { if (r.url().includes('/vendor/katex-0.16.11/fonts/')) fonts.push(r.status()); });
    const body = 'Energy is $E = mc^2$, not $5.\n\n$$\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n$$';
    const note = await (await page.request.post('/api/notes', { data: { title: `Math ${test.info().project.name}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    const inline = page.locator('#doc .math[data-drawn="yes"]:not(.math-block) .katex');
    await expect(inline).toBeVisible({ timeout: 10000 });
    await expect(page.locator('#doc .math-block[data-drawn="yes"] .katex-display')).toBeVisible();
    await expect(page.locator('#doc')).toContainText('not $5.');
    await expect.poll(() => fonts.length).toBeGreaterThan(0);
    expect(fonts.every((status) => status === 200)).toBe(true);

    const said = 'The area is $\\pi r^2$, or \\(\\pi r^2\\), and in full:\n\\[\nA = \\int_0^r 2\\pi t\\,dt\n\\]\n\\begin{aligned}\na &= b\n\\end{aligned}';
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { t: said }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' }));
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('area of a circle?');
    await page.locator('#chat-input').press('Enter');
    await expect(page.locator('#chat .msg.leo .math[data-drawn="yes"]:not(.math-block) .katex')).toHaveCount(2);
    await expect(page.locator('#chat .msg.leo .math-block[data-drawn="yes"] .katex-display')).toHaveCount(2);
    const outside = await page.locator('#chat .msg.leo .prose').evaluate((el) => {
      const copy = el.cloneNode(true);
      copy.querySelectorAll('.katex, .katex-display').forEach((k) => k.remove());
      return copy.textContent;
    });
    expect(outside).not.toMatch(/\\\(|\\\[|\\begin|\$/);
  });
});

test.describe('diagrams', () => {
  test('a mermaid block in a note is drawn, a broken one says why, and labels cannot run code', async ({ page }) => {
    const body = [
      'Breadth-first search:',
      '',
      '```mermaid',
      'flowchart LR',
      '  A["Start (source)"] --> B[Visit neighbours]',
      '  B --> C{Queue empty?}',
      '  C -- no --> B',
      '```',
      '',
      '```mermaid',
      'flowchart LR',
      '  X["<img src=x onerror=window.__pwned=1>"] --> Y',
      '```',
      '',
      '```mermaid',
      'this is not a diagram',
      '```',
    ].join('\n');
    const note = await (await page.request.post('/api/notes', { data: { title: `Diagram ${test.info().project.name}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    const drawn = page.locator('#doc figure.diagram[data-drawn="yes"]');
    await expect(drawn).toHaveCount(2, { timeout: 15000 });
    await expect(drawn.first().locator('svg')).toBeVisible();
    await expect(drawn.first()).toContainText('Visit neighbours');
    await expect(drawn.first().locator('.diagram-src')).toBeHidden();
    await expect(page.locator('#doc .diagram-error')).toContainText('This diagram could not be drawn');
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined();
    await expect(page.locator('#doc figure.diagram img[onerror]')).toHaveCount(0);
  });

  test('Felix can answer with a diagram, drawn once the answer is complete', async ({ page }) => {
    const answer = 'Here is how it flows:\n\n```mermaid\nflowchart TD\n  Q[Queue] --> V[Visit]\n```\n';
    const lines = [{ sources: [] }, ...answer.match(/[\s\S]{1,9}/g).map((t) => ({ t })), { done: true }];
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' }));
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('draw bfs');
    await page.locator('#chat-input').press('Enter');
    const figure = page.locator('#chat .msg.leo figure.diagram[data-drawn="yes"]');
    await expect(figure.locator('svg')).toBeVisible({ timeout: 15000 });
    await expect(figure).toContainText('Visit');
  });
});

test('a fresh launch reopens the last note, while reloads and the workspace stay put', async ({ page, context }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: `Left off ${Date.now().toString(36)}`, body: 'Where I was.' } })).json();
  await page.goto('/');
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toContainText('Where I was.');
  await page.goto('/');
  await expect(page, 'going to the workspace in the same window stays there').toHaveURL(/\/(#\/)?$/);
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toContainText('Where I was.');
  const relaunch = await context.newPage();
  await relaunch.goto('/');
  await expect(relaunch, 'a new launch goes back to the note').toHaveURL(new RegExp(`#/n/${note.id}$`));
  await expect(relaunch.locator('#doc')).toContainText('Where I was.');
  await relaunch.reload();
  await expect(relaunch).toHaveURL(new RegExp(`#/n/${note.id}$`));
  await relaunch.close();
  await page.request.delete(`/api/notes/${note.id}`);
  const gone = await context.newPage();
  await gone.goto('/');
  await expect(gone.locator('.toast')).toContainText('not there any more');
  await gone.close();
  const after = await context.newPage();
  await after.goto('/');
  await expect(after.locator('.toast'), 'a note that is gone is not reopened on every launch').toHaveCount(0);
  await expect(after).not.toHaveURL(/#\/n\//);
  await after.close();
});

test.describe('drag and drop', () => {
  test('a note and a folder drag into other folders, and Undo puts them back', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    const tag = Date.now().toString(36);
    const [a, b] = [`dd-a-${tag}`, `dd-b-${tag}`];
    for (const dir of [a, b]) await page.request.post('/api/dirs', { data: { path: dir } });
    const note = await (await page.request.post('/api/notes', { data: { title: `Drag me ${tag}`, body: 'x' } })).json();
    const where = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).directory;
    await page.goto('/');
    const card = page.locator(`.side-note[data-id="${note.id}"]`);
    await dragBetween(page, card, page.locator(`#side .side-row[data-action="open-folder"][data-dir="${a}"]`));
    await expect(page.locator('.toast')).toContainText(`Moved “Drag me ${tag}” to ${a}`);
    expect(await where()).toBe(a);
    await expect(card).toHaveCount(0);
    await page.locator('.toast button').click();
    await expect(card).toBeVisible();
    await expect(card).toHaveAttribute('data-from', '');
    expect(await where()).toBe('');

    await dragBetween(page, card, page.locator(`#side [data-action="open-folder"][data-dir="${b}"]`));
    await expect.poll(where).toBe(b);
    await expect(card).toHaveCount(0);

    await dragBetween(page, page.locator(`#side .side-row[data-action="open-folder"][data-dir="${b}"]`), page.locator(`#side .side-row[data-action="open-folder"][data-dir="${a}"]`));
    await expect(page.locator('.toast')).toContainText(`Moved ${b} into ${a}`);
    expect(await where()).toBe(`${a}/${b}`);
    await expect(page.locator(`#side .side-row[data-action="open-folder"][data-dir="${b}"]`)).toHaveCount(0);
    const folders = await (await page.request.get('/api/folders')).json();
    expect(folders.map((d) => d.name).filter((name) => name.startsWith(`${a}/`))).toEqual([`${a}/${b}`]);
    await page.locator('.toast button').click();
    await expect(page.locator(`#side .side-row[data-action="open-folder"][data-dir="${b}"]`)).toBeVisible();
    expect(await where()).toBe(b);

    await page.goto(`/#/f/${b}`);
    await dragBetween(page, page.locator(`.side-note[data-id="${note.id}"]`), page.locator('#side .side-group[data-drop-dir=""]'));
    await expect.poll(where).toBe('');
  });

  test('holding a note on another note in the explorer opens Combine, and a quick drop still moves it', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    const tag = Date.now().toString(36);
    const top = `hold-${tag}`;
    const inner = `${top}/inner`;
    await page.request.post('/api/dirs', { data: { path: inner } });
    const make = async (title, directory) => (await page.request.post('/api/notes', { data: { title, body: 'x', directory } })).json();
    const a = await make(`Hold A ${tag}`, top);
    const b = await make(`Hold B ${tag}`, top);
    const c = await make(`Hold C ${tag}`, inner);
    await page.goto(`/#/n/${c.id}`);
    const row = (note) => page.locator(`#side .side-note[data-id="${note.id}"]`);
    await expect(row(b)).toBeVisible();
    await dragBetween(page, row(a), row(b), 'combine-armed');
    const sheet = page.locator('.scrim:not(.scrim-gone) .sheet');
    await expect(sheet.locator('h3')).toHaveText('Combine these notes?');
    await expect(sheet).toContainText(`“${a.title}” will be merged into “${b.title}”.`);
    await expect(page.locator('.combine-wait, .combine-armed')).toHaveCount(0);
    await sheet.locator('[data-action="close"]').click();
    await expect(page.locator('.scrim:not(.scrim-gone)')).toHaveCount(0);

    await row(c).scrollIntoViewIfNeeded();
    await page.evaluate(([from, to]) => {
      const pick = (id) => document.querySelector(`#side .side-note[data-id="${id}"]`);
      const data = new DataTransfer();
      const fire = (el, type) => el.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: data }));
      fire(pick(from), 'dragstart');
      fire(pick(to), 'dragover');
      fire(pick(to), 'drop');
      fire(pick(from), 'dragend');
    }, [a.id, c.id]);
    await expect(page.locator('.toast'), 'a drop that was not held moves the note into that note’s folder').toContainText(`Moved “${a.title}” to inner`);
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${a.id}`)).json()).directory).toBe(inner);
    await expect(page.locator('.scrim:not(.scrim-gone)')).toHaveCount(0);
  });

  test('files dropped on the page make a note, and on Felix go to him', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    await page.goto('/');
    await dropFiles(page, 'main', [{ name: 'dropped.txt', type: 'text/plain', text: 'Heaps keep the minimum at the root.' }]);
    await page.locator('#drop-make').click();
    await expect(page.locator('#upload-list')).toContainText('dropped.txt');
    await page.keyboard.press('Escape');

    const note = await (await page.request.post('/api/notes', { data: { title: 'Dragged to Felix', body: 'x' } })).json();
    await page.reload();
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await dropFiles(page, '#chat', [{ name: 'for-felix.txt', type: 'text/plain', text: 'Heaps keep the minimum at the root.' }]);
    await expect(chat.locator('#chat-refs .file-card', { hasText: 'for-felix.txt' })).toBeVisible();
    await expect(page.locator('.scrim')).toHaveCount(0);
    await dragBetween(page, page.locator(`.side-note[data-id="${note.id}"]`), chat.locator('#chat-input'));
    await expect(chat.locator('.note-card', { hasText: 'Dragged to Felix' })).toBeVisible();
  });
});

test('on a phone a sheet swipes down to close, and a small drag springs back', async ({ page }) => {
  test.skip(test.info().project.name !== 'phone', 'sheets slide up from the bottom on phones');
  await page.goto('/');
  const swipe = (distance, ms) => page.evaluate(async ([distance, ms]) => {
    const sheet = document.querySelector('.sheet');
    const box = sheet.getBoundingClientRect();
    const at = (y) => [new Touch({ identifier: 1, target: sheet, clientX: box.left + 40, clientY: y })];
    const top = box.top + 20;
    let now = performance.now();
    performance.now = () => now;
    try {
      sheet.dispatchEvent(new TouchEvent('touchstart', { touches: at(top), changedTouches: at(top), bubbles: true, cancelable: true }));
      const steps = 6;
      for (let i = 1; i <= steps; i++) {
        now += ms / steps;
        const y = top + (distance * i) / steps;
        sheet.dispatchEvent(new TouchEvent('touchmove', { touches: at(y), changedTouches: at(y), bubbles: true, cancelable: true }));
      }
      sheet.dispatchEvent(new TouchEvent('touchend', { touches: [], changedTouches: at(top + distance), bubbles: true, cancelable: true }));
    } finally {
      delete performance.now;
    }
  }, [distance, ms]);
  await page.locator('#menu').click();
  await expect(page.locator('.sheet')).toBeVisible();
  await swipe(20, 400);
  await page.waitForTimeout(300);
  await expect(page.locator('.sheet')).toBeVisible();
  await swipe(160, 300);
  await expect(page.locator('.scrim')).toHaveCount(0);
  await page.locator('#menu').click();
  await swipe(40, 40);
  await expect(page.locator('.scrim'), 'a quick flick closes it too').toHaveCount(0);
});

test('search finds a note named after a command', async ({ page }) => {
  await page.request.post('/api/notes', { data: { title: 'backup checklist', body: 'Check my backups' } });
  await openSearchBox(page);
  await page.locator('#search-input').fill('backup');
  await expect(page.locator('.card').filter({ hasText: 'backup checklist' })).toBeVisible();
  const match = page.locator('.card').filter({ hasText: 'backup checklist' }).locator('.card-title mark');
  await expect(match).toHaveText('backup');
  const look = await match.evaluate((el) => { const s = getComputedStyle(el), u = getComputedStyle(el, '::after'); return { bg: s.backgroundColor, ink: s.color, text: getComputedStyle(document.body).color, under: u.maskImage || u.webkitMaskImage }; });
  expect(look.bg).toBe('rgba(0, 0, 0, 0)');
  expect(look.ink).toBe(look.text);
  expect(look.under).toContain('svg');
});

test('search shows placeholder cards while it waits', async ({ page }) => {
  await page.route('**/api/search*', async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 800));
    await route.continue();
  });
  await page.goto('/#/search/queue');
  await expect(page.locator('#app .section-title')).toHaveText('Searching…');
  await expect(page.locator('#app .cards .skeleton')).toHaveCount(3);
  await expect(page.locator('#app .cards .skeleton')).toHaveCount(0);
});

test('search reads a page at a time and Show more asks for the next page', async ({ page }) => {
  test.slow();
  const word = `zzpaged${Date.now().toString(36)}${test.info().project.name.replace(/\W/g, '')}`;
  const ids = [];
  try {
    for (let n = 0; n < 130; n++) {
      const note = await (await page.request.post('/api/notes', { data: { title: `${word} ${n}`, body: `${word} note ${n}` } })).json();
      ids.push(note.id);
    }
    await page.evaluate(() => { delete window.IntersectionObserver; });
    const asked = [];
    page.on('request', (request) => { if (request.url().includes('/api/search')) asked.push(new URL(request.url()).searchParams); });
    await page.goto(`/#/search/${word}`);
    expect(await page.evaluate(() => typeof window.IntersectionObserver)).toBe('undefined');
    const cards = page.locator('#app .cards .card');
    await expect(cards).toHaveCount(60);
    await expect(page.locator('#search-count')).toHaveText('60 notes so far');
    const more = page.getByRole('button', { name: 'Show more' });
    await more.click();
    await expect(cards).toHaveCount(120);
    await expect(page.locator('#search-count')).toHaveText('120 notes so far');
    await more.click();
    await expect(cards).toHaveCount(130);
    await expect(page.locator('#search-count')).toHaveText('130 notes');
    await expect(more).toHaveCount(0);
    await expect(page.locator('#app')).not.toContainText('first 300');
    expect(asked.map((params) => params.get('offset'))).toEqual(['0', '60', '120']);
    expect(asked.every((params) => params.get('limit') === '61')).toBe(true);
  } finally {
    if (ids.length) {
      await expect(await page.request.post('/api/trash/move', { data: { notes: ids } })).toBeOK();
    }
  }
});

test('nothing found offers Felix and a new note', async ({ page }) => {
  const tag = Date.now().toString(36);
  const ask = `zzqq-ask-${tag}`;
  const fresh = `zzqq-new-${tag}`;
  await page.goto(`/#/search/${ask}`);
  await expect(page.getByRole('heading', { name: `No notes match “${ask}”` })).toBeVisible();
  await page.locator('#app').getByRole('button', { name: 'Ask Felix' }).click();
  await expect(page.locator('#chat')).toBeVisible();
  await expect(page.locator('#chat-body')).toContainText(ask);
  await page.locator('#chat [data-chat="close"]').click();
  await page.goto(`/#/search/${fresh}`);
  await page.getByRole('button', { name: `New note “${fresh}”` }).click();
  await expect(page).toHaveURL(/#\/n\//);
  await expect(page.locator('#title')).toHaveText(fresh);
});


test.describe('note connections', () => {
  test('graph navigation and endpoints are removed', async ({ page }) => {
    await page.goto('/#/map');
    await expect(page.locator('#map-canvas')).toHaveCount(0);
    await expect(page.locator('[data-action="map"], [data-action="note-map"]')).toHaveCount(0);
    for (const url of ['/graph.js', '/api/graph', '/api/graph/status']) {
      expect((await page.request.get(url)).status()).toBe(404);
    }
    expect((await page.request.post('/api/graph/build')).status()).toBe(404);
  });

  async function seed(page) {
    const made = {};
    for (const [title, body, directory] of [
      ['Graph traversals', 'BFS uses a queue. See [[Scheduling]].', 'cs130'],
      ['Scheduling', 'Round robin takes from a ready queue.', 'cs162'],
      ['Heaps', 'A binary heap backs a priority queue.', 'cs130'],
    ]) {
      await page.request.post('/api/dirs', { data: { path: directory } });
      made[title] = await (await page.request.post('/api/notes', { data: { title, body, directory } })).json();
    }
    const id = (t) => made[t].id;
    const cache = {
      notes: {
        [id('Graph traversals')]: { hash: 'old', summary: 'How BFS explores a graph with a queue', concepts: ['breadth-first search', 'queue'] },
        [id('Scheduling')]: { hash: 'old', summary: 'Round robin runs the next process from a queue', concepts: ['round robin', 'queue'] },
        [id('Heaps')]: { hash: 'old', summary: 'Binary heaps implement priority queues', concepts: ['binary heap'] },
      },
      pairs: {
        '1:0-0': {
          hash: 'old',
          links: [
            { a: id('Scheduling'), b: id('Graph traversals'), kind: 'same method', strength: 3, why: 'Both take the next piece of work from a queue' },
            { a: id('Graph traversals'), b: id('Heaps'), kind: 'builds on', strength: 1, why: 'Dijkstra swaps the queue for a heap' },
          ],
        },
      },
      built_at: '2026-10-06T00:00:00Z',
    };
    fs.writeFileSync(path.join(process.env.LEO_BROWSER_HOME, 'graph.json'), JSON.stringify(cache));
    return made;
  }

  test('search finds a note through the ideas on its map and says so', async ({ page }) => {
    const made = await seed(page);
    await page.goto('/#/search/breadth');
    const card = page.locator(`.card[data-id="${made['Graph traversals'].id}"]`);
    await expect(card).toBeVisible();
    await expect(card.locator('.card-why')).toHaveText('Through the idea “breadth-first search”');
    await page.goto('/#/search/binary%20heaps%20priority');
    await expect(page.locator(`.card[data-id="${made.Heaps.id}"]`)).toBeVisible();
  });

});

test.describe('Felix', () => {
  test('Felix stops and resumes reading the note from the head, by keyboard', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Signup flow', body: 'Save the password.\n\nThen compare it.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const drop = chat.locator('[data-chat="drop"]');
    await expect(drop).toHaveText('Signup flow');
    await expect(drop).not.toHaveClass(/off/);
    await expect(chat.locator('#chat-history')).toBeHidden();
    await expect(chat.locator('#chat-mode-btn')).toHaveCount(0);
    await drop.focus();
    await page.keyboard.press('Enter');
    await expect(drop).toHaveClass(/off/);
    await expect(drop).toBeVisible();
    await expect(drop).toHaveAttribute('aria-label', 'Read this note again');
    await expect(drop).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(drop).not.toHaveClass(/off/);
    await expect(drop).toHaveAttribute('aria-label', 'Stop reading this note');
  });

  test('a lesson card asks how sure you are and takes your thinking, and a prediction is not marked', async ({ page }) => {
    const asked = [];
    const marks = [];
    const quiz = { kind: 'multiple_choice', question: 'Which end does a queue leave from?', options: ['the front', 'the back'], answer: 'the front', explain: 'Oldest first.' };
    await page.route('**/api/quiz/mark', async (route) => {
      marks.push(route.request().postDataJSON());
      await route.fulfill({ json: { correct: true, feedback: 'Yes: the oldest item leaves first.' } });
    });
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const lines = asked.length === 1
        ? [{ quiz: { kind: 'free_response', predict: true, question: 'What must a queue remember?', answer: 'Its oldest item.' } }, { done: true }]
        : asked.length === 2
          ? [{ t: 'You said the order, and that is right: it tracks the oldest item.' }, { quiz: { kind: 'free_response', question: 'Why does order matter here?', answer: 'Fairness.' } }, { done: true }]
          : [{ quiz }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('teach me queues');
    await chat.locator('#chat-input').press('Enter');
    const predict = chat.locator('.quiz-card').first();
    await expect(predict.locator('.quiz-kind')).toHaveText('Predict · a guess, not marked');
    await expect(predict.locator('.quiz-sure')).toHaveCount(0);
    await predict.locator('.quiz-input').fill('the order');
    await predict.locator('[data-chat="quiz-check"]').click();
    await expect(chat.locator('.quiz-card')).toHaveCount(2);
    expect(marks).toEqual([]);
    expect(asked[1].mark).toBe(false);
    await expect(predict).not.toHaveClass(/right|wrong/);

    const check = chat.locator('.quiz-card').nth(1);
    await expect(check.locator('.quiz-sure-pct'), 'nothing is said about confidence until the bar is moved').toHaveText('–');
    await check.locator('.quiz-sure-range').fill('30');
    await expect(check.locator('.quiz-sure-pct')).toHaveText('30%');
    await expect(check.locator('[data-chat="quiz-think"]')).toHaveAttribute('role', 'switch');
    const thinkSwitch = await check.locator('[data-chat="quiz-think"]').elementHandle();
    await check.locator('[data-chat="quiz-think"]').click();
    await expect(check.locator('.quiz-thinking')).toBeFocused();
    expect(await thinkSwitch.evaluate((el) => el.isConnected && el.checked), 'the switch itself moves, it is not redrawn').toBe(true);
    await page.keyboard.type('fairness maybe');
    await check.locator('.quiz-input').fill('so nobody waits forever');
    await check.locator('[data-chat="quiz-check"]').click();
    await expect(check.locator('.quiz-own')).toHaveText('30% sure · fairness maybe');
    expect(marks[0]).toMatchObject({ confidence: 'guess', thinking: 'fairness maybe' });
    await check.locator('[data-chat="quiz-next"]').click();
    await expect.poll(() => asked.length).toBe(3);
    expect(asked[2].messages.at(-1).text).toContain('(how sure I felt: 30%) My thinking: fairness maybe');

    const choice = chat.locator('.quiz-card').last();
    await choice.locator('.quiz-sure-range').fill('90');
    await choice.locator('[data-chat="quiz-think"]').click();
    await page.keyboard.type('not sure I want this sent');
    await choice.locator('[data-chat="quiz-think"]').click();
    await expect(choice.locator('.quiz-thinking'), 'switching thinking off hides it').toHaveCount(0);
    await choice.locator('.quiz-option', { hasText: 'the front' }).click();
    await expect(choice.locator('.quiz-own'), 'thinking switched off is not sent').toHaveText('90% sure');
    expect(marks, 'a multiple-choice answer with no thinking is marked by the page alone').toHaveLength(1);

    await choice.locator('[data-chat="quiz-next"]').click();
    await expect(chat.locator('.quiz-card')).toHaveCount(4);
    const reasoned = chat.locator('.quiz-card').last();
    await reasoned.locator('[data-chat="quiz-think"]').click();
    await page.keyboard.type('a queue is first in, first out');
    await reasoned.locator('.quiz-option', { hasText: 'the back' }).click();
    await expect.poll(() => marks.length, 'thinking on a multiple-choice answer goes to the marker too').toBe(2);
    expect(marks[1]).toMatchObject({ given: 'the back', thinking: 'a queue is first in, first out' });
    await expect(reasoned.locator('.quiz-verdict'), 'the page keeps its own verdict for a choice').toHaveText('Not quite');
    await expect(reasoned.locator('.quiz-explain').first()).toContainText('Yes: the oldest item leaves first.');
  });

  test('a growing reply keeps its finished words, and a citation lights its passage in the open note', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Lit', body: 'First line.\n\nSecond line.\n\nThird line.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await page.evaluate(() => {
      const box = document.createElement('div');
      box.id = 'settle';
      box.innerHTML = '<div class="msg leo pending"><div class="prose"><p>Hello there</p></div></div>';
      document.body.append(box);
    });
    await page.evaluate(() => {
      const next = (html) => { const t = document.createElement('div'); t.innerHTML = html; return t; };
      const box = document.querySelector('#settle');
      window.__first = box.querySelector('p').firstChild;
      leoChat.settle(box, next('<div class="msg leo pending"><div class="prose"><p>Hello there my friend</p></div></div>'));
      window.__again = box.querySelector('p').textContent;
    });
    expect(await page.evaluate(() => ({ kept: document.querySelector('#settle p').firstChild === window.__first, text: window.__again, words: document.querySelectorAll('#settle .word').length }))).toEqual({ kept: true, text: 'Hello there my friend', words: 0 });
    await page.evaluate(() => {
      const t = document.createElement('div');
      t.innerHTML = '<div class="msg leo"><div class="prose"><p>Hello there my friend</p></div></div>';
      leoChat.settle(document.querySelector('#settle'), t);
    });
    expect(await page.evaluate(() => document.querySelector('#settle p').textContent)).toBe('Hello there my friend');
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [{ n: 1, id: note.id, title: 'Lit', folder: '', why: 'open' }] }, { t: 'It says so [n1 L3-3].' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' }));
    await chat.locator('.starter').first().click();
    const chip = chat.locator('.cite[data-line]').first();
    await expect(chip).toBeVisible();
    await chip.hover();
    await expect(page.locator('.blk-glow')).toHaveCount(1);
    await expect(page.locator('.blk-glow')).toContainText('Second line');
    await page.mouse.move(5, 5);
    await expect(page.locator('.blk-glow')).toHaveCount(0);
    await chip.focus();
    await expect(page.locator('.blk-glow')).toHaveCount(1);
    await chat.locator('#chat-input').focus();
    await expect(page.locator('.blk-glow')).toHaveCount(0);
  });

  test('quizzes from the open note, cites it, and dances on a right answer', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Heaps', body: 'A binary heap keeps the minimum at the root.' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      const sent = route.request().postDataJSON();
      asked.push(sent);
      const source = { n: 1, id: note.id, title: 'Heaps', folder: '', why: 'open' };
      const lines = asked.length === 1
        ? [{ sources: [source] }, { t: 'Where is the smallest element of a min-heap? ' }, { t: '[n1]' }, { done: true }]
        : [{ sources: [source] }, { t: '[[correct]] Yes, at the **root** [n1].' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await expect(chat).toBeVisible();
    await expect(chat.locator('[data-chat="drop"]')).toContainText('Heaps');
    await chat.locator('.starter').first().click();
    await expect(chat.locator('.msg.leo').first()).toContainText('Where is the smallest element');
    await expect(chat.locator('.msg.leo .cite-n').first()).toHaveText('1');
    await expect(chat.locator('.msg.leo .msg-from')).toContainText('Heaps');
    expect(asked[0].note).toBe(note.id);
    await chat.locator('#chat-input').fill('At the root');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.verdict.correct')).toBeVisible();
    await expect(chat.locator('.msg.leo').last()).not.toContainText('[[correct]]');
    await expect(chat.locator('#chat-face .felix')).toHaveClass(/dance/);
    expect(asked[1].messages.map((m) => m.role)).toEqual(['user', 'assistant', 'user']);
    await chat.locator('.msg.leo .cite').first().click();
    await expect(page).toHaveURL(new RegExp(`#/n/${note.id}$`));
    await expect(chat).not.toHaveClass(/chat-leaving/);
    if (!(await chat.isVisible())) await page.locator('#chat-toggle').click();
    await expect(chat.locator('.msg')).toHaveCount(4);
    await chat.locator('#chat-input').fill('now explain why');
    await chat.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(3);
    expect(asked[2].messages.map((m) => m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'user']);
    await expect.poll(async () => (await (await page.request.get('/api/chats')).json()).some((c) => c.count === 6)).toBe(true);
  });

  test('closing Felix widens the page as the panel starts to leave, and reopening mid-leave keeps it open', async ({ page }) => {
    const tag = test.info().project.name;
    const note = await (await page.request.post('/api/notes', { data: { title: `Leaving chat ${tag}`, body: 'Body.' } })).json();
    await page.setViewportSize({ width: 1400, height: 800 });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await expect(chat).toBeVisible();
    await expect(page.locator('body')).toHaveClass(/chat-open/);
    await page.waitForTimeout(400);
    const room = () => page.evaluate(() => document.querySelector('main').getBoundingClientRect().width);
    const open = await room();
    const left = await page.evaluate(async () => {
      const panel = document.getElementById('chat');
      panel.querySelector('[data-chat="close"]').click();
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
      return { leaving: panel.classList.contains('chat-leaving'), hidden: panel.hidden, open: document.body.classList.contains('chat-open'), room: document.querySelector('main').getBoundingClientRect().width };
    });
    expect(left.leaving).toBe(true);
    expect(left.hidden).toBe(false);
    expect(left.open).toBe(false);
    expect(left.room).toBeGreaterThan(open + 100);
    await expect(chat).toBeHidden();
    expect(await room()).toBe(left.room);
    await page.locator('#chat-toggle').click();
    await expect(chat).toBeVisible();
    await page.waitForTimeout(400);
    await page.evaluate(() => {
      document.getElementById('chat').querySelector('[data-chat="close"]').click();
      document.getElementById('chat-toggle').click();
    });
    await page.waitForTimeout(400);
    await expect(chat).toBeVisible();
    await expect(chat).not.toHaveClass(/chat-leaving/);
    await expect(page.locator('body')).toHaveClass(/chat-open/);
    expect(await room()).toBe(open);
  });

  test('notes are added with @ or the paperclip and go with every message', async ({ page }) => {
    const tag = test.info().project.name;
    const dijkstra = await (await page.request.post('/api/notes', { data: { title: `Dijkstra shortest paths ${tag}`, body: 'Uses a priority queue.' } })).json();
    const heaps = await (await page.request.post('/api/notes', { data: { title: `Binary heaps ${tag}`, body: 'Minimum at the root.' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Both use a heap."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await expect(chat.locator('.chat-mode')).toHaveCount(0);
    await chat.locator('#chat-input').pressSequentially(`compare @dijkstra shortest paths ${tag}`);
    await expect(chat.locator('.chat-pick-row').first()).toContainText('Dijkstra shortest paths');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('#chat-refs .note-card')).toHaveCount(1);
    await expect(chat.locator('#chat-input')).toHaveValue('compare ');
    await expect(chat.locator('.chat-pick')).toBeHidden();

    await chat.locator('[data-chat="attach"]').click();
    await chat.locator('[data-chat="attach-note"]').click();
    await chat.locator('#chat-pick-search').fill(`Binary heaps ${tag}`);
    await chat.locator('.chat-pick-row', { hasText: `Binary heaps ${tag}` }).click();
    await expect(chat.locator('#chat-refs .note-card')).toHaveCount(2);

    await chat.locator('#chat-input').fill('compare these two');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Both use a heap.');
    expect(asked[0].refs).toEqual([dijkstra.id, heaps.id]);
    await expect(chat.locator('.msg.user .note-card'), 'sent notes show as cards in the message').toHaveCount(2);
    await expect(chat.locator('#chat-refs .note-card'), 'sent notes leave the box').toHaveCount(0);

    await page.reload();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat #chat-refs .note-card')).toHaveCount(0);
    await page.locator('#chat-input').fill('and now?');
    await page.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(2);
    expect(asked[1].refs, 'follow-ups still bring the notes attached earlier').toEqual([dijkstra.id, heaps.id]);
    await page.locator('#chat [data-chat="new"]:visible').click();
    await page.locator('#chat-input').fill('fresh start');
    await page.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(3);
    expect(asked[2].refs).toEqual([]);
  });

  test('past chats are kept on the computer and listed beside the chat', async ({ page }) => {
    const tag = test.info().project.name;
    await page.route('**/api/chat', async (route) => {
      const sent = route.request().postDataJSON();
      const last = sent.messages[sent.messages.length - 1].text;
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: `{"sources":[]}\n{"t":"About ${last}"}\n{"done":true}\n` });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const sidebar = chat.locator('#chat-history');
    const openSidebar = async () => {
      if (!(await sidebar.isVisible())) await chat.locator('.chat-head [data-chat="history"]').click();
      await expect(sidebar).toBeVisible();
    };
    await chat.locator('#chat-input').fill(`first question ${tag}`);
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText(`About first question ${tag}`);
    await expect.poll(async () => (await (await page.request.get('/api/chats')).json()).some((c) => c.title === `first question ${tag}`)).toBe(true);

    await chat.locator('[data-chat="new"]:visible').click();
    await expect(chat.locator('.msg')).toHaveCount(0);
    await chat.locator('#chat-input').fill(`second question ${tag}`);
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText(`About second question ${tag}`);

    const width = async () => (await chat.boundingBox()).width;
    const before = await width();
    await openSidebar();
    expect(await width()).toBe(before);
    if (tag === 'desktop') {
      await chat.locator('.chat-head [data-chat="history"]').click();
      await expect(sidebar).toBeHidden();
      expect(await width()).toBe(before);
      await openSidebar();
    }
    await expect(sidebar.locator('.chat-history-item', { hasText: `second question ${tag}` })).toBeVisible();
    if (tag === 'desktop') {
      await chat.locator('[data-chat="new"]:visible').click();
      const placeholder = sidebar.locator('.chat-history-row.on');
      await expect(placeholder.locator('.chat-history-title')).toHaveText('New chat');
      await expect(placeholder.locator('.chat-history-x')).toHaveCount(0);
      await chat.locator('#chat-input').fill(`third question ${tag}`);
      await chat.locator('#chat-input').press('Enter');
      await expect(placeholder.locator('.chat-history-title')).toHaveText(`third question ${tag}`);
      await chat.locator('[data-chat="new"]:visible').click();
      await expect(sidebar.locator('.chat-history-row', { hasText: 'New chat' })).toHaveCount(1);
    }
    await sidebar.locator('.chat-history-item', { hasText: `first question ${tag}` }).click();
    await expect(sidebar.locator('.chat-history-row', { hasText: 'New chat' })).toHaveCount(0);
    await expect(chat.locator('.msg.leo').last()).toContainText(`About first question ${tag}`);
    await expect(chat.locator('.msg.user')).toHaveCount(1);

    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.locator('#chat-toggle').click();
    await openSidebar();
    const second = sidebar.locator('.chat-history-row', { hasText: `second question ${tag}` });
    await second.locator('.chat-history-item').click();
    await expect(chat.locator('.msg.leo').last()).toContainText(`About second question ${tag}`);
    await openSidebar();
    await second.locator('.chat-history-x').click();
    await expect(second.locator('.chat-history-x')).toHaveClass(/sure/);
    const list = await sidebar.locator('#chat-history-list').boundingBox();
    await page.mouse.click(list.x + list.width / 2, list.y + list.height - 6);
    await expect(second.locator('.chat-history-x')).not.toHaveClass(/sure/);
    await second.locator('.chat-history-x').click();
    await expect(second.locator('.chat-history-x')).toHaveClass(/sure/);
    await second.locator('.chat-history-x').click();
    await expect(second).toHaveCount(0);
    await expect(chat.locator('.msg')).toHaveCount(0);
    expect((await (await page.request.get('/api/chats')).json()).some((c) => c.title === `second question ${tag}`)).toBe(false);
  });

  test('a suggestion asks which note or classes when nothing is open', async ({ page }) => {
    const tag = test.info().project.name;
    const note = await (await page.request.post('/api/notes', { data: { title: `Sorting lecture ${tag}`, body: 'Merge sort.' } })).json();
    await page.request.post('/api/dirs', { data: { path: `algos-${tag}` } });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Here we go."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('.starter', { hasText: 'Quiz me on this note' }).click();
    await expect(chat.locator('.chat-ask')).toContainText('Which note should the quiz cover?');
    expect(asked.length).toBe(0);
    await chat.locator('#chat-pick-search').fill(`Sorting lecture ${tag}`);
    await chat.locator('.chat-pick-row', { hasText: `Sorting lecture ${tag}` }).click();
    await expect(chat.locator('.msg.leo').last()).toContainText('Here we go.');
    expect(asked[0].messages[0].text).toBe('Quiz me on this note');
    expect(asked[0].refs).toEqual([note.id]);

    await chat.locator('[data-chat="new"]:visible').click();
    await chat.locator('.starter', { hasText: 'Make me a 3-day review plan' }).click();
    await expect(chat.locator('.chat-ask')).toContainText('Which classes should the plan cover?');
    await expect(chat.locator('[data-chat="classes-go"]')).toBeDisabled();
    await chat.locator('.chat-class', { hasText: `algos-${tag}` }).click();
    await chat.locator('[data-chat="classes-go"]').click();
    await expect.poll(() => asked.length).toBe(2);
    expect(asked[1].messages[0].text).toBe(`Make me a 3-day review plan for algos-${tag}`);

    await chat.locator('[data-chat="new"]:visible').click();
    await chat.locator('.starter', { hasText: 'Teach me this note step by step' }).click();
    await expect(chat.locator('.chat-ask')).toContainText('Which note should I teach you?');
    await chat.locator('[data-chat="ask-cancel"]').click();
    await expect(chat.locator('.chat-ask')).toHaveCount(0);
    expect(asked.length).toBe(2);
  });

  test('tapping Felix can tickle him, and about one tap in ten he turns into a monster and back', async ({ page }) => {
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const face = page.locator('#chat-face .felix');
    await expect.poll(() => face.evaluate((el) => el.getBoundingClientRect().width / el.viewBox.baseVal.width), { message: 'his composer cameo is drawn at whole pixels' }).toBe(1);
    await page.evaluate(() => { Math.random = () => 0.99; });
    await face.click();
    await expect(face).toHaveClass(/\btickle\b/);
    await page.evaluate(() => { Math.random = () => 0.01; });
    await face.click();
    await expect(face).toHaveClass(/\bmonster\b/);
    await expect(face.locator('.felix-rawr')).toHaveCount(1);
    await face.click();
    await expect(face).toHaveClass(/\bmonster\b/, { timeout: 500 });
    await expect(face).not.toHaveClass(/\bmonster\b/, { timeout: 5000 });
    await expect(face.locator('.felix-monster')).toHaveCSS('opacity', '0');
  });

  test('Demon Felix from Settings has scary taps, and tickling either Felix too long swaps them for a moment', async ({ page }) => {
    await page.goto('/#/settings');
    await page.locator('[data-action="felix-pick"][data-felix="demon"]').click();
    await expect(page.locator('html')).toHaveAttribute('data-felix', 'demon');
    await page.reload();
    await expect(page.locator('[data-action="felix-pick"][data-felix="demon"]')).toHaveAttribute('aria-pressed', 'true');
    await page.locator('#chat-toggle').click();
    const face = page.locator('#chat-face .felix');
    await expect(face.locator('.felix-skin')).toHaveCSS('fill', 'rgb(123, 92, 207)');
    await expect(face.locator('.felix-monster')).toHaveCSS('opacity', '1');
    await page.evaluate(() => { Math.random = () => 0.5; });
    await face.click();
    await expect(face).toHaveClass(/\b(rawr|cackle|blaze|glare|swipe)\b/);
    await expect(face).not.toHaveClass(/\b(boop|hop|spin|giggle|tickle)\b/);
    await page.waitForTimeout(1600);
    for (let i = 0; i < 8; i++) await face.click({ delay: 10 });
    await expect(face).toHaveClass(/\bsweet\b/);
    await expect(face).not.toHaveClass(/\bsweet\b/, { timeout: 5000 });
    await page.locator('[data-chat="close"]').click();
    await expect(page.locator('aside.chat')).toBeHidden();
    await page.goto('/#/settings');
    await page.locator('[data-action="felix-pick"][data-felix="cute"]').click();
    await expect(page.locator('html')).not.toHaveAttribute('data-felix', /./);
    expect(await page.evaluate(() => localStorage.getItem('leo-felix'))).toBeNull();
    await page.locator('#chat-toggle').click();
    const cute = page.locator('#chat-face .felix');
    await expect(cute.locator('.felix-skin')).toHaveCSS('fill', 'rgb(180, 207, 231)');
    await page.waitForTimeout(1600);
    for (let i = 0; i < 8; i++) await cute.click({ delay: 10 });
    await expect(cute).toHaveClass(/\bmonster\b/);
  });

  test('Felix thinks, talks, reacts to answers and to being tapped', async ({ page }) => {
    let release;
    const hold = new Promise((resolve) => { release = resolve; });
    let replies = 0;
    await page.route('**/api/chat', async (route) => {
      replies += 1;
      if (replies === 1) await hold;
      const text = replies === 1 ? 'Hello there' : '[[incorrect]] Not quite.';
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: `{"sources":[]}\n{"t":${JSON.stringify(text)}}\n{"done":true}\n` });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const face = page.locator('#chat-face .felix');
    await page.locator('#chat-input').fill('hi');
    await page.locator('#chat-input').press('Enter');
    await expect(face).toHaveClass(/think/);
    release();
    await expect(page.locator('.msg.leo').last()).toContainText('Hello there');
    await expect(face).not.toHaveClass(/think|talk/);
    await page.locator('#chat-input').fill('the root?');
    await page.locator('#chat-input').press('Enter');
    await expect(face).toHaveClass(/droop/);
    await expect(face).toHaveClass(/perk/, { timeout: 4000 });
    await page.evaluate(() => { Math.random = () => 0.5; });
    await face.click();
    await expect(face).toHaveClass(/boop|hop|spin|giggle|tickle/);
    await face.dblclick();
    await expect(face, 'a double click hugs him').toHaveClass(/\bhug\b/);
    await face.dblclick();
    await face.dblclick();
    await expect(face, 'too many hugs and he squints').toHaveClass(/\bsquint\b/);
    await expect(face).not.toHaveClass(/\bsquint\b/, { timeout: 4000 });
    const sizes = await face.evaluate((svg) => {
      const skin = svg.querySelector('.felix-skin');
      return [Number(skin.getAttribute('width')), Number(skin.getAttribute('height'))];
    });
    expect(sizes[0] / sizes[1]).toBeGreaterThan(1);
    expect(sizes[0] / sizes[1]).toBeLessThan(1.4);
  });

  test('a good answer is saved as a note in the folder of the open note', async ({ page }) => {
    const dir = `saved-${test.info().project.name}`;
    await page.request.post('/api/dirs', { data: { path: dir } });
    const open = await (await page.request.post('/api/notes', { data: { title: 'Graph traversals', body: 'BFS uses a queue.', directory: dir } })).json();
    await page.route('**/api/chat', (route) => route.fulfill({
      status: 200,
      headers: { 'content-type': 'application/x-ndjson' },
      body: `{"sources":[{"n":1,"id":"${open.id}","title":"Graph traversals","why":"open"}]}\n{"t":"BFS takes the oldest vertex from a queue [n1]."}\n{"done":true}\n`,
    }));
    await page.goto(`/#/n/${open.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('How does BFS pick the next vertex?');
    await chat.locator('#chat-input').press('Enter');
    await chat.locator('.msg.leo [data-chat="save"]').click();
    await expect(page.locator('.toast')).toContainText(`Saved as a note in ${dir}`);
    await expect(chat.locator('.msg.leo [data-chat="open"]', { hasText: 'Open the saved note' })).toBeVisible();
    const found = await (await page.request.get(`/api/notes?directory=${encodeURIComponent(dir)}`)).json();
    const saved = found.find((n) => n.title === 'How does BFS pick the next vertex?');
    expect(saved, JSON.stringify(found.map((n) => n.title))).toBeTruthy();
    expect(saved.body).toBe('**Q:** How does BFS pick the next vertex?\n\nBFS takes the oldest vertex from a queue [[Graph traversals]].');
  });

  test('a question missed a day ago is offered for review and asked again', async ({ page }) => {
    const id = `review-chat-${test.info().project.name}`;
    const note = await (await page.request.post('/api/notes', { data: { title: 'First in, first out', body: 'The oldest item leaves first.' } })).json();
    const question = `Which structure does BFS use (${test.info().project.name})?`;
    await page.request.put(`/api/chats/${id}`, {
      data: {
        refs: [],
        messages: [
          { role: 'user', text: 'quiz me' },
          { role: 'assistant', text: `First question.\n\n${question}` },
          { role: 'user', text: 'a stack' },
          { role: 'assistant', text: '[[incorrect]] Not quite, it is a queue [n1].', sources: [{ n: 1, id: note.id, title: 'First in, first out' }], at: '2026-01-02T10:00:00Z' },
        ],
      },
    });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"First one: which structure?"}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const card = chat.locator('.chat-review');
    await expect(card).toContainText('Time to review');
    await card.locator('[data-chat="review"]').click();
    await expect(chat.locator('.msg.leo').last()).toContainText('First one: which structure?');
    const sent = asked[asked.length - 1];
    expect(sent.refs).toContain(note.id);
    expect(sent.messages[sent.messages.length - 1].text).toContain(`${question} (last time I said: a stack)`);
    await expect.poll(async () => (await (await page.request.get('/api/review')).json()).some((m) => m.question === question)).toBe(false);
  });

  test('Felix shows what it looked at and its suggestions wait for the user', async ({ page }) => {
    const tag = test.info().project.name;
    const note = await (await page.request.post('/api/notes', { data: { title: `BFS facts ${tag}`, body: 'BFS takes the newest vertex.' } })).json();
    const lines = [
      { sources: [{ n: 1, id: note.id, title: note.title, folder: '', why: 'found by searching' }] },
      { step: 'Searched your notes for “BFS”', tool: 'search_notes', found: [note.title, 'Heaps'] },
      { step: `Suggested a change to “${note.title}”`, tool: 'edit_note', found: [] },
      { proposal: { kind: 'edit', note: note.id, title: note.title, find: 'newest', replace: 'oldest', why: 'a queue is first in, first out' } },
      { proposal: { kind: 'create', title: `Waiting lines ${tag}`, body: '## Waiting lines\n- first in, first out', folder: '' } },
      { t: 'Your note had BFS backwards [n1]; I suggested a fix and a new note.' },
      { done: true },
    ];
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' }));
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('is my BFS note right?');
    await chat.locator('#chat-input').press('Enter');
    const answer = chat.locator('.msg.leo').last();
    await expect(answer.locator('.msg-trace > summary')).toHaveText(/^Searched your notes( · \d+ s)?$/);
    await expect(answer.locator('.msg-trace > summary > .pencil-tick')).toHaveCount(1);
    await answer.locator('.msg-trace > summary').click();
    const steps = answer.locator('.msg-step');
    await expect(steps).toHaveCount(2);
    await expect(steps.first()).toContainText('Searched your notes for “BFS”');
    await expect(steps.nth(1)).toContainText(`Suggested a change to “${note.title}”`);
    await expect(answer.locator('.msg-step-busy')).toHaveCount(0);
    await expect(steps.first().locator('.msg-step-count')).toHaveText('2');
    await expect(steps.first().locator('li').first()).toBeHidden();
    await steps.first().locator('summary').click();
    await expect(steps.first().locator('li')).toHaveText([note.title, 'Heaps']);
    const cards = answer.locator('.proposal');
    await expect(cards).toHaveCount(2);
    await expect(cards.locator('.proposal-wait')).toHaveText(['Waiting for you', 'Waiting for you']);
    const waiting = await cards.first().evaluate((el) => { const s = getComputedStyle(el), probe = document.createElement('i'); probe.style.color = 'var(--felix)'; el.append(probe); const felix = getComputedStyle(probe).color; probe.remove(); return { label: getComputedStyle(el.querySelector('.proposal-wait')).color, felix, left: s.borderLeftColor, top: s.borderTopColor, width: s.borderLeftWidth }; });
    expect(waiting.label).toBe(waiting.felix);
    expect([waiting.left, waiting.width], 'no coloured edge, only the label').toEqual([waiting.top, '1px']);
    await expect(cards.first().locator('.proposal-marked del')).toHaveText('newest');
    await expect(cards.first().locator('.proposal-marked ins')).toHaveText('oldest');
    await expect(cards.first().locator('.proposal-marked ins .proposal-caret')).toHaveCount(1);
    await expect(cards.first().locator('.proposal-old, .proposal-new'), 'one passage with marks, not two coloured boxes').toHaveCount(0);
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('BFS takes the newest vertex.');
    await cards.first().locator('[data-chat="apply"]').click();
    await expect(cards.first().locator('.proposal-done')).toContainText('Applied');
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('BFS takes the oldest vertex.');
    await expect(cards.first().locator('.proposal-wait'), 'the blue leaves once it is applied').toHaveCount(0);
    await cards.nth(1).locator('[data-chat="apply"]').click();
    await expect(cards.nth(1).locator('.proposal-done')).toContainText('Made');
    const made = await (await page.request.get(`/api/search?q=${encodeURIComponent(`Waiting lines ${tag}`)}`)).json();
    expect(made.some((n) => n.title === `Waiting lines ${tag}`)).toBe(true);
    await page.reload();
    await page.locator('#chat-toggle').click();
    const again = page.locator('#chat .msg.leo').last().locator('.proposal');
    await expect(again.first().locator('.proposal-done')).toContainText('Applied');

    await again.first().locator('[data-chat="undo"]').click();
    await expect(page.locator('.toast')).toContainText(`Undid the change to “${note.title}”`);
    await expect(again.first().locator('[data-chat="apply"]')).toBeVisible();
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('BFS takes the newest vertex.');

    await again.nth(1).locator('[data-chat="undo"]').click();
    await expect(page.locator('.toast')).toContainText(`Moved “Waiting lines ${tag}” to the trash`);
    await expect(again.nth(1).locator('[data-chat="apply"]')).toBeVisible();
    const trashed = await (await page.request.get('/api/trash')).json();
    expect(trashed.some((t) => t.title === `Waiting lines ${tag}`)).toBe(true);

    await again.first().locator('[data-chat="apply"]').click();
    await expect(again.first().locator('.proposal-done')).toContainText('Applied');
    const current = await (await page.request.get(`/api/notes/${note.id}`)).json();
    await page.request.patch(`/api/notes/${note.id}`, { data: { body: `${current.body}\nEdited by hand.`, base: current.version } });
    await again.first().locator('[data-chat="undo"]').click();
    await expect(page.locator('.toast.bad')).toContainText('edited after this change');
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('BFS takes the oldest vertex.\nEdited by hand.');
  });

  test('Felix holds the prop for the tool he is using, and only that one', async ({ page }) => {
    await page.goto('/');
    const shown = await page.evaluate(() => {
      const out = {};
      for (const pose of ['tool-search', 'tool-open', 'tool-map', 'tool-edit', 'tool-create', 'idle']) {
        const holder = document.createElement('div');
        holder.innerHTML = window.leoChat.felix(80, pose);
        document.body.appendChild(holder);
        out[pose] = [...holder.querySelectorAll('.felix-tool')].filter((g) => getComputedStyle(g).opacity === '1').map((g) => g.getAttribute('class').replace('felix-tool ', ''));
        holder.remove();
      }
      return out;
    });
    expect(shown).toEqual({
      'tool-search': ['felix-tool-search'],
      'tool-open': ['felix-tool-open'],
      'tool-map': ['felix-tool-map'],
      'tool-edit': ['felix-tool-edit'],
      'tool-create': ['felix-tool-create'],
      idle: [],
    });
  });

  test('a file from this device is read for Felix, leaves the box once sent, and can be removed before', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Your file says heaps."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const attach = async (file) => {
      await chat.locator('[data-chat="attach"]').click();
      await expect(chat.locator('#chat-attach-menu')).toBeVisible();
      const chooser = page.waitForEvent('filechooser');
      await chat.locator('[data-chat="attach-file"]').click();
      await (await chooser).setFiles(file);
      await expect(chat.locator('#chat-attach-menu')).toBeHidden();
    };
    await attach({ name: 'week3.txt', mimeType: 'text/plain', buffer: Buffer.from('Heaps keep the minimum at the root.') });
    const chip = chat.locator('#chat-refs .file-card', { hasText: 'week3.txt' });
    await expect(chip).toBeVisible();
    await expect(chip.locator('.file-badge')).toHaveText('TXT');
    await expect(chip.locator('.file-page')).toContainText('Heaps keep the minimum at the root.');
    const card = await chip.boundingBox();
    expect(card.width).toBeGreaterThan(card.height);
    expect(card.height, 'a short tile in the composer, so the chat is not pushed down').toBeLessThanOrEqual(60);
    await expect(chip).not.toHaveClass(/reading/);
    await chat.locator('#chat-input').fill('what does my file say?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Your file says heaps.');
    expect(asked[0].files.length).toBe(1);
    const files = await (await page.request.get(`/api/chats/${asked[0].chat}/files`)).json();
    expect(files.map((f) => f.name)).toEqual(['week3.txt']);
    expect(files[0].chars).toBe(35);
    await expect(chat.locator('.msg.user .file-card .file-name')).toHaveText('week3.txt');
    await expect(chat.locator('#chat-refs .file-card')).toHaveCount(0);

    await chat.locator('#chat-input').fill('and where is the minimum?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo')).toHaveCount(2);
    await expect(chat.locator('.msg.user').last().locator('.file-card')).toHaveCount(0);
    expect(asked[1].files, 'Felix still has the file for questions after it').toEqual(asked[0].files);

    await attach({ name: 'draft.txt', mimeType: 'text/plain', buffer: Buffer.from('A draft not meant to go.') });
    const draft = chat.locator('#chat-refs .file-card', { hasText: 'draft.txt' });
    await expect(draft).not.toHaveClass(/reading/);
    await draft.locator('.file-x').click();
    await expect(draft).toHaveCount(0);
    await expect.poll(async () => (await (await page.request.get(`/api/chats/${asked[0].chat}/files`)).json()).map((f) => f.name)).toEqual(['week3.txt']);

    await page.reload();
    const listed = page.waitForResponse((r) => r.url().endsWith(`/api/chats/${asked[0].chat}/files`));
    await page.locator('#chat-toggle').click();
    await listed;
    await expect(chat.locator('.msg.user .file-card .file-name')).toHaveText('week3.txt');
    await expect(chat.locator('#chat-refs .file-card')).toHaveCount(0);

    await attach({ name: 'song.mp3', mimeType: 'audio/mpeg', buffer: Buffer.from('ID3') });
    await expect(page.locator('.toast.bad')).toContainText('Felix could not read song.mp3');
    await expect(chat.locator('#chat-refs .file-card')).toHaveCount(0);
  });

  test('the chat panel follows the size of the window', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'one browser is enough to resize');
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    for (const [width, expected] of [[390, 390], [768, 476], [1024, 560], [1440, 547], [2200, 760]]) {
      await page.setViewportSize({ width, height: 800 });
      await expect.poll(async () => Math.round((await chat.boundingBox()).width)).toBe(expected);
      const fits = await chat.evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
      expect(fits, `nothing spills sideways at ${width}px`).toBe(true);
      await expect(page.locator('#chat-input')).toBeInViewport();
      await expect(page.locator('#chat-send')).toBeInViewport();
      if (width >= 1200) {
        const panel = await chat.boundingBox();
        await expect.poll(async () => {
          const list = await page.locator('main').boundingBox();
          return list.x + list.width;
        }, { message: `the page moves over at ${width}px` }).toBeLessThanOrEqual(panel.x + 1);
      }
    }
    await page.locator('#chat [data-chat="close"]').click();
    await expect.poll(async () => (await page.locator('main').boundingBox()).width).toBeGreaterThan(700);
  });

  test('with Felix beside the page, sheets and the note toolbar stay beside him', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the page only moves over on wide screens');
    await page.setViewportSize({ width: 1440, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Beside Felix', body: 'x' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== 'running' || a.effect.getTiming().iterations === Infinity));
    const chat = await page.locator('#chat').boundingBox();
    const bar = await page.locator('#note-more').boundingBox();
    expect(bar.x + bar.width).toBeLessThanOrEqual(chat.x);
    await page.locator('#note-more').click();
    await page.getByRole('menuitem', { name: 'Tasks' }).click();
    const sheet = await page.locator('.sheet').boundingBox();
    const side = await page.locator('#side').boundingBox();
    expect(sheet.x + sheet.width).toBeLessThanOrEqual(chat.x);
    expect(Math.abs(sheet.x + sheet.width / 2 - (side.x + side.width + chat.x) / 2)).toBeLessThan(30);
  });

  test('Felix can be dragged wider or narrower and keeps that width', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the panel takes the whole screen on phones');
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const settled = () => page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== 'running' || a.effect.getTiming().iterations === Infinity));
    await settled();
    const before = await chat.boundingBox();
    const grip = page.locator('.chat-resize');
    const handle = await grip.boundingBox();
    await page.mouse.move(handle.x + handle.width / 2, 300);
    await page.mouse.down();
    await page.mouse.move(handle.x + handle.width / 2 - 120, 300, { steps: 6 });
    await page.mouse.up();
    const wider = await chat.boundingBox();
    expect(Math.abs(wider.width - (before.width + 120))).toBeLessThan(4);
    const main = await page.locator('main').boundingBox();
    expect(main.x + main.width).toBeLessThanOrEqual(wider.x + 1);

    await page.reload();
    await page.locator('#chat-toggle').click();
    await settled();
    expect(Math.abs((await chat.boundingBox()).width - wider.width)).toBeLessThan(2);

    await grip.focus();
    await page.keyboard.press('ArrowRight');
    expect(Math.abs((await chat.boundingBox()).width - (wider.width - 32))).toBeLessThan(2);

    const moved = await grip.boundingBox();
    await page.mouse.move(moved.x + moved.width / 2, 300);
    await page.mouse.down();
    await page.mouse.move(1400, 300, { steps: 4 });
    await page.mouse.up();
    await expect.poll(async () => (await chat.boundingBox()).width).toBe(420);

    await grip.dblclick();
    expect(Math.abs((await chat.boundingBox()).width - before.width)).toBeLessThan(2);
  });

  test('letting go of Felix’s edge outside the page ends the drag', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the panel takes the whole screen on phones');
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== 'running' || a.effect.getTiming().iterations === Infinity));
    const handle = await page.locator('.chat-resize').boundingBox();
    const x = handle.x + handle.width / 2;
    await page.mouse.move(x, 300);
    await page.mouse.down();
    await page.mouse.move(x - 60, 300, { steps: 3 });
    const held = (await chat.boundingBox()).width;
    const cdp = await page.context().newCDPSession(page);
    for (const dx of [80, 120, 160]) await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x - dx, y: 300, button: 'none', buttons: 0 });
    await expect(page.locator('body')).not.toHaveClass(/pane-resizing/);
    expect(Math.abs((await chat.boundingBox()).width - held)).toBeLessThan(2);
    expect(Math.abs(Number(await page.evaluate(() => localStorage.getItem('leo-chat-width'))) - held)).toBeLessThan(2);
  });

  test('Felix can ask first, change notes himself, or only read, and remembers the choice', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Auto target ${test.info().project.name}`, body: 'BFS uses a queue.' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const applied = { kind: 'edit', note: note.id, title: note.title, find: 'queue', replace: 'queue (first in, first out)', why: '', state: 'applied', before: 'BFS uses a queue.', after: 'v' };
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { step: `Changed “${note.title}”`, tool: 'edit_note', found: [] }, { proposal: applied }, { t: 'I changed it.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const access = chat.locator('#chat-access');
    await expect(access).toHaveText(/Ask first/);
    await access.click();
    await chat.locator('[data-chat="access-pick"][data-access="auto"]').click();
    await expect(access).toHaveText(/Auto/);
    await expect(chat.locator('#chat-access-menu')).toBeHidden();
    await chat.locator('#chat-input').fill('fix my bfs note');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.proposal-done')).toContainText('Applied');
    await expect(chat.locator('.proposal [data-chat="undo"]')).toBeVisible();
    await expect(chat.locator('.proposal [data-chat="apply"]')).toHaveCount(0);
    expect(asked[0].access).toBe('auto');

    await chat.locator('#chat-input').press('Shift+Tab');
    await expect(access).toHaveText(/Read only/);
    await page.reload();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat-access')).toHaveText(/Read only/);
    await page.locator('#chat-input').press('Shift+Tab');
    await expect(page.locator('#chat-access')).toHaveText(/Ask first/);
  });

  test('a message sent while a file is still being read waits for it, with Felix thinking', async ({ page }) => {
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    await page.route(/\/api\/chats\/[^/]+\/files(\/|$)/, async (route) => {
      if (route.request().method() === 'DELETE') return route.fallback();
      if (route.request().method() !== 'PUT') return route.fulfill({ json: [] });
      await held;
      return route.fulfill({ status: 201, json: { id: 'doc-slow-1', name: 'slow.pdf', chars: 30, excerpt: 'Week 4: Graphs' } });
    });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"It covers graphs."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('[data-chat="attach"]').click();
    const chooser = page.waitForEvent('filechooser');
    await chat.locator('[data-chat="attach-file"]').click();
    await (await chooser).setFiles({ name: 'slow.pdf', mimeType: 'application/pdf', buffer: Buffer.from('%PDF-1.4') });
    await expect(chat.locator('#chat-refs .file-card.reading')).toBeVisible();
    await chat.locator('#chat-input').fill('what is in it?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('#chat-refs .file-card')).toHaveCount(0);
    await expect(chat.locator('.msg.user .file-card.reading')).toContainText('slow.pdf');
    await expect(chat.locator('.msg.leo.pending')).toBeVisible();
    await expect(page.locator('#chat-toggle'), 'his button carries the blue dot while he works').toHaveClass(/\bworking\b/);
    expect(asked).toHaveLength(0);
    release();
    await expect(chat.locator('.msg.leo').last()).toContainText('It covers graphs.');
    await expect(page.locator('#chat-toggle')).not.toHaveClass(/\bworking\b/);
    expect(asked[0].files).toEqual(['doc-slow-1']);
    await expect(chat.locator('.msg.user .file-card')).not.toHaveClass(/reading/);
    await expect(chat.locator('.msg.user .file-page')).toContainText('Week 4: Graphs');
  });

  test('moving to another note while Felix works keeps his answer and changes on the first note', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    const make = async (title, body, directory) => {
      await page.request.post('/api/dirs', { data: { path: directory } });
      return (await page.request.post('/api/notes', { data: { title, body, directory } })).json();
    };
    const first = await make(`Asked about ${tag}`, 'BFS uses a stack.', `switch-a-${tag}`);
    const other = await make(`Opened later ${tag}`, 'Untouched text.', `switch-b-${tag}`);
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await held;
      const proposal = { kind: 'edit', note: first.id, title: first.title, find: 'stack', replace: 'queue', why: 'BFS is first in, first out' };
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { proposal }, { t: 'Your note says stack; BFS uses a queue.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${first.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('is this right?');
    await chat.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(1);
    await page.evaluate((id) => { location.hash = `#/n/${id}`; }, other.id);
    await expect(page.locator('#title')).toHaveText(other.title);
    release();
    await expect(chat.locator('.msg.leo').last()).toContainText('BFS uses a queue.');
    expect(asked[0].note).toBe(first.id);
    await chat.locator('.proposal [data-chat="apply"]').click();
    await expect(chat.locator('.proposal-done')).toContainText('Applied');
    const body = async (id) => (await (await page.request.get(`/api/notes/${id}`)).json()).body;
    expect(await body(first.id)).toBe('BFS uses a queue.');
    expect(await body(other.id)).toBe('Untouched text.');
    await expect(page.locator('#title')).toHaveText(other.title);
    await chat.locator('[data-chat="save"]').click();
    await expect(page.locator('.toast')).toContainText(`Saved as a note in switch-a-${tag}`);
  });

  test('the note picker keeps its search box while typing, until a note is chosen', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.request.post('/api/notes', { data: { title: `Dijkstra picker ${tag}`, body: 'x' } });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('[data-chat="attach"]').click();
    await chat.locator('[data-chat="attach-note"]').click();
    const search = chat.locator('#chat-pick-search');
    await expect(search).toBeFocused();
    await search.pressSequentially(`dijkstra picker ${tag}`, { delay: 30 });
    await expect(search).toHaveValue(`dijkstra picker ${tag}`);
    await expect(search).toBeFocused();
    await expect(chat.locator('.chat-pick-row').first()).toContainText(`Dijkstra picker ${tag}`);
    await chat.locator('.chat-pick-row').first().click();
    await expect(chat.locator('#chat-refs .note-card')).toContainText(`Dijkstra picker ${tag}`);
  });

  test('says plainly when no AI is set up', async ({ page }) => {
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"error":"no AI for writing is chosen — type :settings in leo and pick one under writing"}\n' }));
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('hello');
    await page.locator('#chat-input').press('Enter');
    await expect(page.locator('.msg-error')).toContainText(':settings');
    await page.locator('#chat [data-chat="close"]').click();
    await expect(page.locator('#chat')).toBeHidden();
  });
});

test.describe('settings', () => {
  test('writing uses account-based choices and speech keys never appear again', async ({ page }) => {
    await page.goto('/#/settings/writing');
    const writing = page.locator('[data-task-card="writing"]');
    await expect(writing).toBeVisible();
    const provider = writing.getByRole('radiogroup', { name: 'Provider' });
    expect(await provider.getByRole('radio').evaluateAll(options => options.map(option => option.value))).toEqual(['ollama', 'codex', 'claude_code']);
    await provider.getByRole('radio', { name: 'Claude Code' }).click();
    await expect(page.locator('.toast')).toContainText('Writing now uses Claude Code');
    await expect(writing.locator('[data-key-input]')).toHaveCount(0);
    await writing.locator('.picker-button').click();
    await writing.locator('.pick-model[data-model="claude-opus-5-5"]').click();
    await expect(page.locator('.toast')).toContainText('Model set to claude-opus-5-5');
    const modelRows = writing.locator('.pick-model');
    const ids = await modelRows.evaluateAll(rows => rows.map(row => row.dataset.model));
    await writing.locator('.picker-button').focus();
    await page.keyboard.press('ArrowDown');
    await expect(writing.locator('.pick-model.on')).toBeFocused();
    await page.keyboard.press('End');
    await expect(modelRows.last()).toBeFocused();
    await page.keyboard.press('Home');
    await expect(modelRows.first()).toBeFocused();
    let common = 0;
    while (common < ids[0].length && ids[0][common] === ids[1][common]) common++;
    await page.keyboard.type(ids[1].slice(0, common + 1));
    await expect(modelRows.nth(1)).toBeFocused();
    await writing.locator('.pick-chip').first().focus();
    await page.keyboard.press('c');
    await expect(writing.locator('.pick-chip').first()).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(writing.locator('.picker-menu')).toBeHidden();
    const settings = await (await page.request.get('/api/settings')).text();
    expect(settings).not.toMatch(/gemini_speech|openai_audio|xai_stt/);
    await page.locator('.settings-nav a[href="#/settings/models"]').click();
    await expect(page.getByRole('heading', { name: 'In use', exact: true })).toBeVisible();
    await expect(page.locator('[data-task-card="speech"], [data-key-input]')).toHaveCount(0);
    const bad = await page.request.post('/api/settings', { data: { set: 'provider', task: 'writing', value: 'parakeet' } });
    expect(bad.status()).toBe(400);
    await page.locator('.settings-nav a[href="#/settings/writing"]').click();
    await provider.getByRole('radio', { name: 'Codex' }).click();
    await expect(page.locator('.toast')).toContainText('Writing now uses Codex');
    await writing.locator('.picker-button').click();
    await expect(writing.locator('.pick-chip')).toHaveText(['Low', 'Medium', 'High', 'Extra high']);
    await writing.locator('.pick-chip[data-effort="high"]').click();
    await expect(writing.locator('.picker-button')).toContainText('high effort');
    await writing.locator('.picker-button').click();
    await page.keyboard.press('Escape');
    await expect(writing.locator('.picker-menu')).toBeHidden();
    await writing.locator('.picker-button').click();
    await writing.locator('.pick-chip[data-effort="medium"]').click();
    await provider.getByRole('radio', { name: 'Ollama' }).click();
    await expect(writing.locator('input[data-set="provider"]')).toHaveValue('ollama');
  });
});

test.describe('folders', () => {
  test('folders hold folders: the sidebar shows them as a tree and makes one inside another', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the sidebar is for wide screens');
    const top = `tree-${Date.now().toString(36)}`;
    await page.request.post('/api/dirs', { data: { path: `${top}/week1` } });
    await page.goto('/');
    const side = page.locator('#side');
    const row = (path) => side.locator(`.side-row[data-action="open-folder"][data-dir="${path}"]`);
    await expect(row(top)).toBeVisible();
    await expect(row(`${top}/week1`)).toHaveCount(0);
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`)).toBeVisible();
    await row(`${top}/week1`).hover();
    await row(`${top}/week1`).click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'New folder here' }).click();
    await expect(page.locator('#folder-name')).toHaveAttribute('placeholder', 'New folder in week1');
    await page.locator('#folder-name').fill('lab');
    await page.locator('#folder-name').press('Enter');
    await expect(page).toHaveURL(/\/(?:#\/)?$/);
    await expect(row(`${top}/week1/lab`)).toHaveAttribute('aria-current', 'page');
    await page.reload();
    await expect(row(`${top}/week1`), 'open folders stay open').toBeVisible();
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await page.goto('/');
    await expect(row(`${top}/week1`)).toHaveCount(0);
    await page.goto(`/#/f/${top}/week1/lab`);
    await expect(row(`${top}/week1/lab`), 'the way to the open folder unfolds').toBeVisible();
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`), 'an arrow closes even on the way to the open folder').toHaveCount(0);
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`), 'and opens again').toBeVisible();
    await page.goto(`/#/f/${top}`);
    await expect(row(`${top}/week1`)).toBeVisible();
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`), 'the folder you are in can fold its subfolders').toHaveCount(0);
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`)).toBeVisible();
    await page.request.post('/api/dirs', { data: { path: `${top}-other` } });
    await page.goto(`/#/f/${top}-other`);
    await expect(row(`${top}-other`)).toHaveAttribute('aria-current', 'page');
    await expect(row(`${top}/week1`), 'opened folders stay open when another folder is opened').toBeVisible();
  });

  test('a new folder is made on the first try and opens', async ({ page }) => {
    const name = `fresh-${test.info().project.name}-${Date.now().toString(36)}`;
    await page.goto('/');
    if (await page.locator('#side').isHidden()) {
      await page.locator('#menu').click();
      await page.locator(':is(.sheet, .popup-menu) [data-action="new-folder"]').click();
    } else {
      await page.locator('#side .side-group [data-action="new-folder"]').click();
    }
    await page.locator('#folder-name').fill(name);
    await page.locator('#folder-name').press('Enter');
    await expect(page).toHaveURL(/\/(?:#\/)?$/);
    await expect(page.locator('#side .side-row[data-action="open-folder"][data-dir="' + name + '"]')).toBeVisible();
    await expect(page.locator('.toast.bad')).toHaveCount(0);
    await expect(page.locator('.scrim')).toHaveCount(0);
  });





});

test.describe('pictures', () => {
  async function pastePicture(page, target, name = 'image.png') {
    await page.evaluate(async ({ target, name }) => {
      const canvas = document.createElement('canvas');
      canvas.width = 320;
      canvas.height = 200;
      const ctx = canvas.getContext('2d');
      ctx.fillStyle = '#4f46e5';
      ctx.fillRect(0, 0, 320, 200);
      ctx.fillStyle = '#fff';
      ctx.fillRect(40, 40, 120, 80);
      const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
      const data = new DataTransfer();
      data.items.add(new File([blob], name, { type: 'image/png' }));
      const el = document.querySelector(target);
      el.focus();
      el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    }, { target, name });
  }

  test('a pasted picture goes into the note and shows in full', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Pictured ${test.info().project.name}`, body: 'Before the picture.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('Before the picture.');
    await pastePicture(page, '#doc');
    const img = page.locator('#doc img.note-img');
    await expect(img).toBeVisible();
    await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(320);
    await expect(page.locator('.toast')).toContainText('Picture added');
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toMatch(/^Before the picture\.\n\n!\[Pasted picture\]\(attachments\/\d{8}-\d{6}-pasted-1\.png\)$/);
    await page.reload();
    await expect.poll(() => page.locator('#doc img.note-img').evaluate((el) => el.naturalWidth)).toBe(320);
  });

  test('copying a selection with a picture puts the picture on the clipboard as a data url, and one without puts text only', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Copied ${test.info().project.name}`, body: 'Before the picture.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('Before the picture.');
    await pastePicture(page, '#doc');
    const img = page.locator('#doc img.note-img');
    await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(320);
    const copy = (selector) => page.evaluate((target) => {
      getSelection().selectAllChildren(document.querySelector(target));
      const data = new DataTransfer();
      document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
      return { text: data.getData('text/plain'), html: data.getData('text/html') };
    }, selector);
    const both = await copy('#doc');
    expect(both.text).toBe('Before the picture.');
    expect(both.html).toMatch(/^<div data-leo="Before the picture\.\n\n!\[Pasted picture\]\(attachments\/\d{8}-\d{6}-pasted-1\.png\)"/);
    expect(both.html).toMatch(/<img src="data:image\/png;base64,[A-Za-z0-9+/=]+" alt="Pasted picture">/);
    expect(both.html).toContain('style="color:inherit"');
    expect(both.html).not.toMatch(/\/api\/|127\.0\.0\.1|localhost|<script|onerror|onclick/);
    const plain = await copy('#doc .blk:first-child');
    expect(plain.text).toBe('Before the picture.');
    expect(plain.html).toBe('');
  });

  test('a clicked picture copies as a picture and pastes back as the same markdown, without another upload', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    const note = await (await page.request.post('/api/notes', { data: { title: `Picked ${test.info().project.name}`, body: 'Before the picture.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('Before the picture.');
    await pastePicture(page, '#doc');
    const img = page.locator('#doc img.note-img');
    await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(320);
    const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
    await expect.poll(body).toContain('![Pasted picture]');
    const uploads = [];
    page.on('request', (request) => { if (request.url().includes('/api/pictures')) uploads.push(request.url()); });
    await img.click();
    await expect(page.locator('#doc .pic-frame')).toBeVisible();
    await page.keyboard.press('ControlOrMeta+c');
    const held = () => page.evaluate(async () => {
      const [item] = await navigator.clipboard.read();
      const html = item.types.includes('text/html') ? await (await item.getType('text/html')).text() : '';
      return { types: item.types, size: item.types.includes('image/png') ? (await item.getType('image/png')).size : 0, html };
    });
    await expect.poll(async () => (await held()).types).toContain('image/png');
    const { size, html } = await held();
    expect(size).toBeGreaterThan(100);
    expect(html).toContain('data-leo="![Pasted picture](attachments/');
    expect(html).toMatch(/<img src="data:image\/png;base64,/);
    await page.keyboard.press('ControlOrMeta+v');
    await expect(img).toHaveCount(2);
    await expect.poll(body).toMatch(/^Before the picture\.\n\n(!\[Pasted picture\]\(attachments\/\d{8}-\d{6}-pasted-1\.png\))\n\n\1$/);
    expect(uploads).toEqual([]);
  });

  test('a picture still loading when it is copied lands on the clipboard once it has loaded', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    const note = await (await page.request.post('/api/notes', { data: { title: `Late ${test.info().project.name}`, body: 'Before the picture.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('Before the picture.');
    await pastePicture(page, '#doc');
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toContain('![Pasted picture]');
    let release;
    const held = new Promise((done) => { release = done; });
    await page.route('**/api/image**', async (route) => {
      await held;
      await route.continue();
    });
    await page.reload();
    const img = page.locator('#doc img.note-img');
    await expect(img).toHaveCount(1);
    const copied = (selector) => page.evaluate((target) => {
      getSelection().selectAllChildren(document.querySelector(target));
      document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: new DataTransfer(), bubbles: true, cancelable: true }));
    }, selector);
    const html = () => page.evaluate(async () => {
      const [item] = await navigator.clipboard.read();
      return item.types.includes('text/html') ? (await item.getType('text/html')).text() : '';
    });
    await page.evaluate(() => navigator.clipboard.writeText('old'));
    await copied('#doc');
    release();
    await expect.poll(html).toMatch(/Before the picture\.[\s\S]*<img src="data:image\/png;base64,/);
  });

  test('a paste that only looks like leo is not taken as one', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Foreign ${test.info().project.name}`, body: 'Only this line.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('Only this line.');
    const paste = async (markdown, text) => {
      await page.evaluate(() => document.activeElement.blur());
      await expect(page.locator('#doc textarea')).toHaveCount(0);
      await page.evaluate(([markdown, text]) => {
        getSelection().selectAllChildren(document.querySelector('#doc'));
        const data = new DataTransfer();
        data.setData('text/html', `<div data-leo="${markdown}" data-leo-from=""><p>x</p></div>`);
        if (text) data.setData('text/plain', text);
        document.body.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
      }, [markdown, text]);
    };
    const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
    await paste('![x](https://a.b/c.png)', '');
    await paste('One ![x](//a.b/c.png)', 'One');
    await expect.poll(body).toBe('One');
    await paste('Two ![Pasted picture](attachments/a.png)', 'Three');
    await expect.poll(body).toBe('Three');
    await paste('Four ![Pasted picture](attachments/a.png)', 'Four');
    await expect.poll(body).toBe('Four ![Pasted picture](attachments/a.png)');
  });

  test('the transcript pages through a long recording, finds a phrase and saves only the fixed line', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Lecture ${test.info().project.name}`, body: 'Notes from a recording.' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'recording-sources', note.id);
    fs.mkdirSync(folder, { recursive: true });
    const filler = 'lorem ipsum dolor sit amet '.repeat(2000);
    const passages = Array.from({ length: 12 }, (_, i) => ({
      start_secs: i * 60,
      end_secs: i * 60 + 59,
      text: `Passage ${i + 1} ${i === 11 ? 'about the zebrafish ' : ''}${filler}`.slice(0, 50000),
      speaker: '',
    }));
    fs.writeFileSync(path.join(folder, 'browser-recording-1.json'), JSON.stringify({ id: 'browser-recording-1', started: '2026-11-03T09:00:00Z', passages, points: [], context: '', wants: '', warnings: [], trace: [] }));
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#note-facts [data-action="note-sources"]').click();
    const sheet = page.locator('#note-aside');
    await expect(page.locator('#note-aside-facts')).toContainText('Transcript');
    await expect(page.locator('#doc')).toBeHidden();
    await expect(page.locator('#title')).toBeVisible();
    const rows = sheet.locator('.source-text');
    await expect(rows.first()).toHaveText(/^Passage 1 /);
    const first = await rows.count();
    expect(first, 'the first page holds part of the recording').toBeGreaterThan(0);
    expect(first).toBeLessThan(12);
    await expect(sheet.locator('[data-action="source-earlier"]')).toHaveCount(0);
    await sheet.locator('[data-action="source-later"]').click();
    await expect(rows.first()).toHaveText(new RegExp(`^Passage ${first + 1} `));
    expect(first + (await rows.count()), 'the two pages hold every passage once').toBe(12);
    await expect(sheet.locator('[data-action="source-later"]')).toHaveCount(0);
    await sheet.locator('[data-action="source-earlier"]').click();
    await expect(rows.first()).toHaveText(/^Passage 1 /);
    await sheet.locator('#source-query').fill('zebrafish');
    const match = sheet.locator('.source-match');
    await expect(match).toHaveCount(1);
    await match.click();
    const fixed = rows.last();
    await expect(fixed).toHaveText(/Passage 12 about the zebrafish/);
    await expect(sheet.locator('[data-action="source-earlier"]')).toBeVisible();
    await fixed.evaluate((box) => {
      box.textContent = `${box.textContent} Fixed.`;
      box.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const sent = page.waitForRequest((r) => r.method() === 'PUT' && r.url().endsWith(`/api/notes/${note.id}/recording`));
    await sheet.locator('[data-action="source-save"]').click();
    const body = (await sent).postDataJSON();
    expect(body.source).toBe('browser-recording-1');
    expect(body.changes, 'only the fixed line is sent').toHaveLength(1);
    expect(body.changes[0].kind).toBe('passage');
    await expect(page.locator('.toast')).toContainText('Saved your fixes');
    const after = await (await page.request.get(`/api/notes/${note.id}/recording?q=zebrafish`)).json();
    expect(after.matches).toHaveLength(1);
    expect(after.matches[0].passage.text.endsWith(' Fixed.')).toBe(true);
    expect(after.list).toHaveLength(1);
  });

  test('the Picture button adds a picture from the device', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Chosen ${test.info().project.name}`, body: 'x' } })).json();
    await page.goto(`/#/n/${note.id}`);
    const chooser = page.waitForEvent('filechooser');
    await page.locator('#note-more').click();
    await page.getByRole('menuitem', { name: 'Add picture' }).click();
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP8z8DAwMDAxMDAwMAAAAwqAQXxT7KxAAAAAElFTkSuQmCC', 'base64');
    await (await chooser).setFiles({ name: 'Heap diagram.png', mimeType: 'image/png', buffer: png });
    await expect(page.locator('#doc img.note-img')).toHaveAttribute('alt', 'Heap diagram');
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toContain('![Heap diagram](attachments/');
  });

  test('a picture pasted on a folder page opens the upload sheet with it', async ({ page }) => {
    await page.goto('/');
    await pastePicture(page, 'body');
    await expect(page.locator('.upload-file')).toContainText('pasted-1.png');
    await expect(page.locator('#upload-go')).toBeEnabled();
    await pastePicture(page, '#upload-title', 'board.png');
    await expect(page.locator('.upload-file')).toHaveCount(2);
  });

  test('a picture pasted into Felix shows as a thumbnail and goes with the message', async ({ page }) => {
    const asked = [];
    const uploaded = [];
    let finishReading;
    const reading = new Promise(resolve => { finishReading = resolve; });
    await page.route(/\/api\/chats\/[^/]+\/files(\/|$)/, async (route) => {
      if (route.request().method() === 'DELETE') return route.fallback();
      if (route.request().method() !== 'PUT') return route.fulfill({ json: [] });
      const url = new URL(route.request().url());
      const body = { name: decodeURIComponent(url.pathname.split('/').pop()), type: url.searchParams.get('type'), data: route.request().postDataBuffer() };
      uploaded.push(body);
      await reading;
      return route.fulfill({ status: 201, json: { id: 'doc-pic-1', name: body.name, chars: 40 } });
    });
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"That diagram shows a heap."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await pastePicture(page, '#chat-input');
    const chip = page.locator('#chat #chat-refs .file-card');
    try {
      await expect(chip).toContainText('pasted-1.png');
      await expect(chip).toHaveClass(/reading/);
      await expect(chip.locator('.file-peek img')).toBeVisible();
    } finally {
      finishReading();
    }
    await expect(chip).not.toHaveClass(/reading/);
    await expect(chip).toContainText('pasted-1.jpg');
    await expect(chip.locator('.file-peek img')).toBeVisible();
    expect(uploaded).toHaveLength(1);
    expect(uploaded[0]).toMatchObject({ name: 'pasted-1.jpg', type: 'image/jpeg' });
    expect(uploaded[0].data.subarray(0, 3)).toEqual(Buffer.from([0xff, 0xd8, 0xff]));
    await page.locator('#chat-input').fill('what is this?');
    await page.locator('#chat-input').press('Enter');
    await expect(page.locator('#chat .msg.leo').last()).toContainText('That diagram shows a heap.');
    await expect(page.locator('#chat .msg.user .file-card img')).toHaveAttribute('src', /^data:image\/jpeg;base64,/);
    expect(asked[0].files).toEqual(['doc-pic-1']);
    await expect(chip).toHaveCount(0);
  });
});

test.describe('large libraries', () => {

});

test.describe('readability', () => {
  async function lowContrast(page, within = 'body') {
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
    return page.evaluate(async (within) => {
      await Promise.all(document.getAnimations().filter((run) => run.effect && run.effect.getComputedTiming().iterations !== Infinity).map((run) => run.finished.catch(() => {})));
      const parse = (s) => {
        const m = /rgba?\(([^)]+)\)/.exec(s || '');
        if (!m) return [0, 0, 0, 0];
        const p = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
        return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1];
      };
      const over = (top, under) => {
        const a = top[3];
        return [0, 1, 2].map((i) => top[i] * a + under[i] * (1 - a)).concat(1);
      };
      const lum = (c) => {
        const [r, g, b] = c.slice(0, 3).map((v) => {
          v /= 255;
          return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
        });
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
      };
      const backdrop = (el) => {
        const layers = [];
        for (let at = el; at; at = at.parentElement) {
          const c = parse(getComputedStyle(at).backgroundColor);
          if (c[3] > 0) layers.push(c);
          if (c[3] >= 1) break;
        }
        let colour = parse(getComputedStyle(document.body).backgroundColor);
        if (colour[3] < 1) colour = [255, 255, 255, 1];
        for (const layer of layers.reverse()) colour = over(layer, colour);
        return colour;
      };
      const fade = (el) => {
        let o = 1;
        for (let at = el; at; at = at.parentElement) o *= Number(getComputedStyle(at).opacity);
        return o;
      };
      const found = [];
      for (const el of document.querySelectorAll(`${within} *`)) {
        if (![...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim())) continue;
        if (el.closest('[disabled], [aria-disabled="true"], .skeleton, canvas, svg, option, select, [hidden]')) continue;
        const style = getComputedStyle(el);
        const box = el.getBoundingClientRect();
        if (style.visibility !== 'visible' || box.width < 1 || box.height < 1 || style.display === 'none') continue;
        const seen = fade(el);
        if (seen < 0.05) continue;
        const back = backdrop(el);
        const fg = over(parse(style.color).slice(0, 3).concat(parse(style.color)[3] * seen), back);
        const [a, b] = [lum(fg), lum(back)].sort((x, y) => y - x);
        const ratio = (a + 0.05) / (b + 0.05);
        if (ratio < 3) found.push(`${ratio.toFixed(2)} ${el.tagName.toLowerCase()}.${[...el.classList].join('.')} "${el.textContent.trim().slice(0, 40)}"`);
      }
      return [...new Set(found)];
    }, within);
  }

  for (const scheme of ['light', 'dark']) {


    test(`filled danger controls read clearly in ${scheme} mode`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      const tag = `${test.info().project.name}-${scheme}-${Date.now()}`;
      const task = await (await page.request.post('/api/tasks', { data: { title: `Readable task ${tag}` } })).json();
      await page.goto('/#/calendar');
      await page.locator(`.cal-open[data-id="${task.id}"]:visible`).click();
      await expect(page.locator('#cal-side').getByRole('button', { name: 'Delete', exact: true })).toBeVisible();
      expect(await lowContrast(page)).toEqual([]);
      await page.keyboard.press('Escape');
      await page.goto('/#/settings/keeping');
      await expect(page.locator('select[data-keep="trash_days"]')).toBeVisible();
      expect(await lowContrast(page)).toEqual([]);
      await page.request.put(`/api/chats/other-${tag}`, { data: { title: `Other chat ${tag}`, messages: [{ role: 'user', text: 'Hello' }, { role: 'assistant', text: 'Hi' }] } });
      await page.request.put(`/api/chats/readable-${tag}`, { data: { title: `Readable chat ${tag}`, messages: [{ role: 'user', text: 'Hello' }, { role: 'assistant', text: 'Hi' }] } });
      await page.goto('/');
      await page.locator('#chat-toggle').click();
      const history = page.locator('.chat-history');
      if (await history.isHidden()) await page.getByRole('button', { name: 'Your chats', exact: true }).click();
      await history.locator(`[data-chat="forget"][data-id="readable-${tag}"]`).click();
      await expect(history.locator('.chat-history-x.sure')).toHaveAttribute('aria-label', /^Confirm delete/);
      expect(await lowContrast(page)).toEqual([]);
    });

    test(`every page reads clearly in ${scheme} mode`, async ({ page }) => {
      test.skip(test.info().project.name !== 'desktop', 'colours do not depend on the screen size');
      test.setTimeout(90000);
      await page.emulateMedia({ colorScheme: scheme });
      await page.request.post('/api/dirs', { data: { path: 'readable' } });
      const note = await (await page.request.post('/api/notes', { data: { title: 'Readable note', body: '## Heading\n- [x] done\n- [ ] open\n\n> a quote\n\n`code` and a [link](https://example.com)', directory: 'readable' } })).json();
      const gone = await (await page.request.post('/api/notes', { data: { title: 'Readable gone', body: 'x' } })).json();
      await page.request.delete(`/api/notes/${gone.id}`);
      const problems = {};
      const look = async (name) => {
        const low = await lowContrast(page);
        if (low.length) problems[name] = low;
      };
      for (const [name, hash, ready] of [['folders', '#/', '#home-ask'], ['folder', '#/f/readable', '#home-ask'], ['note', `#/n/${note.id}`, '#doc .blk'], ['search', '#/search/readable', '#app .cards .card'], ['trash', '#/trash', '.trash-row'], ['settings', '#/settings', '.settings-section .settings-head'], ['keeping', '#/settings/keeping', 'select[data-keep="trash_days"]'], ['record', '#/record', '#rec-box [data-action="rec-start"]'], ['calendar', '#/calendar', '#cal-main .cal-week, #cal-main .cal-month']]) {
        await page.goto(`/${hash}`);
        await expect(page.locator(ready).first()).toBeVisible();
        await look(name);
      }
      await page.goto('/');
      if (await page.locator('#menu').isVisible()) {
        await page.locator('#menu').click();
        await look('menu');
        await page.keyboard.press('Escape');
      }
      await control(page, 'upload').click();
      await look('upload');
      await page.keyboard.press('Escape');
      await page.locator('#chat-toggle').click();
      await look('felix');
      expect(problems).toEqual({});
    });
  }
});

test.describe('background work', () => {
  test('work still going on shows its progress on every page until it is done', async ({ page }) => {
    let tasks = [
      { kind: 'upload', label: 'Making a note from slides.pdf', step: 'Writing the note', done: 1, total: 4, href: '#/' },
      { kind: 'recording', label: 'Writing the notes from a recording', step: 'Transcribing the recording', done: 0, total: 0, href: '#/record' },
    ];
    await page.route('**/api/activity', (route) => route.fulfill({ json: { tasks } }));
    await page.route('**/api/record', (route) => route.fulfill({ json: { available: true, local: false, job: null } }));
    await page.goto('/');
    await page.reload();
    const tray = page.locator('#activity');
    await expect(tray).toBeVisible();
    await expect(tray.locator('.activity-head')).toContainText('2 things in the background');
    const upload = tray.locator('.activity-row', { hasText: 'slides.pdf' });
    await expect(upload).toContainText('Writing the note · 1/4');
    expect(await upload.locator('.activity-bar i').evaluate((el) => el.style.width)).toBe('25%');
    await expect(tray.locator('.activity-row', { hasText: 'recording' }).locator('.activity-bar')).toHaveClass(/busy/);

    await tray.locator('.activity-head').click();
    await expect(tray.locator('.activity-row')).toHaveCount(0);
    await expect(tray.locator('.activity-head')).toHaveAttribute('aria-expanded', 'false');
    await tray.locator('.activity-head').click();

    await tray.locator('.activity-row', { hasText: 'recording' }).click();
    await expect(page).toHaveURL(/#\/record$/);
    await expect(tray.locator('.activity-row')).toHaveCount(1, { timeout: 4000 });
    await expect(tray).toContainText('slides.pdf');

    tasks = [];
    await expect(tray).toBeHidden({ timeout: 5000 });
  });
});

test.describe('trash', () => {
  test('deletes one note, the chosen notes, or everything for good, and restores the chosen ones', async ({ page }) => {
    const tag = test.info().project.name;
    const made = [];
    for (const n of [1, 2, 3, 4]) {
      const note = await (await page.request.post('/api/notes', { data: { title: `Trashed ${n} ${tag}`, body: 'x' } })).json();
      made.push(note);
      expect((await page.request.delete(`/api/notes/${note.id}`)).ok()).toBe(true);
    }
    await page.goto('/#/trash');
    const row = (n) => page.locator('.trash-row', { hasText: `Trashed ${n} ${tag}` });
    await expect(row(1)).toBeVisible();

    await row(1).locator('[data-action="trash-forget"]').click();
    await expect(page.locator('.sheet h3')).toContainText(`Trashed 1 ${tag}`);
    await page.locator('[data-action="trash-delete-now"]').click();
    await expect(page.locator('.toast')).toContainText('Deleted 1 note for good');
    await expect(row(1)).toHaveCount(0);

    await page.locator('[data-action="trash-select"]').click();
    await expect(page.locator('.select-bar')).toContainText('0 selected');
    await expect(page.locator('[data-action="trash-delete-picked"]')).toBeDisabled();
    await row(2).click();
    await expect(page.locator('.select-bar')).toContainText('1 selected');
    await page.locator('[data-action="trash-restore-picked"]').click();
    await expect(page.locator('.toast')).toContainText('Restored 1 note');
    expect((await page.request.get(`/api/notes/${made[1].id}`)).ok()).toBe(true);
    await expect(row(2)).toHaveCount(0);

    await page.locator('[data-action="trash-select"]').click();
    await page.locator('[data-trash-all]').check();
    await expect(page.locator('.trash-row.picked')).toHaveCount(await page.locator('.trash-row').count());
    await page.locator('[data-trash-all]').uncheck();
    await expect(page.locator('.trash-row.picked')).toHaveCount(0);
    await row(3).click();
    await row(4).click();
    await expect(page.locator('.select-bar')).toContainText('2 selected');
    await page.locator('[data-action="trash-delete-picked"]').click();
    await page.locator('[data-action="close"]').click();
    await expect(row(3)).toBeVisible();
    await page.locator('[data-action="trash-delete-picked"]').click();
    await page.locator('[data-action="trash-delete-now"]').click();
    await expect(row(3)).toHaveCount(0);
    await expect(row(4)).toHaveCount(0);
    const mine = async () => (await (await page.request.get('/api/trash')).json()).filter((t) => /^Trashed \d /.test(t.title) && t.title.endsWith(` ${tag}`));
    expect(await mine()).toEqual([]);

    const again = await (await page.request.post('/api/notes', { data: { title: `Trashed 5 ${tag}`, body: 'x' } })).json();
    await page.request.delete(`/api/notes/${again.id}`);
    await page.goto('/#/');
    await page.goto('/#/trash');
    await expect(page.locator('.trash-row', { hasText: `Trashed 5 ${tag}` })).toBeVisible();
    await page.locator('[data-action="trash-empty"]').click();
    await page.locator('[data-action="trash-delete-now"]').click();
    await expect(page.locator('.empty')).toContainText('Nothing thrown away.');
    await expect(row(5)).toHaveCount(0);
    expect(await mine()).toEqual([]);
  });

  test('an unreadable trashed file shows with Delete for good only', async ({ page }) => {
    const name = `Broken ${test.info().project.name}`;
    const trash = path.join(process.env.LEO_BROWSER_HOME, 'notes', '.trash');
    fs.mkdirSync(trash, { recursive: true });
    fs.writeFileSync(path.join(trash, `${name}.md`), '---\ntitle: [\n---\nx');
    await page.goto('/#/trash');
    const row = page.locator('.trash-row', { hasText: `${name}.md` });
    await expect(row).toContainText('leo cannot read this file');
    await expect(row.locator('[data-action="restore"]')).toHaveCount(0);
    await page.locator('[data-action="trash-select"]').click();
    await row.click();
    await expect(page.locator('[data-action="trash-restore-picked"]')).toBeDisabled();
    await expect(page.locator('[data-action="trash-delete-picked"]')).toBeEnabled();
    await page.locator('[data-action="trash-delete-picked"]').click();
    await page.locator('[data-action="trash-delete-now"]').click();
    await expect(page.locator('.toast')).toContainText('Deleted 1 note for good');
    await expect(row).toHaveCount(0);
    expect(fs.existsSync(path.join(trash, `${name}.md`))).toBe(false);
  });
});

test.describe('export and this browser', () => {
  test('exports a zip with the parts chosen', async ({ page }) => {
    await page.goto('/#/settings/keeping');
    const link = page.locator('#export-link');
    await expect(link).toHaveAttribute('href', '/api/export?uploads=true&chats=true&trash=false');
    await page.locator('[data-export="chats"]').uncheck();
    await page.locator('[data-export="trash"]').check();
    await expect(link).toHaveAttribute('href', '/api/export?uploads=true&chats=false&trash=true');
    const zip = await page.request.get('/api/export?uploads=false&chats=true&trash=false');
    expect(zip.headers()['content-type']).toBe('application/zip');
    expect(zip.headers()['content-disposition']).toMatch(/^attachment; filename="leo-export-\d{4}-\d{2}-\d{2}\.zip"; filename\*=UTF-8''leo-export-\d{4}-\d{2}-\d{2}\.zip$/);
    const bytes = await zip.body();
    expect(bytes.subarray(0, 2).toString()).toBe('PK');
    expect(bytes.includes(Buffer.from('leo/README.txt'))).toBe(true);
    expect(bytes.includes(Buffer.from('serve-token'))).toBe(false);
  });

  test('Felix is the tab icon, even before signing in', async ({ page, playwright }) => {
    await expect(page.locator('link[rel="icon"]')).toHaveAttribute('href', '/favicon.svg');
    const stranger = await playwright.request.newContext({ baseURL: 'http://127.0.0.1:31831' });
    const icon = await stranger.get('/favicon.svg');
    expect(icon.status()).toBe(200);
    expect(icon.headers()['content-type']).toBe('image/svg+xml');
    expect(await icon.text()).toContain('#b4cfe7');
    expect((await stranger.get('/api/notes')).status()).toBe(401);
    await stranger.dispose();
  });
});

test.describe('advanced settings', () => {
  test('chooses how long each kind of data is kept, asking before deleting sooner', async ({ page }) => {
    await page.goto('/#/settings/keeping');
    await expect(page).toHaveURL(/#\/settings\/keeping$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Keeping' })).toBeVisible();
    await expect(page.locator('select[data-keep]')).toHaveCount(5);
    const trash = page.locator('select[data-keep="trash_days"]');
    const chats = page.locator('select[data-keep="chat_days"]');
    const uploads = page.locator('select[data-keep="upload_days"]');
    await expect(trash).toHaveValue('30');
    await expect(chats).toHaveValue('forever');
    const kept = async () => {
      const k = await (await page.request.get('/api/keep')).json();
      return [k.trash_days, k.chat_days];
    };
    await trash.selectOption('forever');
    await expect.poll(kept).toEqual([null, null]);
    await chats.selectOption('30');
    await expect(page.locator('.sheet h3')).toHaveText('Keep chats with Felix for 30 days?');
    await page.locator('[data-action="keep-cancel"]').click();
    await expect(chats).toHaveValue('forever');
    expect(await kept()).toEqual([null, null]);
    await chats.selectOption('90');
    await page.locator('[data-action="keep-now"]').click();
    await expect.poll(kept).toEqual([null, 90]);
    await expect(chats).toHaveValue('90');
    await uploads.selectOption('forever');
    await uploads.selectOption('7');
    await expect(page.locator('.sheet h3')).toHaveText('Keep uploaded files for 7 days?');
    await page.locator('[data-action="keep-cancel"]').click();
    await expect(uploads).toHaveValue('forever');
    await page.goto('/#/trash');
    await page.request.post('/api/keep', { data: { trash_days: 7, chat_days: null } });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Keep check', body: 'x' } })).json();
    await page.request.delete(`/api/notes/${note.id}`);
    await page.goto('/#/');
    await page.goto('/#/trash');
    await expect(page.locator('#app .hint')).toContainText('stay here for 7 days');
    await page.request.post('/api/keep', { data: { trash_days: 30, chat_days: null } });
  });

  test('an uploaded PDF, picture or text opens in a viewer inside the page', async ({ page, browser }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Viewer ${test.info().project.name}`, body: 'x' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
    fs.mkdirSync(folder, { recursive: true });
    const maker = await browser.newPage();
    await maker.setContent('<h1>Neuro symbolic</h1><p>Rules and networks.</p>');
    fs.writeFileSync(path.join(folder, 'paper.pdf'), await maker.pdf());
    await maker.close();
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP8z8DAwMDAxMDAwMAAAAwqAQXxT7KxAAAAAElFTkSuQmCC', 'base64');
    fs.writeFileSync(path.join(folder, 'board.png'), png);
    fs.writeFileSync(path.join(folder, 'notes.txt'), 'Plain text from the upload.');
    fs.writeFileSync(path.join(folder, 'slides.pptx'), 'pptx');
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('.chip.original')).toHaveCount(5);
    await expect(page.locator('.chip.original', { hasText: 'slides.pptx' })).not.toHaveAttribute('data-action', /./);

    const served = page.waitForResponse((r) => r.url().includes('paper.pdf?view=1'));
    await page.locator('.chip.original', { hasText: 'paper.pdf' }).click();
    const aside = page.locator('#note-aside');
    const facts = page.locator('#note-aside-facts');
    await expect(aside.locator('iframe.aside-frame')).toHaveAttribute('src', /paper\.pdf\?view=1$/);
    const pdf = await served;
    expect(pdf.status()).toBe(200);
    expect(pdf.headers()['content-disposition']).toMatch(/^inline/);
    expect(pdf.headers()['x-frame-options']).toBe('SAMEORIGIN');
    await expect(facts).toHaveText('paper.pdf · Download · Back to note');
    await expect(facts.locator('a[download]')).toHaveText('Download');
    await expect(page.locator('#title')).toBeVisible();
    await expect(page.locator('#doc')).toBeHidden();
    await expect(page.locator('#source-pane')).toHaveCount(0);
    await facts.locator('[data-action="note-back"]').click();
    await expect(aside).toBeHidden();
    await expect(page.locator('#doc')).toBeVisible();
    await expect(page.locator('.chip.original', { hasText: 'paper.pdf' })).toBeFocused();

    await page.locator('.chip.original', { hasText: 'board.png' }).click();
    const img = aside.locator('.aside-img');
    await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(2);
    await page.keyboard.press('Escape');
    await expect(aside).toBeHidden();

    await page.locator('.chip.original', { hasText: 'notes.txt' }).click();
    await expect(aside.locator('.aside-text')).toHaveText('Plain text from the upload.');
    await facts.locator('[data-action="note-back"]').click();
    await expect(page.locator('.sheet.viewer')).toHaveCount(0);
    expect(page.url()).toContain(`#/n/${note.id}`);
  });

  test('a note with several uploaded files downloads them together', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Lecture with slides', body: 'x' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'slides.pdf'), '%PDF fake');
    fs.writeFileSync(path.join(folder, 'board.jpg'), 'jpg');
    await page.goto(`/#/n/${note.id}`);
    const all = page.locator('.chip.original.all');
    await expect(all).toHaveText(/Download all 2/);
    const zip = await page.request.get(await all.getAttribute('href'));
    expect(zip.headers()['content-type']).toBe('application/zip');
    expect(zip.headers()['content-disposition']).toContain('Lecture with slides originals.zip');
    const bytes = await zip.body();
    expect(bytes.includes(Buffer.from('Lecture with slides/slides.pdf'))).toBe(true);
  });
});

test.describe('uploads', () => {
  test('a file becomes a note and opens when it is ready', async ({ page }) => {
    const made = await (await page.request.post('/api/notes', { data: { title: 'Lecture 9: Sorting', body: '## Merge sort\n- divide and conquer' } })).json();
    let sent = null;
    let polls = 0;
    const puts = await fakeFileSends(page);
    await page.route('**/api/import', async (route) => {
      sent = route.request().postDataJSON();
      await route.fulfill({ status: 202, json: { id: 'job-1' } });
    });
    await page.route('**/api/import/job-1', (route) => {
      polls += 1;
      route.fulfill({ json: polls < 2 ? { state: 'working', step: 'Writing the note', done: 0, total: 1 } : { state: 'done', step: '', done: 1, total: 1, note: made.id } });
    });
    await page.goto('/');
    await control(page, 'upload').click();
    await page.locator('#upload-input').setInputFiles({ name: 'sorting.txt', mimeType: 'text/plain', buffer: Buffer.from('Merge sort splits the list in half.') });
    await expect(page.locator('.upload-file')).toContainText('sorting.txt');
    await page.locator('#upload-title').fill('Sorting');
    await page.locator('#upload-wants').fill('The algorithm, then the code, explained step by step');
    await page.locator('#upload-go').click();
    await expect(page).toHaveURL(new RegExp(`#/n/${made.id}$`));
    expect(sent.title).toBe('Sorting');
    expect(sent.wants).toBe('The algorithm, then the code, explained step by step');
    expect(sent.files[0].name).toBe('sorting.txt');
    expect(puts.map((p) => p.method)).toEqual(['PUT']);
    expect(puts[0].name).toBe('sorting.txt');
    expect(puts[0].body.toString()).toBe('Merge sort splits the list in half.');
    expect(sent.upload).toBe(puts[0].upload);
    expect(sent.files[0].item).toBe(puts[0].item);
    expect(sent.files[0], 'the file travels in its own request, never in the JSON').not.toHaveProperty('data');
  });

  test('files dropped on an open note can be added to it, or still make a new note', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    const note = await (await page.request.post('/api/notes', { data: { title: `Week 3 ${Date.now().toString(36)}`, body: 'My own notes on heaps.' } })).json();
    let sent = null;
    let polls = 0;
    const puts = await fakeFileSends(page);
    await page.route('**/api/import', async (route) => {
      sent = route.request().postDataJSON();
      await route.fulfill({ status: 202, json: { id: 'job-into' } });
    });
    let inserted = null;
    await page.route('**/api/notes/*/insert', async (route) => {
      inserted = route.request().postDataJSON();
      await route.continue({ postData: JSON.stringify({ text: '## Heap proofs\n\nSift down keeps the order.', line: inserted.line, base: inserted.base }) });
    });
    await page.route('**/api/import/job-into', (route) => {
      polls += 1;
      route.fulfill({ json: polls < 2 ? { state: 'working', step: 'Writing the note', done: 0, total: 1 } : { state: 'done', step: '', done: 1, total: 1, note: note.id, section_chars: 36 } });
    });
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('My own notes on heaps.');
    // On the note itself, so the note's own drop handler sees the mixed drop and passes it on.
    await dropFiles(page, '#doc', [{ name: 'proofs.pdf', type: 'application/pdf', text: '%PDF fake' }, { name: 'board.png', type: 'image/png', text: 'png' }]);
    const sheet = page.locator('.sheet');
    await expect(sheet.locator('#upload-list'), 'a mixed drop keeps every file').toContainText('proofs.pdf');
    await expect(sheet.locator('#upload-list')).toContainText('board.png');
    await expect(sheet.locator('#upload-heading')).toHaveText('Add a file to this note');
    await expect(sheet.locator('.upload-where [data-value="add"]'), 'dropped on a note, adding is the default').toHaveAttribute('aria-checked', 'true');
    await expect(sheet.locator('#upload-dir')).toBeHidden();
    await expect(sheet.locator('#upload-go')).toHaveText('Add to the note');
    await sheet.locator('.upload-where [data-value="new"]').click();
    await expect(sheet.locator('#upload-go')).toHaveText('Make 2 notes');
    await expect(sheet.locator('#upload-dir')).toBeVisible();
    await sheet.locator('.upload-where [data-value="add"]').click();
    await expect(sheet.locator('#upload-vision'), 'a picture brings the image note').toBeVisible();
    await sheet.locator('#upload-wants').fill('just the proofs');
    await sheet.locator('#upload-go').click();
    await expect(page.locator('.toast')).toContainText('Added to');
    await expect(page.locator('#doc')).toContainText('Sift down keeps the order.');
    await expect(page).toHaveURL(new RegExp(`#/n/${note.id}$`));
    expect(sent).toMatchObject({ into: note.id, each: false, wants: 'just the proofs', title: null });
    expect(inserted, 'the page names the upload and sends no text').toMatchObject({ job: 'job-into', line: null });
    expect(inserted.text).toBeUndefined();
    expect(sent.files.map((f) => f.name)).toEqual(['proofs.pdf', 'board.png']);
    expect(sent.files.map((f) => f.item).sort()).toEqual(puts.map((p) => p.item).sort());
    expect(puts.find((p) => p.name === 'proofs.pdf').body.toString()).toBe('%PDF fake');
    expect(new Set(puts.map((p) => p.upload))).toEqual(new Set([sent.upload]));
    const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
    await expect.poll(async () => (await body()).trim()).toBe('My own notes on heaps.\n\n## Heap proofs\n\nSift down keeps the order.');
    await page.locator('.toast button', { hasText: 'Undo' }).click();
    await expect.poll(async () => (await body()).trim()).toBe('My own notes on heaps.');
  });

  test('several files make a note each unless asked for one note, and the folder opens', async ({ page }) => {
    const dir = `each-${test.info().project.name}-${Date.now().toString(36)}`;
    await page.request.post('/api/dirs', { data: { path: dir } });
    const notes = [];
    for (const n of [1, 2]) notes.push(await (await page.request.post('/api/notes', { data: { title: `Lecture ${n}`, body: 'x', directory: dir } })).json());
    const sent = [];
    const puts = await fakeFileSends(page);
    await page.route('**/api/import', async (route) => {
      sent.push(route.request().postDataJSON());
      await route.fulfill({ status: 202, json: { id: 'job-each' } });
    });
    await page.route('**/api/import/job-each', (route) => route.fulfill({ json: { state: 'done', step: '', done: 2, total: 2, note: notes[0].id, notes: notes.map((n) => n.id) } }));
    await page.goto(`/#/f/${dir}`);
    await control(page, 'upload').click();
    const input = page.locator('#upload-input');
    await input.setInputFiles({ name: 'lecture 1.txt', mimeType: 'text/plain', buffer: Buffer.from('One') });
    await expect(page.locator('#upload-together-row')).toBeHidden();
    await expect(page.locator('#upload-vision'), 'text files need no image note').toBeHidden();
    await expect(page.locator('.sheet .note-fields #upload-dir ~ .select-menu-button'), 'the same field group and menu as Record').toBeVisible();
    await expect(page.locator('#upload-title')).toBeVisible();
    await expect(page.locator('#upload-go')).toHaveText('Make the note');
    await input.setInputFiles({ name: 'lecture 2.txt', mimeType: 'text/plain', buffer: Buffer.from('Two') });
    await expect(page.locator('#upload-together-row')).toBeVisible();
    await expect(page.locator('#upload-together')).not.toBeChecked();
    await expect(page.locator('#upload-title')).toBeHidden();
    await expect(page.locator('#upload-go')).toHaveText('Make 2 notes');
    await page.locator('#upload-together').check();
    await expect(page.locator('#upload-title')).toBeVisible();
    await expect(page.locator('#upload-go')).toHaveText('Make the note');
    await page.locator('#upload-together').uncheck();
    await page.locator('#upload-go').click();
    await expect.poll(() => sent.length).toBe(1);
    await expect(page.locator('.upload-working')).toHaveCount(0);
    await expect(page).toHaveURL(/\/(?:#\/)?$/);
    await expect(page.locator('.side-note', { hasText: 'Lecture 2' })).toBeVisible();
    expect(sent[0].each).toBe(true);
    expect(sent[0].title).toBe(null);
    expect(sent[0].files.map((f) => f.name)).toEqual(['lecture 1.txt', 'lecture 2.txt']);
    expect(puts.map((p) => p.body.toString()).sort()).toEqual(['One', 'Two']);
    expect(sent[0].files.map((f) => f.item)).toEqual(['lecture 1.txt', 'lecture 2.txt'].map((name) => puts.find((p) => p.name === name).item));
    for (const n of notes) await page.request.delete(`/api/notes/${n.id}`);
  });

  test('a finished upload shows up in the folder that is open, without a refresh', async ({ page }) => {
    const title = `Uploaded later ${test.info().project.name}`;
    let polls = 0;
    let made = null;
    await fakeFileSends(page);
    await page.route('**/api/import', (route) => route.fulfill({ status: 202, json: { id: 'job-late' } }));
    await page.route('**/api/import/job-late', async (route) => {
      polls += 1;
      if (polls < 3) return route.fulfill({ json: { state: 'working', step: 'Writing the note', done: 0, total: 1 } });
      if (!made) made = await (await page.request.post('/api/notes', { data: { title, body: 'From the upload.' } })).json();
      return route.fulfill({ json: { state: 'done', step: '', done: 1, total: 1, note: made.id } });
    });
    await page.goto('/');
    await control(page, 'upload').click();
    await page.locator('#upload-input').setInputFiles({ name: 'later.txt', mimeType: 'text/plain', buffer: Buffer.from('From the upload.') });
    await page.locator('#upload-go').click();
    await expect(page.locator('.upload-working')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.locator('.scrim')).toHaveCount(0);
    await expect(page.locator('.side-note', { hasText: title })).toBeVisible({ timeout: 8000 });
    await expect(page.locator('.toast')).toContainText('Your note from the upload is ready.');
    expect(page.url()).not.toContain('#/n/');
  });

  test('a finished upload shows up in the explorer while a note is open', async ({ page }) => {
    const title = `Uploaded beside ${test.info().project.name}`;
    const open = await (await page.request.post('/api/notes', { data: { title: `Open while uploading ${test.info().project.name}`, body: 'Reading this.' } })).json();
    let polls = 0;
    let made = null;
    await fakeFileSends(page);
    await page.route('**/api/import', (route) => route.fulfill({ status: 202, json: { id: 'job-beside' } }));
    await page.route('**/api/import/job-beside', async (route) => {
      polls += 1;
      if (polls < 3) return route.fulfill({ json: { state: 'working', step: 'Writing the note', done: 0, total: 1 } });
      if (!made) made = await (await page.request.post('/api/notes', { data: { title, body: 'From the upload.' } })).json();
      return route.fulfill({ json: { state: 'done', step: '', done: 1, total: 1, note: made.id } });
    });
    await page.goto(`/#/n/${open.id}`);
    await expect(page.locator('.side-note', { hasText: open.title })).toBeVisible();
    await control(page, 'upload').click();
    await page.locator('#upload-input').setInputFiles({ name: 'beside.txt', mimeType: 'text/plain', buffer: Buffer.from('From the upload.') });
    await page.locator('#upload-go').click();
    await expect(page.locator('.upload-working')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.locator('.scrim')).toHaveCount(0);
    await expect(page.locator('.toast')).toContainText('Your note from the upload is ready.', { timeout: 8000 });
    await expect(page.locator('.side-note', { hasText: title })).toBeVisible({ timeout: 2000 });
    expect(page.url()).toContain(`#/n/${open.id}`);
    for (const id of [open.id, made.id]) await page.request.delete(`/api/notes/${id}`);
  });

  test('a failed upload says why and offers to try again', async ({ page }) => {
    await fakeFileSends(page);
    await page.route('**/api/import', (route) => route.fulfill({ status: 202, json: { id: 'job-2' } }));
    await page.route('**/api/import/job-2', (route) => route.fulfill({ json: { state: 'failed', step: '', done: 0, total: 1, error: 'qwen3:8b cannot read images' } }));
    await page.goto('/');
    await goPlace(page, 'upload');
    await page.locator('#upload-input').setInputFiles({ name: 'board.png', mimeType: 'image/png', buffer: Buffer.from('89504e470d0a1a0a', 'hex') });
    await page.locator('#upload-go').click();
    await expect(page.locator('.upload-error')).toContainText('cannot read images');
    await expect(page.locator(':is(.sheet, .popup-menu) [data-action="upload"]')).toBeVisible();
  });

  test('an upload that has to wait for its turn says so, then runs and opens its note', async ({ page }) => {
    const made = await (await page.request.post('/api/notes', { data: { title: `Waited ${test.info().project.name} ${Date.now().toString(36)}`, body: 'From the upload.' } })).json();
    await fakeFileSends(page);
    let polls = 0;
    await page.route('**/api/import', (route) => route.fulfill({ status: 202, json: { id: 'job-wait' } }));
    await page.route('**/api/import/job-wait', (route) => {
      polls += 1;
      route.fulfill({ json: polls < 3 ? { state: 'waiting', step: 'Uploading', done: 0, total: 1 } : polls < 4 ? { state: 'working', step: 'Writing the note', done: 0, total: 1 } : { state: 'done', step: '', done: 1, total: 1, note: made.id } });
    });
    await page.goto('/');
    await control(page, 'upload').click();
    await page.locator('#upload-input').setInputFiles({ name: 'queued.txt', mimeType: 'text/plain', buffer: Buffer.from('From the upload.') });
    await page.locator('#upload-go').click();
    const progress = page.locator('.upload-working #upload-progress');
    await expect(progress).toContainText('Waiting for another upload to finish…');
    await expect(progress).toContainText('Writing the note', { timeout: 8000 });
    await expect(page).toHaveURL(new RegExp(`#/n/${made.id}$`), { timeout: 8000 });
  });
});

test.describe('recording', () => {
  function stubRecorder(page, { noteId, local = true }) {
    const seen = { started: null, points: [], stopped: false, paused: false, levels: Array(200).fill(0.1), source: 'microphone' };
    let polls = 0;
    const view = (over = {}) => ({ wants: seen.wants ?? (seen.started && seen.started.profile ? seen.started.profile.wants || '' : ''), id: 'rec-1', source: seen.source, title: seen.started ? seen.started.title || '' : '', directory: seen.started ? seen.started.directory || '' : '', state: seen.paused ? 'paused' : 'recording', secs: 3, step: '', steps: null, transcript: 'Today we cover breadth first search.', warnings: [], points: seen.points.map((t) => [3, t]), levels: seen.levels, note: null, error: null, ...over });
    page.route('**/api/record', async (route) => {
      if (route.request().method() === 'GET') return route.fulfill({ json: { available: true, local, job: null } });
      seen.started = route.request().postDataJSON();
      seen.source = seen.started.source;
      return route.fulfill({ status: 202, json: { id: 'rec-1' } });
    });
    page.route('**/api/record/rec-1', (route) => {
      if (!seen.stopped) return route.fulfill({ json: view() });
      polls += 1;
      return route.fulfill({ json: polls < 2 ? view({ state: 'writing', step: 'Writing the notes', steps: [1, 2] }) : view({ state: 'done', note: noteId }) });
    });
    page.route('**/api/record/rec-1/wants', (route) => {
      seen.wants = route.request().postDataJSON().text;
      return route.fulfill({ json: view({ wants: seen.wants }) });
    });
    page.route('**/api/record/rec-1/point', (route) => {
      seen.points.push(route.request().postDataJSON().text);
      return route.fulfill({ json: view() });
    });
    page.route('**/api/record/rec-1/pause', (route) => {
      seen.paused = route.request().postDataJSON().paused;
      return route.fulfill({ json: view() });
    });
    page.route('**/api/record/rec-1/stop', (route) => {
      seen.stopped = true;
      return route.fulfill({ json: view({ state: 'writing', step: 'Transcribing the recording' }) });
    });
    return seen;
  }

  test('native microphone recording takes points and opens the written note without browser capture', async ({ page, context }) => {
    const made = await (await page.request.post('/api/notes', { data: { title: 'BFS lecture', body: '## BFS\n- queue' } })).json();
    const seen = stubRecorder(page, { noteId: made.id });
    await page.goto('/');
    await control(page, 'record').click();
    await expect(page).toHaveURL(/#\/record$/);
    await expect(page.locator('.rec-kinds .choice-pick')).toHaveCount(3);
    await expect(page.locator('.rec-kinds .choice-title')).toHaveText(['Microphone', 'Screen', 'Call']);
    await page.locator('#rec-title').fill('Graphs');
    await page.locator('#rec-wants').fill('Focus on BFS');
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('#rec-transcript')).toContainText('breadth first search');
    await expect(page.locator('.rec-notepad #rec-point-text')).toBeVisible();
    await expect(page.locator('.rec-head h1')).toHaveText('Graphs');
    await expect(page.locator('[data-rec-tab="points"]')).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('#rec-transcript')).toBeHidden();
    await page.locator('[data-rec-tab="heard"]').click();
    await expect(page.locator('#rec-transcript')).toBeVisible();
    await expect(page.locator('.rec-notepad')).toBeHidden();
    await page.waitForResponse((r) => r.url().endsWith('/api/record/rec-1') && r.request().method() === 'GET');
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
    await expect(page.locator('#rec-transcript'), 'the open tab survives a poll').toBeVisible();
    await expect(page.locator('#rec-wave')).toBeVisible();
    await expect(page.locator('#rec-hear')).toHaveText('Hearing sound', { timeout: 8000 });
    const drawn = await page.locator('#rec-wave').evaluate((canvas) => {
      const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
      let painted = 0;
      for (let i = 3; i < data.length; i += 4) if (data[i] > 0) painted++;
      return painted;
    });
    expect(drawn).toBeGreaterThan(50);
    const leftEdge = await page.locator('#rec-wave').evaluate((canvas) => {
      const data = canvas.getContext('2d').getImageData(0, 0, Math.round(canvas.width * 0.1), canvas.height).data;
      let painted = 0;
      for (let i = 3; i < data.length; i += 4) if (data[i] > 0) painted++;
      return painted;
    });
    expect(leftEdge, 'the wave reaches the left edge of its box').toBeGreaterThan(0);
    expect(seen.started).toEqual({ source: 'microphone', directory: '', title: 'Graphs', profile: { context: '', wants: 'Focus on BFS', speakers: false } });
    await page.locator('[data-rec-tab="wants"]').click();
    const live = page.locator('#rec-wants-live');
    await expect(live).toHaveValue('Focus on BFS');
    await live.click();
    await live.press('End');
    await live.pressSequentially(' and the exam');
    await expect.poll(() => seen.wants).toBe('Focus on BFS and the exam');
    await expect(live).toBeFocused();
    await expect(page.locator('#rec-wants-state')).toContainText('Saved');

    await page.locator('[data-rec-tab="points"]').click();
    await page.locator('#rec-point-text').fill('exam question on BFS');
    await page.locator('#rec-point-text').press('Enter');
    await expect(page.locator('.rec-points')).toContainText('exam question on BFS');
    await expect(page.locator('#rec-point-text')).toHaveValue('');

    await page.locator('[data-action="rec-pause"]').click();
    await expect(page.locator('[data-action="rec-pause"]')).toHaveText('Resume');
    await page.locator('[data-action="rec-pause"]').click();
    await expect(page.locator('[data-action="rec-pause"]')).toHaveText('Pause');

    await page.keyboard.press('Escape');
    const pill = page.locator('#activity .activity-row', { hasText: 'Recording' });
    await expect(pill).toContainText(/Recording · \d+:\d\d/);
    await pill.click();
    await expect(page.locator('[data-action="rec-stop"]')).toBeVisible();
    await page.locator('[data-action="rec-stop"]').click();
    await expect(page).toHaveURL(new RegExp(`#/n/${made.id}$`), { timeout: 8000 });
    await expect(pill).toHaveCount(0);
  });

  test('a wide window puts Felix in a column beside the tabs and keeps the transcript node across polls', async ({ page }) => {
    await page.setViewportSize({ width: 1700, height: 1000 });
    stubRecorder(page, { noteId: null });
    await page.goto('/');
    await control(page, 'record').click();
    await expect(page.locator('#rec-ask')).toBeHidden();
    await page.locator('#rec-wants').fill('Focus on BFS');
    await expect(page.locator('#rec-ask .starter')).toHaveText(['What should I listen for?', 'What should I ask?']);
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('#rec-transcript')).toContainText('breadth first search');
    await expect(page.locator('#rec-ask .starter')).toHaveText(['What did I just miss?', 'Sum up so far', 'What should I ask next?']);
    await page.evaluate(() => { window.__heard = document.querySelector('#rec-transcript'); });
    const box = async (at) => page.locator(at).boundingBox();
    const [head, main, ask] = [await box('.rec-head'), await box('.rec-main'), await box('#rec-ask')];
    expect(main.x + main.width).toBeLessThan(ask.x);
    expect(Math.abs(main.y - ask.y)).toBeLessThan(2);
    expect(head.y + head.height).toBeLessThanOrEqual(main.y);
    await page.waitForResponse((r) => r.url().endsWith('/api/record/rec-1') && r.request().method() === 'GET');
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
    expect(await page.evaluate(() => window.__heard === document.querySelector('#rec-transcript') && window.__heard.isConnected)).toBe(true);
    await page.setViewportSize({ width: 900, height: 1000 });
    const [stackedMain, stackedAsk] = [await box('.rec-main'), await box('#rec-ask')];
    expect(stackedAsk.y).toBeGreaterThan(stackedMain.y + stackedMain.height - 2);
  });

  test('a recording finished while away shows up in the open folder, without a refresh', async ({ page, context }) => {
    const title = `Recorded later ${test.info().project.name} ${test.info().repeatEachIndex}`;
    const seen = stubRecorder(page, { noteId: null });
    let polls = 0;
    let made = null;
    await page.route('**/api/record/rec-1', async (route) => {
      if (!seen.stopped) return route.fulfill({ json: { id: 'rec-1', source: 'microphone', state: 'recording', secs: 3, step: '', steps: null, transcript: '', warnings: [], points: [], levels: [], note: null, error: null } });
      polls += 1;
      if (polls < 4) return route.fulfill({ json: { id: 'rec-1', source: 'microphone', state: 'writing', secs: 3, step: 'Writing the notes', steps: null, transcript: '', warnings: [], points: [], levels: [], note: null, error: null } });
      if (!made) made = await (await page.request.post('/api/notes', { data: { title, body: 'From the recording.' } })).json();
      return route.fulfill({ json: { id: 'rec-1', source: 'microphone', state: 'done', secs: 3, step: '', steps: null, transcript: '', warnings: [], points: [], levels: [], note: made.id, error: null } });
    });
    await page.goto('/#/record');
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('[data-action="rec-stop"]')).toBeVisible();
    await Promise.all([page.waitForResponse('**/api/record/rec-1/stop'), page.locator('[data-action="rec-stop"]').click()]);
    await page.evaluate(() => { location.hash = '#/'; });
    await expect(page.locator('.side-note', { hasText: title })).toBeVisible({ timeout: 10000 });
    await expect(page.locator('.toast')).toContainText('Your recording is now a note.');
    expect(page.url()).not.toContain('#/n/');
  });

  test('screen recording is native and the page never opens a screen picker', async ({ page }) => {
    const made = await (await page.request.post('/api/notes', { data: { title: 'Computer audio lecture', body: 'x' } })).json();
    const seen = stubRecorder(page, { noteId: made.id, local: true });
    await page.goto('/#/record');
    await page.evaluate(() => {
      navigator.mediaDevices.getDisplayMedia = async () => { throw new Error('Browser capture must stay closed'); };
    });
    await page.locator('.rec-kinds .choice-pick', { hasText: 'Screen' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('[data-action="rec-stop"]')).toBeVisible();
    expect(seen.started.source).toBe('screen');
    await page.locator('[data-action="rec-stop"]').click();
    await expect(page).toHaveURL(new RegExp('#/n/' + made.id + '$'), { timeout: 8000 });
  });

  test('the computer’s own sound shows its wave too, and says when nothing is heard', async ({ page }) => {
    const made = await (await page.request.post('/api/notes', { data: { title: 'Computer sound lecture', body: 'x' } })).json();
    const seen = stubRecorder(page, { noteId: made.id, local: true });
    seen.source = 'screen';
    seen.levels = Array(12).fill(0.1);
    await page.goto('/#/record');
    await page.locator('.rec-kinds .choice-pick', { hasText: 'Screen' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('#rec-wave')).toBeVisible();
    await expect(page.locator('#rec-hear')).toHaveText('Hearing sound');
    expect(seen.started.source).toBe('screen');
    seen.levels = [...Array(12).fill(0.1), ...Array(30).fill(0)];
    await expect(page.locator('#rec-hear')).toHaveText('No sound for a while: is something playing on the computer?', { timeout: 12000 });
    await expect(page.locator('#rec-hear')).toHaveClass(/silent/);
  });

  test('native screen permission failures leave recording controls usable', async ({ page }) => {
    await page.route('**/api/record', route => route.fulfill(route.request().method() === 'POST'
      ? { status:400, json:{error:'Allow Leo to record computer sound in System Settings, then try again.'} }
      : { json:{available:true,local:true,job:null} }));
    await page.goto('/#/record');
    await page.locator('.rec-kinds .choice-pick', { hasText: 'Screen' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('.toast.bad')).toContainText('Allow Leo to record computer sound');
    await expect(page.locator('[data-action="rec-start"]')).toBeEnabled();
    await expect(page.locator('[data-action="rec-stop"]')).toHaveCount(0);
  });

  test('recording always uses this computer for microphone screen and call', async ({ page }) => {
    const seen = stubRecorder(page, { noteId: 'x', local: false });
    await page.goto('/#/record');
    await expect(page.locator('.rec-kinds .choice-title')).toHaveText(['Microphone', 'Screen', 'Call']);
    await expect(page.locator('.rec-kinds .choice-pick', { hasText: 'Screen' })).toContainText('Records what this computer plays');
    await expect(page.locator('.rec-kinds .choice-icon > svg')).toHaveCount(3);
    await expect(page.locator('#rec-kind-line')).toHaveText('A lecture, meeting or your voice');
    await expect(page.locator('.rec-start .rec-tip'), 'the tip sits with Start').toBeVisible();
    await expect(page.locator('.rec-head .rec-chips #rec-dir ~ .select-menu-button'), 'the folder uses the shared select menu inside its chip').toBeVisible();
    await page.locator('.rec-kinds .choice-pick', { hasText: 'Call' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('[data-action="rec-stop"]')).toBeVisible();
    expect(seen.started.source).toBe('nativecall');
  });

  test('a call labels speakers on its own, microphone and screen never do, and Start sends it once the model is ready', async ({ page }) => {
    const seen = stubRecorder(page, { noteId: 'x', local: false });
    let model = { id: 'diar', size: 139310336, bytes: 0, verified: false, progress: null };
    await page.route('**/api/speech', (route) => route.fulfill({ json: { models: [], speakers: model } }));
    await page.goto('/#/record');
    await expect(page.locator('#rec-speakers'), 'no switch: leo decides from the source').toHaveCount(0);
    await expect(page.locator('#rec-speakers-model'), 'a microphone recording never asks for the speaker model').toHaveCount(0);
    await page.locator('.rec-kinds .choice-pick', { hasText: 'Call' }).click();
    await expect(page.locator('#rec-speakers-model')).toContainText('133 MB');
    await expect(page.locator('#rec-speakers-get')).toBeVisible();
    await page.locator('.rec-kinds .choice-pick', { hasText: 'Screen' }).click();
    await expect(page.locator('#rec-speakers-model')).toHaveCount(0);
    model = { ...model, bytes: model.size, verified: true };
    await page.reload();
    await page.locator('.rec-kinds .choice-pick', { hasText: 'Call' }).click();
    await expect(page.locator('#rec-speakers-model')).toHaveCount(0);
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('[data-action="rec-stop"]')).toBeVisible();
    expect(seen.started.profile.speakers).toBe(true);
  });

  test('the class or meeting happening now names the recording and tells the AI who and what, without overwriting a typed title', async ({ page, context }) => {
    const seen = stubRecorder(page, { noteId: 'x', local: true });
    let release;
    const late = new Promise((resolve) => { release = resolve; });
    const now = Date.now();
    await page.route('**/api/calendar', async (route) => {
      await late;
      return route.fulfill({ json: { connected: true, calendars: [{ id: 'c1', name: 'Work' }], events: [
        { id: 'e1', title: 'Design review', start: new Date(now - 5 * 60000).toISOString(), end: new Date(now + 55 * 60000).toISOString(), location: 'Room 4', context: 'Calendar event: Design review\nInvited: Sam, Priya' },
        { id: 'e2', title: 'Algorithms lecture', start: new Date(now + 3 * 3600000).toISOString(), end: new Date(now + 4 * 3600000).toISOString(), location: '', context: 'Calendar event: Algorithms lecture' },
      ] } });
    });
    await page.goto('/#/record');
    const title = page.locator('#rec-title');
    await title.click();
    await title.pressSequentially('Weekly sync');
    release();
    await expect(page.locator('#rec-event')).toHaveValue('0');
    await expect(title).toHaveValue('Weekly sync');
    await expect(title).toBeFocused();
    await expect(page.locator('#rec-wants')).toHaveValue(/Meeting notes/);
    await page.locator('#rec-event').selectOption('1');
    await expect(page.locator('#rec-event')).toHaveValue('1');
    await page.locator('#rec-event').selectOption('0');
    await page.locator('[data-action="rec-start"]').click();
    await expect.poll(() => seen.started && seen.started.profile.context).toContain('Invited: Sam, Priya');
    expect(seen.started.title).toBe('Weekly sync');
  });

  test('without a calendar the Record page points to Settings, and a broken calendar is not fatal', async ({ page }) => {
    await page.route('**/api/record', (route) => route.fulfill({ json: { available: true, local: false, job: null, pending: [] } }));
    await page.route('**/api/calendar', (route) => route.fulfill({ json: { connected: false, calendars: [], events: [] } }));
    await page.goto('/#/record');
    await expect(page.locator('.rec-idle .rec-check.optional a[href="#/settings/calendar"]')).toHaveText('Connect');
    await expect(page.locator('#rec-event')).toHaveCount(0);
    await page.unroute('**/api/calendar');
    await page.route('**/api/calendar', (route) => route.fulfill({ status: 500, json: { error: 'The calendars could not be read.' } }));
    await page.goto('/#/');
    await page.goto('/#/record');
    await expect(page.locator('[data-action="rec-start"]')).toBeEnabled();
    await expect(page.locator('#rec-wants')).toBeVisible();
  });

  test('a refused native microphone explains the system permission and starts no job', async ({ page }) => {
    await page.route('**/api/record', route => route.fulfill(route.request().method() === 'POST'
      ? { status:400, json:{error:'Allow Leo in System Settings → Privacy & Security → Microphone.'} }
      : { json:{available:true,local:true,job:null} }));
    await page.goto('/#/record');
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('.toast.bad')).toContainText('Allow Leo in System Settings');
    await expect(page.locator('[data-action="rec-start"]')).toBeEnabled();
    await expect(page.locator('[data-action="rec-stop"]')).toHaveCount(0);
  });
});

test.describe('the kinds of bugs that reached people before', () => {
  const kind = () => test.info().project.name;
  const stamp = () => `${kind()}-${Date.now().toString(36)}`;
  const bodyOf = async (page, id) => (await (await page.request.get(`/api/notes/${id}`)).json()).body;

  async function typeWithPauses(page, box, first, second) {
    await box.pressSequentially(first, { delay: 40 });
    await page.waitForTimeout(900);
    await expect(box, 'still typing in the same box after a pause').toBeFocused();
    await box.pressSequentially(second, { delay: 40 });
    await expect(box).toBeFocused();
  }

  test('every text box keeps focus and every letter while results and saves arrive', async ({ page }) => {
    const tag = stamp();
    const note = await (await page.request.post('/api/notes', { data: { title: `Typing target ${tag}`, body: 'Line one' } })).json();
    await page.request.post('/api/notes', { data: { title: `Dijkstra typing ${tag}`, body: 'Priority queue.' } });

    await page.goto('/');
    await openSearchBox(page);
    const search = page.locator('#search-input');
    await typeWithPauses(page, search, 'dijkstra', ' typing');
    await expect(search).toHaveValue('dijkstra typing');
    await page.keyboard.press('Escape');

    await page.goto('/');
    if (await page.locator('#side').isHidden()) {
      await page.locator('#menu').click();
      await page.locator(':is(.sheet, .popup-menu) [data-action="new-folder"]').click();
    } else {
      await page.locator('#side .side-group [data-action="new-folder"]').click();
    }
    const folder = page.locator('#folder-name');
    await typeWithPauses(page, folder, 'week', ' three');
    await expect(folder).toHaveValue('week three');
    await folder.press('Escape');

    await page.goto('/');
    await control(page, 'upload').click();
    await page.locator('#upload-input').setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('Merge sort.') });
    for (const id of ['#upload-title', '#upload-wants']) {
      const box = page.locator(id);
      await box.click();
      await typeWithPauses(page, box, 'merge', ' sort');
      await expect(box).toHaveValue('merge sort');
    }
    await page.locator('.sheet [data-action="close"]').first().click();

    await page.goto(`/#/n/${note.id}`);
    await page.locator('.blk').first().click();
    const line = page.locator('.line-edit');
    await line.press('End');
    await typeWithPauses(page, line, ' and', ' more');
    await expect.poll(() => bodyOf(page, note.id), { timeout: 5000 }).toBe('Line one and more');
    await expect(line).toBeFocused();

    const title = page.locator('#title');
    await title.click();
    await title.press('ControlOrMeta+A');
    await title.press('ArrowRight');
    await typeWithPauses(page, title, ' A', 'B');
    await expect(title).toHaveText(`Typing target ${tag} AB`);

    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const input = page.locator('#chat-input');
    await typeWithPauses(page, input, 'compare @dijkstra', ' typing');
    await expect(page.locator('.chat-pick-row').first()).toContainText(`Dijkstra typing ${tag}`);
    await expect(input).toHaveValue('compare @dijkstra typing');
    await page.locator('#chat [data-chat="close"]').click();

    await page.goto('/#/settings');
    const key = page.locator('[data-task-card="writing"] [data-key-input]').first();
    if (await key.count()) {
      await key.click();
      await typeWithPauses(page, key, 'sk-not', '-real');
      await expect(key).toHaveValue('sk-not-real');
      await key.fill('');
    }

    await page.goto('/#/record');
    const rec = page.locator('#rec-title');
    await rec.click();
    await typeWithPauses(page, rec, 'graph', ' lecture');
    await expect(rec).toHaveValue('graph lecture');
  });

  test('an edit typed just before opening another note is saved to the note it was typed in', async ({ page }) => {
    const tag = stamp();
    const first = await (await page.request.post('/api/notes', { data: { title: `Typed in ${tag}`, body: 'Start' } })).json();
    const second = await (await page.request.post('/api/notes', { data: { title: `Opened next ${tag}`, body: 'Leave me' } })).json();
    await page.goto(`/#/n/${first.id}`);
    await page.locator('.blk').first().click();
    await page.locator('.line-edit').press('End');
    await page.keyboard.type(' typed fast');
    await page.evaluate((id) => { location.hash = `#/n/${id}`; }, second.id);
    await expect(page.locator('#title')).toHaveText(second.title);
    await expect.poll(() => bodyOf(page, first.id), { timeout: 5000 }).toBe('Start typed fast');
    await page.waitForTimeout(900);
    expect(await bodyOf(page, second.id)).toBe('Leave me');
    await expect(page.locator('#doc')).toContainText('Leave me');
    await expect(page.locator('#doc')).not.toContainText('typed fast');
    await page.goBack();
    await expect(page.locator('#doc')).toContainText('Start typed fast');
  });

  test('an answer still coming when a new chat starts stays in the chat it was asked in', async ({ page }) => {
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await held;
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Late answer about heaps."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('what is a heap?');
    await chat.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(1);
    await chat.locator('[data-chat="new"]:visible').click();
    release();
    await page.waitForTimeout(600);
    await expect(chat.locator('.msg')).toHaveCount(0);
    await expect(chat).not.toContainText('Late answer about heaps.');
    await chat.locator('#chat-input').fill('a new question');
    await expect(chat.locator('#chat-input')).toHaveValue('a new question');
    const old = await (await page.request.get(`/api/chats/${asked[0].chat}`)).json();
    expect(old.messages[0].text).toBe('what is a heap?');
    expect(old.messages).toHaveLength(2);
  });

  test('a page that loads slowly never draws over the page opened after it', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Slow note ${kind()}`, body: 'Slow body' } })).json();
    await page.request.post('/api/dirs', { data: { path: `slow-${kind()}` } });
    const slow = [
      ['#/settings', '**/api/settings'],
      ['#/trash', '**/api/trash'],
      ['#/settings/keeping', '**/api/keep'],
      ['#/search/slow', '**/api/search?*'],
      [`#/f/slow-${kind()}`, '**/api/folders'],
      [`#/n/${note.id}`, `**/api/notes/${note.id}`],
    ];
    for (const [hash, api] of slow) {
      await page.goto('/');
      let release;
      const held = new Promise((resolve) => { release = resolve; });
      let seen = false;
      await page.route(api, async (route) => {
        if (seen) return route.continue();
        seen = true;
        await held;
        await route.continue().catch(() => {});
      });
      await page.evaluate((h) => { location.hash = h; }, hash);
      await expect.poll(() => seen, { message: `${hash} asked leo` }).toBe(true);
      await page.evaluate(() => { location.hash = '#/record'; });
      await expect(page.locator('#rec-title')).toBeVisible();
      release();
      await page.waitForTimeout(700);
      await expect(page.locator('#rec-title'), `${hash} finished late and must not replace Record`).toBeVisible();
      await page.unroute(api);
    }
  });

  test('each checkbox changes its own line, whatever comes before it', async ({ page }) => {
    const body = [
      '- [ ] first',
      '```',
      '- [ ] inside code',
      '```',
      '- [ ] after code',
      '> - [ ] inside a quote',
      '> [!todo]- Folded tasks',
      '> - [ ] inside a callout',
      '$$',
      'x^2',
      '$$',
      '```mermaid',
      'graph TD; A-->B',
      '```',
      '1) [ ] paren number',
      '- [X] capital done',
      '    - [ ] deep',
      '| a | b |',
      '|---|---|',
      '| - [ ] cell | 2 |',
      '- [ ] last',
    ].join('\n');
    const note = await (await page.request.post('/api/notes', { data: { title: `Boxes everywhere ${kind()}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    const boxes = page.locator('.doc input[data-box]');
    await expect(boxes.first()).toBeVisible();
    const count = await boxes.count();
    expect(count).toBeGreaterThanOrEqual(6);
    let before = body.split('\n');
    for (let i = 0; i < count; i++) {
      const box = boxes.nth(i);
      if (!(await box.isVisible())) continue;
      const label = (await box.evaluate((el) => (el.closest('li, .task-item') || el.parentElement).textContent)).trim();
      await box.click();
      await expect(page.locator('.doc textarea')).toHaveCount(0);
      let after;
      await expect.poll(async () => {
        after = (await bodyOf(page, note.id)).split('\n');
        return after.filter((l, n) => l !== before[n]).length;
      }, { timeout: 5000, message: `box ${i + 1} (${label}) changes one line` }).toBe(1);
      const changed = after.findIndex((l, n) => l !== before[n]);
      expect(after[changed], `box ${i + 1} changed the line it shows`).toContain(label.split('\n')[0].trim());
      expect(before[changed].replace(/\[[ xX]\]/, '')).toBe(after[changed].replace(/\[[ xX]\]/, ''));
      before = after;
    }
  });

  test.describe('a selection across any kind of block deletes exactly what it covers', () => {
    const kinds = {
      'a code block': ['```js', 'const a = 1;', '```'],
      'a table': ['| a | b |', '|---|---|', '| 1 | 2 |'],
      'a folded callout': ['> [!example]- Why', '> Because.'],
      'a quote': ['> quoted', '> twice'],
      'a math block': ['$$', 'e^{i\\pi} + 1 = 0', '$$'],
      'a diagram': ['```mermaid', 'graph TD; A-->B', '```'],
      'checkboxes': ['- [ ] one', '- [x] two'],
      'a heading and a rule': ['## Heading', '---'],
      'a nested list': ['1. one', '   - inner', '2. two'],
    };
    for (const [name, lines] of Object.entries(kinds)) {
      for (const backwards of [false, true]) {
        test(`${name}${backwards ? ', selected backwards' : ''}`, async ({ page }) => {
          const body = ['Start here', '', ...lines, '', 'End here'].join('\n');
          const note = await (await page.request.post('/api/notes', { data: { title: `Across ${name} ${backwards} ${kind()}`, body } })).json();
          await page.goto(`/#/n/${note.id}`);
          await expect(page.locator('#doc .blk').last()).toContainText('End here');
          await page.evaluate((back) => {
            const blocks = document.querySelectorAll('#doc .blk');
            const text = (block, word) => {
              const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
              for (let node = walker.nextNode(); node; node = walker.nextNode()) if (node.data.includes(word)) return node;
              return null;
            };
            const start = text(blocks[0], 'Start here');
            const end = text(blocks[blocks.length - 1], 'End here');
            const selection = window.getSelection();
            selection.removeAllRanges();
            if (back) selection.setBaseAndExtent(end, end.data.indexOf('End here') + 4, start, start.data.indexOf('Start here') + 5);
            else selection.setBaseAndExtent(start, start.data.indexOf('Start here') + 5, end, end.data.indexOf('End here') + 4);
            document.activeElement.blur();
          }, backwards);
          await page.keyboard.press('Backspace');
          await expect.poll(() => bodyOf(page, note.id), { timeout: 5000 }).toBe('Starthere');
        });
      }
    }
  });

  test('pressing Create twice makes one folder and no error', async ({ page }) => {
    const name = `twice-${stamp()}`;
    await page.goto('/');
    if (await page.locator('#side').isHidden()) {
      await page.locator('#menu').click();
      await page.locator(':is(.sheet, .popup-menu) [data-action="new-folder"]').click();
    } else {
      await page.locator('#side .side-group [data-action="new-folder"]').click();
    }
    await page.locator('#folder-name').fill(name);
    await page.locator('#folder-name').press('Enter');
    await page.locator('[data-action="create-folder"]').click({ force: true, timeout: 1000 }).catch(() => {});
    await expect(page).toHaveURL(/\/(?:#\/)?$/);
    await expect(page.locator('#side .side-row[data-action="open-folder"][data-dir="' + name + '"]')).toBeVisible();
    await page.waitForTimeout(500);
    await expect(page.locator('.toast.bad')).toHaveCount(0);
    const folders = await (await page.request.get('/api/folders')).json();
    expect(folders.filter((f) => f.name === name)).toHaveLength(1);
  });

  test('pressing Apply twice on a suggestion changes the note once, without an error', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Apply twice ${kind()}`, body: 'BFS uses a stack.' } })).json();
    await page.route('**/api/chat', (route) => route.fulfill({
      status: 200,
      headers: { 'content-type': 'application/x-ndjson' },
      body: [{ sources: [] }, { proposal: { kind: 'edit', note: note.id, title: note.title, find: 'stack', replace: 'queue', why: '' } }, { t: 'Fixed.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n',
    }));
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('fix it');
    await page.locator('#chat-input').press('Enter');
    const apply = page.locator('#chat .proposal [data-chat="apply"]');
    await expect(apply).toBeVisible();
    await apply.dblclick();
    await expect(page.locator('#chat .proposal-done')).toContainText('Applied');
    await page.waitForTimeout(500);
    expect(await bodyOf(page, note.id)).toBe('BFS uses a queue.');
    await expect(page.locator('.toast.bad')).toHaveCount(0);
  });

  test('every successful answer from leo is JSON, or 204 with nothing', async ({ page }) => {
    const tag = stamp();
    const answers = [];
    const call = async (method, url, data) => {
      const response = await page.request.fetch(url, { method, data });
      answers.push({ what: `${method} ${url}`, status: response.status(), type: response.headers()['content-type'] || '', text: await response.text() });
      return answers[answers.length - 1];
    };
    const json = (answer) => JSON.parse(answer.text);
    const made = json(await call('POST', '/api/notes', { title: `Contract ${tag}`, body: '- [ ] box\nold words' }));
    await call('PATCH', `/api/notes/${made.id}`, { body: '- [ ] box\nnew words' });
    await call('POST', '/api/dirs', { path: `contract-${tag}` });
    await call('POST', '/api/dirs', { path: `contract-${tag}/inner` });
    await call('POST', `/api/notes/${made.id}/move`, { directory: `contract-${tag}` });
    await call('POST', '/api/dirs', { path: `contract-moved-${tag}` });
    await call('POST', '/api/dirs/move', { from: `contract-moved-${tag}`, into: `contract-${tag}` });
    await call('POST', `/api/notes/${made.id}/suggestion`, { find: 'new', replace: 'newer' });
    await call('PUT', `/api/chats/contract-${tag}`, { title: 'Contract', messages: [{ role: 'user', text: 'hi' }] });
    for (const url of ['/api/notes', '/api/folders', `/api/search?q=contract`, '/api/trash', '/api/chats', `/api/chats/contract-${tag}`, '/api/review', '/api/activity', '/api/settings', '/api/keep', '/api/speech', '/api/record']) {
      await call('GET', url);
    }
    const trashed = json(await call('POST', '/api/trash/move', { notes: [made.id], dirs: [] }));
    await call('POST', '/api/trash/restore', { ids: trashed.ids || [made.id] });
    await call('DELETE', `/api/notes/${made.id}`);
    await call('DELETE', `/api/chats/contract-${tag}`);
    const board = json(await call('POST', '/api/boards', { name: `Contract ${tag}` }));
    const card = json(await call('POST', `/api/boards/${board.id}/cards`, { title: 'Contract card' }));
    await call('PATCH', `/api/cards/${card.id}`, { description: 'Details' });
    await call('POST', `/api/cards/${card.id}/move`, { column: board.columns[1].id, after: null });
    await call('PATCH', `/api/boards/${board.id}`, { name: `Contract ${tag} renamed` });
    await call('GET', '/api/boards');
    await call('GET', `/api/boards/${board.id}`);
    await call('DELETE', `/api/cards/${card.id}`);
    await call('DELETE', `/api/boards/${board.id}`);
    await call('POST', `/api/boards/${board.id}/undelete`);
    const gone = json(await call('DELETE', `/api/boards/${board.id}`));
    await call('POST', '/api/boards/restore', gone);
    await call('DELETE', `/api/boards/${board.id}`);

    for (const a of answers) {
      expect(a.status, `${a.what} succeeded`).toBeLessThan(300);
      if (a.status === 204) {
        expect(a.text, `${a.what} is 204, so it has no body`).toBe('');
        continue;
      }
      expect(a.type, `${a.what} says it is JSON`).toContain('application/json');
      expect(() => JSON.parse(a.text), `${a.what} answers JSON`).not.toThrow();
    }
  });
});

test.describe('Felix asks, quizzes and listens while he works', () => {
  const lines = (list) => list.map((l) => JSON.stringify(l)).join('\n') + '\n';
  const answerWith = (list) => ({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines(list) });

  async function openFelix(page) {
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    return page.locator('#chat');
  }

  test('a multiple choice question is checked at once, Felix reacts, and he is told how it went', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      const body = route.request().postDataJSON();
      asked.push(body);
      if (asked.length === 1) {
        return route.fulfill(answerWith([{ answer: 'a1' }, { sources: [{ n: 1, id: 'note-bfs', title: 'Graph traversals', folder: '', why: 'open' }] }, { step: 'Asked you a practice question', tool: 'quiz', found: [] }, { quiz: { kind: 'multiple_choice', question: 'What does BFS use?', options: ['a stack', 'a queue', 'a heap'], answer: 'a queue', explain: 'BFS takes the oldest vertex first [n1].' } }, { t: 'Give it a try!' }, { done: true }]));
      }
      return route.fulfill(answerWith([{ answer: 'a2' }, { sources: [] }, { t: '[[incorrect]] A stack gives the newest vertex; BFS needs the oldest.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('quiz me on BFS');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.quiz-card');
    await expect(card.locator('.quiz-kind')).toHaveText(/Multiple choice/i);
    await expect(card.locator('.quiz-option')).toHaveCount(3);
    await card.locator('.quiz-option', { hasText: 'a stack' }).click();
    await expect(card.locator('.quiz-option.wrong')).toContainText('a stack');
    await expect(card.locator('.quiz-option.right')).toContainText('a queue');
    await expect(card.locator('.quiz-option.wrong .quiz-mark')).toHaveAttribute('aria-label', 'Your answer, not right');
    await expect(card.locator('.quiz-option.right .quiz-mark')).toHaveAttribute('aria-label', 'The right answer');
    await expect(card.locator('.quiz-option:not(.right, .wrong) .quiz-mark')).toHaveCount(0);
    const chalk = await card.evaluate((el) => { const tone = (name) => { const probe = document.createElement('i'); probe.style.color = `var(${name})`; el.append(probe); const c = getComputedStyle(probe).color; probe.remove(); return c; }; return { good: tone('--chalk-good'), bad: tone('--chalk-bad') }; });
    await expect(card).toHaveCSS('border-top-color', chalk.bad);
    await expect(card.locator('.quiz-option.wrong')).toHaveCSS('border-top-color', chalk.bad);
    await expect(card.locator('.quiz-option.right')).toHaveCSS('border-top-color', chalk.good);
    await expect(card.locator('.quiz-option.right .quiz-mark')).toHaveCSS('stroke', chalk.good);
    await expect(card.locator('.quiz-result'), 'what is said about the answer comes from Felix, not the page').toHaveCount(0);
    await expect(card).toHaveClass(/wrong/);
    await expect(card.locator('[data-chat="quiz-hint"]'), 'no hint once it is answered').toHaveCount(0);
    await expect(card.locator('.quiz-option').first()).toBeDisabled();
    expect(asked, 'the answer is marked without waiting for Felix').toHaveLength(1);
    await card.locator('[data-chat="quiz-why"]').click();
    await expect(card.locator('.quiz-reply')).toContainText('BFS needs the oldest');
    await expect(card.locator('.quiz-reply')).not.toContainText('[[incorrect]]');
    await expect(chat.locator('.msg.user'), 'no message pretends to be the user').toHaveCount(1);
    await expect(chat.locator('.msg.leo')).toHaveCount(1);
    expect(asked[0].practice).toBe(false);
    expect(asked[1].practice, 'a card answer is marked as practice, so it is never nudged into a note change').toBe(true);
    const told = asked[1].messages.at(-1).text;
    expect(told).toContain('Question: What does BFS use?');
    expect(told, 'asking why explains this answer and stops').toContain('do not ask a new practice question');
    expect(told).toContain('My answer: a stack');
    expect(told).toContain('the right answer is a queue');
    expect(told).toContain('why each of the other choices is wrong');
    await page.reload();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat .quiz-card')).toHaveClass(/wrong/, { timeout: 5000 });
    await expect(page.locator('#chat .quiz-option').first()).toBeDisabled();
    await expect(page.locator('#chat .quiz-reply')).toContainText('BFS needs the oldest');
  });

  test('a citation with lines opens the note at those lines, and one into an uploaded file shows that passage', async ({ page }) => {
    const tag = Date.now().toString(36);
    const note = await (await page.request.post('/api/notes', { data: { title: `Cited ${tag}`, body: '# Graphs\n\n## BFS\nUses a queue.\n\n## DFS\nGoes deep first.\nUses a stack.' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'lecture.txt'), 'Week 2\nBFS explores level by level.\nDFS goes as deep as it can before it backtracks.\n');
    await page.route('**/api/chat', (route) => route.fulfill(answerWith([
      { sources: [{ n: 1, id: note.id, title: note.title, folder: '', why: 'open' }] },
      { files: [{ n: 1, note: note.id, title: note.title, name: 'lecture.txt' }] },
      { t: 'DFS goes deep first [n1 L7-8]. The lecture says "DFS goes as deep as it can before it backtracks." [f1 L3]' },
      { done: true },
    ])));
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('where does it talk about DFS?');
    await chat.locator('#chat-input').press('Enter');
    const lineChip = chat.locator('.prose .cite[data-line="7-8"]');
    await expect(lineChip).toHaveText('1');
    await expect(lineChip).toHaveAttribute('title', `Cited ${tag}, lines 7–8`);
    await expect(chat.locator('.msg-from .cite[data-line="7-8"]')).toHaveText(`1Cited ${tag} · lines 7–8`);
    const fileChip = chat.locator('.cite-file');
    await expect(fileChip).toContainText('lecture.txt');
    await expect(fileChip).toContainText('line 3');
    await fileChip.click();
    const sheet = page.locator('.sheet');
    await expect(sheet.locator('h3')).toHaveText('lecture.txt');
    await expect(sheet.locator('.passage-text')).toHaveText('DFS goes as deep as it can before it backtracks.');
    await sheet.locator('[data-action="close"]').click();
    if (!(await lineChip.isVisible())) await page.locator('#chat-toggle').click();
    await lineChip.click();
    await expect(page).toHaveURL(new RegExp(`#/n/${note.id}$`));
    const marked = page.locator('#doc .blk-cited');
    await expect(marked).toHaveCount(2);
    await expect(marked.first()).toContainText('Goes deep first.');
    await expect(marked.last()).toContainText('Uses a stack.');
  });

  test('a hint helps without giving the answer away, and the card can still be answered', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) return route.fulfill(answerWith([{ sources: [] }, { quiz: { kind: 'multiple_choice', question: 'An interior stationary point of $f(x)=x^2+4\\cos x$ satisfies?', options: ['$x = 2\\sin x$', '$x = 0$'], answer: '$x = 2\\sin x$', explain: '' } }, { t: 'Try it.' }, { done: true }]));
      if (asked.length === 2) return route.fulfill(answerWith([{ sources: [] }, { t: 'Set the derivative $f\'(x)$ to zero first.' }, { done: true }]));
      if (asked.length === 3) return route.fulfill(answerWith([{ sources: [] }, { t: 'The derivative is $2x - 4\\sin x$.' }, { done: true }]));
      return route.fulfill(answerWith([{ sources: [] }, { t: '[[correct]] Well done! Setting $2x - 4\\sin x = 0$ gives it.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('quiz me');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.quiz-card');
    await card.locator('[data-chat="quiz-hint"]').click();
    await expect(card.locator('.quiz-hint')).toContainText('Set the derivative');
    await expect(card.locator('[data-chat="quiz-hint"]')).toHaveText('Another hint');
    expect(asked[1].practice).toBe(true);
    expect(asked[1].mark, 'a hint is not a marking').toBe(false);
    expect(asked[1].messages.at(-1).text).toContain('Do not give the answer');
    await card.locator('[data-chat="quiz-hint"]').click();
    await expect(card.locator('.quiz-hint')).toHaveCount(2);
    await expect(card.locator('.quiz-hint').last()).toContainText('The derivative is');
    expect(asked[2].messages.at(-1).text).toContain('Hints you already gave me');
    await expect(card.locator('.quiz-option').first()).toBeEnabled();
    await card.locator('.quiz-option').first().click();
    await expect(card).toHaveClass(/right/);
    await expect(card.locator('[data-chat="quiz-next"]')).toBeVisible();
    expect(asked.length, 'a multiple choice answer is marked at once, without Felix').toBe(3);
    await card.locator('[data-chat="quiz-why"]').click();
    await expect(card.locator('.quiz-reply')).toContainText('Well done!');
    await expect(card.locator('.quiz-hint').first()).toContainText('Set the derivative');
    await expect(card.locator('[data-chat="quiz-hint"]'), 'no more hints once answered').toHaveCount(0);
    expect(asked[3].messages.at(-1).text).toContain('praise me briefly');
    expect(asked[3].messages.at(-1).text, 'asking why does not jump to the next question').toContain('do not ask a new practice question');
    await expect(card.locator('[data-chat="quiz-why"]')).toHaveCount(0);
    await expect(card.locator('[data-chat="quiz-next"]'), 'the explanation leaves Next question for when they are ready').toBeVisible();
  });

  test('a question typed about a marked answer is answered without moving on', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) return route.fulfill(answerWith([{ sources: [] }, { quiz: { kind: 'multiple_choice', question: 'Which structure does BFS use?', options: ['a queue', 'a stack'], answer: 'a queue', explain: 'Oldest first.' } }, { done: true }]));
      if (asked.length === 2) return route.fulfill(answerWith([{ sources: [] }, { t: 'A queue hands back the oldest vertex, so levels come out in order.' }, { done: true }]));
      return route.fulfill(answerWith([{ sources: [] }, { quiz: { kind: 'free_response', question: 'Why does BFS find shortest paths?', options: [], answer: 'Level by level.', explain: '' } }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('quiz me');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.quiz-card').first();
    await card.locator('.quiz-option', { hasText: 'a queue' }).click();
    await expect(card.locator('[data-chat="quiz-next"]')).toBeVisible();
    await chat.locator('#chat-input').fill('why a queue and not a stack?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('levels come out in order');
    const said = asked[1].messages.at(-1).text;
    expect(said).toContain('leo marked it right');
    expect(said).toContain('Do not ask a new practice question unless I ask for one');
    expect(said).toContain('why a queue and not a stack?');
    await expect(chat.locator('.quiz-card'), 'no new question until they ask').toHaveCount(1);
    await card.locator('[data-chat="quiz-next"]').click();
    await expect(chat.locator('.quiz-card')).toHaveCount(2);
    const next = asked[2].messages.at(-1).text;
    expect(next).toContain('Next question, please.');
    expect(next, 'the result is not told twice').not.toContain('leo marked it');
    await expect(chat.locator('.msg-next'), 'Next question is a slim divider, not a chat bubble').toHaveCount(1);
    await expect(chat.locator('.msg.user .bubble', { hasText: 'Next question' })).toHaveCount(0);
  });

  test('a fill in the blank and a free answer are marked quickly for what was meant, and Felix explains only when asked', async ({ page }) => {
    const asked = [];
    const marks = [];
    await page.route('**/api/quiz/mark', (route) => { marks.push(route.request().postDataJSON()); return route.fulfill({ json: { correct: true, feedback: marks.length === 1 ? 'Yes: the earliest vertex.' : 'Right: level by level.' } }); });
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const n = asked.length;
      if (n === 1) return route.fulfill(answerWith([{ sources: [] }, { quiz: { kind: 'fill_blank', question: 'BFS takes the ___ vertex first, in $O(V+E)$ time.', options: [], answer: 'oldest | earliest', explain: '' } }, { t: 'Fill it in.' }, { done: true }]));
      if (n === 2) return route.fulfill(answerWith([{ sources: [] }, { t: '[[correct]] Yes.' }, { quiz: { kind: 'free_response', question: 'Why does BFS find shortest paths?', options: [], answer: 'It explores level by level.', explain: '' } }, { done: true }]));
      return route.fulfill(answerWith([{ sources: [] }, { t: '[[correct]] Right: level by level.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('practice');
    await chat.locator('#chat-input').press('Enter');
    const blank = chat.locator('.quiz-card').first();
    await expect(blank.locator('.quiz-blank')).toHaveCount(1);
    await expect(blank.locator('.quiz-q .math[data-drawn="yes"] .katex'), 'math in a question is drawn').toBeVisible();
    const box = blank.locator('.quiz-input');
    await box.pressSequentially('the earliest one', { delay: 20 });
    await box.press('Enter');
    await expect(blank).toHaveClass(/right/);
    await expect(blank.locator('.quiz-explain')).toContainText('Yes: the earliest vertex.');
    expect(marks[0]).toMatchObject({ given: 'the earliest one', answer: 'oldest | earliest' });
    expect(asked, 'no Felix reply is needed to mark it').toHaveLength(1);
    await blank.locator('[data-chat="quiz-next"]').click();
    await expect(chat.locator('.quiz-card')).toHaveCount(2);
    expect(asked[1].messages.at(-1).text).toContain('leo marked it right');
    expect(asked[1].messages.at(-1).text).toContain('Ask the next question with the quiz tool');
    const free = chat.locator('.quiz-card').nth(1);
    await expect(free.locator('textarea.quiz-input')).toBeEnabled();
    await free.locator('textarea.quiz-input').fill('Because it goes out one level at a time.');
    await free.locator('[data-chat="quiz-check"]').click();
    await expect(free.locator('textarea.quiz-input')).toBeDisabled();
    await expect(free).toHaveClass(/right/);
    await expect(free.locator('.quiz-verdict')).toHaveText('Right');
    await expect(free.locator('.quiz-explain')).toContainText('Right: level by level.');
    expect(asked).toHaveLength(2);
    await free.locator('[data-chat="quiz-why"]').click();
    await expect(free.locator('.quiz-reply')).toContainText('Right: level by level.');
    expect(asked[2].messages.at(-1).text).toContain('leo checked it: right');
  });

  test('Felix asks which one, and a tap on a choice answers him', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) return route.fulfill(answerWith([{ sources: [] }, { step: 'Asked you a question', tool: 'ask_user', found: [] }, { ask: { question: 'Which week should I cover?', options: ['Week 1', 'Week 2'] } }, { t: 'Tell me which week.' }, { done: true }]));
      return route.fulfill(answerWith([{ sources: [] }, { t: 'Week 2 covers heaps.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('summarise the lecture');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.ask-card');
    await expect(card.locator('.ask-q')).toHaveText('Which week should I cover?');
    await card.locator('.ask-option', { hasText: 'Week 2' }).click();
    await expect(chat.locator('.msg.leo').last()).toContainText('Week 2 covers heaps.');
    expect(asked[1].messages.at(-1).text).toBe('Week 2');
    await expect(chat.locator('.ask-option.chosen')).toHaveText('Week 2');
    await expect(chat.locator('.ask-option').first()).toBeDisabled();
    await expect(chat.locator('.ask-hint')).toHaveText('You answered: Week 2');
  });

  test('a message sent while Felix works reaches him, and one too late is asked next', async ({ page }) => {
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    const steers = [];
    const asked = [];
    await page.route('**/api/chat/answer-1/steer', async (route) => {
      steers.push(route.request().postDataJSON().text);
      await route.fulfill({ status: 202, json: { waiting: steers.length } });
    });
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) {
        await held;
        return route.fulfill(answerWith([{ answer: 'answer-1' }, { sources: [] }, { step: 'Searched your notes for “heap”', tool: 'search_notes', found: [] }, { steered: ['only the min-heap part'] }, { t: 'A min-heap keeps the smallest on top.' }, { done: true }]));
      }
      return route.fulfill(answerWith([{ answer: 'answer-2' }, { sources: [] }, { t: 'And in Python, heapq.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    const input = chat.locator('#chat-input');
    await input.fill('explain heaps');
    await input.press('Enter');
    await expect(chat.locator('.msg.leo.pending')).toBeVisible();
    await expect(input).toHaveAttribute('placeholder', 'Add to what Felix is doing…');
    await input.fill('only the min-heap part');
    await expect(chat.locator('#chat-send')).not.toHaveClass(/stop/);
    await input.press('Enter');
    await expect(chat.locator('.msg.user.queued')).toContainText('only the min-heap part');
    await expect(chat.locator('.msg.user.queued .msg-note')).toHaveText('Felix reads this at his next step');
    await input.fill('and in Python?');
    await chat.locator('#chat-send').click();
    await expect(chat.locator('.msg.user.queued')).toHaveCount(2);
    await expect(chat.locator('#chat-send')).toHaveClass(/stop/);
    release();
    await expect.poll(() => steers).toEqual(['only the min-heap part', 'and in Python?']);
    await expect(chat.locator('.msg.leo').last()).toContainText('And in Python, heapq.');
    const texts = await chat.locator('.msg').evaluateAll((all) => all.map((m) => (m.classList.contains('user') ? 'U: ' : 'F: ') + m.querySelector('.bubble, .prose').textContent.trim()));
    expect(texts).toEqual(['U: explain heaps', 'U: only the min-heap part', 'F: A min-heap keeps the smallest on top.', 'U: and in Python?', 'F: And in Python, heapq.']);
    await expect(chat.locator('.msg-note', { hasText: 'Sent while Felix worked' })).toHaveCount(1);
    expect(asked[1].messages.map((m) => m.text.split('\n').pop())).toEqual(['explain heaps', 'only the min-heap part', 'A min-heap keeps the smallest on top.', 'and in Python?']);
    expect(asked[1].messages[2].text).toContain('[What Felix did for this answer: Searched your notes for “heap”]');
  });

  test('a broken first try leaves no trace, and a verdict from leo colours a free answer', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) {
        return route.fulfill(answerWith([{ sources: [] }, { t: 'Half an answer' }, { quiz: { kind: 'free_response', question: 'Broken card?', options: [], answer: 'x', explain: '' } }, { reset: true }, { quiz: { kind: 'free_response', question: 'Why compare the endpoints?', options: [], answer: 'A minimum can sit at an end.', explain: '' } }, { t: 'Try the card below.' }, { done: true }]));
      }
      return route.fulfill(answerWith([{ sources: [] }, { t: 'Close, but you left out the ends.' }, { verdict: 'incorrect' }, { done: true }]));
    });
    await page.route('**/api/quiz/mark', (route) => route.fulfill({ status: 503, json: { error: 'No quick model in this test.' } }));
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('quiz me');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Try the card below.');
    await expect(chat.locator('.msg.leo').last()).not.toContainText('Half an answer');
    await expect(chat.locator('.quiz-card')).toHaveCount(1);
    await expect(chat.locator('.quiz-q')).toHaveText('Why compare the endpoints?');
    await chat.locator('textarea.quiz-input').fill('no idea');
    await chat.locator('[data-chat="quiz-check"]').click();
    await expect(chat.locator('.quiz-card')).toHaveClass(/wrong/);
    await expect(chat.locator('.quiz-reply')).toContainText('left out the ends');
    expect(asked[1].mark).toBe(true);
  });

  test('Esc stops Felix while he answers, and closes the chat only when he is idle', async ({ page }) => {
    await page.route('**/api/chat', async () => {});
    const chat = await openFelix(page);
    const input = chat.locator('#chat-input');
    await input.fill('explain heaps');
    await input.press('Enter');
    await expect(chat.locator('.msg.leo.pending')).toBeVisible();
    await input.press('Escape');
    await expect(chat.locator('.msg-error')).toHaveText('Stopped.');
    await expect(chat, 'the chat stays open').toBeVisible();
    await input.press('Escape');
    await expect(chat).toBeHidden();
  });

  test('stopping Felix puts messages he never read back in the box', async ({ page }) => {
    await page.route('**/api/chat', async () => {});
    await page.route('**/api/chat/*/steer', (route) => route.fulfill({ status: 410, json: { error: 'That answer has finished.' } }));
    const chat = await openFelix(page);
    const input = chat.locator('#chat-input');
    await input.fill('explain heaps');
    await input.press('Enter');
    await input.fill('shorter please');
    await input.press('Enter');
    await expect(chat.locator('.msg.user.queued')).toHaveCount(1);
    await chat.locator('#chat-send').click();
    await expect(chat.locator('.msg.user.queued')).toHaveCount(0);
    await expect(input).toHaveValue('shorter please');
    await expect(chat.locator('.msg-error')).toHaveText('Stopped.');
  });
});

test.describe('switching models mid-chat', () => {
  test('the next model is told what the last one looked up, and the switch is shown', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const first = asked.length === 1;
      const lines = first
        ? [{ sources: [{ n: 1, id: 'x1', title: 'Graph traversals', folder: '', why: 'opened by Felix' }] }, { step: 'Searched your notes for “queue”', tool: 'search_notes', found: ['Graph traversals'] }, { t: 'BFS uses a queue.' }, { spent: { by: 'Codex', model: 'gpt-6.1-sol', plan: true, steps: 2 } }, { done: true }]
        : [{ sources: [] }, { t: 'Building on that: oldest first.' }, { spent: { by: 'Claude Code', model: 'claude-opus-5-5', plan: true, steps: 1 } }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('what does bfs use?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('BFS uses a queue.');
    await chat.locator('#chat-input').fill('why?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Building on that');
    const before = asked[1].messages[1].text;
    expect(before).toContain('[Written by Codex · gpt-6.1-sol]');
    expect(before).toContain('[What Felix did for this answer: Searched your notes for “queue” (found: Graph traversals)]');
    expect(before.endsWith('BFS uses a queue.')).toBe(true);
    expect(asked[1].recent).toEqual(['x1']);
    await expect(chat.locator('.chat-switch')).toHaveText('Now answered by Claude Code · claude-opus-5-5, with everything said so far');
  });
});

test.describe('past chats', () => {
  test('chats are found by what was said in them, and opening one does not reorder them', async ({ page }) => {
    const tag = `${test.info().project.name}${Date.now().toString(36)}`;
    const put = (id, text, title) => page.request.put(`/api/chats/${id}`, { data: { title, messages: [{ role: 'user', text }, { role: 'assistant', text: 'ok' }] } });
    await put(`chat-dij-${tag}`, `how does dijkstra${tag} relax edges`, `Shortest paths ${tag}`);
    await put(`chat-heap-${tag}`, `what is a heap${tag}`, `Heaps ${tag}`);
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const history = chat.locator('#chat-history');
    if (!(await history.isVisible())) await chat.locator('.chat-head [data-chat="history"]').click();
    const search = chat.locator('#chat-history-search');
    await search.pressSequentially(`dijkstra${tag}`, { delay: 15 });
    await expect(chat.locator('.chat-history-item')).toHaveCount(1);
    await expect(chat.locator('.chat-history-title')).toHaveText(`Shortest paths ${tag}`);
    await expect(search).toBeFocused();
    await search.fill(`nothing${tag}`);
    await expect(chat.locator('.chat-history-none')).toHaveText('No chat mentions that.');
    await search.fill('');
    await expect(chat.locator('.chat-history-title').first()).toHaveText(`Heaps ${tag}`);
    await chat.locator('.chat-history-item', { hasText: `Shortest paths ${tag}` }).click();
    await expect.poll(async () => (await (await page.request.get('/api/chats')).json()).filter((c) => c.id.endsWith(tag)).map((c) => c.id)).toEqual([`chat-heap-${tag}`, `chat-dij-${tag}`]);
  });
});







test('Settings chooses light or dark for this browser, or follows the device', async ({ page }) => {
  const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/#/settings');
  const pick = (name) => page.locator(`[data-action="theme-pick"][data-theme="${name}"]`);
  await expect(pick('system')).toHaveAttribute('aria-pressed', 'true');
  const light = await background();
  await pick('dark').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(pick('dark')).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(background).not.toBe(light);
  const dark = await background();
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(await background()).toBe(dark);
  await expect(pick('dark')).toHaveAttribute('aria-pressed', 'true');
  await page.emulateMedia({ colorScheme: 'dark' });
  await pick('light').click();
  await expect.poll(background).toBe(light);
  await pick('system').click();
  await expect(page.locator('html')).not.toHaveAttribute('data-theme', /./);
  await expect.poll(background).toBe(dark);
  expect(await page.evaluate(() => localStorage.getItem('leo-theme'))).toBeNull();
  await page.emulateMedia({ colorScheme: 'light' });
  await expect.poll(background).toBe(light);
});

test('Settings changes Quick Leo and New note keys, refuses reserved and taken ones, and resets', async ({ page }) => {
  await page.goto('/#/settings/shortcuts');
  const mac = await page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent));
  const label = (key) => (mac ? `⌘${key}` : `Ctrl+${key}`);
  const row = (name) => page.locator(`[data-find="shortcut-${name}"]`);
  const change = (name) => row(name).locator('[data-action="shortcut-record"]');
  const hint = (name) => row(name).locator('.hint');
  const search = page.locator('[data-action="quick-leo"]');
  const quick = page.locator('.scrim:not(.scrim-gone) .quick-leo');
  await expect(search).toHaveAttribute('title', `Search or ask (${label('K')})`);

  await change('quick').click();
  await expect(row('quick').locator('.shortcut-field')).toHaveClass(/recording/);
  await page.keyboard.press('Escape');
  await expect(row('quick').locator('.shortcut-field')).not.toHaveClass(/recording/);
  await expect(row('quick').getByRole('button', { name: 'Reset' })).toHaveCount(0);

  await change('quick').click();
  await page.keyboard.press('ControlOrMeta+b');
  await expect(hint('quick')).toContainText('reserved for bold');
  await expect(hint('quick')).toHaveClass(/bad/);
  await change('quick').click();
  await page.keyboard.press('ControlOrMeta+n');
  await expect(hint('quick')).toHaveText('Already used by New note.');
  await change('quick').click();
  await page.keyboard.press('j');
  await expect(hint('quick')).toHaveText(`Use ${mac ? '⌘' : 'Ctrl'} with a key.`);
  await expect(search).toHaveAttribute('title', `Search or ask (${label('K')})`);

  await change('quick').click();
  await page.keyboard.press('ControlOrMeta+j');
  await expect(hint('quick')).toHaveText('Saved');
  await expect(search).toHaveAttribute('title', `Search or ask (${label('J')})`);
  await page.keyboard.press('ControlOrMeta+k');
  await page.waitForTimeout(300);
  await expect(page.locator('.quick-leo'), 'the old keys no longer open Quick Leo').toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+j');
  await expect(quick).toBeVisible();
  await expect(quick.locator('.quick-bar kbd')).toHaveText(label('J'));
  await page.keyboard.press('Escape');
  await expect(page.locator('.scrim:not(.scrim-gone)')).toHaveCount(0);

  await change('new').click();
  await page.keyboard.press('ControlOrMeta+Shift+m');
  await expect(hint('new')).toHaveText('Saved');
  await expect(page.locator('.side-new')).toHaveAttribute('aria-keyshortcuts', `${mac ? 'Meta' : 'Control'}+Shift+M`);
  await page.keyboard.press('ControlOrMeta+n');
  await page.waitForTimeout(300);
  await expect(page).toHaveURL(/#\/settings\/shortcuts$/);
  await page.keyboard.press('ControlOrMeta+Shift+m');
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#/new');
  await page.goto('/#/settings/shortcuts');
  await expect(row('new').locator('.shortcut-field')).toContainText('M');

  await row('quick').getByRole('button', { name: 'Reset' }).click();
  await expect(row('quick').getByRole('button', { name: 'Reset' })).toHaveCount(0);
  await expect(search).toHaveAttribute('title', `Search or ask (${label('K')})`);
  await page.keyboard.press('ControlOrMeta+k');
  await expect(quick).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.scrim:not(.scrim-gone)')).toHaveCount(0);
});

test('a calendar is connected in Settings by pasting one link, and can be removed', async ({ page }) => {
  let linked = [];
  let posted = null;
  await page.route('**/api/calendar', async (route) => {
    if (route.request().method() === 'POST') {
      posted = route.request().postDataJSON();
      if (!posted.link.includes('calendar.google.com')) return route.fulfill({ status: 400, json: { error: 'That link does not lead to a calendar. In Google Calendar, copy “Secret address in iCal format”.' } });
      linked = [{ id: 'c1', name: 'Classes', added_at: new Date().toISOString(), problem: null }];
    }
    return route.fulfill({ json: { connected: linked.length > 0, calendars: linked, events: linked.length ? [{ id: 'e', title: 'Algorithms lecture', start: new Date(Date.now() + 3600000).toISOString(), end: new Date(Date.now() + 7200000).toISOString(), context: '' }] : [], refreshing: false } });
  });
  await page.route('**/api/calendar/c1', (route) => {
    linked = [];
    return route.fulfill({ json: { connected: false, calendars: [], events: [] } });
  });
  await page.goto('/#/settings/calendar');
  const card = page.locator('.cal-card');
  await expect(card.locator('h3')).toHaveText('Calendar');
  await expect(card).toContainText('Not connected');
  await expect(card.locator('#calendar-link')).toBeVisible();
  await expect(card.locator('a[href="https://calendar.google.com/calendar/r/settings"]')).toBeVisible();
  await card.locator('#calendar-link').fill('https://example.com/nope');
  await card.locator('[data-action="calendar-add"]').click();
  await expect(page.locator('.toast.bad')).toContainText('Secret address in iCal format');
  await expect(card.locator('#calendar-link')).toHaveValue('https://example.com/nope');
  await card.locator('#calendar-link').fill('https://calendar.google.com/calendar/ical/me/private-abc/basic.ics');
  await card.locator('[data-action="calendar-add"]').click();
  await expect(page.locator('.cal-card')).toContainText('Classes');
  await expect(page.locator('.cal-card .cal-icon')).toHaveCount(1);
  await expect(page.locator('.cal-card')).not.toContainText('Next up');
  await expect(page.locator('#calendar-link')).toHaveCount(0);
  await page.locator('.cal-card [data-action="calendar-more"]').click();
  await expect(page.locator('#calendar-link')).toBeFocused();
  await page.locator('.cal-card [data-action="calendar-cancel"]').click();
  await expect(page.locator('#calendar-link')).toHaveCount(0);
  expect(posted.link).toContain('private-abc');
  await page.locator('.cal-card [data-action="calendar-remove"]').click();
  await expect(page.locator('.cal-card')).toContainText('Not connected');
});

test('dropping one note on another combines them in the background with progress, then shows the result first, and Undo brings both back', async ({ page }) => {
  const tag = Date.now().toString(36);
  const folder = `combine-${tag}`;
  await page.request.post('/api/dirs', { data: { path: folder } });
  const a = await (await page.request.post('/api/notes', { data: { title: `BFS ${tag}`, body: 'BFS uses a queue.', directory: folder } })).json();
  const b = await (await page.request.post('/api/notes', { data: { title: `Graphs ${tag}`, body: 'A graph has nodes and edges.', directory: folder } })).json();
  const versions = async (id) => (await (await page.request.get(`/api/notes/${id}`)).json()).version;
  const job = { id: `job-${tag}`, into: b.id, with: a.id, into_title: b.title, with_title: a.title, state: 'working', parts: 3 };
  let percent = 30;
  let forgotten = false;
  await page.route(`**/api/notes/${b.id}/combine`, (route) => {
    expect(route.request().postDataJSON()).toEqual({ with: a.id });
    return route.fulfill({ json: job });
  });
  await page.route(`**/api/combines/${job.id}`, (route) => {
    if (route.request().method() === 'DELETE') { forgotten = true; return route.fulfill({ status: 204 }); }
    return route.fulfill({ json: job });
  });
  await page.route('**/api/activity', (route) => route.fulfill({ json: { tasks: forgotten ? [] : [{ kind: 'combine', label: `Combining “${a.title}” into “${b.title}”`, step: job.state === 'working' ? 'Felix is writing one note' : 'Ready to review', done: job.state === 'working' ? percent : 100, total: 100, href: `#combine/${job.id}`, ...(job.state === 'working' ? {} : { ready: true }) }] } }));
  await page.goto('/#/search/' + tag);
  await dragBetween(page, page.locator(`.card[data-id="${a.id}"]`), page.locator(`.card[data-id="${b.id}"]`));
  const sheet = page.locator('.sheet');
  await expect(sheet.locator('h3')).toHaveText('Combine these notes?');
  await expect(sheet).toContainText(`“${a.title}” will be merged into “${b.title}”.`);
  await sheet.locator('#combine-go').click();
  await expect(sheet).toHaveCount(0);
  await expect(page.locator('.toast')).toContainText('Combining in the background');
  const panel = page.locator('#activity');
  await expect(panel).toContainText('Felix is writing one note · 30%');
  await expect(panel.locator('.activity-bar i')).toHaveAttribute('style', /width:\s*(3\d)%/);
  await page.goto('/#/');
  percent = 70;
  await expect(panel).toContainText('· 70%');
  Object.assign(job, { state: 'ready', result: { title: b.title, with_title: a.title, body: '## Graphs\nA graph has nodes and edges.\n\n## BFS\nBFS uses a queue.', added: ['![Tree](tree.png)'], kept: 0.7, kept_enough: false, base: await versions(b.id), with_base: await versions(a.id) } });
  await expect(page.locator('.toast')).toContainText('are ready to review');
  await expect(panel).toContainText('Ready to review');
  await expect(panel).toContainText('Ready in the background');
  await panel.locator('.activity-row').click();
  const review = page.locator('.sheet');
  await expect(review.locator('.combine-preview')).toContainText('BFS uses a queue.');
  await expect(review).toContainText('put back one thing');
  await expect(review.locator('.set-note.warn')).toContainText('about 70%');
  await review.locator('#combine-save').click();
  await expect(page.locator('.toast')).toContainText(`Combined into “${b.title}”`);
  await expect.poll(() => forgotten).toBe(true);
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toContain('BFS uses a queue.');
  await page.locator('.toast button', { hasText: 'Undo' }).click();
  await expect(page.locator('.toast')).toContainText('Both notes are back');
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toBe('A graph has nodes and edges.');
});

test('a combine of many requests names the count, shows its part, and Stop deletes it and says neither note changed', async ({ page }) => {
  const tag = Date.now().toString(36);
  const a = await (await page.request.post('/api/notes', { data: { title: `Long A ${tag}`, body: 'First note.' } })).json();
  const b = await (await page.request.post('/api/notes', { data: { title: `Long B ${tag}`, body: 'Second note.' } })).json();
  const job = { id: `long-${tag}`, into: b.id, with: a.id, into_title: b.title, with_title: a.title, state: 'working', parts: 313 };
  let done = 0;
  let stopped = false;
  await page.route(`**/api/notes/${b.id}/combine`, (route) => route.fulfill({ json: job }));
  await page.route(`**/api/combines/${job.id}`, (route) => {
    if (route.request().method() === 'DELETE') { stopped = true; return route.fulfill({ status: 204 }); }
    return route.fulfill({ json: stopped ? { error: 'gone' } : { ...job, done }, status: stopped ? 404 : 200 });
  });
  await page.route('**/api/activity', (route) => route.fulfill({ json: { tasks: [] } }));
  await page.goto('/#/search/' + tag);
  await dragBetween(page, page.locator(`.card[data-id="${a.id}"]`), page.locator(`.card[data-id="${b.id}"]`));
  await page.locator('.sheet #combine-go').click();
  const sheet = page.locator('.sheet');
  await expect(sheet.locator('h3')).toHaveText('This combine takes 313 requests to the AI');
  await expect(sheet.locator('#combine-part')).toHaveText('Part 1 of 313');
  done = 41;
  await expect(sheet.locator('#combine-part')).toHaveText('Part 42 of 313', { timeout: 6000 });
  await sheet.locator('#combine-stop').click();
  await expect(sheet).toHaveCount(0);
  await expect(page.locator('.toast')).toHaveText('Combining was stopped. Neither note was changed.');
  expect(stopped).toBe(true);
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toBe('Second note.');
  expect((await (await page.request.get(`/api/notes/${a.id}`)).json()).body).toBe('First note.');
});

test('Combine into… reaches the 51st of many same-titled notes in different folders through Show more', async ({ page }) => {
  test.skip(test.info().project.name === 'phone', 'the explorer is beside the page on wider screens');
  const tag = Date.now().toString(36);
  const title = `Twin ${tag}`;
  const folders = Array.from({ length: 120 }, (_, i) => `twin-${tag}-${i}`);
  for (let i = 0; i < folders.length; i += 20) {
    await Promise.all(folders.slice(i, i + 20).map(async (folder) => {
      await page.request.post('/api/dirs', { data: { path: folder } });
      await page.request.post('/api/notes', { data: { title, body: 'x', directory: folder } });
    }));
  }
  const mine = await (await page.request.post('/api/notes', { data: { title: `Mine ${tag}`, body: 'x' } })).json();
  await page.goto(`/#/n/${mine.id}`);
  const row = page.locator(`#side .side-note[data-id="${mine.id}"]`);
  await expect(row).toBeVisible();
  await row.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Combine into…' }).click();
  const picker = page.getByRole('menu', { name: `Combine “${mine.title}” into` });
  await picker.locator('input').fill(title);
  const twins = picker.getByRole('menuitem', { name: new RegExp(`^${title} · twin-${tag}-`) });
  await expect(twins).toHaveCount(50);
  const more = picker.getByRole('menuitem', { name: 'Show more' });
  while (await more.count()) {
    const before = await twins.count();
    await more.click();
    await expect(twins).not.toHaveCount(before);
  }
  await expect(twins).toHaveCount(120);
  await picker.getByRole('menuitem', { name: `${title} · twin-${tag}-119`, exact: true }).click();
  const sheet = page.locator('.scrim:not(.scrim-gone) .sheet');
  await expect(sheet.locator('h3')).toHaveText('Combine these notes?');
  await sheet.locator('[data-action="close"]').click();
});

test('Combine into… in the explorer menu and a “merge” line in Quick Leo pick the other note from a list', async ({ page }) => {
  test.skip(test.info().project.name === 'phone', 'the explorer is beside the page on wider screens');
  const tag = Date.now().toString(36);
  const folder = `pick-${tag}`;
  await page.request.post('/api/dirs', { data: { path: folder } });
  const make = async (title) => (await page.request.post('/api/notes', { data: { title, body: 'x', directory: folder } })).json();
  const a = await make(`Pick A ${tag}`);
  const b = await make(`Pick B ${tag}`);
  await page.goto(`/#/n/${a.id}`);
  const row = page.locator(`#side .side-note[data-id="${a.id}"]`);
  await expect(row).toBeVisible();
  await row.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Combine into…' }).click();
  const picker = page.getByRole('menu', { name: `Combine “${a.title}” into` });
  await expect(picker.locator('input')).toBeFocused();
  await picker.locator('input').fill(b.title);
  await expect(picker.getByRole('menuitem', { name: b.title })).toBeVisible();
  await picker.getByRole('menuitem', { name: b.title }).click();
  const sheet = page.locator('.scrim:not(.scrim-gone) .sheet');
  await expect(sheet.locator('h3')).toHaveText('Combine these notes?');
  await expect(sheet).toContainText(`“${a.title}” will be merged into “${b.title}”.`);
  await expect(sheet).toContainText('may not be fully accurate');
  await sheet.locator('[data-action="close"]').click();
  await expect(page.locator('.scrim:not(.scrim-gone)')).toHaveCount(0);

  await page.keyboard.press('ControlOrMeta+k');
  const box = page.locator('.scrim:not(.scrim-gone) .quick-leo');
  const input = box.locator('#quick-input');
  await input.fill('merge');
  const first = box.locator('.quick-row.on');
  await expect(first).toContainText(`Combine “${a.title}” into…`);
  await expect(first).toContainText('Combine notes');
  await input.press('Enter');
  await expect(box).toHaveCount(0);
  await expect(picker.locator('input')).toBeFocused();
  await picker.locator('input').fill(b.title);
  await picker.getByRole('menuitem', { name: b.title }).click();
  await expect(sheet.locator('h3')).toHaveText('Combine these notes?');
  await sheet.locator('[data-action="close"]').click();

  await page.goto('/');
  await page.keyboard.press('ControlOrMeta+k');
  await input.fill('combine');
  await expect(first).toContainText('Open a note to combine it');
  await input.press('Enter');
  await expect(box).toBeVisible();
  await expect(page.getByRole('menu')).toHaveCount(0);
});

test('PDF export leaves out the tasks panel and every overlay, with the same full-width page on any screen', async ({ page }) => {
  test.skip(test.info().project.name === 'phone', 'the tasks panel is for wider windows');
  const tag = Date.now().toString(36);
  const note = await (await page.request.post('/api/notes', { data: { title: `Print ${tag}`, body: 'A paragraph to print.' } })).json();
  const task = await (await page.request.post('/api/tasks', { data: { title: `Print task ${tag}` } })).json();
  const covering = () => page.evaluate(() => [...document.body.querySelectorAll('*')].filter((el) => {
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return ['fixed', 'sticky'].includes(s.position) && s.display !== 'none' && s.visibility !== 'hidden' && r.width > 0 && r.height > 0;
  }).map((el) => el.id || String(el.className)));
  const layout = () => page.evaluate(() => {
    const main = document.querySelector('main');
    const body = getComputedStyle(document.body);
    return { left: Math.round(main.getBoundingClientRect().left), width: Math.round(main.getBoundingClientRect().width), page: document.documentElement.clientWidth, padding: [body.paddingLeft, body.paddingRight, getComputedStyle(main).paddingLeft] };
  });
  for (const width of [1440, 1100, 760]) {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ media: 'screen' });
    await page.goto(`/#/n/${note.id}`);
    await page.reload();
    await expect(page.locator('#title, .note-head h1').first()).toContainText(`Print ${tag}`);
    await expect(page.locator('#tasks-window'), 'note pages carry the Tasks panel').toBeVisible();
    await page.emulateMedia({ media: 'print' });
    expect(await covering(), `${width}px`).toEqual([]);
    const shape = await layout();
    expect(shape.padding, `${width}px`).toEqual(['0px', '0px', '0px']);
    expect(shape.left, `${width}px`).toBe(0);
    expect(shape.width, `${width}px`).toBe(shape.page);
  }
  await page.request.delete(`/api/tasks/${task.id}`);
});

test('only the folders scroll in the sidebar; the places above and below stay put', async ({ page }) => {
  test.skip(test.info().project.name === 'phone', 'the sidebar is for wide windows');
  const tag = Date.now().toString(36);
  for (let i = 0; i < 40; i++) await page.request.post('/api/dirs', { data: { path: `scroll-${tag}-${String(i).padStart(2, '0')}` } });
  await page.setViewportSize({ width: 1280, height: 640 });
  await page.reload();
  const tree = page.locator('#side .side-tree');
  await expect(tree).toBeVisible();
  expect((await tree.boundingBox()).height).toBeGreaterThanOrEqual(100);
  const settings = page.locator('#side .side-foot [data-action="settings"]');
  const record = page.locator('#side [data-action="record"]');
  const before = [await settings.boundingBox(), await record.boundingBox()];
  expect(await tree.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
  await tree.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await expect(page.locator('#side .side-group[data-drop-dir=""]')).toBeInViewport();
  await page.mouse.wheel(0, 2000);
  const after = [await settings.boundingBox(), await record.boundingBox()];
  expect(after.map((b) => Math.round(b.y))).toEqual(before.map((b) => Math.round(b.y)));
  expect(before[0].y + before[0].height).toBeLessThanOrEqual(640);
  const scrolled = await tree.evaluate((el) => el.scrollTop);
  expect(scrolled).toBeGreaterThan(0);
  await page.locator('#side [data-action="home"]').first().click();
  await expect.poll(() => tree.evaluate((el) => el.scrollTop)).toBe(scrolled);
});

test('Search cards remain read only and open notes directly', async ({ page }) => {
  const tag = 'search-card-' + Date.now();
  const note = await (await page.request.post('/api/notes', { data: { title: tag, body: '- [ ] A task stays unticked' } })).json();
  await page.goto('/#/search/' + tag);
  const card = page.locator('.card[data-id="' + note.id + '"]');
  await expect(card).toBeVisible();
  await expect(card.locator('input')).toHaveCount(0);
  await card.click();
  await expect(page).toHaveURL(new RegExp('#/n/' + note.id + '$'));
  expect((await (await page.request.get('/api/notes/' + note.id)).json()).body).toContain('- [ ]');
});





test('sidebar folders can move to the top level with their contents and Undo', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'sidebar dragging needs a mouse');
  const tag = Date.now().toString(36);
  const from = `sidebar-root-${tag}/Nested-${tag}`;
  const root = from.split('/').pop();
  const note = await (await page.request.post('/api/notes', { data: { title: 'Nested folder move', directory: `${from}/Child`, body: 'Keep descendants.' } })).json();
  const where = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).directory;
  await page.goto(`/#/f/${encodeURIComponent(from)}`);
  const target = page.locator('#side .side-group[data-drop-dir=""]');
  await expect(target).toContainText('New note');
  await dragBetween(page, page.locator(`#side [data-action="open-folder"][data-dir="${from}"]`), target);
  await expect.poll(where).toBe(`${root}/Child`);
  await expect(page).toHaveURL(/\/(?:#\/)?$/);
  await expect(page.locator(`#side [data-action="open-folder"][data-dir="${root}"]`)).toBeVisible();
  await page.locator('.toast button', { hasText: 'Undo' }).last().click();
  await expect.poll(where).toBe(`${from}/Child`);
  await expect(page).toHaveURL(/\/(?:#\/)?$/);
  const moves = [];
  page.on('request', request => { if (request.method() === 'POST' && new URL(request.url()).pathname === '/api/dirs/move') moves.push(request.postDataJSON()); });
  const transfer = await page.evaluateHandle(() => new DataTransfer());
  const parent = from.split('/')[0];
  for (const [source, into] of [[parent, from], [from, parent], [parent, '']]) {
    const dragged = `#side [data-action="open-folder"][data-dir="${source}"]`;
    const dropped = into ? `#side [data-action="open-folder"][data-dir="${into}"]` : '#side .side-group[data-drop-dir=""]';
    await page.dispatchEvent(dragged, 'dragstart', { dataTransfer: transfer });
    await page.dispatchEvent(dropped, 'dragover', { dataTransfer: transfer });
    await expect(page.locator(dropped)).not.toHaveClass(/drop-ok/);
    await page.dispatchEvent(dropped, 'drop', { dataTransfer: transfer });
  }
  expect(moves).toEqual([]);
  await page.dispatchEvent(`#side [data-action="open-folder"][data-dir="${from}"]`, 'dragstart', { dataTransfer: transfer });
  await page.dispatchEvent('#side .side-tree', 'drop', { dataTransfer: transfer });
  await expect.poll(where).toBe(`${root}/Child`);
  expect(moves).toEqual([{ from, into: '' }]);
});







test('dropdowns and nested disclosures share real icons and keep their native controls', async ({ page }) => {
  const inspect = async () => {
    const selects = page.locator('select:visible');
    expect(await selects.count()).toBeGreaterThan(0);
    const issues = () => selects.evaluateAll(els => els.flatMap(el => {
      const wrap = el.closest('.select-control');
      const menu = wrap && wrap.classList.contains('has-menu') && wrap.querySelector(':scope > .select-menu-button');
      if (menu) {
        const name = el.id || el.dataset.set || el.dataset.keep;
        const icon = menu.querySelector('svg')?.getBoundingClientRect();
        const box = menu.getBoundingClientRect(), under = el.getBoundingClientRect();
        if (!icon) return [`${name}: menu missing chevron`];
        if (getComputedStyle(el).opacity !== '0' || getComputedStyle(el).pointerEvents !== 'none') return [`${name}: native select still shown`];
        if (Math.abs(under.width - box.width) > 1 || Math.abs(under.height - box.height) > 1) return [`${name}: select does not sit under its menu`];
        return Math.abs(box.y + box.height / 2 - icon.y - icon.height / 2) > 1 || icon.right > box.right - 4 || icon.left < box.left ? [`${name}: misaligned menu chevron`] : [];
      }
      const arrow = wrap && wrap.querySelector(':scope > svg[aria-hidden="true"]');
      if (!arrow) return [`${el.id || el.dataset.set || el.dataset.keep}: missing chevron`];
      const box = el.getBoundingClientRect();
      const icon = arrow.getBoundingClientRect();
      const style = getComputedStyle(el);
      return Math.abs(box.y + box.height / 2 - icon.y - icon.height / 2) > 1 || icon.right > box.right - 4 || icon.left < box.left || parseFloat(style.paddingRight) < icon.width + 12
        ? [`${el.id || el.dataset.set || el.dataset.keep}: misaligned chevron`]
        : [];
    }));
    await expect.poll(issues).toEqual([]);
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  };
  await page.goto('/#/settings/keeping');
  await expect(page.locator('select[data-keep="trash_days"]')).toBeVisible();
  await inspect();
  await page.goto('/#/');
  await control(page, 'upload').click();
  await expect(page.locator('#upload-dir')).toBeVisible();
  await inspect();
  await page.locator('.sheet [data-action="close"]').click();
  await page.route('**/api/record', route => route.fulfill({ json: { available: true, local: true, job: null, pending: [] } }));
  await page.route('**/api/calendar', route => route.fulfill({ json: { connected: true, tasks: [], events: [{ id: 'controls-event', title: 'A class with a long title for the folder selector', start: new Date().toISOString(), end: new Date(Date.now() + 3600000).toISOString(), all_day: false }], warnings: [] } }));
  await page.goto('/#/record');
  await expect(page.locator('#rec-event')).toBeVisible();
  await inspect();
  await page.locator('#rec-event').selectOption('');
  await expect(page.locator('#rec-event')).toHaveValue('');
  await page.emulateMedia({ forcedColors: 'active' });
  await expect(page.locator('#rec-dir + svg')).toBeHidden();
  await expect(page.locator('#rec-dir ~ .select-menu-button'), 'forced colours fall back to the native select').toBeHidden();
  expect(await page.locator('#rec-dir').evaluate(el => getComputedStyle(el).appearance)).toBe('auto');
  await page.emulateMedia({ forcedColors: 'none' });
  await page.goto('/#/calendar');
  await page.locator('#app [data-action="task-new"]').first().click();
  await expect(page.locator('.sheet .task-kinds .pick-chip')).toHaveCount(6);
  await expect(page.locator('.sheet .task-kinds .pick-chip.on')).toHaveText('Other');
  await page.locator('.sheet [data-action="close"]').click();
  await page.locator('#chat-toggle').click();
  await page.locator('[data-chat="attach"]').click();
  await expect(page.locator('#chat-attach-menu [data-chat="attach-note"] > svg')).toHaveCount(1);
  await expect(page.locator('#chat-attach-menu [data-chat="attach-file"] > svg')).toHaveCount(1);
  await expect(page.locator('[data-chat="attach"]')).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('[data-chat="attach-note"]')).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(page.locator('[data-chat="attach-file"]')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#chat-attach-menu')).toBeHidden();
  await expect(page.locator('[data-chat="attach"]')).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('[data-chat="attach"]')).toBeFocused();
  await expect(page.locator('#chat')).toBeVisible();
  await page.locator('[data-chat="access"]').click();
  await page.keyboard.press('End');
  await expect(page.locator('[data-chat="access-pick"][data-access="read"]')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#chat-access-menu')).toBeHidden();
  await expect(page.locator('#chat-access')).toBeFocused();
  await page.route('**/api/chat', route => route.fulfill({ contentType: 'application/x-ndjson', body: [{ step: 'Found relevant notes', tool: 'search_notes', found: ['Queues', 'Heaps'] }, { t: 'Here are the notes.' }, { done: true }].map(line => JSON.stringify(line)).join('\n') + '\n' }));
  await page.locator('#chat-input').fill('Find my algorithm notes');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat .msg').last()).toContainText('Here are the notes.');
  await page.locator('.msg-trace').last().locator('> summary').click();
  const step = page.locator('details.msg-step').last();
  await expect(step.locator('summary > .msg-step-chevron')).toHaveCount(1);
  await step.locator('summary').click();
  await expect(step.locator('li')).toHaveText(['Queues', 'Heaps']);
  await page.locator('[data-chat="close"]').click();
  const job = { id: 'controls-failed', source: 'nativecall', state: 'failed', secs: 0, step: '', steps: null, transcript: '', warnings: ['Recovered audio from the last interrupted recording.'], points: [], wants: '', levels: [], levels_start: 0, next_seq: 0, note: null, error: 'Others: No sound was recorded.; You: No sound was recorded.' };
  await page.route('**/api/record', route => route.fulfill({ json: { available: true, local: true, job, pending: [] } }));
  await page.route('**/api/record/controls-failed', route => route.fulfill({ json: job }));
  await page.goto('/#/record');
  await expect(page.locator('.rec-error-details summary > svg')).toHaveCount(1);
  await expect(page.locator('.rec-error-details')).toBeVisible();
  await page.locator('.rec-error-details summary').click();
  await expect(page.locator('.rec-error-details')).toHaveAttribute('open', '');
});

test('a failed call has calm details and can be dismissed or deleted directly', async ({ page }) => {
  let job = { id: 'failed-call-fixture', source: 'nativecall', state: 'failed', secs: 0, step: '', steps: null, transcript: '', warnings: ['Recovered audio from the last interrupted recording.'], points: [], wants: '', levels: [], levels_start: 0, next_seq: 0, note: null, error: 'Others: No sound was recorded. Check your microphone.; You: No sound was recorded. Check your microphone.' };
  let deletions = 0;
  await page.route('**/api/record', route => route.fulfill({ json: { available: true, local: true, job, pending: [] } }));
  await page.route('**/api/record/failed-call-fixture', route => {
    if (route.request().method() === 'DELETE') { deletions++; job = null; return route.fulfill({ json: { deleted: true } }); }
    return route.fulfill({ json: job });
  });
  await page.goto('/#/record');
  await expect(page.locator('.rec-failed-card h3')).toHaveText('No audio was captured');
  await expect(page.locator('.rec-error-details')).not.toHaveAttribute('open', '');
  await expect(page.locator('.rec-failed-card .hint')).not.toContainText('Others:');
  await page.locator('[data-action="rec-dismiss"]').click();
  await expect(page.locator('#rec-box .rec-idle')).toBeVisible();
  await page.reload();
  await expect(page.locator('.rec-idle')).toBeVisible();
  expect(deletions).toBe(0);
  await page.evaluate(() => sessionStorage.removeItem('leo-record-dismissed'));
  await page.reload();
  await expect(page.locator('.rec-failed-card')).toBeVisible();
  await page.locator('[data-action="rec-discard"]').click();
  await page.locator('.scrim [data-action="close"]').click();
  expect(deletions).toBe(0);
  await page.locator('[data-action="rec-discard"]').click();
  await page.locator('[data-action="rec-discard-now"]').click();
  await expect(page.locator('.rec-idle')).toBeVisible();
  expect(deletions).toBe(1);
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  await page.reload();
  await expect(page.locator('.rec-idle')).toBeVisible();
  await page.unroute('**/api/record');
  await page.unroute('**/api/record/failed-call-fixture');
  const base = { id: 'rec-2', session: 'sess-1', source: 'microphone', secs: 5, step: '', steps: null, transcript: '', warnings: [], points: [], wants: '', levels: [], levels_start: 0, next_seq: 0, note: null, error: null };
  let polled = false;
  const recovered = [];
  await page.route('**/api/record', route => route.fulfill({ json: { available: true, local: true, job: polled ? { ...base, state: 'failed', error: 'Provider failed' } : { ...base, state: 'writing' }, pending: polled ? [{ id: 'sess-1', title: null, directory: '', source: 'microphone' }] : [] } }));
  await page.route('**/api/record/rec-2', route => { polled = true; return route.fulfill({ json: { ...base, state: 'failed', error: 'Provider failed' } }); });
  await page.route('**/api/record/sess-1/recover', route => { recovered.push(route.request().method()); return route.fulfill({ status: 202, json: { id: 'rec-3' } }); });
  await page.evaluate(() => sessionStorage.removeItem('leo-record-dismissed'));
  await page.reload();
  const recover = page.getByRole('button', { name: 'Recover recording' });
  await expect(recover).toBeVisible({ timeout: 8000 });
  await recover.click();
  await expect.poll(() => recovered).toEqual(['POST']);
});







test.describe('passages', () => {
  test('selected note text goes to Felix as a passage he is asked about, not an edit', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Heaps', body: 'Intro line\n\nA binary heap keeps the minimum at the root.\n\nLast line' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const lines = [{ sources: [{ n: 1, id: note.id, title: 'Heaps', folder: '', why: 'open' }] }, { t: 'Because the heap property holds [n1].' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#doc .blk').nth(2).click();
    const area = page.locator('#doc textarea.line-edit');
    await expect(area).toHaveCount(1);
    await area.evaluate((a) => {
      const at = a.value.indexOf('keeps the minimum');
      a.focus();
      a.setSelectionRange(at, at + 'keeps the minimum'.length);
      a.dispatchEvent(new Event('select'));
    });
    const ask = page.locator('.ask-float');
    await expect(ask).toBeVisible();
    await ask.getByRole('button', { name: 'Ask Felix' }).click();
    const question = page.locator('.margin-note[data-id="draft"]').getByRole('textbox', { name: 'Ask Felix about this passage' });
    await expect(question).toBeFocused();
    await question.fill('Why?');
    await question.press('Enter');
    await page.locator('.margin-note[data-open]').getByRole('button', { name: 'Ask in chat' }).click();
    const chat = page.locator('#chat');
    await expect(chat).toBeVisible();
    await expect(chat.locator('.chat-passage')).toContainText('keeps the minimum');
    await chat.locator('#chat-input').fill('Why is that true?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('heap property');
    expect(asked[0].selection).toEqual({ title: 'Heaps', from: 3, to: 3, text: 'keeps the minimum' });
    expect(asked[0].refs).toContain(note.id);
    expect(asked[0].messages.at(-1).text).toBe('Why is that true?');
    await expect(chat.locator('.msg.user .msg-passage')).toContainText('Heaps · line 3');
    await expect(chat.locator('.chat-passage')).toHaveCount(0);
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('Intro line\n\nA binary heap keeps the minimum at the root.\n\nLast line');
  });

  test('Felix reads along only after an edit, speaks in the margin, and his answer lands in the note', async ({ page }) => {
    await page.request.post('/api/margin', { data: { layout: 'both' } });
    await page.setViewportSize({ width: 1600, height: 900 });
    const start = 'Save the password in the users table.';
    const note = await (await page.request.post('/api/notes', { data: { title: 'Signup flow', body: start } })).json();
    const reads = [];
    const said = (from, text) => ({ from, text, at: '2026-11-04T10:00:00Z' });
    const thread = { id: 'stub-thread', note: note.id, quote: 'just checks if it matches', prefix: 'Then login ', suffix: ', I think', status: 'open', source: 'read', messages: [said('felix', `In plain text? If this table leaks, every account goes. ${'Hash it first. '.repeat(20)}`)], created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z' };
    await page.route('**/margin/read', async (route) => {
      reads.push(route.request().postDataJSON());
      await new Promise((done) => setTimeout(done, 2500));
      await route.fulfill({ json: { note: thread } });
    });
    await page.route('**/margin/threads/stub-thread/reply', (route) => route.fulfill({ json: { ...thread, messages: [...thread.messages, said('you', 'So hash it?'), said('felix', 'Yes. Argon2id, then verify at login.')] } }));
    await page.route('**/margin/threads/stub-thread', (route) => route.fulfill({ json: { ...thread, status: 'resolved' } }));
    const weaves = [];
    await page.route('**/margin/threads/stub-thread/weave', async (route) => {
      weaves.push(route.request().postDataJSON());
      const now = await (await page.request.get(`/api/notes/${note.id}`)).json();
      const body = now.body.replace('Then login just checks if it matches, I think', 'Then login checks it against the Argon2id hash.');
      const saved = await (await page.request.patch(`/api/notes/${note.id}`, { data: { body, base: now.version } })).json();
      await route.fulfill({ json: { ...saved, before: now.body, from: 3, to: 3 } });
    });
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#margin-mode')).toHaveText('When you ask');
    await page.locator('#margin-mode').click();
    await page.getByRole('menuitemradio', { name: 'Reads along' }).click();
    await expect(page.locator('#margin-mode')).toHaveText('Reads along');
    await expect(page.locator('.note-columns')).toHaveAttribute('data-margin', 'both');
    await page.reload();
    await expect(page.locator('#margin-mode')).toHaveText('Reads along');
    await page.waitForTimeout(4500);
    expect(reads, 'opening a note must not read it').toEqual([]);

    await page.locator('#doc .blk').first().click();
    const area = page.locator('#doc textarea.line-edit');
    await area.press('End');
    await area.pressSequentially('\n\nThen login just checks if it matches, I think');
    await expect(page.locator('.margin-waiting')).toBeVisible({ timeout: 9000 });
    await expect(page.locator('.margin-note')).toBeVisible({ timeout: 9000 });
    await expect(page.locator('.margin-waiting')).toHaveCount(0);
    await expect(page.locator('#margin-status')).toHaveText('');
    expect(reads).toHaveLength(1);
    expect(reads[0].blocks).toEqual(['Then login just checks if it matches, I think']);
    await expect(area).toBeFocused();
    await expect(page.locator('.margin-ink .squiggle')).toHaveCount(1);
    await expect(page.locator('.margin-ink .tether')).toHaveClass(/\brest\b/);
    await expect(page.locator('.margin-ink .tether')).not.toHaveClass(/\bon\b/);
    expect(await page.locator('.margin-ink .squiggle').getAttribute('d')).not.toMatch(/ q| C/);
    await expect(page.locator('.margin-note .margin-felix')).toContainText('In plain text?');
    const delays = await page.locator('.ink-word').evaluateAll((words) => words.map((word) => parseFloat(word.style.animationDelay)));
    expect(delays.length).toBeGreaterThan(40);
    expect(delays[0]).toBe(0);
    expect(Math.max(...delays)).toBeLessThanOrEqual(651);
    await expect(page.locator('.ink-word')).toHaveCount(delays.length);

    const toggle = page.locator('.margin-note .margin-toggle');
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(page.locator('.margin-note')).not.toHaveAttribute('tabindex', '0');
    await expect(page.locator('.margin-note.arriving, .margin-ink path.arriving')).toHaveCount(0);
    await page.waitForFunction(() => document.getAnimations().every((run) => run.playState !== 'running' || run.effect.getComputedTiming().iterations === Infinity));
    await toggle.focus();
    await page.keyboard.press('Enter');
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await page.keyboard.press('Escape');
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(toggle).toBeFocused();
    await page.locator('.margin-note').click();
    await expect(page.locator('.margin-note').getByRole('button', { name: 'Open in chat' })).toHaveCount(0);
    await expect(page.locator('.margin-note').getByRole('button', { name: 'Ask in chat' })).toBeVisible();
    const reply = page.locator('.margin-reply textarea');
    await expect(reply).toBeFocused();
    expect(await reply.evaluate((el) => { const css = getComputedStyle(el); return css.boxShadow.includes('0px 1px 0px') && !css.boxShadow.includes('rgba(0, 0, 0, 0)'); })).toBe(true);
    await reply.fill('So hash it?');
    await reply.press('Enter');
    await expect(page.locator('.margin-note .margin-felix').last()).toContainText('Argon2id');
    await page.locator('.margin-note').getByRole('button', { name: 'Add to note' }).click();
    await expect(page.locator('.toast')).toContainText('Worked into the note');
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('Save the password in the users table.\n\nThen login checks it against the Argon2id hash.');
    expect(weaves).toHaveLength(1);
    expect(weaves[0].request).toBeTruthy();
    await page.locator('.toast').getByRole('button', { name: 'Undo' }).click();
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body).not.toContain('Argon2id');
  });

  test('the page and the real margin routes work together with a stubbed provider', async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Real margin', body: 'Save the password in the users table.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#margin-mode').click();
    await page.getByRole('menuitemradio', { name: 'Reads along' }).click();
    await expect(page.locator('#margin-mode')).toHaveText('Reads along');
    await page.locator('#doc .blk').first().click();
    const area = page.locator('#doc textarea.line-edit');
    await area.press('End');
    await area.pressSequentially('\n\nThen login just checks if it matches, I think');
    await expect(page.locator('.margin-note .margin-felix')).toContainText('In plain text?', { timeout: 12000 });
    await page.reload();
    await expect(page.locator('.margin-note .margin-felix')).toContainText('In plain text?');
    await page.locator('.margin-note').click();
    const reply = page.locator('.margin-reply textarea');
    await reply.fill('So hash it?');
    await reply.press('Enter');
    await expect(page.locator('.margin-note .margin-felix').last()).toContainText('Argon2id');
    await page.locator('.margin-note').getByRole('button', { name: 'Add to note' }).dblclick();
    await expect(page.locator('.toast')).toContainText('Worked into the note');
    const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
    await expect.poll(body).toContain('Argon2id');
    expect((await body()).match(/Argon2id/g)).toHaveLength(1);
    expect(await body()).not.toContain('just checks');
    const left = async () => (await (await page.request.get(`/api/notes/${note.id}/margin`)).json()).threads.length;
    await expect.poll(left).toBe(0);
    await page.locator('.toast').getByRole('button', { name: 'Undo' }).click();
    await expect.poll(body).not.toContain('Argon2id');
    await expect.poll(left).toBe(1);
  });

  test('Read through leaves margin notes on an old note in When you ask mode, and Undo takes them out', async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Old note', body: 'Then login just checks if it matches, I think' } })).json();
    let analyzed = 0;
    page.on('request', (r) => { if (r.url().includes('/margin/analyze')) analyzed++; });
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#margin-mode')).toHaveText('When you ask');
    expect(analyzed).toBe(0);
    await page.locator('#note-more').click();
    await page.getByRole('menuitem', { name: 'Read through' }).click();
    await page.getByRole('menuitemradio', { name: /^Reader/ }).click();
    await expect(page.locator('.margin-note .margin-felix')).toContainText('In plain text?', { timeout: 12000 });
    await expect(page.locator('.toast')).toContainText('Felix added 1 margin note');
    await page.locator('.toast').getByRole('button', { name: 'Undo' }).click();
    const left = async () => (await (await page.request.get(`/api/notes/${note.id}/margin`)).json()).threads.length;
    await expect.poll(left).toBe(0);
    await expect(page.locator('#margin-mode')).toHaveText('When you ask');
  });

  test('Stop ends a Read through at once, even while Felix is still reading', async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Stopped note', body: 'Then login just checks if it matches, I think' } })).json();
    await page.route('**/margin/analyze', () => {});
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#note-more').click();
    await page.getByRole('menuitem', { name: 'Read through' }).click();
    await page.getByRole('menuitemradio', { name: /^Reader/ }).click();
    await expect(page.locator('#margin-status')).toContainText('Reading through');
    await page.waitForTimeout(500);
    await page.locator('#note-more').click();
    await page.getByRole('menuitem', { name: 'Stop reading through' }).click();
    await expect(page.locator('#margin-status')).not.toContainText('Reading through', { timeout: 2000 });
    await expect(page.locator('.margin-note')).toHaveCount(0);
  });

  for (const [title, later, says] of [
    ['re-finds its passage when the note changed underneath it', 'A new first line.\n\nFirst.\n\nThe quoted words stay here.\n\nLast.', 'Worked into the note'],
    ['says so when its passage is gone', 'Everything was rewritten.', 'went at the end of the note'],
  ]) {
    test(`Add to note ${title}`, async ({ page }) => {
      await page.setViewportSize({ width: 1600, height: 900 });
      const note = await (await page.request.post('/api/notes', { data: { title: `Moved ${says}`, body: 'First.\n\nThe quoted words stay here.\n\nLast.' } })).json();
      const asked = await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote: 'quoted words', question: 'Why?' } });
      expect(asked.ok()).toBeTruthy();
      await page.goto(`/#/n/${note.id}`);
      await expect(page.locator('.margin-note')).toBeVisible();
      let refused = 0;
      await page.route('**/weave', async (route) => {
        if (refused++) return route.continue();
        await page.request.patch(`/api/notes/${note.id}`, { data: { body: later } });
        await route.fulfill({ status: 409, json: { error: 'changed' } });
      });
      await page.locator('.margin-note').click();
      await page.locator('.margin-note').getByRole('button', { name: 'Add to note' }).click();
      await expect(page.locator('.toast').first()).toContainText(says);
      const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
      await expect.poll(body).toContain('Argon2id');
      await expect(page.locator('.margin-note')).toHaveCount(0);
      if (says === 'Worked into the note') {
        await expect(page.locator('#doc .blk-glow').first()).toBeVisible();
        expect(await body()).toBe('A new first line.\n\nFirst.\n\nYes. Argon2id, then verify at login.\n\nLast.');
      } else {
        expect(await body()).toBe('Everything was rewritten.\n\nYes. Argon2id, then verify at login.\n');
        await page.waitForTimeout(300);
        expect(await page.locator('#doc .blk-glow').count(), 'nothing glows when the passage is gone').toBe(0);
      }
    });
  }

  test('a failed read keeps its text for the next one and a refused read waits for an edit', async ({ page }) => {
    await page.clock.install();
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Waits', body: 'Opening line that is long enough.' } })).json();
    await page.request.post(`/api/notes/${note.id}/margin`, { data: { mode: 'along' } });
    const reads = [];
    await page.route('**/margin/read', async (route) => {
      reads.push(route.request().postDataJSON().blocks);
      if (reads.length === 1) return route.fulfill({ json: { note: null, failed: true } });
      await route.fulfill({ status: 429, json: { why: 'soon', error: 'Soon.' } });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#doc .blk').first().click();
    const area = page.locator('#doc textarea.line-edit');
    await area.press('End');
    await area.pressSequentially('\n\nFirst changed paragraph long enough.');
    await expect.poll(async () => { await page.clock.fastForward(1000); return reads.length; }).toBe(1);
    await area.pressSequentially('\n\nSecond changed paragraph long enough.');
    await expect.poll(async () => { await page.clock.fastForward(30000); return reads.length; }).toBe(2);
    expect(reads[1]).toEqual(['First changed paragraph long enough.', 'Second changed paragraph long enough.']);
    await page.clock.fastForward(300000);
    await page.clock.fastForward(300000);
    await page.waitForTimeout(200);
    expect(reads).toHaveLength(2);
  });

  test('two selected characters go to the chat, not a margin note that cannot save', async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Short pick', body: 'An ox stood here.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#doc .blk').first().click();
    const area = page.locator('#doc textarea.line-edit');
    await area.evaluate((a) => {
      a.focus();
      a.setSelectionRange(3, 5);
      a.dispatchEvent(new Event('select'));
    });
    await page.keyboard.press('ControlOrMeta+Shift+e');
    await expect(page.locator('#chat .chat-passage')).toContainText('ox');
    await expect(page.locator('.margin-note')).toHaveCount(0);
  });

  test('the note column stays put through a draft, a thread and a dismissal, and Marks only centres it', async ({ page }) => {
    await page.request.post('/api/margin', { data: { layout: 'right' } });
    await page.setViewportSize({ width: 1700, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Steady column', body: 'A binary heap keeps the minimum at the root.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#margin-mode')).toBeEnabled();
    const box = async () => (await page.locator('article.note').boundingBox());
    const first = await box();
    expect(first.width).toBeCloseTo(980, -1);
    const columns = await page.locator('.note-columns').boundingBox();
    expect(first.x - columns.x).toBeLessThan(columns.x + columns.width - first.x - first.width - 100);
    await page.locator('#doc .blk').first().click();
    await page.locator('#doc textarea.line-edit').evaluate((a) => {
      const at = a.value.indexOf('keeps the minimum');
      a.focus();
      a.setSelectionRange(at, at + 'keeps the minimum'.length);
      a.dispatchEvent(new Event('select'));
    });
    await page.keyboard.press('ControlOrMeta+Shift+e');
    await expect(page.locator('.margin-note[data-open] .margin-felix')).toBeVisible();
    expect(await box()).toEqual(first);
    await page.locator('.margin-note[data-open]').evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((run) => run.finished)));
    await page.locator('.margin-note[data-open]').getByRole('button', { name: 'Dismiss' }).click();
    await expect(page.locator('.margin-note')).toHaveCount(0);
    expect(await box()).toEqual(first);

    await page.setViewportSize({ width: 1360, height: 900 });
    await expect.poll(async () => (await box()).width).toBeLessThan(800);

    await page.setViewportSize({ width: 1700, height: 900 });
    await page.addInitScript(() => {
      window.__layouts = [];
      new MutationObserver(() => {
        const columns = document.querySelector('.note-columns');
        if (columns && window.__layouts.at(-1) !== columns.dataset.margin) window.__layouts.push(columns.dataset.margin);
      }).observe(document, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-margin'] });
    });
    await page.goto('/#/settings/felix');
    await page.locator('[data-find="felix-where"] .choice-pick', { hasText: 'Marks only' }).click();
    await expect.poll(async () => (await (await page.request.get('/api/margin')).json()).layout).toBe('marks');
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#margin-mode')).toBeEnabled();
    await page.reload();
    await expect(page.locator('#margin-mode')).toBeEnabled();
    expect(await page.evaluate(() => window.__layouts)).toEqual(['marks']);
    const centred = await box();
    const room = await page.locator('.note-columns').boundingBox();
    expect(Math.abs((centred.x - room.x) - (room.x + room.width - centred.x - centred.width))).toBeLessThanOrEqual(2);
    await page.request.post('/api/margin', { data: { layout: 'right' } });
    await page.evaluate(() => localStorage.removeItem('leo-margin-layout'));
  });

  test('the Felix chip is in the row from the first paint, so the note never jumps', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 800 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Phone row', body: 'A line of text.' } })).json();
    let release;
    const held = new Promise((done) => { release = done; });
    await page.route(`**/api/notes/${note.id}/margin`, async (route) => {
      await held;
      await route.continue();
    });
    await page.goto(`/#/n/${note.id}`);
    const chip = page.locator('#margin-mode');
    await expect(chip).toBeVisible();
    await expect(chip).toBeDisabled();
    const before = await page.locator('#doc').boundingBox();
    release();
    await expect(chip).toBeEnabled();
    expect(await page.locator('#doc').boundingBox()).toEqual(before);
  });

  test('the margin mark is easy to hit with a mouse and a finger, and keeps its edges in forced colors', async ({ browser, page }) => {
    await page.setViewportSize({ width: 1000, height: 800 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Marks', body: 'First.\n\nThe quoted words stay here.' } })).json();
    await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote: 'quoted words', question: 'Why?' } });
    await page.goto(`/#/n/${note.id}`);
    const mark = page.locator('.margin-mark');
    await expect(mark).toBeVisible();
    const mouse = await mark.boundingBox();
    expect(Math.min(mouse.width, mouse.height)).toBeGreaterThanOrEqual(24);
    expect((await mark.locator('.margin-anchor').boundingBox()).width).toBeLessThanOrEqual(14);
    await page.emulateMedia({ forcedColors: 'active' });
    expect(await page.locator('.margin-ink .squiggle').evaluate((el) => getComputedStyle(el).stroke)).not.toBe('none');
    await page.emulateMedia({ forcedColors: 'none' });
    const baseURL = new URL(page.url()).origin;
    const touch = await browser.newContext({ baseURL, hasTouch: true, isMobile: true, viewport: { width: 390, height: 800 } });
    const phone = await touch.newPage();
    await phone.goto(`/?token=${token()}`);
    await phone.goto(`/#/n/${note.id}`);
    const finger = await phone.locator('.margin-mark').boundingBox();
    expect(Math.min(finger.width, finger.height)).toBeGreaterThanOrEqual(44);
    await touch.close();
  });

  test('a margin note whose words are gone stays out of the page and the Felix panel, and Settings starts with read-along off', async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Lost place', body: 'Nothing like it here.' } })).json();
    const lost = { id: 'stub-lost', note: note.id, quote: 'words that went away', prefix: 'nothing near ', suffix: ' nothing after', status: 'open', source: 'read', messages: [{ from: 'felix', text: 'This passage has gone.', at: '2026-11-04T10:00:00Z' }], created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z' };
    await page.route(`**/api/notes/${note.id}/margin`, (route) => route.fulfill({ json: { mode: 'ask', threads: [lost] } }));
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#margin-mode')).toBeVisible();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat .chat-hello-lead')).toBeVisible();
    await expect(page.locator('#chat')).not.toContainText('lost its place');
    await expect(page.locator('.margin-note')).toHaveCount(0);

    await page.goto('/#/settings/felix');
    const newNotes = page.locator('[data-find="felix-new-notes"]');
    await expect(newNotes.locator('.choice-pick.on')).toContainText('When you ask');
    await expect(newNotes).toContainText('the note’s title and the text you change');
    await expect(newNotes.locator('p.hint'), 'one line: the privacy statement').toHaveCount(1);
    await expect(newNotes.locator('.choice-about')).toHaveCount(2);
    await expect(page.locator('#margin-today')).toHaveCount(0);
    expect((await (await page.request.get('/api/margin')).json()).cap).toBe(40);
    await page.locator('[data-find="felix-where"] .choice-pick', { hasText: 'Marks only' }).click();
    await expect.poll(async () => (await (await page.request.get('/api/margin')).json()).layout).toBe('marks');
    await page.locator('[data-find="felix-where"] .choice-pick', { hasText: 'Right margin' }).click();
    await expect.poll(async () => (await (await page.request.get('/api/margin')).json()).layout).toBe('right');
  });

  test('selected words get their answer in the margin without read-along', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Heaps margin', body: 'A binary heap keeps the minimum at the root.' } })).json();
    let asked = null;
    await page.route('**/margin/threads', async (route) => {
      asked = route.request().postDataJSON();
      await route.fulfill({ json: { id: 'stub-ask', note: note.id, quote: asked.quote, prefix: asked.prefix, suffix: asked.suffix, status: 'open', source: 'ask', messages: [{ from: 'felix', text: 'The heap property.', at: '2026-11-04T10:00:00Z' }], created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z' } });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#doc .blk').first().click();
    const area = page.locator('#doc textarea.line-edit');
    await area.evaluate((a) => {
      const at = a.value.indexOf('keeps the minimum');
      a.focus();
      a.setSelectionRange(at, at + 'keeps the minimum'.length);
      a.dispatchEvent(new Event('select'));
    });
    await page.keyboard.press('ControlOrMeta+Shift+e');
    await expect(page.locator('.margin-note .margin-felix')).toContainText('The heap property.');
    expect(asked.quote).toBe('keeps the minimum');
    expect(asked.question).toBe('');
    await expect(page.locator('.ink-word, .margin-ink path.arriving')).toHaveCount(0);
    await page.locator('#chat-toggle').click();
    await expect(page.locator('.note-columns')).toHaveAttribute('data-margin', 'marks');
    await expect(page.locator('.margin-mark')).toHaveCount(1);
  });

  test('a multi-paragraph answer arrives word by word in colour, never code, and never by opacity', async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Prose margin', body: 'A binary heap keeps the minimum at the root.' } })).json();
    await page.route('**/margin/threads', async (route) => {
      const asked = route.request().postDataJSON();
      await route.fulfill({ json: { id: 'stub-prose', note: note.id, quote: asked.quote, prefix: asked.prefix, suffix: asked.suffix, status: 'open', source: 'ask', messages: [{ from: 'felix', text: 'First paragraph of the answer.\n\nSecond has **bold** words and `code here`.\n\n```\nlet kept = 1;\n```\n\nLast paragraph.', at: '2026-11-04T10:00:00Z' }], created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z' } });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#doc .blk').first().click();
    await page.locator('#doc textarea.line-edit').evaluate((a) => {
      const at = a.value.indexOf('keeps the minimum');
      a.focus();
      a.setSelectionRange(at, at + 'keeps the minimum'.length);
      a.dispatchEvent(new Event('select'));
    });
    await page.keyboard.press('ControlOrMeta+Shift+e');
    const said = page.locator('.margin-note .margin-felix.prose');
    await expect(said).toContainText('Last paragraph.');
    const words = said.locator('.ink-word');
    expect(await words.count()).toBeGreaterThan(10);
    await expect(said.locator('p')).toHaveCount(3);
    await expect(said.locator('pre .ink-word, code .ink-word')).toHaveCount(0);
    const delays = await words.evaluateAll((all) => all.map((w) => parseFloat(w.style.animationDelay)));
    expect(delays[0]).toBe(0);
    expect(Math.max(...delays)).toBeGreaterThan(delays[1]);
    expect(Math.max(...delays)).toBeLessThanOrEqual(651);
    const names = await said.evaluate((el) => [el, ...el.querySelectorAll('.ink-word')].flatMap((n) => n.getAnimations().map((a) => a.animationName)));
    expect(new Set(names)).toEqual(new Set(['ink-in']));
  });

  test('the selection shortcut explains when nothing is selected without a bottom Ask button', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Toolbar ask', body: 'Only line here' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc .blk').first()).toBeVisible();
    await expect(page.locator('nav.actions [data-action="ask-selection"]')).toHaveCount(0);
    await page.keyboard.press('ControlOrMeta+Shift+e');
    await expect(page.locator('.toast')).toContainText('Select some text in the note first');
  });

  test('part of an answer is added to the open note exactly once, and Undo takes it out', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Collect', body: 'Mine already' } })).json();
    await page.route('**/api/chat', async (route) => {
      const lines = [{ sources: [{ n: 1, id: note.id, title: 'Collect', folder: '', why: 'open' }] }, { t: 'First idea stays out.\n\nThe **key** point is here [n1].\n\nLast bit.' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('Tell me');
    await chat.locator('#chat-input').press('Enter');
    const answer = chat.locator('.msg.leo').last();
    await expect(answer).toContainText('Last bit');
    await answer.locator('strong', { hasText: 'key' }).evaluate((el) => {
      const range = document.createRange();
      range.selectNodeContents(el.parentElement);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    });
    const add = chat.locator('.float-act[data-chat="add-picked"]');
    await expect(add).toBeVisible();
    await add.click();
    const sheet = page.locator('.sheet');
    await expect(sheet.locator('.add-title')).toHaveText('Collect');
    await expect(sheet.locator('.add-preview')).toContainText('The key point is here');
    await expect(sheet.locator('.add-preview')).not.toContainText('First idea');
    await sheet.locator('#add-go').dblclick();
    await expect(page.locator('.toast')).toContainText('Added to “Collect”');
    const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
    await expect.poll(body).toBe('Mine already\n\nThe **key** point is here [[Collect]].\n');
    await page.locator('.toast button').click();
    await expect.poll(body).toBe('Mine already');
  });
});

test.describe('study sessions', () => {
  test('a session shows its ideas, ticks one off after two right answers, and ends with a recap', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Heaps', body: 'A heap keeps the minimum at the root.' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const source = { n: 1, id: note.id, title: 'Heaps', folder: '', why: 'open' };
      const lines = asked.length === 1
        ? [{ sources: [source] }, { step: 'Planned a study session', tool: 'study_plan', found: [] }, { plan: { ideas: ['Root', 'Sift down'] } }, { quiz: { kind: 'multiple_choice', question: 'What sits at the root?', options: ['the minimum', 'the maximum'], answer: 'the minimum', idea: 1 } }, { t: 'First one.' }, { done: true }]
        : [{ sources: [source] }, { quiz: { kind: 'multiple_choice', question: 'Where is the smallest key of a min-heap?', options: ['at the root', 'in a leaf'], answer: 'at the root', idea: 1 } }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('quiz me on heaps');
    await chat.locator('#chat-input').press('Enter');
    const progress = chat.locator('#study-progress');
    await expect(progress).toBeVisible();
    await expect(progress).toContainText('Root');
    await expect(progress).toContainText('0 of 2');
    await expect(progress.locator('.study-idea.now')).toHaveText('Root');
    await expect(progress.locator('.study-mark')).toHaveCount(2);
    await chat.locator('.quiz-card button', { hasText: 'the minimum' }).first().click();
    await expect(progress).toContainText('0 of 2');
    await expect(progress.locator('.study-ok.study-once')).toHaveCount(1);
    await expect(progress.locator('.study-idea').first().locator('.study-mark')).toHaveAttribute('aria-label', 'right once, one more to go');
    await expect(progress.locator('.study-idea').nth(1).locator('.study-mark')).toHaveAttribute('aria-label', 'not yet');
    await chat.locator('.quiz-card [data-chat="quiz-next"]').click();
    await expect.poll(() => asked.length).toBe(2);
    expect(asked[1].session).toEqual({ ideas: ['Root', 'Sift down'], closed: [false, false], asked: 1, right: [1, 0], misses: [0, 0] });
    await chat.locator('.quiz-card button', { hasText: 'at the root' }).first().click();
    await expect(progress).toContainText('1 of 2');
    await expect(progress.locator('.study-idea.first .study-mark')).toHaveAttribute('aria-label', 'right twice, never missed');
    await expect(progress.locator('.study-idea.first .study-ok')).toHaveCount(1);
    await expect(progress.locator('.study-once')).toHaveCount(0);
    await expect(progress.locator('.study-idea.first')).toContainText('Root');
    await progress.locator('[data-chat="session-finish"]').click();
    await expect(progress).toBeHidden();
    const recap = chat.locator('.session-recap');
    await expect(recap).toContainText('1 of 2 ideas covered');
    await expect(recap).toContainText('Not shown yet: Sift down');
    await recap.locator('[data-chat="session-done"]').click();
    await expect(recap).toHaveCount(0);
  });
});

test.describe('reading', () => {
  test('related notes sit under a note and open with a tap', async ({ page }) => {
    const other = await (await page.request.post('/api/notes', { data: { title: 'Graph basics', body: 'Vertices and edges.' } })).json();
    const note = await (await page.request.post('/api/notes', { data: { title: 'Dijkstra', body: 'Builds on [[Graph basics]].' } })).json();
    await page.goto(`/#/n/${note.id}`);
    const related = page.locator('#related');
    await expect(related).toBeVisible();
    await expect(related.locator('.related-row')).toHaveCount(1);
    await expect(related).toContainText('Graph basics');
    await expect(related).toContainText('Linked from this note');
    await related.locator('.related-row').click();
    await expect(page).toHaveURL(new RegExp(`#/n/${other.id}$`));
    await expect(page.locator('#related')).toBeHidden();
  });

  test('a long note opens again where it was being read', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'scrolled with a mouse wheel');
    const body = Array.from({ length: 160 }, (_, i) => `Paragraph number ${i + 1} about reading.`).join('\n\n');
    const note = await (await page.request.post('/api/notes', { data: { title: 'Long read', body } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc .blk').first()).toBeVisible();
    await page.locator('#doc .blk').first().hover();
    await page.mouse.wheel(0, 2400);
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}/reading`)).json()).anchor, { timeout: 8000 }).not.toBeNull();
    const saved = (await (await page.request.get(`/api/notes/${note.id}/reading`)).json()).anchor;
    expect(saved.text).toMatch(/^Paragraph number \d+/);
    await page.goto('/#/');
    await page.goto(`/#/n/${note.id}`);
    await expect.poll(() => page.evaluate(() => window.scrollY), { timeout: 5000 }).toBeGreaterThan(1500);
  });
});

test.describe('calendar', () => {
  test('task descriptions can be added, edited and cleared beside the calendar without losing focus', async ({ page }) => {
    const title = `Description ${test.info().project.name} ${Date.now().toString(36)}`;
    const details = 'Read chapter 3\nBring café notes; <b>plain text</b>';
    const saved = async () => (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.title === title);
    await page.goto('/#/calendar');
    await page.locator('.cal-head [data-action="task-new"]').click();
    await page.locator('#task-title').fill(title);
    await page.locator('.sheet [data-pick-day=""]').click();
    const description = page.locator('#task-description');
    await expect(description).toHaveAttribute('placeholder', 'Add a description…');
    await description.fill(details);
    await page.locator('#task-add').click();
    await expect(page.locator('.toast')).toContainText(`Added “${title}”`);
    await page.locator('#cal-side .cal-tray-row .cal-open', { hasText: title }).click();
    const about = page.locator('#cal-peek-about');
    await expect(about).toHaveValue(details);
    await about.fill('Revised details');
    await expect.poll(async () => (await saved()).description).toBe('Revised details');
    await expect(page.locator('#cal-peek-saved')).toHaveText('Saved');
    await about.fill('');
    await expect.poll(async () => (await saved()).description).toBe('');
    await expect(about).toHaveAttribute('placeholder', 'Add a description…');
    let inFlight = false;
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    await page.route('**/api/tasks/*', async (route) => {
      if (route.request().method() !== 'PATCH') return route.continue();
      inFlight = true;
      await held;
      await route.continue();
    });
    await about.fill('Saving this');
    await expect.poll(() => inFlight).toBe(true);
    await about.pressSequentially(' and still typing');
    release();
    await expect(about).toBeFocused();
    await expect(about).toHaveValue('Saving this and still typing');
    await expect.poll(async () => (await saved()).description).toBe('Saving this and still typing');
    await page.locator('.cal-name').click();
    await page.keyboard.press('Escape');
    await expect(page.locator('#cal-side.peek')).toHaveCount(0);
  });

  test('a task sheet description saves itself, even when closed mid-typing', async ({ page }) => {
    const title = `Autosave ${test.info().project.name} ${Date.now().toString(36)}`;
    const made = await (await page.request.post('/api/tasks', { data: { title, description: '', date: null, time: null, kind: 'other', repeat: [] } })).json();
    const saved = async () => (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.id === made.id);
    await page.goto('/#/calendar');
    await page.evaluate((id) => {
      const open = document.createElement('button');
      open.dataset.action = 'task-open-here';
      open.dataset.id = id;
      document.body.append(open);
      open.click();
      open.remove();
    }, made.id);
    const description = page.locator('#task-description');
    await description.fill('First draft');
    await expect.poll(async () => (await saved()).description).toBe('First draft');
    await expect(page.locator('#task-description-state')).toHaveText('Saved');
    await expect(page.locator('#task-description-save')).toHaveCount(0);
    await description.fill('Typed then closed');
    await page.keyboard.press('Escape');
    await expect.poll(async () => (await saved()).description).toBe('Typed then closed');
  });

  test('tasks are added, dated, ticked off and sent to Google Calendar from the Calendar page', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.addInitScript(() => {
      window.__opened = [];
      window.open = (url) => {
        window.__opened.push(String(url));
        return null;
      };
    });
    const undated = await (await page.request.post('/api/tasks', { data: { title: `Ask tutor ${tag}` } })).json();
    const note = await (await page.request.post('/api/notes', { data: { title: `Lecture ${tag}`, body: '## Action items\n- [ ] Submit HW 4 — due October 16, 2026 at 11:59 pm.\n## Check yourself\n- [ ] What is a heap?' } })).json();
    await page.clock.install({ time: FIXED_NOW });
    await page.goto('/');
    await goPlace(page, 'calendar');
    await expect(page).toHaveURL(/#\/calendar$/);
    const box = page.locator('#cal-page');
    await expect(page.locator('#side')).toBeVisible();
    const noDate = box.locator('.cal-tray-list', { hasText: 'No date' });
    await expect(noDate).toContainText(`Ask tutor ${tag}`);

    await noDate.locator(`.cal-open[data-id="${undated.id}"]`).click();
    await page.locator('#cal-peek-date').fill(FIXED_TODAY);
    await page.locator('#cal-peek-date').dispatchEvent('change');
    await expect(page.locator('#cal-peek-start')).toBeEnabled();
    await page.locator('#cal-peek-start').fill('09:00');
    await page.locator('#cal-peek-start').dispatchEvent('change');
    await expect.poll(async () => (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.id === undated.id).time).toBe('09:00:00');
    const item = box.locator(`.cal-col .cal-item[data-id="${undated.id}"]`);
    await expect(item).toContainText(`Ask tutor ${tag}`);

    await page.locator('#cal-side [data-action="task-google"]').click();
    const opened = await page.evaluate(() => window.__opened);
    expect(opened.length).toBe(1);
    const link = new URL(opened[0]);
    expect(link.host).toBe('calendar.google.com');
    expect(link.searchParams.get('action')).toBe('TEMPLATE');
    expect(link.searchParams.get('details')).toContain(`leo-task:${undated.id.slice(0, 12)}`);
    await expect(page.locator('.toast')).toContainText('Opened in Google Calendar');

    await item.locator('.cal-check').click();
    await expect(page.locator('.toast')).toContainText(`Marked “Ask tutor ${tag}” done`);
    await expect.poll(async () => (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.id === undated.id).done).toBe(true);
    await page.locator('.toast button', { hasText: 'Undo' }).click();
    await expect.poll(async () => (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.id === undated.id).done).toBe(false);

    const hw = (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.note && t.note.id === note.id);
    expect(hw.title).toContain('Submit HW 4');
    expect(hw.date).toBe('2026-10-16');
    const all = (await (await page.request.get('/api/tasks')).json()).tasks;
    expect(all.some((t) => t.title === 'What is a heap?')).toBe(false);
  });

  test('a new task can be typed in, and a note checkbox can be added to tasks', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    const note = await (await page.request.post('/api/notes', { data: { title: `Plans ${tag}`, body: 'Ideas\n- [ ] Book the room\nEnd' } })).json();
    await page.goto('/#/calendar');
    await page.locator('.cal-head [data-action="task-new"]').click();
    await page.locator('#task-title').fill(`Send slides ${tag}`);
    await page.locator('#task-title').press('Enter');
    await expect(page.locator('.toast')).toContainText(`Added “Send slides ${tag}”`);
    await expect(page.locator('#cal-page .cal-item', { hasText: `Send slides ${tag}` })).toBeVisible();

    await page.goto(`/#/n/${note.id}`);
    await page.locator('#note-more').click();
    await page.getByRole('menuitem', { name: 'Tasks' }).click();
    const sheet = page.locator('.sheet');
    await expect(sheet).toContainText('Book the room');
    await sheet.locator('[data-action="task-promote"]').click();
    await expect(page.locator('.toast')).toContainText('Added “Book the room” to your tasks');
    await expect(page.locator('.sheet [data-action="task-open-here"]')).toHaveText('In your tasks');
    const tasks = (await (await page.request.get('/api/tasks')).json()).tasks;
    expect(tasks.some((t) => t.title === 'Book the room' && t.note.id === note.id)).toBe(true);
  });
});

test.describe('beside the note', () => {
  test('an original opens in the note\'s place at the note\'s width, and another chip switches the file', async ({ page, browser }) => {
    test.skip(test.info().project.name !== 'desktop', 'one width is enough');
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Beside the source', body: 'My notes on the paper.' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'notes.txt'), 'Plain text from the upload.');
    const maker = await browser.newPage();
    await maker.setContent('<h1>Paper</h1>');
    fs.writeFileSync(path.join(folder, 'paper.pdf'), await maker.pdf());
    await maker.close();
    await page.goto(`/#/n/${note.id}`);
    const width = (await page.locator('#doc').boundingBox()).width;
    await page.locator('.chip.original', { hasText: 'notes.txt' }).click();
    const aside = page.locator('#note-aside');
    await expect(aside.locator('.aside-text')).toHaveText('Plain text from the upload.');
    await expect(page.locator('.sheet.viewer')).toHaveCount(0);
    expect(Math.abs((await aside.boundingBox()).width - width)).toBeLessThan(2);
    await page.locator('.chip.original', { hasText: 'paper.pdf' }).click();
    await expect(aside.locator('iframe.aside-frame')).toHaveAttribute('src', /paper\.pdf\?view=1$/);
    await expect(page.locator('#note-aside-facts')).toContainText('paper.pdf');
    await page.locator('#side [data-action="home"]').first().click();
    await expect(aside).toHaveCount(0);
  });

  test('the Tasks panel sits folded in the bottom corner, opens on hover or keyboard focus only, and stays across pages', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the panel is for wider screens');
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.request.post('/api/tasks', { data: { title: `Panel task ${tag}` } });
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.goto('/#/settings');
    await page.reload();
    const box = page.locator('#dock #tasks-window');
    const body = box.locator('.tasks-body');
    await expect(box).toBeVisible();
    await expect(body, 'folded at start').toBeHidden();
    await expect(box.locator('.tasks-badge')).toHaveText(/^\d+\+?$/);
    await expect(box).toHaveAttribute('aria-label', /^Tasks: .*to do$/);
    const pill = await box.boundingBox();
    expect(pill.width).toBeLessThan(90);
    expect(await page.evaluate(() => localStorage.getItem('leo-tasks-folded'))).toBeNull();
    await expect(page.locator('#side [data-action="tasks-toggle"]')).toHaveCount(0);
    await box.hover();
    await expect(body, 'hovering opens the panel').toContainText(`Panel task ${tag}`);
    await expect(box).toHaveClass(/\bopen\b/);
    await page.mouse.move(640, 300);
    await page.mouse.move(pill.x + pill.width / 2, pill.y + pill.height / 2);
    await expect(box, 'coming back while it folds opens it again').toHaveClass(/\bopen\b/);
    await expect(body).toBeVisible();
    await expect.poll(() => box.evaluate((el) => el.getAnimations().length), 'the reveal finishes').toBe(0);
    await body.hover();
    await expect(body, 'it stays open while the pointer is over it').toBeVisible();
    await page.mouse.move(640, 300);
    await expect(body, 'leaving folds it again').toBeHidden();
    await expect(box).not.toHaveClass(/\bopen\b/);
    const spot = await box.boundingBox();
    await page.mouse.click(spot.x + spot.width / 2, spot.y + spot.height / 2);
    await expect(body).toBeVisible();
    await page.mouse.move(640, 300);
    await expect(body, 'a click does not keep it open').toBeHidden();
    await box.focus();
    await expect(body, 'keyboard focus opens it').toBeVisible();
    await page.keyboard.press('Tab');
    await expect(box.locator('.tasks-new')).toBeFocused();
    await expect(body).toBeVisible();
    await page.evaluate(() => document.activeElement.blur());
    await expect(body, 'moving focus away folds it').toBeHidden();
    await page.goto('/#/search/panel');
    await expect(box).toBeVisible();
    await expect(body).toBeHidden();
    await page.reload();
    await expect(box).toBeVisible();
    await expect(body, 'folded again after a reload').toBeHidden();
    await box.hover();
    await expect(box).toHaveClass(/\bopen\b/);
    await page.evaluate(() => { location.hash = '#/calendar'; });
    await expect(page.locator('#tasks-window')).toBeHidden();
    await expect(box, 'a card hidden while open folds').not.toHaveClass(/\bopen\b/);
  });

  test('the note layout switches to its wide form at 900 px of room and back as the window changes', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'wide layouts are for desktop windows');
    const note = await (await page.request.post('/api/notes', { data: { title: 'Wide switch', body: '## One\n\nWords.' } })).json();
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.goto(`/#/n/${note.id}`);
    const layout = page.locator('.note-layout');
    await expect(layout, 'set before the note is drawn').toHaveAttribute('data-wide', '');
    expect(await layout.evaluate((el) => el.clientWidth)).toBeGreaterThanOrEqual(900);
    await page.setViewportSize({ width: 900, height: 900 });
    await expect(layout).not.toHaveAttribute('data-wide', '');
    expect(await layout.evaluate((el) => el.clientWidth)).toBeLessThan(900);
    await expect(page.locator('.page-marks')).toBeHidden();
    await page.setViewportSize({ width: 1400, height: 900 });
    await expect(layout).toHaveAttribute('data-wide', '');
    await expect(page.locator('.note-outline .outline-ticks')).toBeVisible();
  });

  test('a narrow window keeps a toggleable explorer beside the rail', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the drawer is for windows between phone and wide');
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    const note = await (await page.request.post('/api/notes', { data: { title: `Drawer ${tag}`, body: 'Words.' } })).json();
    await page.setViewportSize({ width: 900, height: 800 });
    await page.goto('/#/');
    await page.reload();
    const side = page.locator('#side');
    const fold = side.locator('.side-fold');
    await expect(side.locator('.side-rail')).toBeVisible();
    await expect(page.locator('#menu')).toBeHidden();
    await expect(side.locator('.side-explorer')).toBeHidden();
    await expect(fold).toHaveAttribute('aria-expanded', 'false');
    await fold.click();
    await expect(side.locator('.side-explorer')).toBeVisible();
    await expect(fold).toHaveAttribute('aria-expanded', 'true');
    await expect(page.locator('#side-filter')).toBeFocused();
    const main = await page.locator('main').boundingBox();
    expect(main.x, 'the drawer overlays the page instead of pushing it').toBeLessThan(120);
    await page.keyboard.press('Escape');
    await expect(side.locator('.side-explorer')).toBeHidden();
    await expect(fold).toBeFocused();
    await fold.click();
    await page.locator('#side-filter').fill(`Drawer ${tag}`);
    await side.locator('.side-note', { hasText: `Drawer ${tag}` }).first().click();
    await expect(page).toHaveURL(new RegExp(`#/n/${note.id}`));
    await expect(side.locator('.side-explorer')).toBeHidden();
    await fold.click();
    await page.mouse.click(860, 400);
    await expect(side.locator('.side-explorer')).toBeHidden();
  });

  test('home lists today', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the panel is for wider screens');
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    const today = new Date().toLocaleDateString('en-CA');
    await page.request.post('/api/tasks', { data: { title: `Home today ${tag}`, date: today, time: '00:00' } });
    await page.request.post('/api/tasks', { data: { title: `Home later ${tag}` } });
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.goto('/#/');
    await page.reload();
    const list = page.locator('#home-today');
    const row = list.locator('.tasks-row', { hasText: `Home today ${tag}` });
    await expect(row).toBeVisible();
    await expect(list.locator('.tasks-row', { hasText: `Home later ${tag}` }), 'only today and earlier').toHaveCount(0);
    await expect(page.locator('#tasks-window'), 'the card gives way on Home').toBeHidden();
    const ask = await page.locator('.home-ask').boundingBox();
    const at = await list.boundingBox();
    expect(at.y, 'under the quick bar').toBeGreaterThanOrEqual(ask.y + ask.height);
    expect(at.x + at.width, 'inside the page').toBeLessThanOrEqual(1280);
    await row.locator('.sched-check').click();
    await expect(row.locator('.sched-check'), 'the box ticks once leo saved it').toBeChecked();
    await page.waitForTimeout(1500);
    await expect(row, 'a finished task stays, ticked, until midnight').toHaveClass(/done/);
    await expect(page.locator('#home-day')).toHaveText(new Date().toLocaleDateString(await page.evaluate(() => navigator.language), { weekday: 'long', month: 'long', day: 'numeric' }));
    await expect(page.locator('#home-line')).not.toBeEmpty();
    await page.goto('/#/settings');
    await expect(page.locator('#tasks-window')).toBeVisible();
    await expect(page.locator('#home-today')).toHaveCount(0);
  });

  test('tasks window folds while reading, also on note pages', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the panel is for wider screens');
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.request.post('/api/tasks', { data: { title: `Reading task ${tag}`, date: new Date().toLocaleDateString('en-CA') } });
    const note = await (await page.request.post('/api/notes', { data: { title: `Reading ${tag}`, body: 'Words to read.' } })).json();
    await page.setViewportSize({ width: 1280, height: 1000 });
    const box = page.locator('#dock #tasks-window');
    const body = box.locator('.tasks-body');
    await page.reload();
    for (const [hash, content] of [[`#/n/${note.id}`, '#doc'], ['#/new', '#doc'], ['#/record', '#rec-box']]) {
      await page.goto(`/${hash}`);
      await expect(page.locator(content), hash).toBeVisible();
      await expect(box, hash).toBeVisible();
      await expect(body, `${hash} starts folded`).toBeHidden();
      await expect(box.locator('.tasks-badge'), hash).toHaveText(/^\d+\+?$/);
      const chip = await box.boundingBox();
      const text = await page.locator(content).boundingBox();
      expect(chip.width, hash).toBeLessThan(90);
      expect(chip.x + chip.width <= text.x || chip.y + chip.height <= text.y || chip.x >= text.x + text.width || chip.y >= text.y + text.height, `${hash}: the chip stays off the page`).toBe(true);
      await box.hover();
      await expect(body, `${hash}: hovering opens it`).toContainText(`Reading task ${tag}`);
      await page.mouse.move(640, 300);
      await expect(body, `${hash}: leaving folds it`).toBeHidden();
      expect(await page.evaluate(() => Object.keys(localStorage).filter((key) => /tasks/.test(key)))).toEqual([]);
    }
  });

  test('with Felix open, the notes scrollbar sits beside the notes, not past the chat', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'Felix covers the page on phones');
    const body = Array.from({ length: 160 }, (_, i) => `Line ${i + 1} of a long note to scroll through.`).join('\n\n');
    const note = await (await page.request.post('/api/notes', { data: { title: `Long scroll ${Date.now().toString(36)}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('Line 160');
    const bar = page.locator('.page-bar');
    await expect(bar).toBeHidden();
    expect(await page.evaluate(() => document.documentElement.classList.contains('page-bar-on'))).toBe(false);
    await page.locator('#chat-toggle').click();
    await expect(page.locator('aside.chat')).toBeVisible();
    await page.locator('aside.chat').evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
    await expect(bar).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.classList.contains('page-bar-on'))).toBe(true);
    const chatLeft = (await page.locator('aside.chat').boundingBox()).x;
    const barBox = await bar.boundingBox();
    expect(barBox.x + barBox.width).toBeLessThanOrEqual(chatLeft + 1);
    expect(barBox.x + barBox.width).toBeGreaterThan(chatLeft - 4);
    const thumb = page.locator('.page-bar-thumb');
    await page.mouse.move(10, 400);
    await expect.poll(() => thumb.evaluate((el) => getComputedStyle(el).opacity), { message: 'the scroll thumb hides at rest, so no rule stands between the note and Felix' }).toBe('0');
    const start = await thumb.boundingBox();
    await page.mouse.move(start.x + start.width / 2, start.y + 10);
    await expect.poll(() => thumb.evaluate((el) => getComputedStyle(el).opacity)).toBe('1');
    await page.mouse.down();
    await page.mouse.move(start.x + start.width / 2, start.y + 210, { steps: 5 });
    await page.mouse.up();
    await expect.poll(() => page.evaluate(() => scrollY)).toBeGreaterThan(200);
    const moved = await thumb.boundingBox();
    expect(moved.y).toBeGreaterThan(start.y + 150);
    await page.mouse.wheel(0, -100000);
    await expect.poll(() => page.evaluate(() => scrollY)).toBe(0);
    await page.locator('[data-chat="close"]').click();
    await expect(page.locator('aside.chat')).toBeHidden();
    await expect(bar).toBeHidden();
    expect(await page.evaluate(() => document.documentElement.classList.contains('page-bar-on'))).toBe(false);
    await page.request.delete(`/api/notes/${note.id}`);
  });
});

















test.describe('Felix organizes', () => {
  test('a suggested move is applied from its card and undone', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.request.post('/api/dirs', { data: { path: `archive-${tag}` } });
    const note = await (await page.request.post('/api/notes', { data: { title: `Move me ${tag}`, body: 'x' } })).json();
    await page.route('**/api/chat', async (route) => {
      const lines = [{ sources: [] }, { proposal: { kind: 'move_note', summary: `Move “Move me ${tag}” to archive-${tag}`, note: note.id, title: `Move me ${tag}`, to: `archive-${tag}` } }, { t: 'I suggest moving it.' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('tidy my notes');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.proposal.organize');
    await expect(card).toContainText(`Move “Move me ${tag}” to archive-${tag}`);
    await card.locator('[data-chat="apply"]').click();
    await expect(card).toContainText('Done');
    const where = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).directory;
    await expect.poll(where).toBe(`archive-${tag}`);
    await card.locator('[data-chat="undo"]').click();
    await expect.poll(where).toBe('');
    await expect(card.locator('[data-chat="apply"]')).toBeVisible();
  });
});






test('Felix permission changes reach an active reply and Ask first keeps later edits as proposals', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('leo-felix-access', 'auto'));
  const updates = [];
  const acts = [];
  let access = 'auto';
  let fail = false;
  await page.route('**/api/felix/act', route => {
    const proposal = route.request().postDataJSON();
    acts.push(proposal);
    return route.fulfill({ json: { revision: acts.length, undo: { ...proposal, before: proposal.schedule, schedule: proposal.before } } });
  });
  await page.route('**/api/chat/*/access', route => { const value = route.request().postDataJSON().access; updates.push(value); if (fail) return route.fulfill({ status: 503, json: { error: 'Unavailable' } }); access = value; return route.fulfill({ json: { access } }); });
  await page.goto('/#/');
  await page.reload();
  await page.evaluate(() => {
    const real = window.fetch;
    window.fetch = async (url, options) => {
      if (url !== '/api/chat') return real(url, options);
      const encoder = new TextEncoder();
      const stream = new ReadableStream({ start(controller) {
        controller.enqueue(encoder.encode('{"answer":"active-permission-reply"}\n{"t":"I am preparing the change."}\n'));
        window.finishPermissionReply = proposal => { if (proposal) controller.enqueue(encoder.encode(JSON.stringify({ proposal }) + '\n')); controller.enqueue(encoder.encode('{"t":" Ready."}\n{"done":true}\n')); controller.close(); };
        options.signal.addEventListener('abort', () => controller.error(new DOMException('Stopped', 'AbortError')), { once: true });
      } });
      return new Response(stream, { status: 200, headers: { 'Content-Type': 'application/x-ndjson' } });
    };
  });
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('Create a study note');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat .msg:not(.user)').last()).toContainText('preparing');
  await page.locator('#chat-access').click();
  await page.locator('[data-chat="access-pick"][data-access="ask"]').click();
  await expect.poll(() => updates).toEqual(['ask']);
  expect(access).toBe('ask');
  await page.evaluate(() => window.finishPermissionReply({ kind: 'create', title: 'Daily study', body: 'Study on weekdays', folder: '' }));
  await expect(page.locator('.proposal [data-chat="apply"]')).toBeVisible();
  await expect(page.locator('#chat .msg:not(.user)').last()).toContainText('I am preparing the change. Ready.');
  await page.locator('#chat-input').fill('Plan my weekday studying');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
  await page.evaluate(() => window.finishPermissionReply({ kind: 'add_task', summary: 'Add the task “Study weekdays” · every Mon, Tue, Wed, Thu, Fri', title: 'Study weekdays', date: '2026-10-12', repeat: [0, 1, 2, 3, 4] }));
  const taskCard = page.locator('.proposal.add_task');
  await expect(taskCard).toContainText('Study weekdays');
  expect(acts).toHaveLength(0);
  await taskCard.locator('[data-chat="apply"]').click();
  await expect(taskCard.locator('[data-chat="undo"]')).toBeVisible();
  expect(acts[0].kind).toBe('add_task');
  expect(acts[0].repeat).toEqual([0, 1, 2, 3, 4]);
  await taskCard.locator('[data-chat="undo"]').click();
  await expect(taskCard.locator('[data-chat="apply"]')).toBeVisible();
  expect(acts).toHaveLength(2);
  await page.locator('#chat-input').fill('One more study task');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
  await page.locator('#chat-access').click();
  await page.locator('[data-chat="access-pick"][data-access="read"]').click();
  await expect.poll(() => updates.at(-1)).toBe('read');
  await page.evaluate(() => window.finishPermissionReply(null));
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Send');
  await page.locator('#chat-input').fill('One more study plan');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
  fail = true;
  await page.locator('#chat-access').click();
  await page.locator('[data-chat="access-pick"][data-access="ask"]').click();
  await expect(page.locator('.toast.bad')).toContainText('Permissions could not be changed');
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Send');
  expect(await page.locator('#chat-access').textContent()).toContain('Ask first');
});

test('Felix keeps his passages in order with his tool activity folded above them after reload', async ({ page }) => {
  const events = [{ t: 'I will read your submitted code first.\n\n' }, { step: 'Read your submitted attempt', tool: 'open_note' }, { t: 'Your sliding-window approach fits the problem.\n\n' }, { step: 'Searched your notes', tool: 'search_notes' }, { step: 'Read your saved notes', tool: 'open_note' }, { t: 'Next, check repeated categories and ties.' }, { done: true }];
  await page.route('**/api/chat', (route) => route.fulfill({ contentType: 'application/x-ndjson', body: events.map((event) => JSON.stringify(event)).join('\n') + '\n' }));
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('Review my code.');
  await page.locator('#chat-input').press('Enter');
  const answer = page.locator('.msg.leo').last();
  await expect(answer).toContainText('Next, check repeated categories and ties.');
  const ordered = async () => {
    const trace = answer.locator('.msg-trace');
    if (!(await trace.evaluate((node) => node.open))) await trace.locator('> summary').click();
    const content = await answer.innerText();
    const pieces = ['Read your submitted attempt', 'Searched your notes', 'Read your saved notes', 'I will read your submitted code first.', 'Your sliding-window approach fits the problem.', 'Next, check repeated categories and ties.'];
    for (let i = 1; i < pieces.length; i++) expect(content.indexOf(pieces[i])).toBeGreaterThan(content.indexOf(pieces[i - 1]));
  };
  await ordered();
  await page.reload();
  await page.locator('#chat-toggle').click();
  await expect(answer).toContainText('Next, check repeated categories and ties.');
  await ordered();
});

















test('Felix shows one live step, then one line, and marks an answer from outside the notes', async ({ page }) => {
  const lines = [
    { step: 'Searched your notes for “lab safety”', tool: 'search_notes', found: [] },
    { wait: 700 },
    { step: 'Worked out the numbers', tool: 'calculate', found: [] },
    { wait: 700 },
    { t: 'Wear goggles.' },
    { wait: 700 },
    { done: true },
  ];
  await page.addInitScript((script) => {
    const real = window.fetch.bind(window);
    window.fetch = (url, options) => {
      if (String(url) !== '/api/chat') return real(url, options);
      const encoder = new TextEncoder();
      const body = new ReadableStream({
        async start(stream) {
          for (const line of script) {
            if (line.wait) await new Promise((done) => setTimeout(done, line.wait));
            else stream.enqueue(encoder.encode(`${JSON.stringify(line)}\n`));
          }
          stream.close();
        },
      });
      return Promise.resolve(new Response(body, { status: 200, headers: { 'content-type': 'application/x-ndjson' } }));
    };
  }, lines);
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('what are the lab safety rules?');
  await page.locator('#chat-input').press('Enter');
  const answer = page.locator('.msg.leo').last();
  await expect(answer.locator('.msg-step')).toHaveText(['Searching your notes for “lab safety”…']);
  const blot = answer.locator('.msg-status .msg-step-busy');
  await expect(blot, 'the live step shows Felix\'s blot, not a spinner').toHaveAttribute('aria-label', 'working');
  await expect(blot.locator('use')).toHaveAttribute('href', '#leo-mark');
  expect(await blot.evaluate((el) => { const probe = document.createElement('i'); probe.style.color = 'var(--felix)'; el.parentElement.append(probe); const felix = getComputedStyle(probe).color; probe.remove(); return getComputedStyle(el).color === felix; })).toBe(true);
  await expect(answer.locator('.msg-status .msg-step-icon'), 'the blot stands in for the tool icon while it works').toHaveCount(0);
  await expect(answer.locator('.msg-step')).toHaveText(['Working out the numbers…']);
  await expect(answer.locator('.msg-trace')).toHaveCount(0);
  await expect(answer.locator('.prose')).toHaveText('Wear goggles.');
  const inReply = () => answer.evaluate((el) => Math.round(el.querySelector('.prose').getBoundingClientRect().y - el.getBoundingClientRect().y));
  const status = await answer.locator('.msg-status').boundingBox();
  const words = await answer.locator('.prose').boundingBox();
  expect(status.y, 'the live step sits above the words, where the summary will be').toBeLessThan(words.y);
  const before = await inReply();
  await expect(answer.locator('.msg-trace > summary'), 'the summary says how long the work took').toHaveText(/^Searched your notes · \d+ s$/);
  expect(await inReply(), 'the words do not move inside the reply when Felix finishes').toBe(before);
  await expect(answer.locator('.msg-steps .msg-step').first()).toBeHidden();
  await expect(answer.locator('.msg-outside')).toHaveText('Not from your notes');
});

test('status line: the working blot sits on the middle of the words beside it', async ({ page }) => {
  await page.addInitScript(() => {
    const real = window.fetch.bind(window);
    window.fetch = (url, options) => {
      if (String(url) !== '/api/chat') return real(url, options);
      const body = new ReadableStream({
        async start(stream) {
          stream.enqueue(new TextEncoder().encode(`${JSON.stringify({ step: 'Searched your notes for “heaps”', tool: 'search_notes', found: [] })}\n`));
          await new Promise((done) => setTimeout(done, 4000));
          stream.enqueue(new TextEncoder().encode(`${JSON.stringify({ done: true })}\n`));
          stream.close();
        },
      });
      return Promise.resolve(new Response(body, { status: 200, headers: { 'content-type': 'application/x-ndjson' } }));
    };
  });
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('what is a heap?');
  await page.locator('#chat-input').press('Enter');
  const step = page.locator('.msg.leo .msg-status .msg-step');
  await expect(step).toHaveText('Searching your notes for “heaps”…');
  const off = await step.evaluate((el) => {
    const blot = el.querySelector('.msg-step-busy').getBoundingClientRect();
    const probe = document.createElement('i');
    probe.style.cssText = 'display: inline-block; width: 0; height: 1ex; vertical-align: baseline';
    el.querySelector('span').append(probe);
    const ex = probe.getBoundingClientRect();
    probe.remove();
    return Math.abs((blot.top + blot.bottom) / 2 - (ex.top + ex.bottom) / 2);
  });
  expect(off, 'the blot centre is the centre of the lower-case letters').toBeLessThanOrEqual(1);
});

test('felix accent: his replies carry the leo mark and his name in his blue, and buttons stay ink', async ({ page }) => {
  await page.route('**/api/chat', (route) => route.fulfill({ contentType: 'application/x-ndjson', body: `${JSON.stringify({ t: 'A heap keeps the smallest on top.' })}\n${JSON.stringify({ done: true })}\n` }));
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('what is a heap?');
  await page.locator('#chat-input').press('Enter');
  const answer = page.locator('.msg.leo').last();
  await expect(answer.locator('.prose')).toHaveText('A heap keeps the smallest on top.');
  const head = answer.locator('.msg-who');
  await expect(head).toHaveText('Felix');
  await expect(head.locator('svg use')).toHaveAttribute('href', '#leo-mark');
  const colours = await answer.evaluate((el) => {
    const probe = document.createElement('i');
    probe.style.color = 'var(--felix)';
    el.append(probe);
    const felix = getComputedStyle(probe).color;
    probe.remove();
    return { felix, head: getComputedStyle(el.querySelector('.msg-who')).color, words: getComputedStyle(el.querySelector('.prose')).color, act: getComputedStyle(el.querySelector('.msg-act')).color };
  });
  expect(colours.head, 'the head is in Felix blue').toBe(colours.felix);
  expect(colours.words, 'his words stay ink').not.toBe(colours.felix);
  expect(colours.act, 'his buttons stay ink').not.toBe(colours.felix);
});

test('an opened trace stays open through the next reply, and a failed read is not counted', async ({ page }) => {
  const reply = (lines) => lines.map((l) => JSON.stringify(l)).join('\n') + '\n';
  await page.route('**/api/chat', (route) => route.fulfill({ contentType: 'application/x-ndjson', body: reply([
    { step: 'Searched your notes for “lab”', tool: 'search_notes', found: [] },
    { step: 'Looked for “x”', tool: 'open_note', found: [], failed: true },
    { t: 'Wear goggles.' },
    { done: true },
  ]) }));
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('what are the lab rules?');
  await page.locator('#chat-input').press('Enter');
  const first = page.locator('.msg.leo').first();
  await expect(first.locator('.msg-trace > summary')).toHaveText(/^Searched your notes( · \d+ s)?$/);
  await expect(first.locator('.msg-outside')).toHaveText('Not from your notes');
  await first.locator('.msg-trace > summary').click();
  await expect(first.locator('.msg-trace')).toHaveAttribute('open', '');
  await page.locator('#chat-input').fill('and the next one?');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('.msg.leo')).toHaveCount(2);
  await expect(page.locator('.msg.leo').last()).toContainText('Wear goggles.');
  await expect(first.locator('.msg-trace')).toHaveAttribute('open', '');
});

test('choosing the model automatically can be turned off and stays off', async ({ page }) => {
  await page.goto('/#/settings/felix');
  const choice = page.locator('input[data-margin-pref="auto_model"]');
  await expect(choice).toBeChecked();
  await choice.click();
  await expect.poll(async () => (await (await page.request.get('/api/margin')).json()).auto_model).toBe(false);
  await page.reload();
  await expect(choice).not.toBeChecked();
  await choice.click();
  await expect.poll(async () => (await (await page.request.get('/api/margin')).json()).auto_model).toBe(true);
});

test('the same step repeated in a row shows once with how many times', async ({ page }) => {
  const step = (text, tool = 'open_note') => JSON.stringify({ step: text, tool, found: [] });
  await page.route('**/api/chat', (route) => route.fulfill({ contentType: 'application/x-ndjson', body: [step('Read “Pipelining”'), step('Read “Pipelining” again (unchanged)'), step('Read “Pipelining” again (unchanged)'), step('Read “Pipelining” again (unchanged)'), step('Retried open note: "note" is missing.', 'open_note'), JSON.stringify({ t: 'It overlaps instructions.' }), JSON.stringify({ done: true })].join('\n') + '\n' }));
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('what does my pipelining note say?');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('.msg.leo').last()).toContainText('It overlaps instructions.');
  await page.locator('.msg.leo').last().locator('.msg-trace > summary').click();
  const steps = page.locator('.msg.leo').last().locator('.msg-step');
  await expect(steps).toHaveCount(3);
  await expect(steps.nth(1)).toContainText('Read “Pipelining” again (unchanged)');
  await expect(steps.nth(1).locator('.msg-step-times')).toHaveText('×3');
  await expect(steps.nth(0).locator('.msg-step-times')).toHaveCount(0);
  await expect(steps.nth(2)).toContainText('Retried open note: "note" is missing.');
});

test('the Tasks panel keeps its own size through page changes and refreshes', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'phones have no Tasks panel');
  await page.setViewportSize({ width: 1400, height: 1000 });
  const tag = Date.now().toString(36);
  const tasks = [];
  for (let n = 0; n < 3; n++) tasks.push(await (await page.request.post('/api/tasks', { data: { title: `Steady ${tag} ${n}: attempt section ${n} before reading its solutions on Piazza, it is practice and is not graded` } })).json());
  const note = await (await page.request.post('/api/notes', { data: { title: `Steady note ${tag}`, body: 'x' } })).json();
  const panel = page.locator('#tasks-window');
  const steady = async () => {
    await page.mouse.move(5, 5);
    await panel.hover();
    await expect(panel.locator('.tasks-body')).toBeVisible();
    expect((await panel.boundingBox()).width).toBeLessThanOrEqual(340);
  };
  await page.goto('/#/settings');
  await page.reload();
  await steady();
  for (const hash of [`#/n/${note.id}`, '#/search/steady', '#/record', `#/n/${note.id}`, '#/settings']) {
    await page.evaluate((h) => { location.hash = h; }, hash);
    await page.waitForTimeout(150);
    await steady();
  }
  for (const task of tasks) await page.request.delete(`/api/tasks/${task.id}`);
  await page.request.delete(`/api/notes/${note.id}`);
});



test('letting go of a picture’s corner outside the page ends the resize and keeps the size', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
  const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="300" height="150"><rect width="300" height="150" fill="#9ab"/></svg>');
  const note = await (await page.request.post('/api/notes', { data: { title: `Picture out ${Date.now().toString(36)}`, body: `Intro line\n\n![Box](data:image/svg+xml;utf8,${svg})\n\nAfter line` } })).json();
  const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
  await page.goto(`/#/n/${note.id}`);
  const img = page.locator('#doc img.note-img');
  await expect(img).toBeVisible();
  await img.click();
  const shown = Math.round((await img.boundingBox()).width);
  const at = await page.locator('.pic-handle').boundingBox();
  const x = at.x + at.width / 2, y = at.y + at.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 100, y, { steps: 4 });
  const cdp = await page.context().newCDPSession(page);
  for (const dx of [140, 180, 220]) await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x - dx, y, button: 'none', buttons: 0 });
  await expect.poll(body, { timeout: 5000 }).toContain(`![Box|${shown - 100}](data:image`);
  await expect(page.locator('#doc img.note-img')).toHaveAttribute('width', String(shown - 100));
  await page.request.delete(`/api/notes/${note.id}`);
});

test('a picture in a note can be resized by dragging its corner, and keeps that size', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
  const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="300" height="150"><rect width="300" height="150" fill="#9ab"/></svg>');
  const note = await (await page.request.post('/api/notes', { data: { title: `Picture ${Date.now().toString(36)}`, body: `Intro line\n\n![Box](data:image/svg+xml;utf8,${svg})\n\nAfter line` } })).json();
  const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
  await page.goto(`/#/n/${note.id}`);
  const img = page.locator('#doc img.note-img');
  await expect(img).toBeVisible();
  const beside = await img.boundingBox();
  await page.mouse.click(beside.x + beside.width + 40, beside.y + beside.height / 2);
  await expect(img).toHaveClass(/picked/);
  await expect(page.locator('#doc textarea')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(img).not.toHaveClass(/picked/);
  await img.click();
  await expect(img).toHaveClass(/picked/);
  await expect(page.locator('#doc textarea')).toHaveCount(0);
  const handle = page.locator('.pic-handle');
  const shown = Math.round((await img.boundingBox()).width);
  await expect(page.locator('.pic-size')).toHaveText(`${shown} px`);
  const at = await handle.boundingBox();
  await page.mouse.move(at.x + at.width / 2, at.y + at.height / 2);
  await page.mouse.down();
  await page.mouse.move(at.x + at.width / 2 - 60, at.y + at.height / 2, { steps: 4 });
  await page.mouse.move(at.x + at.width / 2 - 120, at.y + at.height / 2, { steps: 4 });
  await page.mouse.up();
  const smaller = shown - 120;
  await expect.poll(body, { timeout: 5000 }).toContain(`![Box|${smaller}](data:image`);
  await expect(page.locator('#doc img.note-img')).toHaveAttribute('width', String(smaller));
  await expect(page.locator('.pic-size')).toHaveText(`${smaller} px`);
  await page.locator('.pic-handle').press('ArrowRight');
  await expect.poll(body, { timeout: 5000 }).toContain(`![Box|${smaller + 20}](`);
  await page.locator('.pic-handle').dblclick();
  await expect.poll(body, { timeout: 5000 }).toMatch(/!\[Box\]\(data:image/);
  await page.keyboard.press('Escape');
  await expect(page.locator('.pic-frame')).toHaveCount(0);
  await page.locator('#doc img.note-img').dblclick();
  await expect(page.locator('#doc img.note-img')).toHaveClass(/picked/);
  await expect(page.locator('#doc textarea')).toHaveCount(0);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('Backspace or Delete on a selected picture takes it out of the note and stays on the note', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'the click is a mouse click');
  const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="300" height="150"><rect width="300" height="150" fill="#9ab"/></svg>');
  const note = await (await page.request.post('/api/notes', { data: { title: `Gone picture ${Date.now().toString(36)}`, body: `Intro line\n\n![Box](data:image/svg+xml;utf8,${svg}) ![Two](data:image/svg+xml;utf8,${svg})\n\nAfter line` } })).json();
  const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
  await page.goto('/#/');
  await page.goto(`/#/n/${note.id}`);
  const img = page.locator('#doc img.note-img');
  await expect(img).toHaveCount(2);
  await img.first().click();
  await expect(img.first()).toHaveClass(/picked/);
  await page.keyboard.press('Backspace');
  await expect.poll(body, { timeout: 5000 }).not.toContain('![Box]');
  expect(await body()).toMatch(/^Intro line\n\n *!\[Two\]\(data:image[^)]*\)\n\nAfter line$/);
  await expect(page.locator('.pic-frame')).toHaveCount(0);
  await expect(page.locator('#doc textarea')).toBeFocused();
  expect(page.url()).toContain(`#/n/${note.id}`);
  await page.keyboard.press('Escape');
  const backspace = (selector) => page.evaluate((s) => { const el = s ? document.createElement(s) : document.body; if (s) document.body.append(el); const e = new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true }); el.dispatchEvent(e); if (s) el.remove(); return e.defaultPrevented; }, selector);
  expect(await backspace(''), 'Backspace outside a text field never goes back a page').toBe(true);
  expect(await backspace('input'), 'Backspace in a text field still deletes').toBe(false);
  expect(await backspace('textarea')).toBe(false);
  await expect(img).toHaveCount(1);
  await img.click();
  await page.locator('.pic-handle').focus();
  await page.keyboard.press('Delete');
  await expect.poll(body, { timeout: 5000 }).not.toContain('![Two]');
  expect(await body()).toMatch(/^Intro line\n+ *\n*After line$/);
  expect(page.url()).toContain(`#/n/${note.id}`);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('clicking text beside a picture puts the caret in the text, not beside the picture', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'the click is a mouse click');
  const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#9ab"/></svg>');
  const note = await (await page.request.post('/api/notes', { data: { title: `Beside ${Date.now().toString(36)}`, body: `Figure ![Box](data:image/svg+xml;utf8,${svg}) shows the loop` } })).json();
  const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
  await page.goto(`/#/n/${note.id}`);
  const line = page.locator('#doc .blk').first();
  await expect(line.locator('img.note-img')).toBeVisible();
  const word = await line.evaluate((el) => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let text = walker.nextNode();
    while (text && !text.data.trim()) text = walker.nextNode();
    const range = document.createRange();
    range.setStart(text, text.data.indexOf('F'));
    range.setEnd(text, text.data.indexOf('F') + 1);
    const { x, y, height } = range.getBoundingClientRect();
    return { x, y, height };
  });
  await page.mouse.click(word.x + 1, word.y + word.height / 2);
  await expect(page.locator('#doc textarea')).toBeVisible();
  await page.keyboard.type('X');
  await expect.poll(body, { timeout: 5000 }).toMatch(/^XFigure !\[Box\]/);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('a click or a double-click on a picture outlines it once and opens no editor', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'the press is sampled before the mouse comes up');
  const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="300" height="150"><rect width="300" height="150" fill="#9ab"/></svg>');
  const note = await (await page.request.post('/api/notes', { data: { title: `Picked ${Date.now().toString(36)}`, body: `Intro line\n\n![Box](data:image/svg+xml;utf8,${svg})\n\nAfter line` } })).json();
  await page.goto(`/#/n/${note.id}`);
  const img = page.locator('#doc img.note-img');
  await expect(img).toBeVisible();
  const outlined = () => page.evaluate(() => [...document.querySelectorAll('#doc *')].filter((el) => { const style = getComputedStyle(el); return style.outlineStyle !== 'none' && parseFloat(style.outlineWidth) > 0; }).map((el) => el.className));
  const box = await img.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  expect(await outlined()).toEqual([]);
  await page.mouse.up();
  await expect(img).toHaveClass(/picked/);
  expect(await outlined()).toHaveLength(1);
  await img.dblclick();
  await expect(img).toHaveClass(/picked/);
  await expect(page.locator('#doc textarea')).toHaveCount(0);
  expect(await outlined()).toHaveLength(1);
  expect(await page.evaluate(() => getSelection().isCollapsed)).toBe(true);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('in a checkbox item a click on a picture selects it, a click beside it puts the caret after it, and Enter there starts a checkbox', async ({ page }) => {
  const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="300" height="150"><rect width="300" height="150" fill="#9ab"/></svg>');
  const note = await (await page.request.post('/api/notes', { data: { title: `Boxed ${Date.now().toString(36)}`, body: `- [ ] ![Box](data:image/svg+xml;utf8,${svg})` } })).json();
  await page.goto(`/#/n/${note.id}`);
  const img = page.locator('#doc img.note-img');
  const area = page.locator('#doc textarea');
  await expect(img).toBeVisible();
  await img.click();
  await expect(img).toHaveClass(/picked/);
  await expect(page.locator('.pic-handle')).toBeVisible();
  await expect(area).toHaveCount(0);
  const right = await img.boundingBox();
  await page.mouse.click(right.x + right.width + 40, right.y + right.height / 2);
  await expect(area).toHaveValue(/^!\[Box\]\(data:image/);
  expect(await area.evaluate((el) => el.selectionStart === el.value.length)).toBe(true);
  await page.keyboard.press('Enter');
  await expect(area).toHaveValue('');
  await expect(page.locator('#doc input[data-box]')).toHaveCount(2);
  await expect(page.locator('#doc .blk').first().locator('img.note-img')).toHaveCount(1);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('find in note counts the matches, steps through them and gives focus back', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: `Sought ${Date.now().toString(36)}`, body: 'Alpha one\n\nalpha two\n\n- [ ] ALPHA three' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await page.locator('#doc .blk').first().click();
  await expect(page.locator('#doc textarea')).toBeFocused();
  await page.keyboard.press('ControlOrMeta+f');
  const input = page.getByRole('textbox', { name: 'Find in note' });
  const count = page.locator('.note-find [role="status"]');
  const painted = () => page.evaluate(() => [CSS.highlights.get('leo-search')?.size ?? 0, String(CSS.highlights.get('leo-found')?.values().next().value ?? '')]);
  await expect(input).toBeFocused();
  await input.fill('alpha');
  await expect(count).toHaveText('1 of 3');
  await expect(page.locator('#doc textarea')).toHaveCount(0);
  expect(await painted()).toEqual([3, 'Alpha']);
  await input.press('Enter');
  await expect(count).toHaveText('2 of 3');
  expect(await painted()).toEqual([3, 'alpha']);
  await input.press('Shift+Enter');
  await expect(count).toHaveText('1 of 3');
  await page.getByRole('button', { name: 'Previous match' }).click();
  await expect(count).toHaveText('3 of 3');
  await expect(input).toBeFocused();
  expect(await painted()).toEqual([3, 'ALPHA']);
  await page.getByRole('button', { name: 'Next match' }).click();
  await expect(count).toHaveText('1 of 3');
  await input.fill('when');
  await expect(count).toHaveText('No matches');
  await input.press('Escape');
  await expect(page.locator('.note-find')).toHaveCount(0);
  expect(await painted()).toEqual([0, '']);
  await expect(page.locator('#doc textarea')).toBeFocused();
  await expect(page.locator('#doc textarea')).toHaveValue('Alpha one');
  await page.goto('/#/');
  await expect(page.locator('#doc')).toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+f');
  await expect(page.locator('.note-find')).toHaveCount(0);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('beside a lone picture a double-click or a Shift-click opens its Markdown, and a broken picture never hides it', async ({ page }) => {
  const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="300" height="150"><rect width="300" height="150" fill="#9ab"/></svg>');
  const make = async (body) => (await page.request.post('/api/notes', { data: { title: `Beside ${Date.now().toString(36)}`, body } })).json();
  const pictured = await make(`Intro line\n\n![Box](data:image/svg+xml;utf8,${svg})\n\nAfter line`);
  await page.goto(`/#/n/${pictured.id}`);
  const img = page.locator('#doc img.note-img');
  await expect(img).toBeVisible();
  const box = await img.boundingBox();
  await page.mouse.dblclick(box.x + box.width + 40, box.y + box.height / 2);
  await expect(page.locator('#doc textarea')).toHaveValue(/^!\[Box\]\(data:image/);
  const broken = await make('Intro line\n\n![](missing.png)\n\nAfter line');
  await page.goto(`/#/n/${broken.id}`);
  const line = await page.locator('#doc .blk').nth(2).boundingBox();
  await page.mouse.click(line.x + 200, line.y + line.height / 2);
  await expect(page.locator('#doc textarea')).toHaveValue('![](missing.png)');
  for (const note of [pictured, broken]) await page.request.delete(`/api/notes/${note.id}`);
});

test('Felix\'s button sits in the same place on every page, a little in from the right', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'widths matter on a wide screen');
  await page.setViewportSize({ width: 1600, height: 900 });
  const note = await (await page.request.post('/api/notes', { data: { title: `Edge ${Date.now().toString(36)}`, body: 'A line.' } })).json();
  const board = await (await page.request.post('/api/boards', { data: { name: 'Edge' } })).json();
  let first;
  for (const [hash, selector] of [['#/', '.workspace-empty'], [`#/n/${note.id}`, '.note-columns > .note'], ['#/calendar', '.cal-head'], ['#/boards', '.kanban-head'], ['#/record', 'main']]) {
    await page.goto('/' + hash);
    await expect(page.locator(selector).first()).toBeVisible();
    const box = await page.locator('#chat-toggle').boundingBox();
    first ||= box;
    expect(Math.abs(box.x - first.x), hash).toBeLessThanOrEqual(1);
    expect(Math.abs(box.y - first.y), hash).toBeLessThanOrEqual(1);
  }
  const bar = await page.locator('#bar').boundingBox();
  const inset = bar.x + bar.width - first.x - first.width;
  expect(inset, 'not hard against the edge').toBeGreaterThan(48);
  expect(inset, 'not toward the middle').toBeLessThan(bar.width / 8);
  await page.request.delete(`/api/notes/${note.id}`);
  await page.request.delete(`/api/boards/${board.id}`);
});



test('notes read wide by default, and Settings shows a dragged width and resets it', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'widths matter on a wide screen');
  await page.setViewportSize({ width: 2000, height: 900 });
  const note = await (await page.request.post('/api/notes', { data: { title: `Wide ${Date.now().toString(36)}`, body: 'A line that should have room to breathe.' } })).json();
  const width = async () => { await page.goto(`/#/n/${note.id}`); await expect(page.locator('#doc')).toContainText('room to breathe'); return (await page.locator('.note').boundingBox()).width; };
  expect(await width()).toBeCloseTo(980, -1);
  await page.evaluate(() => window.leoTheme.setPageWidth(700));
  expect(await width()).toBeCloseTo(700, -1);
  await page.goto('/#/settings');
  const row = page.locator('[data-find="note-width"]');
  await expect(row).toContainText('700 px wide.');
  await expect(row.locator('.choice-cards')).toHaveCount(0);
  await row.getByRole('button', { name: 'Reset' }).click();
  await expect(row).toContainText('The default.');
  await expect(row.getByRole('button', { name: 'Reset' })).toBeDisabled();
  expect(await width()).toBeCloseTo(980, -1);
  await page.goto('/#/');
  await expect(page.locator('.workspace-empty')).toBeVisible();
  const content = await page.locator('main').evaluate((main) => { const cs = getComputedStyle(main); return main.getBoundingClientRect().width - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight); });
  expect(content, 'the workspace keeps its own width').toBeLessThanOrEqual(1080);
  await page.request.delete(`/api/notes/${note.id}`);
});

test("Text size and Felix's lettering change reading text at once, survive a reload, and leave Medium as it was", async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'one browser is enough for computed styles');
  const note = await (await page.request.post('/api/notes', { data: { title: `Read ${Date.now().toString(36)}`, body: '## Part\n\nA line of the note.' } })).json();
  const px = () => page.locator('#doc .blk p').first().evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  const chrome = () => page.locator('.note-head h1').evaluate((el) => getComputedStyle(el).fontSize);
  const read = async () => { await page.goto(`/#/n/${note.id}`); await expect(page.locator('#doc')).toContainText('A line of the note'); };
  const letters = () => page.evaluate(() => {
    const family = (html, pick) => { const box = document.createElement('div'); box.innerHTML = html; document.body.append(box); const found = getComputedStyle(box.querySelector(pick)).fontFamily; box.remove(); return found; };
    return { chat: family('<div class="msg leo"><div class="prose">x</div></div>', '.prose'), margin: family('<div class="margin-felix">x</div>', '.margin-felix'), notes: family('<div class="doc">x</div>', '.doc'), hello: family('<div class="chat-hello-lead"><b>x</b></div>', 'b') };
  });
  await read();
  expect(await px()).toBe(16.5);
  const chromeSize = await chrome();
  for (const [choice, size] of [['large', 16.5 * 1.12], ['small', 16.5 * 0.9]]) {
    await page.goto('/#/settings');
    await page.locator(`[data-action="text-size-pick"][data-size="${choice}"]`).click();
    await expect(page.locator(`[data-action="text-size-pick"][data-size="${choice}"]`)).toHaveAttribute('aria-pressed', 'true');
    expect(await page.locator(`.read-sample[data-text-size="${choice}"]`).evaluate((el) => parseFloat(getComputedStyle(el).fontSize))).toBeCloseTo(size, 1);
    await read();
    expect(await px()).toBeCloseTo(size, 1);
    expect(await chrome(), 'the title is chrome-sized and keeps its size').toBe(chromeSize);
    await page.reload();
    await expect(page.locator('#doc')).toContainText('A line of the note');
    expect(await px(), 'kept after a reload').toBeCloseTo(size, 1);
  }
  await page.goto('/#/settings');
  await page.getByRole('button', { name: 'Reset Text size to default' }).click();
  await read();
  expect(await px()).toBe(16.5);
  expect(await page.evaluate(() => localStorage.getItem('leo-text-size'))).toBeNull();
  const own = await letters();
  expect(own.chat).toBe(own.margin);
  expect(own.chat).not.toBe(own.notes);
  await page.goto('/#/settings');
  await page.locator('[data-action="letters-pick"][data-letters="notes"]').click();
  await page.reload();
  const like = await letters();
  expect(like.chat).toBe(like.notes);
  expect(like.margin).toBe(like.notes);
  expect(like.hello, 'only answers and margin notes change lettering').toBe(own.hello);
  await page.getByRole('button', { name: "Reset Felix's lettering to default" }).click();
  expect((await letters()).chat).toBe(own.chat);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('On a Mac only Command opens Quick Leo, so Control+K stays a text-editing key', async ({ page }) => {
  await page.addInitScript(() => Object.defineProperty(navigator, 'platform', { get: () => 'MacIntel' }));
  await page.goto('/');
  await page.keyboard.press('Control+k');
  await expect(page.locator('.quick-leo')).toHaveCount(0);
  await page.keyboard.press('Meta+k');
  await expect(page.locator('.scrim:not(.scrim-gone) .quick-leo')).toBeVisible();
});

test('Quick Leo is one Spotlight-style line that works out what you want: a reminder, a question, a note to open or a new one', async ({ page }) => {
  const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
  const due = await (await page.request.post('/api/tasks', { data: { title: `Due ${tag}`, date: FIXED_TODAY } })).json();
  const asked = [];
  await page.route('**/api/chat', (route) => { asked.push(route.request().postDataJSON()); return route.fulfill({ contentType: 'application/x-ndjson', body: '{"t":"A heap keeps the smallest on top."}\n{"done":true}\n' }); });
  await page.clock.install({ time: FIXED_NOW });
  await page.goto('/');
  for (const key of ['j', 'y']) {
    await page.keyboard.press(`ControlOrMeta+${key}`);
    await expect(page.locator('.quick-leo')).toHaveCount(0);
  }
  await page.keyboard.press('ControlOrMeta+k');
  const box = page.locator('.scrim:not(.scrim-gone) .quick-leo');
  await expect(box).toBeVisible();
  const input = box.locator('#quick-input');
  await expect(input).toBeFocused();
  await expect(box.locator('.quick-kinds, #quick-go, h3')).toHaveCount(0);
  await expect(box.locator('.quick-task', { hasText: `Due ${tag}` })).toBeVisible();
  await box.locator('.quick-task', { hasText: `Due ${tag}` }).locator('input').click();
  await expect(box.locator('.quick-task', { hasText: `Due ${tag}` })).toHaveCount(0);
  await expect.poll(async () => (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.id === due.id)?.done).toBe(true);
  const top = box.locator('.quick-row.on');
  await input.fill(`email Bob ${tag} tomorrow at 3pm`);
  await expect(top).toContainText(`email Bob ${tag}`);
  await expect(top).toContainText(`Add “email Bob ${tag}” to tasks for tomorrow, 3`);
  await input.press('Enter');
  await expect(box).toHaveCount(0);
  await expect(page.locator('.toast')).toContainText(`Added “email Bob ${tag}”`);
  const made = (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.title === `email Bob ${tag}`);
  expect([made.date, String(made.time).slice(0, 5)]).toEqual([FIXED_TOMORROW, '15:00']);

  await page.keyboard.press('ControlOrMeta+k');
  await input.fill(`Pay rent ${tag}`);
  await expect(box.locator('.quick-row')).toHaveCount(4);
  await expect(top).toContainText(`Add “Pay rent ${tag}” to tasks`);
  await expect(top.locator('.quick-verb')).toHaveText(['Add', 'to tasks']);
  await expect(top.locator('.quick-hint')).toHaveCount(0);
  await input.press('ArrowDown');
  await expect(top).toContainText(`Ask Felix about “Pay rent ${tag}”`);
  await input.press('ArrowUp');
  await expect(top).toContainText(`Add “Pay rent ${tag}” to tasks`);
  await input.press('Enter');
  await expect(page.locator('.toast')).toContainText(`Added “Pay rent ${tag}”`);

  await page.keyboard.press('ControlOrMeta+k');
  await input.fill(`note: Idea ${tag}`);
  await expect(top).toContainText(`Start a note called “Idea ${tag}”`);
  await input.press('Enter');
  await expect(page.locator('.toast')).toContainText(`Saved “Idea ${tag}”`);

  await page.keyboard.press('ControlOrMeta+k');
  await input.fill(`Idea ${tag}`);
  await input.press('Enter');
  await expect.poll(() => page.evaluate(() => location.hash)).toMatch(/^#\/n\//);
  await expect(page.locator('main h1', { hasText: `Idea ${tag}` })).toBeVisible();

  await page.keyboard.press('ControlOrMeta+k');
  await input.fill(`Idea ${tag}`);
  await box.locator('.quick-row', { hasText: `Search every note for “Idea ${tag}”` }).click();
  await expect.poll(() => page.evaluate(() => decodeURIComponent(location.hash))).toBe(`#/search/Idea ${tag}`);
  await expect(page.locator('.side-note').filter({ hasText: `Idea ${tag}` })).toBeVisible();
  await expect(page.locator('#search-input')).toBeVisible();

  await page.keyboard.press('ControlOrMeta+k');
  await input.fill('What is a heap?');
  await expect(top).toContainText('Ask Felix');
  await input.press('Enter');
  await expect(page.locator('aside.chat')).toBeVisible();
  await expect(page.locator('.msg.leo').last()).toContainText('A heap keeps the smallest on top.');
  expect(asked.at(-1).messages.at(-1).text).toContain('What is a heap?');
  expect(asked.at(-1).messages).toHaveLength(1);

  const dir = `Shortcuts ${tag}`;
  const kept = await (await page.request.post('/api/notes', { data: { title: `Chord ${tag}`, body: 'A note.', directory: dir } })).json();
  await page.goto(`/#/n/${kept.id}`);
  await expect(page.locator('#title')).toHaveText(`Chord ${tag}`);
  await page.keyboard.press('ControlOrMeta+f');
  await expect(page.locator('.quick-leo')).toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+n');
  await expect.poll(() => page.evaluate(() => decodeURIComponent(location.hash))).toBe(`#/new/${dir}`);
  await page.goto('/#/calendar');
  await page.keyboard.press('ControlOrMeta+f');
  await expect(page.locator('.quick-leo'), 'only Cmd/Ctrl+K opens Quick Leo').toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+k');
  await expect(page.locator('.quick-leo')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.quick-leo')).toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+n');
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#/new');
  await page.goto('/#/calendar');
  await page.locator('#side .side-group [data-action="new-folder"]').click();
  await page.keyboard.press('ControlOrMeta+n');
  await expect(page.locator('#folder-name')).toBeVisible();
  expect(await page.evaluate(() => location.hash)).toBe('#/calendar');
});

test('Quick Leo finds folders and past chats and opens them', async ({ page }) => {
  const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
  const folder = `Shelf ${tag}`;
  await page.request.post('/api/dirs', { data: { path: `Classes/${folder}` } });
  await page.request.put(`/api/chats/quick-${tag}`, { data: { title: `Heap talk ${tag}`, messages: [{ role: 'user', text: `Heap talk ${tag}` }, { role: 'assistant', text: 'A heap keeps the smallest on top.' }] } });
  await page.goto('/');
  await page.keyboard.press('ControlOrMeta+k');
  const box = page.locator('.scrim:not(.scrim-gone) .quick-leo');
  const input = box.locator('#quick-input');
  await input.fill(folder);
  const top = box.locator('.quick-row.on');
  await expect(top.locator('.quick-hint')).toHaveText('Classes');
  await expect(box.locator('.quick-due-head')).toHaveText(['Notes', 'Do']);
  await expect(box.locator('.quick-foot')).toContainText('esc close');
  await input.press('Enter');
  await expect(page).toHaveURL(/\/(?:#\/)?$/);
  await expect(page.locator('#side .side-row[data-action="open-folder"][data-dir="Classes/' + folder + '"]')).toBeVisible();
  await expect(page.locator('#side .side-row[data-action="open-folder"][data-dir="Classes/' + folder + '"]')).toHaveAttribute('aria-current', 'page');
  await page.keyboard.press('ControlOrMeta+k');
  await input.fill(`Heap talk ${tag}`);
  const talk = box.locator('.quick-row', { hasText: 'Chat ·' });
  await expect(talk).toContainText(`Heap talk ${tag}`);
  await talk.click();
  await expect(page.locator('aside.chat')).toBeVisible();
  await expect(page.locator('.msg.leo').last()).toContainText('A heap keeps the smallest on top.');
});

test('a long note reads to its end with the Tasks panel clear of its text on short and tall screens', async ({ page }) => {
  const task = await (await page.request.post('/api/tasks', { data: { title: 'Task beside reading' } })).json();
  const note = await (await page.request.post('/api/notes', { data: { title: 'Long reading', body: ('Reading paragraph.\n\n').repeat(100) } })).json();
  await page.setViewportSize({ width: 1280, height: 600 });
  await page.goto('/#/n/' + note.id);
  await page.reload();
  await expect(page.locator('#doc')).toContainText('Reading paragraph.');
  const panel = page.locator('#tasks-window');
  for (const height of [600, 900]) {
    await page.setViewportSize({ width: 1280, height });
    await expect(panel).toBeVisible();
    await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
    await expect(page.locator('#doc .blk').last()).toBeInViewport();
    const [text, dock] = await Promise.all([page.locator('#doc .blk').last().boundingBox(), panel.boundingBox()]);
    expect(text.y + text.height <= dock.y || text.x + text.width <= dock.x || text.x >= dock.x + dock.width, `${height}px: the panel stays off the last line`).toBe(true);
  }
  await page.request.delete('/api/tasks/' + task.id);
  await page.request.delete('/api/notes/' + note.id);
});

test('a calendar event shows its organiser, guests and formatted description, with nothing that can run', async ({ page }) => {
  const start = new Date(2026, 10, 3, 12, 0);
  const end = new Date(start.getTime() + 3600000);
  const description = 'Itinerary: Charlton S<br />Monday<br/><br/><strong>How to connect:</strong><ul><li>Click "Join with Google Meet" then “Join Now”.</li><li>Wait to be admitted.</li></ul><a href="https://meet.google.com/abc-defg-hij">Join the call</a><img src=x onerror="window.__ran = 1"><script>window.__ran = 2</script>';
  const event = { id: 'event-rich-1', title: 'Data/Algos Interview', start: start.toISOString(), end: end.toISOString(), all_day: false, location: '', calendar: 'Work', context: `Calendar event: Data/Algos Interview\nOrganised by: c_c463e763f192@group.calendar.google.com\nInvited: GoodTime Sync, charlton@ucla.edu\nDescription:\n${description}` };
  await page.route('**/api/schedule?*', (route) => route.fulfill({ json: { connected: true, events: [event], tasks: [], warnings: [] } }));
  await page.clock.install({ time: FIXED_NOW });
  await page.goto('/#/calendar');
  await page.locator('.cal-open[data-id="event-rich-1"]').first().click();
  const sheet = page.locator('#cal-side.peek');
  await expect(sheet).toContainText('Organiser');
  await expect(sheet).toContainText('A shared calendar');
  await expect(sheet).not.toContainText('group.calendar.google.com');
  await expect(sheet.locator('.sched-people .chip')).toHaveText(['GoodTime Sync', 'charlton@ucla.edu']);
  const about = sheet.locator('.sched-about');
  await expect(about.locator('strong')).toHaveText('How to connect:');
  await expect(about.locator('li')).toHaveText(['Click "Join with Google Meet" then “Join Now”.', 'Wait to be admitted.']);
  await expect(about.locator('a')).toHaveAttribute('href', 'https://meet.google.com/abc-defg-hij');
  await expect(about.locator('a')).toHaveAttribute('rel', 'noopener noreferrer');
  await expect(about).not.toContainText('<br');
  await expect(about.locator('img, script')).toHaveCount(0);
  await expect(about).not.toContainText('__ran');
  expect(await page.evaluate(() => window.__ran)).toBeUndefined();
});

test('a late missing note response keeps a new draft open', async ({ page }) => {
  let release;
  const held = new Promise(resolve => { release = resolve; });
  await page.route('**/api/notes/late-route-note', async route => {
    await held;
    await route.fulfill({status:404,json:{error:'That note is no longer there.'}});
  });
  await page.goto('/#/n/late-route-note');
  await page.evaluate(() => { location.hash = '#/new'; });
  const title = `Kept draft ${Date.now().toString(36)}`;
  await page.locator('#title').fill(title);
  const saved = page.waitForResponse(response => response.request().method() === 'POST' && new URL(response.url()).pathname === '/api/notes');
  release();
  const note = await (await saved).json();
  await expect(page.locator('#title')).toHaveText(title);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe(`#/n/${note.id}`);
  await expect(page.locator('.toast.bad')).toHaveCount(0);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('Escape from Settings, Keeping or Trash returns to the note you were reading', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: `Back here ${Date.now().toString(36)}`, body: 'Keep my place.' } })).json();
  const visit = (hash) => page.evaluate((h) => { location.hash = h; }, hash);
  const back = async () => { await page.evaluate(() => document.activeElement?.blur()); await page.keyboard.press('Escape'); };
  const at = () => page.evaluate(() => location.hash);
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toContainText('Keep my place.');
  await visit('#/settings');
  await expect(page.locator('.settings')).toBeVisible();
  const depth = await page.evaluate(() => history.length);
  await back();
  await expect.poll(at).toBe(`#/n/${note.id}`);
  await expect(page.locator('#doc')).toContainText('Keep my place.');
  expect(await page.evaluate(() => history.length)).toBe(depth);
  await page.goForward();
  await expect.poll(at).toBe('#/settings');
  await page.goBack();
  await expect.poll(at).toBe(`#/n/${note.id}`);
  await page.reload();
  await expect(page.locator('#doc')).toContainText('Keep my place.');
  await visit('#/trash');
  await back();
  await expect.poll(at).toBe(`#/n/${note.id}`);
  expect(await page.evaluate(() => history.length)).toBe(depth);
  await visit('#/settings/keeping');
  await back();
  await expect.poll(at).toBe(`#/n/${note.id}`);
  await visit('#/settings');
  await visit('#/settings/keeping');
  await back();
  await expect.poll(at).toBe('#/settings');
  await back();
  await expect.poll(at).toBe(`#/n/${note.id}`);
  await visit('#/trash');
  await back();
  await expect.poll(at).toBe(`#/n/${note.id}`);
  await page.request.delete(`/api/notes/${note.id}`);

  const title = `Fresh ${test.info().project.name} ${Date.now().toString(36)}`;
  await visit('#/new');
  await page.locator('#title').click();
  await page.keyboard.type(title);
  await expect.poll(at, { timeout: 10000 }).toMatch(/^#\/n\//);
  const made = await at();
  await visit('#/settings');
  await expect(page.locator('.settings')).toBeVisible();
  await back();
  await expect.poll(at).toBe(made);
  await expect(page.locator('#title')).toHaveText(title);
  const all = await (await page.request.get('/api/notes?limit=500')).json();
  expect((Array.isArray(all) ? all : all.notes || []).filter((n) => !n.title || n.title === 'Untitled').length).toBe(0);
  await page.request.delete(`/api/notes/${made.slice(4)}`);
});









test('buttons keep room from the text and sections around them', async ({ page }) => {
  test.setTimeout(60000);
  const cramped = () => {
    const shown = (el) => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.position !== 'fixed' && s.position !== 'absolute'; };
    const out = [];
    for (const b of document.querySelectorAll('main button, main a.btn, main .link-btn')) {
      if (!shown(b) || b.matches('.rec-go') || b.getBoundingClientRect().height < 18) continue;
      const br = b.getBoundingClientRect();
      for (const dir of [-1, 1]) {
        let sib = dir < 0 ? b.previousElementSibling : b.nextElementSibling;
        while (sib && !shown(sib)) sib = dir < 0 ? sib.previousElementSibling : sib.nextElementSibling;
        if (!sib || sib.matches('button, a, .chip, label') || (sib.tagName === b.tagName && sib.className === b.className)) continue;
        const sr = sib.getBoundingClientRect();
        if (Math.min(br.right, sr.right) - Math.max(br.left, sr.left) <= 0) continue;
        const gap = dir < 0 ? br.top - sr.bottom : sr.top - br.bottom;
        if (gap >= -1 && gap < 8) out.push(`${b.textContent.trim()} is ${gap.toFixed(0)}px from ${sib.tagName.toLowerCase()} "${sib.textContent.trim().slice(0, 30)}"`);
      }
    }
    return out;
  };
  await page.request.post('/api/tasks', { data: { title: 'Spacing task' } });
  for (const hash of ['/#/', '/#/settings', '/#/calendar']) {
    await page.goto(hash);
    await page.waitForTimeout(500);
    expect(await page.evaluate(cramped), hash).toEqual([]);
  }

});



























test('Usage is a Settings page with remaining limits and its own refresh', async ({ page }) => {
  await page.route(/\/api\/settings(?:\/usage)?$/, route => route.fulfill({ json: {
    tasks: [{ task: 'writing', provider: 'ollama' }],
    plans: [{ provider: 'codex', name: 'Codex', installed: true, signed_in: true,
      limits: { five_hour: { used: .25, resets_at: Date.now() / 1000 + 3600 }, seen_at: new Date().toISOString() } }],
  } }));
  await page.goto('/#/settings');
  await page.locator('.settings-nav a[href="#/settings/usage"]').click();
  await expect(page).toHaveURL(/#\/settings\/usage$/);
  const plans = page.locator('.usage-plans');
  await expect(plans.getByRole('meter').first()).toHaveAttribute('aria-valuenow', '75');
  await expect(plans).toContainText('75% left');
  await expect(plans.locator('[data-action="use-plan"]')).toHaveCount(0);
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator('[data-find="usage-activity"]')).toBeVisible();
  const refresh = plans.getByRole('button', { name: 'Refresh', exact: true });
  await expect(refresh).toBeEnabled();
  await refresh.click();
  await expect(plans.getByRole('meter').first()).toHaveAttribute('aria-valuenow', '75');
  await expect(refresh).toBeEnabled();
  await expect(refresh).toBeFocused();
});

for (const [width, layout] of [[800, 'right'], [1280, 'right'], [1800, 'right'], [1280, 'marks'], [1800, 'both']]) test(`note search temporarily highlights the match without changing the note at ${width}px with ${layout} margins`, async ({ page }) => {
  await page.setViewportSize({ width, height: 900 });
  await page.route('**/api/margin', route => route.fulfill({ json: { layout, pace: 'regular' } }));
  await page.evaluate(() => localStorage.removeItem('leo-margin-layout'));
  const note = await (await page.request.post('/api/notes', { data: { title: 'Search highlight fixture', body: 'A long paragraph of reading material. '.repeat(2000) + 'needlepassage', directory: '' } })).json();
  try {
    await page.goto('/#/search/needlepassage');
    await page.locator(`.card[data-id="${note.id}"]`).click();
    await expect(page.locator('#doc')).toContainText('needlepassage');
    if (layout !== 'right') await expect(page.locator('.note-columns')).toHaveAttribute('data-margin', layout);
    await expect.poll(() => page.evaluate(() => CSS.highlights.get('leo-search')?.size || 0)).toBeGreaterThan(0);
    const visible = await page.evaluate(() => {
      const range = CSS.highlights.get('leo-search').values().next().value;
      const rect = range.getBoundingClientRect();
      return rect.top >= 80 && rect.bottom <= innerHeight;
    });
    expect(visible).toBe(true);

    expect(await page.locator('#doc mark').count()).toBe(0);
    await expect.poll(() => page.evaluate(() => CSS.highlights.has('leo-search')), { timeout: 8000 }).toBe(false);
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe(note.body);
  } finally { await page.request.delete(`/api/notes/${note.id}`); }
});

test('Usage shows saved limits during refresh and keeps them after a refresh failure', async ({ page }) => {
  const saved = { tasks: [], plans: [{ provider: 'codex', name: 'Codex', installed: true, signed_in: true, limits: { five_hour: { used: .25 }, seen_at: new Date().toISOString() } }] };
  await page.route('**/api/settings', route => route.fulfill({ json: saved }));
  for (const outcome of ['success', 'failure', 'left']) {
    let release;
    const waiting = new Promise(resolve => { release = resolve; });
    await page.route('**/api/settings/usage', async route => {
      await waiting;
      if (outcome === 'failure') return route.fulfill({ status: 503, json: { error: 'Usage could not refresh. Try again in a moment.' } });
      return route.fulfill({ json: { ...saved, plans: [{ ...saved.plans[0], limits: { five_hour: { used: .6 }, seen_at: new Date().toISOString() } }] } });
    });
    await page.goto('/#/settings/general');
    await page.locator('.settings-nav a[href="#/settings/usage"]').click();
    const plans = page.locator('.usage-plans');
    await expect(plans.getByRole('meter').first()).toHaveAttribute('aria-valuenow', '75');
    if (outcome === 'left') await page.locator('.settings-nav a[href="#/settings/general"]').click();
    const response = page.waitForResponse('**/api/settings/usage');
    release();
    await response;
    if (outcome === 'success') await expect(plans.getByRole('meter').first()).toHaveAttribute('aria-valuenow', '40');
    else if (outcome === 'failure') {
      await expect(plans.locator('.usage-error')).toHaveText('Refresh failed · showing saved usage');
      await expect(plans.getByRole('meter').first()).toHaveAttribute('aria-valuenow', '75');
    } else {
      await expect(page.locator('#app .settings-head h1')).toHaveText('General');
      await expect(plans).toHaveCount(0);
    }
    await page.unroute('**/api/settings/usage');
  }
});

for (const width of [1280, 390]) {
  test(`Record and Settings keep consistent headers at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    let reference;
    for (const [route, title] of [
      ['record', 'Record · leo'],
      ['settings/general', 'Settings · leo'],
      ['settings/keeping', 'Keeping · leo'],
      ['settings/trash', 'Trash · leo'],
    ]) {
      await page.goto('/#/' + route);
      await expect(page).toHaveTitle(title);
      const header = await page.locator('.bar-row').boundingBox();
      if (width < 1000) expect(header.width).toBeLessThanOrEqual(760);
      if (reference) {
        if (width < 1000) {
          expect(header.x).toBeCloseTo(reference.x, 0);
          expect(header.width).toBeCloseTo(reference.width, 0);
        }
        expect(header.height).toBeCloseTo(reference.height, 0);
      }
      if (width >= 1000) {
        const bounds = await page.locator('#bar').boundingBox();
        expect(header.x, 'the header stays inside the page toolbar').toBeGreaterThanOrEqual(bounds.x);
        expect(header.x + header.width).toBeLessThanOrEqual(bounds.x + bounds.width + 1);
        expect(header.x + header.width / 2, 'the header remains centered').toBeCloseTo(bounds.x + bounds.width / 2, 0);
      }
      reference = header;
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }
  });
}
