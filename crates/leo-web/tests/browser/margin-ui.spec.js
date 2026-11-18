const { test, expect } = require('@playwright/test');
const { signIn } = require('./sign-in');

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

const quiet = (page) => expect.poll(() => page.evaluate(() => document.getAnimations().filter((run) => run.playState === 'running' && run.effect && run.effect.getComputedTiming().iterations !== Infinity).length)).toBe(0);

test('the leo mark is one symbol that the brand uses', async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.goto('/');
  await expect(page.locator('svg.sprite symbol#leo-mark')).toHaveCount(1);
  await expect(page.locator('.bar .brand svg use[href="#leo-mark"]')).toHaveCount(1);
  await expect(page.locator('.side-head .brand svg use[href="#leo-mark"]')).toHaveCount(1);
  await expect(page.locator('.side-head .brand')).toHaveAccessibleName('leo');
});

test('note titles and text are set in Instrument Sans and Felix in the margin in Newsreader, from the bundled fonts', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Fonts', body: 'Words.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toBeVisible();
  await page.evaluate(() => {
    const said = document.createElement('div');
    said.className = 'margin-felix';
    said.id = 'voice';
    said.textContent = 'Here it is.';
    document.querySelector('#app').append(said);
  });
  await expect(page.locator('.note-head h1')).toHaveCSS('font-family', /^"?Instrument Sans"?,/);
  await expect(page.locator('#doc')).toHaveCSS('font-family', /^"?Instrument Sans"?,/);
  await expect(page.locator('#voice')).toHaveCSS('font-family', /^"?Newsreader"?,/);
  await expect.poll(() => page.evaluate(() => document.fonts.load('500 1em "Instrument Sans"').then(() => document.fonts.check('500 1em "Instrument Sans"') && document.fonts.check('1em Newsreader') && document.fonts.check('italic 1em Newsreader')))).toBe(true);
});

test('a stepped drawing steps through its parts with captions and theme colours, and stays an image', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Stepped', body: 'Words.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toBeVisible();
  await page.evaluate(() => {
    const stepped = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 120"><title>Heat</title><g data-step="1" data-caption="Hot side"><circle cx="61" cy="60" r="30" style="stroke: var(--accent); fill: none"/></g><g data-step="2" data-caption="Heat moves"><path d="M100 60h120" style="stroke: var(--text)"/></g></svg>';
    const broken = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 80"><g data-step="1"><path d="M0 0h9"></svg>';
    for (const [id, svg] of [['stepped', stepped], ['broken', broken]]) {
      const box = document.createElement('div');
      box.className = 'msg leo';
      box.id = id;
      box.innerHTML = `<div class="prose">${window.leoMarkdown.render('```svg\n' + svg + '\n```')}</div>`;
      document.querySelector('#app').append(box);
    }
  });
  const fig = page.locator('#stepped figure.drawing.inked');
  const src = async () => decodeURIComponent(await fig.locator('img').getAttribute('src'));
  await expect(fig.locator('.drawing-count')).toHaveText('1/2');
  await expect(fig.locator('.drawing-caption')).toHaveText('Hot side');
  await expect(fig.getByRole('button', { name: 'Previous step' })).toBeDisabled();
  expect(await src()).not.toContain('var(--');
  expect(await src()).toContain('opacity="0"');
  await fig.getByRole('button', { name: 'Next step' }).click();
  await expect(fig.locator('.drawing-count')).toHaveText('2/2');
  await expect(fig.locator('.drawing-caption')).toHaveText('Heat moves');
  await expect(fig.getByRole('button', { name: 'Next step' })).toBeDisabled();
  expect(await src()).not.toContain('opacity="0"');
  await fig.getByRole('button', { name: 'Start again' }).click();
  await expect(fig.locator('.drawing-count')).toHaveText('1/2');
  await expect(fig.locator('img')).toHaveCSS('filter', 'none');
  expect(await page.locator('circle[cx="61"]').count()).toBe(0);
  await expect(page.locator('#broken figure.drawing img')).toHaveCount(1);
  await expect(page.locator('#broken figure.drawing.inked, #broken .drawing-steps')).toHaveCount(0);
});

test('the selection menu offers Felix this, Ask Felix and Visualize, and the two quick ones ask with their own message', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Selection menu', body: 'Intro line\n\nHeat flows from the hot side to the cold side.\n\nLast line' } })).json();
  const asked = [];
  await page.route(`**/api/notes/${note.id}/margin/threads`, async (route) => {
    const body = route.request().postDataJSON();
    asked.push(body);
    await route.fulfill({ json: { id: `t${asked.length}`, note: note.id, quote: body.quote, prefix: body.prefix, suffix: body.suffix, status: 'open', source: 'ask', messages: [{ from: 'felix', text: 'Here it is.', at: '2026-11-14T10:00:00Z' }], created_at: '2026-11-14T10:00:00Z', updated_at: '2026-11-14T10:00:00Z' } });
  });
  const replied = [];
  await page.route(`**/api/notes/${note.id}/margin/threads/t1/reply`, async (route) => {
    const body = route.request().postDataJSON();
    replied.push(body);
    await route.fulfill({ json: { id: 't1', note: note.id, quote: 'the hot side', prefix: '', suffix: '', status: 'open', source: 'ask', messages: [{ from: 'felix', text: 'Here it is.', at: '2026-11-14T10:00:00Z' }, { from: 'you', text: body.text, at: '2026-11-14T10:01:00Z' }, { from: 'felix', text: 'Still true.', at: '2026-11-14T10:01:00Z' }], created_at: '2026-11-14T10:00:00Z', updated_at: '2026-11-14T10:01:00Z', pinned: false } });
  });
  await page.goto(`/#/n/${note.id}`);
  const select = async () => {
    await page.locator('#doc .blk', { hasText: 'Heat flows' }).click();
    const area = page.locator('#doc textarea.line-edit');
    await expect(area).toHaveCount(1);
    await area.evaluate((a) => {
      const at = a.value.indexOf('the hot side');
      a.focus();
      a.setSelectionRange(at, at + 'the hot side'.length);
      a.dispatchEvent(new Event('select'));
    });
  };
  await select();
  const menu = page.getByRole('toolbar', { name: 'Felix and this passage' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('button')).toHaveText([/Felix this/, 'Ask Felix', 'Visualize']);
  await menu.getByRole('button', { name: 'Visualize' }).click();
  await expect.poll(() => asked.length).toBe(1);
  expect(asked[0]).toMatchObject({ quote: 'the hot side', question: 'Draw this.' });
  await expect(page.locator('.margin-note[data-open] .margin-felix')).toHaveText('Here it is.');
  await page.keyboard.press('Escape');
  await select();
  await expect(menu).toBeVisible();
  await page.keyboard.press('ControlOrMeta+Enter');
  await expect(page.locator('.margin-note[data-id="t1"]')).toHaveAttribute('data-open', '');
  await page.waitForTimeout(300);
  expect(replied.length).toBe(0);
  expect(asked.length).toBe(1);
  await page.keyboard.press('Escape');
  await select();
  await menu.getByRole('button', { name: 'Visualize' }).click();
  await expect.poll(() => replied.length).toBe(1);
  expect(replied[0]).toMatchObject({ text: 'Draw this.' });
  expect(asked.length).toBe(1);
  await expect(page.locator('.margin-note')).toHaveCount(1);
});

const answered = (note, body, text) => ({ id: 't1', note, quote: body.quote, prefix: body.prefix, suffix: body.suffix, status: 'open', source: 'ask', messages: [{ from: 'you', text: body.question, at: '2026-11-14T10:00:00Z' }, { from: 'felix', text, at: '2026-11-14T10:00:00Z' }], created_at: '2026-11-14T10:00:00Z', updated_at: '2026-11-14T10:00:00Z', pinned: false });

const pickText = (page, words) => page.evaluate((words) => {
  const doc = document.querySelector('#doc');
  const walker = document.createTreeWalker(doc, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const at = node.data.indexOf(words);
    if (at < 0) continue;
    const range = document.createRange();
    range.setStart(node, at);
    range.setEnd(node, at + words.length);
    getSelection().removeAllRanges();
    getSelection().addRange(range);
    return true;
  }
  return false;
}, words);

test('a new stepped drawing plays once, then steps on a folded note without opening it and keeps its step', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Folded steps', body: 'Intro line\n\nHeat flows from the hot side to the cold side.\n\nLast line' } })).json();
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 80"><title>Heat</title><g data-step="1" data-caption="Hot side"><circle cx="40" cy="40" r="20" style="stroke: var(--text); fill: none"/></g><g data-step="2" data-caption="It moves"><path d="M60 40h80" style="stroke: var(--text)"/></g><g data-step="3" data-caption="Cold side"><circle cx="160" cy="40" r="20" style="stroke: var(--text); fill: none"/></g></svg>';
  let release;
  const held = new Promise((r) => { release = r; });
  await page.route(`**/api/notes/${note.id}/margin/threads`, async (route) => { await held; await route.fulfill({ json: answered(note.id, route.request().postDataJSON(), 'Like this.\n\n```svg\n' + svg + '\n```') }); });
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toBeVisible();
  expect(await pickText(page, 'the hot side')).toBe(true);
  await page.getByRole('toolbar', { name: 'Felix and this passage' }).getByRole('button', { name: 'Visualize' }).click();
  await expect(page.getByRole('status', { name: 'Felix is sketching' })).toContainText('Sketching it…');
  release();
  const thread = page.locator('.margin-note[data-id="t1"]');
  await expect(thread.locator('.drawing-count')).toHaveText('1/3');
  await expect(thread.locator('.drawing-count')).toHaveText('3/3', { timeout: 8000 });
  await page.keyboard.press('Escape');
  await expect(thread).not.toHaveAttribute('data-open', '');
  await thread.getByRole('button', { name: 'Previous step' }).click();
  await expect(thread.locator('.drawing-count')).toHaveText('2/3');
  await page.waitForTimeout(1500);
  await expect(thread.locator('.drawing-count')).toHaveText('2/3');
  await expect(thread).not.toHaveAttribute('data-open', '');
  await thread.locator('.margin-toggle').click({ force: true });
  await expect(thread).toHaveAttribute('data-open', '');
  await expect(thread.locator('.drawing-count')).toHaveText('2/3');
  await page.keyboard.press('Escape');
  await expect(thread).not.toHaveAttribute('data-open', '');
  await expect(thread.locator('.drawing-count')).toHaveText('2/3');
});

test('selected note text is painted as its own text only, clipped to the note, and leaves the line editor native', async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Painted', body: 'Short\n\n- one item\n- two\n\n```\ncode line\n```\n\nLast paragraph here.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toBeVisible();
  const painted = () => page.evaluate(() => {
    const hl = CSS.highlights.get('leo-pick');
    return hl ? [...hl].map((r) => r.toString()).join('|') : null;
  });
  await page.evaluate(() => {
    const doc = document.querySelector('#doc');
    const range = document.createRange();
    range.setStart(doc, 0);
    range.setEnd(doc, doc.childNodes.length);
    getSelection().removeAllRanges();
    getSelection().addRange(range);
  });
  await expect.poll(painted).toContain('Short');
  expect(await painted()).toBe(await page.evaluate(() => getSelection().getRangeAt(0).toString()));
  await expect(page.locator('#doc')).toHaveClass(/pick-paint/);
  await page.evaluate(() => {
    const range = document.createRange();
    range.setStart(document.querySelector('#title'), 0);
    range.setEnd(document.querySelector('#doc'), 1);
    getSelection().removeAllRanges();
    getSelection().addRange(range);
  });
  await expect.poll(painted).not.toContain('Painted');
  await page.evaluate(() => getSelection().removeAllRanges());
  await expect.poll(painted).toBeNull();
  await page.locator('#doc .blk', { hasText: 'Last paragraph' }).click();
  await page.locator('#doc textarea.line-edit').evaluate((a) => a.setSelectionRange(0, 4));
  await page.evaluate(() => document.dispatchEvent(new Event('selectionchange')));
  await expect.poll(painted).toBeNull();
});

test('the Felix bar opens in the margin beside a selection when there is room, and under its last word when not', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Bar place', body: 'One\n\nTwo\n\nThree short lines here.' } })).json();
  const menu = page.getByRole('toolbar', { name: 'Felix and this passage' });
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('.note-columns')).toHaveAttribute('data-margin', /right|both/);
  await page.evaluate(() => {
    const texts = [];
    const walker = document.createTreeWalker(document.querySelector('#doc'), NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) texts.push(node);
    const range = document.createRange();
    range.setStart(texts.find((n) => n.data.includes('One')), 0);
    range.setEnd(texts.find((n) => n.data.includes('Three short')), 5);
    getSelection().removeAllRanges();
    getSelection().addRange(range);
  });
  await expect(menu).toBeVisible();
  const doc = await page.locator('#doc').boundingBox();
  expect((await menu.boundingBox()).x).toBeGreaterThan(doc.x + doc.width);
  await page.setViewportSize({ width: 1100, height: 800 });
  await expect(page.locator('.note-columns')).toHaveAttribute('data-margin', 'marks');
  expect(await pickText(page, 'Three short')).toBe(true);
  await expect(menu).toBeVisible();
  const word = await page.locator('#doc .blk', { hasText: 'Three short' }).boundingBox();
  await expect.poll(async () => (await menu.boundingBox()).y, 'the bar moves under the new selection on the next frame').toBeGreaterThan(word.y);
  expect((await menu.boundingBox()).x).toBeLessThan(word.x + 400);
});

test('Ask Felix opens a question box beside the passage, keeps a half-typed question and asks only when sent', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Ask first', body: 'Intro line\n\nHeat flows from the hot side to the cold side.\n\nLast line' } })).json();
  const asked = [];
  await page.route(`**/api/notes/${note.id}/margin/threads`, (route) => {
    asked.push(route.request().postDataJSON());
    return route.fulfill({ json: answered(note.id, asked.at(-1), 'Because heat moves.') });
  });
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toBeVisible();
  const menu = page.getByRole('toolbar', { name: 'Felix and this passage' });
  const draft = page.locator('.margin-note[data-id="draft"]');
  const question = draft.getByRole('textbox', { name: 'Ask Felix about this passage' });
  expect(await pickText(page, 'the hot side')).toBe(true);
  await menu.getByRole('button', { name: 'Ask Felix' }).click();
  await expect(draft).toHaveAttribute('data-open', '');
  await expect(question).toBeFocused();
  await expect(draft.locator('[data-draft-hint]')).toHaveText('Enter to ask · Esc to close');
  await expect(draft.locator('.margin-reply')).toHaveCSS('border-top-style', 'solid');
  await expect(page.locator('.margin-ink path.solid[data-id="draft"]')).toHaveClass(/on/);
  expect(asked.length).toBe(0);
  await question.pressSequentially('Why does it');
  await page.keyboard.press('Escape');
  await expect(draft).toHaveCount(0);
  expect(await pickText(page, 'the hot side')).toBe(true);
  await menu.getByRole('button', { name: 'Ask Felix' }).click();
  await expect(question).toHaveValue('Why does it');
  await question.pressSequentially(' move?');
  await question.press('Enter');
  await expect.poll(() => asked.length).toBe(1);
  expect(asked[0]).toMatchObject({ quote: 'the hot side', question: 'Why does it move?' });
  await expect(page.locator('.margin-note[data-id="t1"] .margin-felix')).toHaveText('Because heat moves.');
  await page.keyboard.press('Escape');
  expect(await pickText(page, 'the hot side')).toBe(true);
  await menu.getByRole('button', { name: 'Ask Felix' }).click();
  await expect(page.locator('.margin-note[data-id="t1"]')).toHaveAttribute('data-open', '');
  await expect(draft).toHaveCount(0);
  await expect(page.locator('.margin-note[data-id="t1"]').getByRole('button', { name: 'Pin open' })).toBeVisible();
  await expect(page.locator('.margin-note[data-id="t1"]').getByRole('button', { name: 'Copy' })).toBeVisible();
  expect(await page.locator('.margin-note[data-id="t1"] .margin-pin').evaluate((el) => getComputedStyle(el, '::after').content)).toBe('"Pin open"');
  expect(await page.locator('.margin-note[data-id="t1"] .margin-chat').evaluate((el) => getComputedStyle(el, '::after').content)).toBe('"Ask in chat"');
});

test('note reading modes sit beside the save state, work in both orders and retain the confirmed mode on failure', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Reading modes', body: 'Words about a passage.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  const button = page.locator('#margin-mode');
  await expect(button).toBeEnabled();
  expect((await button.boundingBox()).y).toBeLessThan((await page.locator('#title').boundingBox()).y);
  for (const [key, label] of [['Home', 'Reads along'], ['End', 'When you ask'], ['Home', 'Reads along'], ['End', 'When you ask']]) {
    await button.click();
    await expect(page.getByRole('menu', { name: 'Note reading mode' })).toBeVisible();
    await quiet(page);
    await page.keyboard.press(key);
    await page.keyboard.press('Enter');
    await expect(button).toHaveText(label);
    await expect(button).toBeEnabled();
  }
  await page.reload();
  await expect(button).toHaveText('When you ask');
  await page.route(`**/api/notes/${note.id}/margin`, route => route.request().method() === 'POST'
    ? route.fulfill({ status: 503, json: { error: 'Try again.' } }) : route.continue());
  await button.click();
  await page.getByRole('menuitemradio', { name: 'Reads along' }).click();
  await expect(button).toHaveText('When you ask');
  await expect(page.locator('#margin-status')).toHaveText('Could not save this mode. Try again.');
  await button.click();
  await page.keyboard.press('Escape');
  await expect(button).toBeFocused();
  await expect(page.getByRole('menu', { name: 'Note reading mode' })).toBeHidden();
});

test('hovering an underlined passage lights its margin note', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const quote = 'The password is stored as plain text.';
  const note = await (await page.request.post('/api/notes', { data: { title: 'Hover link', body: `Opening words.\n\n${quote} More words after it.` } })).json();
  expect((await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote, question: 'Why?' } })).ok()).toBe(true);
  await page.goto(`/#/n/${note.id}`);
  const card = page.locator('.margin-note');
  await expect(card).toBeVisible();
  await expect(card).not.toHaveAttribute('data-active', '');
  const line = await page.locator('.margin-ink .squiggle').first().boundingBox();
  await page.mouse.move(line.x + line.width / 2, line.y - 6);
  await expect(card).toHaveAttribute('data-active', '');
  await expect.poll(() => card.evaluate((el) => getComputedStyle(el).backgroundColor)).not.toBe('rgba(0, 0, 0, 0)');
  await page.mouse.move(5, 5);
  await expect(card).not.toHaveAttribute('data-active', '');
});

test('a note keeps its own persona, and a persona made in Settings joins the note menu', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Persona pick', body: 'Words about a passage.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  const button = page.locator('#margin-persona');
  await expect(button).toHaveText('Felix as Reader');
  await button.click();
  const menu = page.getByRole('menu', { name: 'Who reads this note' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitemradio')).toHaveText([/Reader/, /Critic/, /Encourager/]);
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(button).toHaveText('Felix as Critic');
  await expect(menu).toBeHidden();
  await page.reload();
  await expect(button).toHaveText('Felix as Critic');
  expect((await (await page.request.get(`/api/notes/${note.id}/margin`)).json()).persona).toBe('critic');
  await button.click();
  await page.getByRole('menuitem', { name: 'New persona' }).click();
  await expect(page).toHaveURL(/#\/settings\/felix/);
  await page.locator('#felix-persona-card form').getByRole('button', { name: 'Cancel' }).click();
  await expect(page).toHaveURL(/#\/settings\/felix/);
  await expect(page.getByRole('radiogroup', { name: 'Who reads your notes' })).toBeVisible();
  await page.goto(`/#/n/${note.id}`);
  await button.click();
  await page.getByRole('menuitem', { name: 'New persona' }).click();
  await expect(page).toHaveURL(/#\/settings\/felix/);
  const form = page.locator('#felix-persona-card form');
  await form.getByLabel('Name').fill('Lab review');
  await form.getByLabel('What should he look for?').fill('Check each claim against its evidence.');
  await form.getByRole('button', { name: 'Save' }).click();
  await expect(page).toHaveURL(new RegExp(`#/n/${note.id}`));
  await expect(button).toHaveText('Felix as Lab review');
  await button.click();
  await expect(menu.getByRole('menuitemradio')).toHaveText([/Reader/, /Critic/, /Encourager/, /Lab review/]);
  await expect(menu.getByRole('menuitemradio', { name: /Lab review/ })).toHaveAttribute('title', 'Check each claim against its evidence.');
  await page.keyboard.press('Escape');
  await page.goto('/#/settings/felix');
  const tiles = page.getByRole('radiogroup', { name: 'Who reads your notes' });
  await expect(tiles.locator('.persona-tile')).toHaveText([/Reader/, /Critic/, /Encourager/, /Lab review/, /New/]);
  await page.getByRole('button', { name: 'Edit Lab review' }).click();
  await expect(page.locator('#felix-persona-card form [name="name"]')).toHaveValue('Lab review');
  expect((await (await page.request.get('/api/margin')).json()).default_persona).toBe('reader');
  await page.getByRole('button', { name: 'Delete' }).click();
  await page.getByRole('button', { name: 'Delete for good' }).click();
  await expect(tiles.locator('.persona-tile')).toHaveText([/Reader/, /Critic/, /Encourager/, /New/]);
  await expect(tiles.getByRole('radio', { name: 'Reader' })).toBeChecked();
  await page.getByRole('button', { name: 'See details' }).click();
  await expect(page.locator('.persona-details')).toContainText('Stays quiet otherwise.');
  await page.getByRole('button', { name: 'Hide details' }).click();
  await expect(page.locator('.persona-details')).toHaveCount(0);
  expect(await page.locator('#felix-persona-card').evaluate((card) => card.getAnimations().length)).toBe(0);
  await expect(page.getByRole('button', { name: 'See details' })).toBeFocused();
  await page.goto(`/#/n/${note.id}`);
  await expect(button).toHaveText('Felix as Reader');
  await page.request.post('/api/margin', { data: { default_persona: 'reader' } });
});

test('compact margin anchors stay outside the passage and open real persisted replies without a mascot', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const quote = 'The password is stored as plain text.';
  const note = await (await page.request.post('/api/notes', { data: { title: 'Compact annotation', body: quote } })).json();
  const response = await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote, question: 'What should I change?' } });
  expect(response.ok()).toBe(true);
  const thread = await response.json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('.note-columns')).toHaveAttribute('data-margin', 'marks');
  const mark = page.locator('.margin-mark');
  await expect(mark).toBeVisible();
  await expect(mark.locator('.felix')).toHaveCount(0);
  const anchor = await mark.locator('.margin-anchor').boundingBox();
  const paragraph = await page.locator('#doc .blk').first().boundingBox();
  expect(anchor.x).toBeGreaterThan(paragraph.x);
  expect(anchor.y + anchor.height / 2).toBeGreaterThan(paragraph.y);
  expect(anchor.y + anchor.height / 2).toBeLessThan(paragraph.y + paragraph.height);
  const stroke = page.locator('.margin-ink .squiggle');
  const dashes = await stroke.evaluateAll(lines => lines.map(el => {
    const length = el.getTotalLength();
    const units = el.hasAttribute('pathLength') ? el.pathLength.baseVal.value : length;
    return parseFloat(getComputedStyle(el).strokeDasharray) * length / units;
  }));
  expect(dashes.length).toBeGreaterThan(0);
  for (const dash of dashes) expect(dash).toBeLessThanOrEqual(2);
  await mark.click();
  await expect(page.locator('.margin-note .margin-felix')).toContainText(thread.messages.at(-1).text);
  await expect(page.locator('.margin-reply textarea')).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const animations = await stroke.evaluateAll(lines => lines.map(el => getComputedStyle(el).animationName));
  for (const animation of animations) expect(animation).toBe('none');
});

test('a reading mode picked on a new note is saved with the note', async ({ page }) => {
  await page.goto('/#/new');
  const button = page.locator('#margin-mode');
  await expect(button).toBeEnabled();
  await button.click();
  await expect(page.getByRole('menuitemradio', { name: 'Reads along' })).toBeVisible();
  await quiet(page);
  await page.getByRole('menuitemradio', { name: 'Reads along' }).click();
  await expect(button).toHaveText('Reads along');
  const persona = page.locator('#margin-persona');
  await persona.click();
  await page.getByRole('menuitemradio', { name: /Encourager/ }).click();
  await expect(persona).toHaveText('Felix as Encourager');
  await page.locator('#title').click();
  await page.keyboard.type('Picked early');
  await expect(page).toHaveURL(/#\/n\//);
  const id = page.url().match(/#\/n\/([^/?]+)/)[1];
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${id}/margin`)).json()).mode).toBe('along');
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${id}/margin`)).json()).persona).toBe('encourager');
  await expect(button).toHaveText('Reads along');
  await expect(page.locator('#title')).toBeFocused();
});

test('a drag across page chrome paints nothing, fields stay selectable once focused, and the mode menu clears a selection', async ({ page }) => {
  await page.goto('/#/record');
  await expect(page.locator('#rec-title')).toBeVisible();
  await page.mouse.move(60, 20);
  await page.mouse.down();
  await page.mouse.move(640, 400, { steps: 6 });
  await page.mouse.move(1270, 700, { steps: 6 });
  await page.mouse.up();
  expect(await page.evaluate(() => getSelection().toString())).toBe('');
  const field = page.locator('#rec-title');
  await field.click();
  await page.keyboard.type('hello world');
  await page.mouse.click(5, 5);
  const box = await field.boundingBox();
  await page.mouse.move(box.x + 10, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 200, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
  expect(await field.evaluate(i => i.value.slice(i.selectionStart, i.selectionEnd))).toContain('world');
  const note = await (await page.request.post('/api/notes', { data: { title: 'Selection chrome', body: 'Alpha beta gamma.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  const word = page.locator('#doc .blk').first();
  const at = await word.boundingBox();
  await page.mouse.move(at.x + 1, at.y + at.height / 2);
  await page.mouse.down();
  await page.mouse.move(at.x + 40, at.y + at.height / 2, { steps: 4 });
  await page.mouse.up();
  expect(await page.evaluate(() => getSelection().toString())).not.toBe('');
  await expect(page.locator('#margin-mode')).toBeEnabled();
  await page.locator('#margin-mode').click();
  expect(await page.evaluate(() => getSelection().toString())).toBe('');
});

test('the Tasks window leaves every outline tick reachable and the preview sits above it', async ({ page }) => {
  await page.setViewportSize({ width: 1300, height: 640 });
  const today = new Date().toLocaleDateString('en-CA');
  for (const title of ['Submit lab report', 'Email prof', 'Pay rent', 'Buy lab coat', 'Book flights']) {
    await page.request.post('/api/tasks', { data: { title, date: today } });
  }
  const body = Array.from({ length: 12 }, (_, i) => `## Heading number ${i + 1}\n\nText under heading ${i + 1}.\n`).join('\n');
  const note = await (await page.request.post('/api/notes', { data: { title: 'Outline over tasks', body } })).json();
  await page.goto(`/#/n/${note.id}`);
  await page.reload();
  await expect(page.locator('#tasks-window'), 'note pages carry no Tasks panel').toBeHidden();
  await expect(page.locator('.outline-ticks span')).toHaveCount(12);
  await quiet(page);
  const hits = await page.evaluate(() => [...document.querySelectorAll('.outline-ticks span')].map((tick) => {
    const r = tick.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + 4, r.top + r.height / 2);
    return hit && !!hit.closest('.dock');
  }));
  expect(hits).not.toContain(true);
  const last = await page.locator('.outline-ticks span').last().boundingBox();
  await page.mouse.move(last.x + 3, last.y + last.height / 2);
  await expect(page.locator('.outline-preview')).toBeVisible();
  const z = await page.evaluate(() => ({ outline: +getComputedStyle(document.querySelector('.note-outline')).zIndex, dock: +getComputedStyle(document.querySelector('.dock')).zIndex }));
  expect(z.outline).toBeGreaterThan(z.dock);
});

test('lit passages join across the blank line between them without a seam', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Glow seams', body: 'First paragraph.\n\nSecond paragraph.\n\nThird paragraph.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc .blk')).toHaveCount(5);
  await page.evaluate(() => [0, 2, 4].forEach(i => document.querySelectorAll('#doc .blk')[i].classList.add('blk-glow')));
  await quiet(page);
  const paint = await page.evaluate(() => [...document.querySelectorAll('#doc .blk')].map(el => {
    const style = getComputedStyle(el);
    return { fill: style.backgroundColor !== 'rgba(0, 0, 0, 0)', top: style.borderTopLeftRadius, bottom: style.borderBottomLeftRadius };
  }));
  expect(paint.map(p => p.fill)).toEqual([true, true, true, true, true]);
  expect(paint.map(p => p.top + p.bottom)).toEqual(Array(5).fill('0px0px'));
});

test('the New task title placeholder fits its field', async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 760 });
  await page.goto('/#/calendar');
  await page.locator('[data-action="task-new"]').first().click();
  const title = page.locator('#task-title');
  await expect(title).toBeVisible();
  const fit = await title.evaluate(el => {
    const style = getComputedStyle(el);
    const ctx = document.createElement('canvas').getContext('2d');
    ctx.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    return { text: ctx.measureText(el.placeholder).width, room: el.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) };
  });
  expect(fit.text).toBeLessThanOrEqual(fit.room);
});

test('a narrow Felix panel keeps the placeholder on one line, the note chip readable and the send button inside', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Word study notes', body: 'Hello.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await page.locator('#chat-toggle').click();
  await expect(page.locator('.chat-chip:not([hidden])')).toBeVisible();
  await page.evaluate(() => document.documentElement.style.setProperty('--chat-w', '330px'));
  await expect.poll(() => page.locator('#chat').evaluate(el => el.getBoundingClientRect().width)).toBeLessThan(400);
  const fit = await page.evaluate(() => {
    const input = document.querySelector('#chat-input');
    const form = document.querySelector('#chat-form').getBoundingClientRect();
    const chip = document.querySelector('.chat-chip:not([hidden])').getBoundingClientRect();
    const send = document.querySelector('#chat-send').getBoundingClientRect();
    return { lines: input.scrollHeight <= input.clientHeight, chip: chip.width, inside: send.right <= form.right };
  });
  expect(fit.lines).toBe(true);
  expect(fit.chip).toBeGreaterThanOrEqual(76);
  expect(fit.inside).toBe(true);
});

test('the note tools live in one menu beside the Felix chips, and there is no floating toolbar', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Note menu', body: 'Hello.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('nav.actions')).toHaveCount(0);
  await expect(page.locator('.dock')).toBeHidden();
  const more = page.locator('.note-meta #note-more');
  await expect(more).toHaveAccessibleName('More for this note');
  await more.click();
  await expect(page.getByRole('menu', { name: 'This note' }).getByRole('menuitem')).toHaveText(['Tasks', 'Add picture', 'Move to folder', 'Share', 'Move to trash']);
  await page.getByRole('menuitem', { name: 'Share' }).click();
  await expect(page.getByRole('menuitem', { name: 'Copy as Markdown' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#note-facts')).toHaveText(/^Edited /);
  await expect(page.locator('#crumbs')).toHaveCSS('visibility', 'hidden');
});

test('the Usage sheet icon buttons name themselves on hover', async ({ page }) => {
  await page.goto('/#/settings');
  await expect(page.locator('#side [data-action="usage"]'), 'Usage lives in Settings, not the rail').toHaveCount(0);
  await page.locator('.settings-nav [data-action="usage"]').click();
  const tools = page.locator('.usage-tools .icon-btn');
  await expect(tools).toHaveCount(2);
  for (const button of await tools.all()) expect(await button.getAttribute('title')).toBe(await button.getAttribute('aria-label'));
});

test('Felix only bobs while his panel is open', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Bob', body: 'Hello.' } })).json();
  const bobbing = () => page.evaluate(() => document.getAnimations().filter(a => a.animationName === 'felix-bob' && a.playState === 'running').length);
  await page.goto(`/#/n/${note.id}`);
  await page.waitForTimeout(800);
  expect(await bobbing()).toBe(0);
  await page.goto('/#/settings');
  await page.waitForTimeout(500);
  expect(await bobbing()).toBe(0);
  await page.goto(`/#/n/${note.id}`);
  await page.locator('#chat-toggle').click();
  await expect.poll(bobbing).toBeGreaterThan(0);
  await page.locator('#chat .chat-head [data-chat="close"], #chat [data-chat="close"]').first().click();
  await expect(page.locator('#chat')).toBeHidden();
  expect(await bobbing()).toBe(0);
});

test('turning the eye off hides the starters that need the open note', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Starters', body: 'Hello.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await page.locator('#chat-toggle').click();
  await expect(page.locator('.starter')).toHaveText(['Teach me this note step by step', 'Quiz me on this note', 'Make me a 3-day review plan']);
  await page.locator('[data-chat="drop"]').click();
  await expect(page.locator('.starter')).toHaveText(['Make me a 3-day review plan']);
  await page.locator('[data-chat="drop"]').click();
  await expect(page.locator('.starter')).toHaveCount(3);
});

test('margin notes on passages inside tables, code and quotes sit beside their passage without overlapping', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const quotes = ['Seek time dominates small reads.', 'rotation = 60 / rpm / 2', 'Flash wears out after many writes.', 'Optane sat between DRAM and flash.'];
  const body = `Opening words about storage.\n\n| Part | Cost |\n| --- | --- |\n| Disk | ${quotes[0]} |\n\nMiddle words.\n\n\`\`\`\n${quotes[1]}\n\`\`\`\n\nMore words.\n\n> ${quotes[2]}\n\nLast words. ${quotes[3]}`;
  const note = await (await page.request.post('/api/notes', { data: { title: 'Storage gap', body } })).json();
  for (const quote of quotes) expect((await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote, question: 'Why?' } })).ok()).toBe(true);
  await page.goto(`/#/n/${note.id}`);
  const cards = page.locator('.margin-note');
  await expect(cards).toHaveCount(4);
  await expect.poll(async () => {
    const boxes = await cards.evaluateAll((els) => els.filter((el) => !el.hidden).map((el) => el.getBoundingClientRect()).map(({ top, bottom, left, right }) => ({ top, bottom, left, right })));
    if (boxes.length !== 4) return `shown ${boxes.length}`;
    const clash = boxes.some((a, i) => boxes.some((b, j) => i < j && a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom));
    return clash ? 'overlap' : 'apart';
  }).toBe('apart');
  const title = await page.locator('#title').boundingBox();
  for (const [i, quote] of quotes.entries()) {
    const card = await cards.nth(i).boundingBox();
    expect(card.y, quote).toBeGreaterThan(title.y + title.height);
  }
});

test('a margin note on the second copy of a line in a code block sits by that copy', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const quote = 'retry the write';
  const body = `Opening words.\n\n\`\`\`\n${quote}\nwait a moment\n${quote}\n\`\`\`\n\nClosing words.`;
  const note = await (await page.request.post('/api/notes', { data: { title: 'Second copy', body } })).json();
  expect((await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote, prefix: 'wait a moment\n', suffix: '\n```', question: 'Why twice?' } })).ok()).toBe(true);
  await page.goto(`/#/n/${note.id}`);
  const line = await page.locator('.margin-ink .squiggle').first().boundingBox();
  const code = await page.locator('#doc pre').first().boundingBox();
  expect(line.y).toBeGreaterThan(code.y + code.height * 0.6);
});

test('a resolved passage reopens', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const quote = 'The password is stored as plain text.';
  const note = await (await page.request.post('/api/notes', { data: { title: 'Reopen', body: `Opening words.\n\n${quote} More words after it.` } })).json();
  expect((await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote, question: 'Why?' } })).ok()).toBe(true);
  const open = async () => (await (await page.request.get(`/api/notes/${note.id}/margin`)).json()).threads.length;
  await page.goto(`/#/n/${note.id}`);
  const card = page.locator('.margin-note');
  const back = page.getByTitle("Reopen Felix's note");
  await expect(card).toBeVisible();
  await expect(back).toHaveCount(0);
  await card.click();
  await card.getByRole('button', { name: 'Resolve' }).click();
  await expect(card, 'a resolved note stays faded until the note is left').toHaveAttribute('data-resolved', /.+/);
  await expect(page.locator('.toast')).toHaveCount(0);
  await expect(back).toBeVisible();
  await expect(back).toHaveCSS('border-bottom-style', 'dotted');
  expect(await open()).toBe(0);
  await back.click();
  await expect(card).toBeVisible();
  await expect(card).toHaveAttribute('data-open', '');
  await expect(card.locator('.margin-felix')).toBeVisible();
  await expect(back).toHaveCount(0);
  expect(await open()).toBe(1);
  await card.getByRole('button', { name: 'Resolve' }).click();
  await expect(card).toHaveAttribute('data-resolved', /.+/);
  await expect(back).toBeVisible();
  await back.focus();
  await page.keyboard.press('Enter');
  await expect(card).toHaveAttribute('data-open', '');
  await expect(back).toHaveCount(0);
  await card.getByRole('button', { name: 'Resolve' }).click();
  await expect(card).toHaveAttribute('data-resolved', /.+/);
  await back.focus();
  await page.keyboard.press('Space');
  await expect(card).toHaveAttribute('data-open', '');
  await expect(back).toHaveCount(0);
  expect(await open()).toBe(1);
  await page.reload();
  await expect(card).toBeVisible();
  await expect(back).toHaveCount(0);
});

test('a changed passage says so', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const quote = 'The password is stored as plain text.';
  const note = await (await page.request.post('/api/notes', { data: { title: 'Changed passage', body: `Opening words.\n\n${quote} More words after it.` } })).json();
  expect((await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote, prefix: 'Opening words.\n\n', suffix: ' More words after it.', question: 'Why?' } })).ok()).toBe(true);
  await page.goto(`/#/n/${note.id}`);
  const card = page.locator('.margin-note');
  await expect(card).toBeVisible();
  await expect(card.locator('.margin-changed')).toBeHidden();
  await page.locator('#doc .blk', { hasText: 'More words' }).click();
  const moved = page.waitForResponse((r) => r.url().includes(`/api/notes/${note.id}/margin/threads/`) && r.request().method() === 'PATCH');
  await page.locator('#doc textarea.line-edit').fill('The password is stored as clear text. More words after it.');
  await expect(card.locator('.margin-changed')).toHaveText('The passage changed since Felix wrote this.');
  await expect(card.locator('.margin-changed')).toBeVisible();
  expect((await moved).status()).toBe(200);
  await page.reload();
  await expect(card).toBeVisible();
  await expect(card.locator('.margin-changed')).toBeVisible();
});

test('earlier messages fold', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const quote = 'The password is stored as plain text.';
  const note = await (await page.request.post('/api/notes', { data: { title: 'Long thread', body: `Opening words.\n\n${quote} More words after it.` } })).json();
  expect((await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote, question: 'Why?' } })).ok()).toBe(true);
  await page.route(`**/api/notes/${note.id}/margin`, async (route) => {
    if (route.request().method() !== 'GET') return route.continue();
    const response = await route.fetch();
    const body = await response.json();
    body.threads[0].messages = Array.from({ length: 8 }, (_, i) => ({ from: i % 2 ? 'you' : 'felix', text: `Message ${i + 1}`, at: '2026-11-14T10:00:00Z' }));
    await route.fulfill({ response, json: body });
  });
  await page.goto(`/#/n/${note.id}`);
  const card = page.locator('.margin-note');
  await expect(card).toBeVisible();
  await card.click();
  const earlier = card.getByRole('button', { name: 'Show 2 earlier' });
  await expect(earlier).toBeVisible();
  await expect(card.locator('.margin-said > div')).toHaveCount(6);
  await earlier.click();
  await expect(card.locator('.margin-said > div')).toHaveCount(8);
  await expect(card.locator('.margin-said').getByText('Message 1')).toBeVisible();
  await expect(earlier).toHaveCount(0);
});

test('Export as PDF with Felix’s notes prints numbered notes and leaves the note text, quotes and saved body as they were', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const body = 'A transition matrix holds the chance of moving.\n\n- Each row sums to one hundred\n- Absorbing states never leave';
  const note = await (await page.request.post('/api/notes', { data: { title: 'Markov chains', body } })).json();
  const thread = (id, quote, text) => ({ id, note: note.id, quote, prefix: '', suffix: '', status: 'open', source: 'read', pinned: false, messages: [{ from: 'felix', text, at: '2026-11-14T10:00:00Z' }], created_at: '2026-11-14T10:00:00Z', updated_at: '2026-11-14T10:00:00Z' });
  await page.route(`**/api/notes/${note.id}/margin`, async (route) => {
    if (route.request().method() !== 'GET') return route.continue();
    const view = await (await route.fetch()).json();
    await route.fulfill({ json: { ...view, threads: [thread('t2', 'Each row sums to one hundred', 'Rows sum to one.'), thread('t1', 'transition matrix', 'Indexed by state.')] } });
  });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('.margin-note')).toHaveCount(2);
  await page.evaluate(() => { window.print = async () => { window.printSaw = document.querySelectorAll('[data-print]').length; }; });
  const text = await page.locator('#doc').textContent();
  await page.locator('#note-more').click();
  await page.getByRole('menuitem', { name: 'Share' }).click();
  await page.getByRole('menuitem', { name: 'Export as PDF with Felix’s notes' }).click();
  await expect.poll(async () => (await page.evaluate(() => window.printSaw)) ?? `no print: ${JSON.stringify({ errors, toasts: await page.locator('.toast').allTextContents() })}`).toBe(4);
  await expect(page.locator('#doc')).toHaveText(text);
  await page.emulateMedia({ media: 'print' });
  await expect(page.locator('#doc sup[data-print]')).toHaveCount(2);
  expect(await page.locator('#doc sup[data-print]').evaluateAll((sups) => sups.map((s) => [s.dataset.print, s.previousSibling.textContent.trim().slice(-6)]))).toEqual([['1', 'matrix'], ['2', 'undred']]);
  await expect(page.locator('.print-notes li')).toHaveText(['Indexed by state.', 'Rows sum to one.']);
  await page.emulateMedia({ media: 'screen' });
  await expect(page.locator('.print-notes')).toBeHidden();
  await page.keyboard.press('Shift');
  await expect(page.locator('[data-print]')).toHaveCount(0);
  await expect(page.locator('#doc')).toHaveText(text);
  await page.locator('#note-more').click();
  await page.getByRole('menuitem', { name: 'Share' }).click();
  await page.getByRole('menuitem', { name: 'Export as PDF', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.printSaw)).toBe(0);
  expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe(body);
});

test('opening the open note again keeps its column still while it loads', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 820 });
  const stamp = Date.now().toString(36);
  const note = await (await page.request.post('/api/notes', { data: { title: `Still ${stamp}`, body: 'One line.\n\nAnother line.', directory: `Still ${stamp}` } })).json();
  await page.goto(`/#/n/${note.id}`);
  const row = page.locator(`#side .side-note[data-id="${note.id}"]`);
  await expect(row).toBeVisible();
  await quiet(page);
  const before = await page.locator('#doc').boundingBox();
  let release;
  const held = new Promise((done) => { release = done; });
  await page.route(`**/api/notes/${note.id}**`, async (route) => { await held; await route.continue(); });
  await row.click();
  await expect(page.locator('.margin-layer')).toHaveCount(0);
  const during = await page.locator('#doc').boundingBox();
  release();
  expect([during.x, during.width]).toEqual([before.x, before.width]);
  await page.request.delete(`/api/notes/${note.id}`);
});

test('a resolved note stays faded with a Resolved label until it is reopened', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Faded', body: 'Intro line\n\nHeat flows from the hot side to the cold side.\n\nLast line' } })).json();
  await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote: 'the hot side', question: 'Why?' } });
  await page.goto(`/#/n/${note.id}`);
  const thread = page.locator('.margin-note').first();
  await expect(thread).toBeVisible();
  await thread.locator('.margin-toggle').click({ force: true });
  await page.locator('.margin-note[data-open]').getByRole('button', { name: 'Resolve' }).click();
  await expect(thread).toHaveAttribute('data-resolved', /.+/);
  await expect(thread.locator('.margin-resolved-label')).toHaveText('Resolved');
  await page.mouse.move(5, 5);
  await page.evaluate(() => document.activeElement?.blur());
  await expect(thread).toHaveCSS('opacity', '0.45');
  await thread.locator('.margin-toggle').click({ force: true });
  await expect(thread).not.toHaveAttribute('data-resolved', /.+/);
  await expect(thread).toHaveAttribute('data-open', '');
});

test('dragging a top corner of the note sets its width live, never past the room its margins need, and a double-click resets it', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Wide page', body: 'Words.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  const article = page.locator('article.note');
  await expect(article).toBeVisible();
  const before = (await article.boundingBox()).width;
  const mark = page.locator('.page-mark[data-edge="right"]');
  await expect(mark).toHaveAttribute('role', 'separator');
  const box = await mark.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 160, box.y + box.height / 2, { steps: 6 });
  await expect(page.locator('.page-width-label')).toHaveText(/^\d+ px$/);
  await page.mouse.up();
  await expect(page.locator('.page-width-label')).toBeHidden();
  const columns = page.locator('.note-columns');
  const most = await columns.evaluate((el) => window.leoMargin.mostColumn({ width: el.clientWidth, setting: 'both' }));
  const wide = Number(await page.evaluate(() => localStorage.getItem('leo-page-width')));
  expect(wide, 'a drag stops where both margins still fit').toBeLessThanOrEqual(most);
  await expect(columns).toHaveAttribute('data-margin', 'both');
  const held = await mark.boundingBox();
  await page.mouse.move(held.x + held.width / 2, held.y + held.height / 2);
  await page.mouse.down();
  await page.mouse.move(held.x + held.width / 2 - 100, held.y + held.height / 2, { steps: 6 });
  await page.mouse.up();
  const saved = Number(await page.evaluate(() => localStorage.getItem('leo-page-width')));
  expect(saved).toBeLessThan(wide);
  await expect.poll(async () => Math.round((await article.boundingBox()).width)).toBe(saved);
  await expect(columns).toHaveAttribute('data-margin', 'both');
  const after = await mark.boundingBox();
  await page.mouse.dblclick(after.x + after.width / 2, after.y + after.height / 2);
  await expect.poll(() => page.evaluate(() => localStorage.getItem('leo-page-width'))).toBeNull();
  await expect.poll(async () => Math.round((await article.boundingBox()).width)).toBe(Math.round(before));
  await mark.focus();
  await page.keyboard.press('ArrowLeft');
  await expect.poll(() => page.evaluate(() => Number(localStorage.getItem('leo-page-width')))).toBe(Math.round(before) - 20);
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => page.evaluate(() => Number(localStorage.getItem('leo-page-width')))).toBe(Math.min(Math.round(before), most));
  await page.keyboard.press('Enter');
  await expect.poll(() => page.evaluate(() => localStorage.getItem('leo-page-width'))).toBeNull();
  for (let i = 0; i < 30; i++) await page.keyboard.press('ArrowRight');
  await expect.poll(() => page.evaluate(() => Number(localStorage.getItem('leo-page-width'))), 'the keyboard stops where both margins still fit').toBe(most);
  await expect(columns).toHaveAttribute('data-margin', 'both');
});

test('dragging the note wider with the margin on the right stops where one margin still fits', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.request.post('/api/margin', { data: { layout: 'right' } });
  try {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Wide right page', body: 'Words.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    const columns = page.locator('.note-columns');
    await expect(columns).toHaveAttribute('data-margin', 'right');
    const mark = page.locator('.page-mark[data-edge="right"]');
    const box = await mark.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 600, box.y + box.height / 2, { steps: 6 });
    await page.mouse.up();
    const most = await columns.evaluate((el) => window.leoMargin.mostColumn({ width: el.clientWidth, setting: 'right' }));
    const wide = Number(await page.evaluate(() => localStorage.getItem('leo-page-width')));
    expect(wide, 'a drag stops where the one margin still fits').toBeLessThanOrEqual(most);
    await expect(columns).toHaveAttribute('data-margin', 'right');
  } finally {
    await page.request.post('/api/margin', { data: { layout: 'both' } });
  }
});

test('a long selection stays in the margin and the Felix bar says what he reads', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const long = 'A long sentence about heat moving between two places over time. '.repeat(20);
  const note = await (await page.request.post('/api/notes', { data: { title: 'Long pick', body: `Short line.\n\n${long}` } })).json();
  const asked = [];
  await page.route(`**/api/notes/${note.id}/margin/threads`, (route) => {
    asked.push(route.request().postDataJSON());
    return route.fulfill({ json: answered(note.id, asked.at(-1), 'Heat flows from hot to cold.') });
  });
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc')).toBeVisible();
  const menu = page.getByRole('toolbar', { name: 'Felix and this passage' });
  expect(await pickText(page, 'Short line.')).toBe(true);
  await expect(menu).toBeVisible();
  await expect(menu.locator('.ask-long')).toBeHidden();
  expect(await pickText(page, long.trim())).toBe(true);
  await expect(menu.locator('.ask-long')).toBeVisible();
  await expect(menu.locator('.ask-long')).toHaveText('Felix reads the first 1,000 characters of the selected passage.');
  await expect(menu.getByRole('button')).toHaveText([/Felix this/, 'Ask Felix', 'Visualize']);
  await menu.getByRole('button', { name: 'Ask Felix' }).click();
  const draft = page.locator('.margin-note[data-id="draft"]');
  await expect(draft.locator('[data-passage-hint]')).toBeVisible();
  const question = draft.getByRole('textbox', { name: 'Ask Felix about this passage' });
  await question.fill('Which way does heat flow?');
  await question.press('Enter');
  await expect(page.locator('.margin-note[data-id="t1"] .margin-felix')).toHaveText('Heat flows from hot to cold.');
  expect(asked).toHaveLength(1);
  expect(asked[0]).toMatchObject({ quote: long.trim(), question: 'Which way does heat flow?' });
});
