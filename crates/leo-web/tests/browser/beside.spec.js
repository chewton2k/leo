const { test, expect } = require('@playwright/test');
const { signIn } = require('./sign-in');

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64');
let made = 0;

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await signIn(page);
});

async function notes(page, ...bodies) {
  const directory = `Split ${test.info().project.name} ${Date.now()} ${++made}`;
  const out = [];
  for (const [i, body] of bodies.entries()) out.push(await (await page.request.post('/api/notes', { data: { title: `Pane ${'ABCD'[i]} ${made}`, body, directory } })).json());
  return out;
}

async function listed(page, directory) {
  const got = await (await page.request.get(`/api/notes?dir=${encodeURIComponent(directory)}`)).json();
  return Array.isArray(got) ? got : got.items;
}

async function open(page, note) {
  await page.evaluate((hash) => { location.hash = hash; }, `#/n/${note.id}`);
  await expect(page.locator('#app #title')).toHaveText(note.title);
  await expect(page.locator(`#side .side-note[data-id="${note.id}"]`)).toBeVisible();
}

async function besideMenu(page, note, choice = 'Open beside') {
  await page.locator(`#side .side-note[data-id="${note.id}"]`).click({ button: 'right' });
  await page.getByRole('menuitem', { name: choice }).click();
}

async function split(page, left, right) {
  await open(page, left);
  await besideMenu(page, right);
  await expect(page.locator('#beside #title')).toHaveText(right.title);
  await expect(page.locator('body')).toHaveClass(/\bsplit\b/);
}

async function typeIn(page, pane, text) {
  await page.locator(`${pane} #doc .blk`).first().click();
  await page.keyboard.press('End');
  await page.keyboard.type(text);
}

test('a note dropped on the right third opens beside with its own width marks and margin, and on the left it opens here', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const [a, b, c] = await notes(page, 'Alpha text.', 'Beta text.', 'Gamma text.');
  await open(page, a);
  const drag = async (note, x) => {
    const transfer = await page.evaluateHandle(() => new DataTransfer());
    await page.locator(`#side .side-note[data-id="${note.id}"]`).dispatchEvent('dragstart', { dataTransfer: transfer });
    const box = await page.locator('#app').boundingBox();
    const at = { dataTransfer: transfer, clientX: x(box), clientY: 400 };
    await page.locator('#app').dispatchEvent('dragover', at);
    return { transfer, at };
  };
  const right = await drag(b, (box) => box.x + box.width - 40);
  await expect(page.locator('body')).toHaveAttribute('data-note-drop', 'beside');
  await page.locator('#app').dispatchEvent('drop', right.at);
  await expect(page.locator('body')).not.toHaveAttribute('data-note-drop', /./);
  await expect(page.locator('#beside')).toBeVisible();
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  await expect(page.locator('#beside #doc')).toContainText('Beta text.');
  await expect(page.locator('#beside .page-mark')).toHaveCount(2);
  await expect(page.locator('#beside .note-columns')).toHaveAttribute('data-margin', /^(marks|right|both)$/);
  await expect(page.locator('#beside .margin-layer')).toHaveCount(1);
  await expect(page.locator('#app .margin-layer')).toHaveCount(1);
  await expect(page.locator('#app #title')).toHaveText(a.title);

  const left = await drag(c, (box) => box.x + 80);
  await expect(page.locator('body')).toHaveAttribute('data-note-drop', 'here');
  await page.locator('#app').dispatchEvent('drop', left.at);
  await expect(page.locator('#app #title')).toHaveText(c.title);
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  expect(new URL(page.url()).hash).toBe(`#/n/${c.id}`);

  const files = await page.evaluateHandle(() => { const t = new DataTransfer(); t.items.add(new File(['x'], 'x.txt', { type: 'text/plain' })); return t; });
  await page.locator('#app').dispatchEvent('dragover', { dataTransfer: files, clientX: 1800, clientY: 400 });
  await expect(page.locator('body')).not.toHaveAttribute('data-note-drop', /./);
});

test('Open beside in the explorer note menu opens a second pane and Close split view closes it', async ({ page }) => {
  const [a, b] = await notes(page, 'One.', 'Two.');
  await open(page, a);
  await page.locator(`#side .side-note[data-id="${b.id}"]`).click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: 'Close split view' })).toHaveCount(0);
  await page.getByRole('menuitem', { name: 'Open beside' }).click();
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  await expect(page.locator('#app .pane-close')).toBeVisible();
  await expect(page.locator('#beside .pane-close')).toBeVisible();
  await page.locator(`#side .side-note[data-id="${b.id}"]`).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Close split view' }).click();
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('body')).not.toHaveClass(/\bsplit\b/);
  await expect(page.locator('#app .pane-close')).toBeHidden();
  await expect(page.locator('#app #title')).toHaveText(a.title);
});

test('the divider resizes the panes, is remembered, resets to half on double-click and moves with the arrow keys', async ({ page }) => {
  const [a, b] = await notes(page, 'One.', 'Two.');
  await split(page, a, b);
  const divider = page.getByRole('separator', { name: 'Note panes width' });
  await expect(divider).toBeVisible();
  const width = () => page.locator('#beside').evaluate((el) => Math.round(el.getBoundingClientRect().width));
  const half = await width();
  const box = await divider.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + 300);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 - 60, box.y + 300, { steps: 4 });
  await page.mouse.move(box.x + box.width / 2 - 120, box.y + 300, { steps: 4 });
  await page.mouse.up();
  await expect.poll(width).toBeGreaterThan(half + 100);
  const wider = await width();
  expect(Number(await page.evaluate(() => localStorage.getItem('leo-beside-width')))).toBe(wider);
  const line = await divider.boundingBox();
  const pane = await page.locator('#beside').boundingBox();
  expect(Math.abs(line.x + line.width / 2 - pane.x)).toBeLessThan(2);

  await besideMenu(page, b, 'Close split view');
  await besideMenu(page, b);
  await expect.poll(width).toBe(wider);

  await divider.focus();
  await page.keyboard.press('ArrowRight');
  await expect.poll(width).toBe(wider - 32);
  await page.keyboard.press('ArrowLeft');
  await expect.poll(width).toBe(wider);

  const at = await divider.boundingBox();
  await page.mouse.dblclick(at.x + at.width / 2, at.y + 300);
  await expect.poll(async () => Math.abs((await width()) - half)).toBeLessThan(2);
  expect(await page.evaluate(() => localStorage.getItem('leo-beside-width'))).toBeNull();
});

test('closing the left pane keeps the right note and its unsaved typing, with no conflict copy', async ({ page }) => {
  const [a, b] = await notes(page, 'Left body.', 'Right body.');
  await split(page, a, b);
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  await page.route(`**/api/notes/${b.id}`, async (route) => {
    if (route.request().method() === 'PATCH') await held;
    await route.fallback();
  });
  await typeIn(page, '#beside', ' typed beside');
  const before = await page.evaluate(() => history.length);
  await page.locator('#app .pane-close').click();
  await expect(page.locator('body')).not.toHaveClass(/\bsplit\b/);
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('#app #title')).toHaveText(b.title);
  await expect(page.locator('#app #doc')).toContainText('Right body. typed beside');
  expect(new URL(page.url()).hash).toBe(`#/n/${b.id}`);
  expect(await page.evaluate(() => history.length)).toBe(before);
  release();
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toContain('typed beside');
  await expect(page.locator('#app #save-state')).toHaveClass(/save-dry/);
  const titles = (await listed(page, b.directory)).map((n) => n.title);
  expect(titles.filter((t) => /conflict from leo/.test(t))).toEqual([]);
});

test('the right pane × saves and returns to one note, and a page that is not a note closes the right pane', async ({ page }) => {
  const [a, b] = await notes(page, 'Left.', 'Right.');
  await split(page, a, b);
  await typeIn(page, '#beside', ' saved on close');
  await page.locator('#beside .pane-close').click();
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('body')).not.toHaveClass(/\bsplit\b/);
  await expect(page.locator('#app #title')).toHaveText(a.title);
  expect(new URL(page.url()).hash).toBe(`#/n/${a.id}`);
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toContain('saved on close');

  await besideMenu(page, b);
  await typeIn(page, '#beside', ' kept on calendar');
  await page.evaluate(() => { location.hash = '#/calendar'; });
  await expect(page.locator('body')).toHaveClass(/on-calendar/);
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('body')).not.toHaveClass(/\bsplit\b/);
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toContain('kept on calendar');
  await page.goBack();
  await expect(page.locator('#app #title')).toHaveText(a.title);
  await expect(page.locator('#beside')).toBeHidden();
});

test('Felix follows the last clicked pane, and an open chat keeps its messages', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const [a, b] = await notes(page, 'Left words.', 'Right words.');
  const asked = [];
  await page.route('**/api/chat', async (route) => {
    asked.push(route.request().postDataJSON());
    await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Noted."}\n{"done":true}\n' });
  });
  await split(page, a, b);
  await page.locator('#chat-toggle').click();
  const chat = page.locator('#chat');
  await expect(page.locator('body')).toHaveClass(/\bchat-open\b/);
  await expect(page.locator('body')).toHaveClass(/\bside-mini\b/);
  await expect(page.locator('body')).toHaveClass(/\bsplit\b/);
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  expect(await page.locator('#beside').evaluate((el) => el.getBoundingClientRect().width)).toBeGreaterThanOrEqual(380);
  expect(await page.locator('#app').evaluate((el) => el.getBoundingClientRect().width)).toBeGreaterThanOrEqual(380);
  await page.locator('#app #note-facts').click();
  await chat.locator('#chat-input').fill('about the left');
  await chat.locator('#chat-input').press('Enter');
  await expect.poll(() => asked.length).toBe(1);
  expect(asked[0].note).toBe(a.id);
  await expect(chat.locator('.msg.leo').last()).toContainText('Noted.');

  await page.locator('#beside #note-facts').click();
  await expect(page.locator('#beside')).toHaveClass(/\bpane-on\b/);
  await expect(chat.locator('.msg.user')).toHaveCount(1);
  await chat.locator('#chat-input').fill('about the right');
  await chat.locator('#chat-input').press('Enter');
  await expect.poll(() => asked.length).toBe(2);
  expect(asked[1].note).toBe(b.id);
  await expect(chat.locator('.msg.user')).toHaveCount(2);
  await expect(page).toHaveTitle(new RegExp(b.title));
});

test('Felix changing the right note reloads only that pane while the left pane still has unsaved typing', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1000 });
  const [a, b] = await notes(page, 'Left words.', 'Right words.');
  await split(page, a, b);
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  await page.route(`**/api/notes/${a.id}`, async (route) => {
    if (route.request().method() === 'PATCH') await held;
    await route.fallback();
  });
  await typeIn(page, '#app', ' still typing');
  await page.request.patch(`/api/notes/${b.id}`, { data: { body: 'Right words, changed by Felix.' } });
  await page.route('**/api/chat', async (route) => {
    const applied = { kind: 'edit', note: b.id, title: b.title, find: 'words', replace: 'words, changed by Felix', why: '', state: 'applied', before: 'Right words.', after: 'v' };
    await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { proposal: applied }, { t: 'Changed it.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' });
  });
  await page.locator('#chat-toggle').click();
  await page.locator('#chat #chat-input').fill('change the right note');
  await page.locator('#chat #chat-input').press('Enter');
  await expect(page.locator('#beside #doc')).toContainText('Right words, changed by Felix.');
  await expect(page.locator('#app #doc')).toContainText('Left words. still typing');
  release();
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${a.id}`)).json()).body).toContain('still typing');
  const titles = (await listed(page, a.directory)).map((n) => n.title);
  expect(titles.filter((t) => /conflict from leo/.test(t))).toEqual([]);
});

test('a picture still uploading lands in its own pane after focus moves to the other', async ({ page }) => {
  const [a, b] = await notes(page, 'Left.', 'Right.');
  await split(page, a, b);
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  let started;
  const begun = new Promise((resolve) => { started = resolve; });
  await page.route('**/api/pictures/**', async (route) => {
    started();
    await held;
    await route.fallback();
  });
  await page.locator('#beside #note-more').click();
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('menuitem', { name: 'Add picture' }).click();
  await (await chooser).setFiles({ name: 'dot.png', mimeType: 'image/png', buffer: PNG });
  await begun;
  await page.locator('#app #note-facts').click();
  await expect(page.locator('#app')).toHaveClass(/\bpane-on\b/);
  release();
  await expect(page.locator('#beside #doc img')).toHaveCount(1);
  await expect(page.locator('#app #doc img')).toHaveCount(0);
});

test('Back and Forward change only the left pane while the right is focused, and opening the right note promotes it', async ({ page }) => {
  const [a, b, c] = await notes(page, 'First.', 'Beside.', 'Third.');
  await open(page, a);
  await open(page, c);
  await besideMenu(page, b);
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  await page.locator('#beside #note-facts').click();
  await page.goBack();
  await expect(page.locator('#app #title')).toHaveText(a.title);
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  await page.goForward();
  await expect(page.locator('#app #title')).toHaveText(c.title);
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  await page.evaluate((hash) => { location.hash = hash; }, `#/n/${b.id}`);
  await expect(page.locator('#app #title')).toHaveText(b.title);
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('body')).not.toHaveClass(/\bsplit\b/);
});

test('moving the right pane note to the trash closes that pane only', async ({ page }) => {
  const [a, b] = await notes(page, 'Stays.', 'Goes.');
  await split(page, a, b);
  await page.locator('#beside #note-more').click();
  await page.getByRole('menuitem', { name: 'Move to trash' }).click();
  await expect(page.locator('.toast')).toContainText('Moved');
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('body')).not.toHaveClass(/\bsplit\b/);
  await expect(page.locator('#app #title')).toHaveText(a.title);
  expect(new URL(page.url()).hash).toBe(`#/n/${a.id}`);
});

test('a note already open in one pane is refused in the other and focus goes to the pane holding it', async ({ page }) => {
  const [a, b] = await notes(page, 'One.', 'Two.');
  await split(page, a, b);
  await page.locator('#beside #note-facts').click();
  await expect(page.locator('#beside')).toHaveClass(/\bpane-on\b/);
  await besideMenu(page, a);
  await expect(page.locator('.toast')).toContainText('already open');
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  await expect(page.locator('#app #title')).toHaveText(a.title);
  await expect(page.locator('#app')).toHaveClass(/\bpane-on\b/);
  await expect(page.locator('#beside')).not.toHaveClass(/\bpane-on\b/);
});

test('a window narrowed to 760px closes the right pane with a toast', async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const [a, b] = await notes(page, 'One.', 'Two.');
  await split(page, a, b);
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(page.locator('.toast')).toContainText('Not enough room for two notes');
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('body')).not.toHaveClass(/\bsplit\b/);
  await expect(page.locator('#app #title')).toHaveText(a.title);
});

test('select all and typing stay in the pane that was clicked', async ({ page }) => {
  const [a, b] = await notes(page, 'Left stays put.', 'Right gets replaced.');
  await split(page, a, b);
  await page.locator('#beside #note-facts').click();
  await page.keyboard.press('ControlOrMeta+a');
  const picked = await page.evaluate(() => {
    const at = getSelection();
    return { text: at.toString(), inside: Boolean(at.anchorNode && document.querySelector('#beside').contains(at.anchorNode)) };
  });
  expect(picked.inside).toBe(true);
  expect(picked.text).toContain('Right gets replaced.');
  expect(picked.text).not.toContain('Left stays put.');
  await page.keyboard.type('Z');
  await expect(page.locator('#beside #doc')).not.toContainText('Right gets replaced.');
  await expect(page.locator('#app #doc')).toContainText('Left stays put.');
});

test('quitting settles the typing in both panes', async ({ page }) => {
  const [a, b] = await notes(page, 'Left.', 'Right.');
  await split(page, a, b);
  await typeIn(page, '#app', ' left before quit');
  await typeIn(page, '#beside', ' right before quit');
  await page.evaluate(() => window.leoFromApp('quit', 'now'));
  expect((await (await page.request.get(`/api/notes/${a.id}`)).json()).body).toContain('left before quit');
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toContain('right before quit');
});

test('a wide window with a 1000px left pane and margin on the right shows cards on the left and marks on the right', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const [a, b] = await notes(page, 'Left words.', 'Right words.');
  const was = (await (await page.request.get('/api/margin')).json()).layout;
  await page.request.post('/api/margin', { data: { layout: 'right' } });
  try {
    await split(page, a, b);
    const room = await page.evaluate(() => document.documentElement.clientWidth - Math.round(document.querySelector('#side').getBoundingClientRect().width));
    await page.evaluate((w) => localStorage.setItem('leo-beside-width', String(w)), room - 1000);
    await page.reload();
    await split(page, a, b);
    await expect.poll(() => page.locator('#app').evaluate((el) => Math.round(el.getBoundingClientRect().width))).toBe(1000);
    await expect(page.locator('#app .note-columns')).toHaveAttribute('data-margin', 'right');
    await expect(page.locator('#beside .note-columns')).toHaveAttribute('data-margin', 'marks');
    await expect(page.locator('#app .note-layout')).toHaveAttribute('data-wide', '');
    await expect(page.locator('#app .page-mark').first()).toBeVisible();
    await expect(page.locator('#beside .note-layout')).not.toHaveAttribute('data-wide', /.*/);
    await expect(page.locator('#beside .page-mark').first()).toBeHidden();
  } finally {
    await page.request.post('/api/margin', { data: { layout: was } });
  }
});

test('the explorer marks both open notes', async ({ page }) => {
  const [a, b, c] = await notes(page, 'One.', 'Two.', 'Three.');
  await split(page, a, b);
  await expect(page.locator(`#side .side-note[data-id="${a.id}"]`)).toHaveAttribute('aria-current', 'page');
  await expect(page.locator(`#side .side-note[data-id="${b.id}"]`)).toHaveAttribute('aria-current', 'page');
  await expect(page.locator(`#side .side-note[data-id="${c.id}"]`)).not.toHaveAttribute('aria-current', /.*/);
});

test('a right pane reloaded by Felix while focused stays the focused pane', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1000 });
  const [a, b] = await notes(page, 'Left stays put.', 'Right words.');
  await split(page, a, b);
  await page.locator('#beside #note-facts').click();
  await page.request.patch(`/api/notes/${b.id}`, { data: { body: 'Right words, changed by Felix.' } });
  await page.route('**/api/chat', async (route) => {
    const applied = { kind: 'edit', note: b.id, title: b.title, find: 'words', replace: 'words, changed by Felix', why: '', state: 'applied', before: 'Right words.', after: 'v' };
    await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { proposal: applied }, { t: 'Changed it.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' });
  });
  await page.locator('#chat-toggle').click();
  await page.locator('#chat #chat-input').fill('change the right note');
  await page.locator('#chat #chat-input').press('Enter');
  await expect(page.locator('#beside #doc')).toContainText('changed by Felix');
  await expect(page.locator('#beside')).toHaveClass(/\bpane-on\b/);
  await page.evaluate(() => document.activeElement.blur());
  await page.keyboard.press('ControlOrMeta+a');
  const inside = await page.evaluate(() => { const at = getSelection(); return Boolean(at.anchorNode && document.querySelector('#beside').contains(at.anchorNode)) && at.toString().includes('changed by Felix'); });
  expect(inside).toBe(true);
});

test('an explorer click with the right pane focused replaces the right note, saving it, and leaves the left pane and address alone', async ({ page }) => {
  const [a, b, c] = await notes(page, 'Left.', 'Right.', 'Third.');
  await split(page, a, b);
  await typeIn(page, '#beside', ' typed before switching');
  await page.locator(`#side .side-note[data-id="${c.id}"]`).click();
  await expect(page.locator('#beside #title')).toHaveText(c.title);
  await expect(page.locator('#app #title')).toHaveText(a.title);
  expect(new URL(page.url()).hash).toBe(`#/n/${a.id}`);
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toContain('typed before switching');
});

test('hovering a margin mark in the pane that is not focused lights it', async ({ page }) => {
  const [a, b] = await notes(page, 'Left words here.', 'The quote just checks if it matches the passage.');
  await page.request.post(`/api/notes/${b.id}/margin/threads`, { data: { version: b.version, quote: 'just checks if it matches', question: 'Why?' } });
  await split(page, a, b);
  await page.locator('#app #note-facts').click();
  const mark = page.locator('#beside .margin-mark').first();
  await expect(mark).toBeVisible();
  await expect(mark).not.toHaveAttribute('data-active', /.*/);
  await mark.hover();
  await expect(mark).toHaveAttribute('data-active', '');
  await expect(page.locator('#app')).toHaveClass(/\bpane-on\b/);
});

test('printing with the right pane focused prints only the right note', async ({ page }) => {
  const [a, b] = await notes(page, 'Left.', 'Right.');
  await split(page, a, b);
  await page.locator('#beside #note-facts').click();
  await page.emulateMedia({ media: 'print' });
  await expect(page.locator('#app')).toBeHidden();
  await expect(page.locator('#beside #doc')).toBeVisible();
  await page.emulateMedia({ media: 'screen' });
  await page.locator('#app #note-facts').click();
  await page.emulateMedia({ media: 'print' });
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('#app #doc')).toBeVisible();
  await page.emulateMedia({ media: 'screen' });
});

test('the right pane opens at its saved reading place, in its own scroll', async ({ page }) => {
  const body = Array.from({ length: 80 }, (_, i) => `Paragraph number ${i} keeps the note long enough to scroll.`).join('\n\n');
  const [a, b] = await notes(page, 'Left.', body);
  await page.request.put(`/api/notes/${b.id}/reading`, { data: { anchor: { line: 100, text: 'Paragraph number 50 keeps the note long e', dy: 0 } } });
  await split(page, a, b);
  await expect.poll(() => page.locator('#beside').evaluate((el) => el.scrollTop)).toBeGreaterThan(200);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  const top = await page.locator('#beside #doc .blk', { hasText: 'Paragraph number 50 ' }).evaluate((el) => el.getBoundingClientRect().top);
  expect(top).toBeLessThan(200);
});

test('a right pane whose save fails stays open and offers Close anyway', async ({ page }) => {
  const [a, b] = await notes(page, 'Left.', 'Right.');
  await split(page, a, b);
  await page.route(`**/api/notes/${b.id}`, (route) => (route.request().method() === 'PATCH' ? route.fulfill({ status: 400, json: { error: 'Refused for the test.' } }) : route.fallback()));
  await typeIn(page, '#beside', ' cannot save');
  await page.locator('#beside .pane-close').click();
  const toast = page.locator('.toast');
  await expect(toast).toContainText('Refused for the test.');
  await expect(page.locator('#beside')).toBeVisible();
  await expect(page.locator('body')).toHaveClass(/\bsplit\b/);
  await toast.getByRole('button', { name: 'Close anyway' }).click();
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('#app #title')).toHaveText(a.title);
});

test('the divider shade is a theme token with its own dark value', async ({ page }) => {
  const [a, b] = await notes(page, 'Left.', 'Right.');
  await split(page, a, b);
  await page.locator('#app #note-facts').click();
  const shade = () => page.evaluate(() => {
    const probe = document.createElement('i');
    probe.style.backgroundColor = 'var(--split-shade)';
    document.body.append(probe);
    const colour = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return { token: getComputedStyle(document.documentElement).getPropertyValue('--split-shade').trim(), colour, drawn: getComputedStyle(document.querySelector('.beside-resize'), '::before').backgroundImage };
  });
  await page.emulateMedia({ colorScheme: 'light' });
  const light = await shade();
  await page.emulateMedia({ colorScheme: 'dark' });
  const dark = await shade();
  expect(light.token).not.toBe('');
  expect(dark.token).not.toBe('');
  expect(dark.colour).not.toBe(light.colour);
  expect(light.drawn).toContain(light.colour);
  expect(dark.drawn).toContain(dark.colour);
  await page.emulateMedia({ colorScheme: 'light' });
});

test('the right pane saves its reading place, so closing and reopening it beside comes back there', async ({ page }) => {
  const body = Array.from({ length: 80 }, (_, i) => `Paragraph number ${i} keeps the note long enough to scroll.`).join('\n\n');
  const [a, b] = await notes(page, 'Left.', body);
  await split(page, a, b);
  const saved = page.waitForRequest((request) => request.method() === 'PUT' && request.url().endsWith(`/api/notes/${b.id}/reading`));
  await page.locator('#beside').hover();
  await page.mouse.wheel(0, 1500);
  await expect.poll(() => page.locator('#beside').evaluate((el) => el.scrollTop)).toBeGreaterThan(1000);
  const anchor = (await saved).postDataJSON().anchor;
  expect(anchor.line).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  await page.locator('#beside .pane-close').click();
  await expect(page.locator('#beside')).toBeHidden();
  await besideMenu(page, b);
  await expect.poll(() => page.locator('#beside').evaluate((el) => el.scrollTop)).toBeGreaterThan(800);
});

test('unfolding the explorer while two notes need the room opens it as a drawer over the page', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const [a, b, c] = await notes(page, 'Left.', 'Right.', 'Third.');
  await split(page, a, b);
  await page.locator('#chat-toggle').click();
  await expect(page.locator('body')).toHaveClass(/\bside-mini\b/);
  const widths = () => page.evaluate(() => ['#app', '#beside'].map((p) => Math.round(document.querySelector(p).getBoundingClientRect().width)));
  const before = await widths();
  const fold = page.locator('#side .side-fold');
  await fold.click();
  await expect(page.locator('body')).toHaveClass(/\bside-drawer\b/);
  await expect(page.locator(`#side .side-note[data-id="${c.id}"]`)).toBeVisible();
  await expect(page.locator('#side-filter')).toBeFocused();
  expect(await widths()).toEqual(before);
  await expect(page.locator('body')).toHaveClass(/\bsplit\b/);
  await page.keyboard.press('Escape');
  await expect(page.locator('body')).not.toHaveClass(/\bside-drawer\b/);
  await expect(page.locator('body')).toHaveClass(/\bside-mini\b/);
  await fold.click();
  await page.locator('#app #note-facts').click();
  await expect(page.locator('body')).not.toHaveClass(/\bside-drawer\b/);
  await fold.click();
  await page.locator(`#side .side-note[data-id="${c.id}"]`).click();
  await expect(page.locator('#app #title')).toHaveText(c.title);
  await expect(page.locator('body')).not.toHaveClass(/\bside-drawer\b/);
  await expect(page.locator('#beside #title')).toHaveText(b.title);
  expect(await widths()).toEqual(before);
});

test('an answer to a margin question asked in the left pane lands in the left margin after the right pane is clicked', async ({ page }) => {
  const [a, b] = await notes(page, 'Intro line\n\nA binary heap keeps the minimum at the root.\n\nLast line', 'Right words.');
  await split(page, a, b);
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  const at = '2026-11-26T10:00:00Z';
  await page.route(`**/api/notes/${a.id}/margin/threads`, async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    await held;
    await route.fulfill({ json: { id: 'asked-left', note: a.id, quote: 'keeps the minimum', prefix: 'A binary heap ', suffix: ' at the root.', status: 'open', source: 'ask', messages: [{ from: 'you', text: 'Why?', at }, { from: 'felix', text: 'Because the heap property holds.', at }], created_at: at, updated_at: at } });
  });
  await page.locator('#app #doc .blk', { hasText: 'keeps the minimum' }).click();
  const area = page.locator('#app #doc textarea.line-edit');
  await expect(area).toHaveCount(1);
  await area.evaluate((el) => {
    const from = el.value.indexOf('keeps the minimum');
    el.focus();
    el.setSelectionRange(from, from + 'keeps the minimum'.length);
    el.dispatchEvent(new Event('select'));
  });
  await page.locator('.ask-float').getByRole('button', { name: 'Ask Felix' }).click();
  const question = page.locator('#app .margin-note[data-id="draft"]').getByRole('textbox', { name: 'Ask Felix about this passage' });
  await question.fill('Why?');
  await question.press('Enter');
  await page.locator('#beside #note-facts').click();
  await expect(page.locator('#beside')).toHaveClass(/\bpane-on\b/);
  release();
  await expect(page.locator('#app .margin-note[data-id="asked-left"]')).toHaveCount(1);
  await expect(page.locator('#app .margin-note[data-id="draft"]')).toHaveCount(0);
  await expect(page.locator('#beside .margin-note[data-id="asked-left"]')).toHaveCount(0);
});

const longBody = (tag) => Array.from({ length: 80 }, (_, i) => `Paragraph ${tag} ${i} keeps the note long enough to scroll.`).join('\n\n');

test('closing the right pane right after scrolling keeps its saved reading place', async ({ page }) => {
  const [a, b] = await notes(page, 'Left.', longBody('b'));
  await page.request.put(`/api/notes/${b.id}/reading`, { data: { anchor: { line: 100, text: 'Paragraph b 50 keeps the note long enough', dy: 0 } } });
  await split(page, a, b);
  await expect.poll(() => page.locator('#beside').evaluate((el) => el.scrollTop)).toBeGreaterThan(800);
  await page.waitForTimeout(1700);
  await page.locator('#beside').hover();
  await page.mouse.wheel(0, 200);
  await page.waitForTimeout(200);
  await page.locator('#beside .pane-close').click();
  await expect(page.locator('#beside')).toBeHidden();
  await page.waitForTimeout(2600);
  const anchor = (await (await page.request.get(`/api/notes/${b.id}/reading`)).json()).anchor;
  expect(anchor.line).toBeGreaterThan(0);
  await besideMenu(page, b);
  await expect.poll(() => page.locator('#beside').evaluate((el) => el.scrollTop)).toBeGreaterThan(800);
});

test('leaving for another page while the right note is still loading never draws it', async ({ page }) => {
  const [a, b] = await notes(page, 'Left.', 'Right.');
  await open(page, a);
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  await page.route(`**/api/notes/${b.id}`, async (route) => {
    if (route.request().method() === 'GET') await held;
    await route.fallback();
  });
  await besideMenu(page, b);
  await page.evaluate(() => { location.hash = '#/calendar'; });
  await expect(page.locator('body')).toHaveClass(/on-calendar/);
  release();
  await page.waitForTimeout(500);
  await expect(page.locator('#beside')).toBeHidden();
  await expect(page.locator('#beside #title')).toHaveCount(0);
  await expect(page.locator('body')).not.toHaveClass(/\bsplit\b/);
});

test('a wiki link or a related note inside the right pane opens in the right pane', async ({ page }) => {
  const [a, b, c, d] = await notes(page, 'Left.', 'Right.', 'Third.', 'Fourth.');
  await page.request.patch(`/api/notes/${b.id}`, { data: { body: `Right, see [[${c.title}]].` } });
  await page.request.patch(`/api/notes/${c.id}`, { data: { body: 'Third, related below.' } });
  await split(page, a, b);
  await page.locator('#beside #doc a.wiki-link', { hasText: c.title }).click();
  await expect(page.locator('#beside #title')).toHaveText(c.title);
  await expect(page.locator('#app #title')).toHaveText(a.title);
  expect(new URL(page.url()).hash).toBe(`#/n/${a.id}`);
  await page.locator('#beside #related').evaluate((box, id) => {
    box.hidden = false;
    box.innerHTML = `<div class="related-list"><a class="list-row related-row" href="#/n/${encodeURIComponent(id)}"><span class="related-title">Fourth</span></a></div>`;
  }, d.id);
  await page.locator('#beside .related-row').click();
  await expect(page.locator('#beside #title')).toHaveText(d.title);
  await expect(page.locator('#app #title')).toHaveText(a.title);
  expect(new URL(page.url()).hash).toBe(`#/n/${a.id}`);
});
