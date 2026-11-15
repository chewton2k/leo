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
  expect(asked[0]).toMatchObject({ quote: 'the hot side', question: 'Draw this step by step.' });
  await expect(page.locator('.margin-note[data-open] .margin-felix')).toHaveText('Here it is.');
  await page.keyboard.press('Escape');
  await select();
  await expect(menu).toBeVisible();
  await page.keyboard.press('ControlOrMeta+Enter');
  await expect.poll(() => asked.length).toBe(2);
  expect(asked[1]).toMatchObject({ quote: 'the hot side', question: 'Respond to this passage.' });
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
  expect(anchor.x + anchor.width).toBeLessThanOrEqual(paragraph.x);
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
  await expect(page.locator('#tasks-window')).toBeVisible();
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

test('the note toolbar moves clear of an overlaying Felix panel instead of hiding behind it', async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 700 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Overlay toolbar', body: 'Hello.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('nav.actions')).toBeVisible();
  await page.locator('#chat-toggle').click();
  await expect(page.locator('#chat')).toBeVisible();
  await quiet(page);
  const gap = await page.evaluate(() => ({ pill: document.querySelector('nav.actions').getBoundingClientRect(), chat: document.querySelector('#chat').getBoundingClientRect() }));
  expect(gap.pill.left).toBeGreaterThanOrEqual(0);
  expect(gap.pill.right).toBeLessThanOrEqual(gap.chat.left + 1);
});

test('the Usage sheet icon buttons name themselves on hover', async ({ page }) => {
  await page.goto('/');
  await page.locator('#side [data-action="usage"]').click();
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
  await expect(page.locator('.starter')).toHaveCount(5);
  await page.locator('[data-chat="drop"]').click();
  await expect(page.locator('.starter')).toHaveText(['Quiz me across my classes', 'Make me a 3-day review plan']);
  await page.locator('[data-chat="drop"]').click();
  await expect(page.locator('.starter')).toHaveCount(5);
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
  await expect(card).toHaveCount(0);
  await expect(page.locator('.margin-ink .squiggle')).toHaveCount(0);
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
  await expect(card).toHaveCount(0);
  await expect(back).toBeVisible();
  await back.focus();
  await page.keyboard.press('Enter');
  await expect(card).toHaveAttribute('data-open', '');
  await expect(back).toHaveCount(0);
  await card.getByRole('button', { name: 'Resolve' }).click();
  await expect(card).toHaveCount(0);
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
  await page.locator('#doc textarea.line-edit').fill('The password is stored as clear text. More words after it.');
  await expect(card.locator('.margin-changed')).toHaveText('The passage changed since Felix wrote this.');
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
  await page.locator('[data-action="share"]').first().click();
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
  await page.locator('[data-action="share"]').first().click();
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
