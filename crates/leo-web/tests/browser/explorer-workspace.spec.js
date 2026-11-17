const { test, expect } = require('@playwright/test');
const { signIn } = require('./sign-in');

let counter = 0;
const name = () => `Explorer ${Date.now()} ${counter++}`;
const noteRow = (page, id, host = '#side') => page.locator(`${host} .side-note[data-id="${id}"]`);
const folderRow = (page, dir, host = '#side') => page.locator(`${host} .side-row[data-action="open-folder"][data-dir="${dir}"]`);
async function createNote(page, directory = '', title = `! ${name()}`) {
  const response = await page.request.post('/api/notes', { data: { title, body: 'A saved note for the explorer.', directory } });
  expect(response.ok()).toBe(true);
  return response.json();
}
async function createFolder(page, dir = name()) {
  const response = await page.request.post('/api/dirs', { data: { path: dir } });
  expect(response.ok()).toBe(true);
  return dir;
}
async function openNote(page, note) {
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#title')).toHaveText(note.title);
}
async function options(page, row, label) {
  await row.click({ button: 'right' });
  await page.getByRole('menuitem', { name: label, exact: true }).click();
}
async function folders(page) {
  await page.locator('#menu').click();
  await page.locator(':is(.sheet, .popup-menu) [data-action="folders"]').click();
  await expect(page.locator('#folders-sheet')).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

test('workspace and legacy folder links reveal folders without a library or a created file', async ({ page }) => {
  const dir = await createFolder(page);
  const nested = await createFolder(page, `${dir}/Nested`);
  const note = await createNote(page, nested);
  const before = await (await page.request.get('/api/notes?limit=1')).headers();
  await page.goto('/#/');
  await expect(page.locator('#app .workspace-empty')).toBeVisible();
  await expect(page.locator('.home-foot svg.drawn-arrow'), 'the arrow shows on the first day only').toBeHidden();
  await expect(page.locator('.home-foot'), 'with the explorer open, home has no New note footer').toBeHidden();
  await expect(page.locator('.library-tools, .library-cards, #app .cards')).toHaveCount(0);
  for (const target of [dir, nested]) {
    await page.goto(`/#/f/${encodeURIComponent(target)}`);
    await expect(page).toHaveURL(/\/#\/$/);
    await expect(folderRow(page, target)).toHaveAttribute('aria-current', 'page');
    await expect(page.locator('#app .workspace-empty')).toBeVisible();
  }
  await expect(noteRow(page, note.id)).toBeVisible();
  await noteRow(page, note.id).click();
  await expect(page.locator('#title')).toHaveText(note.title);
  await page.goBack();
  await expect(page.locator('#app .workspace-empty')).toBeVisible();
  await page.goForward();
  await expect(page.locator('#title')).toHaveText(note.title);
  const after = await (await page.request.get('/api/notes?limit=1')).headers();
  expect(after['x-total']).toBe(before['x-total']);
  await page.goto('/#/f/no-such-explorer-folder');
  await expect(page.locator('.toast')).toContainText('That folder is not there');
  await expect(page.locator('#app .workspace-empty')).toBeVisible();
});

test('a note and a folder never both look selected', async ({ page }) => {
  const dir = await createFolder(page);
  const other = await createFolder(page);
  const note = await createNote(page, dir);
  await openNote(page, note);
  await expect(noteRow(page, note.id)).toHaveClass(/\bon\b/);
  await folderRow(page, other).click();
  await expect(noteRow(page, note.id)).toHaveClass(/\bopen\b/);
  await expect(noteRow(page, note.id)).not.toHaveClass(/\bon\b/);
  await expect(page.locator('#side .side-tree .on')).toHaveCount(1);
  await expect(page.locator('#side .side-tree .side-pill')).toHaveCount(1);
  await expect(page.locator('#side .side-tree [aria-current="page"]')).toHaveCount(1);
  await noteRow(page, note.id).click();
  await expect(noteRow(page, note.id)).toHaveClass(/\bon\b/);
  await expect(page.locator('#side .side-tree .on')).toHaveCount(1);
});

test('context note rename, move and trash keep an unrelated open note and support Undo', async ({ page }) => {
  const dir = await createFolder(page);
  const destination = await createFolder(page);
  const current = await createNote(page);
  const target = await createNote(page, dir);
  await openNote(page, current);
  await folderRow(page, dir).click();
  const row = noteRow(page, target.id);
  await expect(row).toBeVisible();
  await options(page, row, 'Rename');
  const renamed = name();
  await page.getByRole('textbox', { name: 'Note name' }).fill(renamed);
  await page.getByRole('textbox', { name: 'Note name' }).press('Enter');
  await expect(row).toHaveText(renamed);
  await expect(page.locator('#title')).toHaveText(current.title);
  await options(page, row, 'Move to…');
  await page.getByRole('menuitem', { name: destination, exact: true }).click();
  await expect(page.locator('.toast')).toContainText('Moved');
  await expect(page.locator('#title')).toHaveText(current.title);
  await page.locator('.toast').getByRole('button', { name: 'Undo' }).click();
  await expect(row).toHaveText(renamed);
  await options(page, row, 'Move to trash');
  await expect(row).toHaveCount(0);
  await expect(page.locator('#title')).toHaveText(current.title);
  await page.locator('.toast').getByRole('button', { name: 'Undo' }).click();
  await expect(row).toHaveText(renamed);
});

test('folder context actions use the captured folder, preserve the editor and restore recursive trash', async ({ page }) => {
  const dir = await createFolder(page);
  const nested = await createFolder(page, `${dir}/Child`);
  const target = await createNote(page, nested);
  const current = await createNote(page);
  await openNote(page, current);
  await options(page, folderRow(page, dir), 'Rename');
  const renamed = name();
  await page.getByRole('textbox', { name: 'Folder name' }).fill(renamed);
  await page.getByRole('textbox', { name: 'Folder name' }).press('Enter');
  await expect(folderRow(page, renamed)).toBeVisible();
  await expect(page.locator('#title')).toHaveText(current.title);
  await options(page, folderRow(page, renamed), 'Move to trash');
  await expect(folderRow(page, renamed)).toHaveCount(0);
  await page.locator('.toast').getByRole('button', { name: 'Undo' }).click();
  await expect(folderRow(page, renamed)).toBeVisible();
  expect((await (await page.request.get(`/api/notes/${target.id}`)).json()).directory).toBe(`${renamed}/Child`);
  await expect(page.locator('#title')).toHaveText(current.title);
});

test('explorer keyboard menu clamps to the viewport and blank space cannot delete root', async ({ page }) => {
  const note = await createNote(page);
  await openNote(page, note);
  const row = noteRow(page, note.id);
  await row.focus();
  await page.keyboard.press('Shift+F10');
  await expect(page.getByRole('menuitem', { name: 'Open', exact: true })).toBeFocused();
  await page.keyboard.press('End');
  await expect(page.getByRole('menuitem', { name: 'Move to trash', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(row).toBeFocused();
  await page.locator('#side .side-tree').dispatchEvent('contextmenu', { clientX: 1279, clientY: 719 });
  await expect(page.getByRole('menuitem')).toHaveCount(2);
  const menu = await page.getByRole('menu', { name: 'Folder options' }).boundingBox();
  const viewport = page.viewportSize();
  expect(menu.x).toBeGreaterThanOrEqual(0);
  expect(menu.x + menu.width).toBeLessThanOrEqual(viewport.width);
  expect(menu.y + menu.height).toBeLessThanOrEqual(viewport.height);
  await page.keyboard.press('Escape');
  await page.locator('#title').click({ button: 'right' });
  await expect(page.locator('.popup-menu')).toHaveCount(0);
});

for (const width of [390]) test(`compact Folders at ${width} shares rows, traps focus and returns to More`, async ({ page }, testInfo) => {
  const dir = await createFolder(page);
  const nested = await createFolder(page, `${dir}/Child`);
  const current = await createNote(page, nested);
  await openNote(page, current);
  await page.setViewportSize({ width, height: 844 });
  await folders(page);
  const host = '#folders-sheet';
  await expect(page.locator('#side')).toBeHidden();
  await expect(noteRow(page, current.id, host)).toBeFocused();
  await expect(page.locator('#side-explorer')).toHaveCount(1);
  await page.locator(`${host} [data-action="close"]`).focus();
  await page.keyboard.press('Shift+Tab');
  expect(await page.locator(host).evaluate(el => el.contains(document.activeElement))).toBe(true);
  const old = await noteRow(page, current.id, host).elementHandle();
  await folderRow(page, nested, host).click();
  await folderRow(page, nested, host).click();
  await expect(noteRow(page, current.id, host)).toBeVisible();
  await page.locator(host).evaluate(el => Promise.allSettled(el.getAnimations({ subtree: true }).map(run => run.finished)));
  await page.screenshot({ path: testInfo.outputPath(`folders-${width}.png`) });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await noteRow(page, current.id, host).click();
  await expect(page.locator(host)).toHaveCount(0);
  await expect(page.locator('#title')).toHaveText(current.title);
  await folders(page);
  await page.keyboard.press('Escape');
  await expect(page.locator('#menu')).toBeFocused();
  await folders(page);
  await page.locator('.scrim').click({ position: { x: 3, y: 3 } });
  await expect(page.locator(host)).toHaveCount(0);
  await expect(page.locator('#menu')).toBeFocused();
  if (width === 390) {
    await folders(page);
    await page.locator('.sheet').evaluate(panel => {
      const y = panel.getBoundingClientRect().top + 20;
      for (const [type, touches] of [['touchstart', [{ clientY: y }]], ['touchmove', [{ clientY: y + 160 }]], ['touchend', []]]) {
        const event = new Event(type, { bubbles: true, cancelable: true });
        Object.defineProperty(event, 'touches', { value: touches });
        panel.dispatchEvent(event);
      }
    });
    await expect(page.locator(host)).toHaveCount(0);
    await expect(page.locator('#menu')).toBeFocused();
  }
  await folders(page);
  await page.locator(`${host} [data-action="new-folder"]`).click();
  await page.locator('#folder-name').fill('Unsafe/Name');
  await page.locator('#folder-name').press('Enter');
  await expect(page.locator('.toast')).toContainText('cannot contain /');
  await expect(page.locator('#folder-name')).toHaveValue('Unsafe/Name');
  await page.locator('#folder-name').press('Escape');
  await expect(page.locator(host)).toBeVisible();
  await page.setViewportSize({ width: 1100, height: 844 });
  await expect(page.locator(host)).toHaveCount(0);
  await expect(page.locator('#side .side-fold')).toBeFocused();
  expect(await old.evaluate(el => el.isConnected)).toBe(false);
  await expect(page.locator('#side .side-explorer')).toBeVisible();
});

test('compact failed save and failed open keep Folders and the current draft', async ({ page }) => {
  const current = await createNote(page);
  const target = await createNote(page);
  await openNote(page, current);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator('#title').fill('My unsaved title');
  await page.route(`**/api/notes/${current.id}`, route => route.request().method() === 'PATCH' ? route.fulfill({ status: 500, contentType: 'application/json', body: JSON.stringify({ error: 'Save is unavailable. Try again.' }) }) : route.continue());
  await folders(page);
  await noteRow(page, target.id, '#folders-sheet').click();
  await expect(page.locator('#folders-sheet')).toBeVisible();
  await expect(page.locator('#title')).toHaveText('My unsaved title');
  await expect(page.locator('.toast')).toContainText('Save is unavailable');
  await page.unroute(`**/api/notes/${current.id}`);
  await page.route(`**/api/notes/${target.id}`, route => route.fulfill({ status: 404, contentType: 'application/json', body: JSON.stringify({ error: 'That note is gone. Choose another note.' }) }));
  await noteRow(page, target.id, '#folders-sheet').click();
  await expect(page.locator('#folders-sheet')).toBeVisible();
  await expect(page.locator('.toast')).toContainText('That note is gone');
});

test('selected folder creation wins over the open note folder', async ({ page }) => {
  const dir = await createFolder(page);
  const current = await createNote(page);
  await openNote(page, current);
  await folderRow(page, dir).click();
  await page.locator('#side .side-new').click();
  await expect(page.locator('#title')).toHaveText('');
  await page.locator('#title').fill(name());
  await page.keyboard.press('ControlOrMeta+s');
  await expect(page).toHaveURL(/#\/n\//);
  const id = new URL(page.url()).hash.split('/').pop();
  expect((await (await page.request.get(`/api/notes/${id}`)).json()).directory).toBe(dir);
});

test('startup restores one remembered note or legacy folder and missing notes return to the workspace', async ({ page }) => {
  const dir = await createFolder(page);
  const note = await createNote(page, dir);
  await page.addInitScript(id => {
    if (sessionStorage.getItem('explorer-startup-seeded')) return;
    localStorage.setItem('leo-last-place', `#/n/${id}`);
    sessionStorage.removeItem('leo-launched');
    sessionStorage.setItem('explorer-startup-seeded', '1');
  }, note.id);
  await page.goto('/#/');
  await page.reload();
  await expect(page.locator('#title')).toHaveText(note.title);
  await page.goto('/#/');
  await expect(page.locator('#app .workspace-empty')).toBeVisible();
  await page.addInitScript(dir => {
    if (sessionStorage.getItem('explorer-folder-seeded')) return;
    localStorage.setItem('leo-last-place', `#/f/${encodeURIComponent(dir)}`);
    sessionStorage.removeItem('leo-launched');
    sessionStorage.setItem('explorer-folder-seeded', '1');
  }, dir);
  await page.reload();
  await expect(folderRow(page, dir)).toHaveAttribute('aria-current', 'page');
  await expect(page).toHaveURL(/\/#\/$/);
  await page.goto('/#/n/no-such-explorer-note');
  await expect(page.locator('#app .workspace-empty')).toBeVisible();
  await expect(page.locator('.toast')).toContainText('not there any more');
});

test('folder move and Undo retain the current note, and trashing its ancestor saves the latest draft', async ({ page }) => {
  const dir = await createFolder(page);
  const destination = await createFolder(page);
  const note = await createNote(page, dir);
  await openNote(page, note);
  await options(page, folderRow(page, dir), 'Move to…');
  await page.getByRole('menuitem', { name: destination, exact: true }).click();
  await expect(page.locator('#title')).toHaveText(note.title);
  await expect(page.locator('.toast')).toContainText('Moved');
  await page.locator('.toast').getByRole('button', { name: 'Undo' }).click();
  await expect(folderRow(page, dir)).toBeVisible();
  await page.locator('#title').fill('The latest unsaved title');
  await options(page, folderRow(page, dir), 'Move to trash');
  await expect(page.locator('#app .workspace-empty')).toBeVisible();
  await page.locator('.toast').getByRole('button', { name: 'Undo' }).click();
  await expect(folderRow(page, dir)).toBeVisible();
  expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).title).toBe('The latest unsaved title');
});

test('captured folder creation overrides another selection and note rename retains useful errors', async ({ page }) => {
  const selected = await createFolder(page);
  const target = await createFolder(page);
  const current = await createNote(page);
  const note = await createNote(page, target);
  await openNote(page, current);
  await folderRow(page, selected).click();
  await options(page, folderRow(page, target), 'New folder here');
  await page.locator('#folder-name').fill('Child');
  await page.locator('#folder-name').press('Enter');
  await expect(folderRow(page, `${target}/Child`)).toBeVisible();
  await expect(page.locator('#title')).toHaveText(current.title);
  await options(page, folderRow(page, target), 'New folder here');
  await page.locator('#folder-name').fill('Child');
  await page.locator('#folder-name').press('Enter');
  await expect(page.locator('.toast')).toContainText('already exists');
  await expect(page.locator('#folder-name')).toHaveValue('Child');
  await page.locator('#folder-name').press('Escape');
  await options(page, noteRow(page, note.id), 'Rename');
  const input = page.getByRole('textbox', { name: 'Note name' });
  await input.fill(name());
  await page.route(`**/api/notes/${note.id}`, route => route.fulfill({ status: 409, contentType: 'application/json', body: JSON.stringify({ error: 'That note changed elsewhere. Open it again before renaming.' }) }));
  await page.getByRole('textbox', { name: 'Note name' }).press('Enter');
  await expect(page.locator('.toast')).toContainText('changed elsewhere');
  await expect(input).toBeVisible();
  await input.press('Escape');
  await page.unroute(`**/api/notes/${note.id}`);
  await page.request.delete(`/api/notes/${note.id}`);
  await options(page, noteRow(page, note.id), 'Rename');
  await expect(page.locator('.toast')).toContainText(/not found|not there|no longer|gone/i);
  await expect(page.locator('#title')).toHaveText(current.title);
});

test('compact paging survives repeated Folders sheets and late reads cannot reopen a closed sheet', async ({ page }) => {
  test.setTimeout(60000);
  const dir = await createFolder(page);
  for (let start = 0; start < 105; start += 8) await Promise.all(Array.from({ length: Math.min(8, 105 - start) }, (_, i) => createNote(page, dir, `Page ${String(start + i).padStart(3, '0')}`)));
  await page.goto('/#/');
  await page.setViewportSize({ width: 390, height: 844 });
  await folders(page);
  const host = '#folders-sheet';
  await folderRow(page, dir, host).click();
  const kids = page.locator(`${host} .side-kids[data-kids="${dir}"]`);
  await expect(kids.locator('.side-note')).toHaveCount(100);
  await kids.getByRole('button', { name: 'Next notes' }).click();
  await expect(kids.locator('.side-note')).toHaveCount(5);
  await page.locator(`${host} [data-action="close"]`).click();
  await folders(page);
  await expect(kids.locator('.side-note')).toHaveCount(5);
  await kids.getByRole('button', { name: 'Previous notes' }).click();
  await expect(kids.locator('.side-note')).toHaveCount(100);
  await page.locator(`${host} [data-action="close"]`).click();
  let release;
  await page.route('**/api/notes?*', async route => {
    if (new URL(route.request().url()).searchParams.get('dir') === dir) await new Promise(resolve => { release = resolve; });
    await route.continue();
  });
  await folders(page);
  await expect.poll(() => Boolean(release)).toBe(true);
  await page.keyboard.press('Escape');
  release();
  await expect(page.locator(host)).toHaveCount(0);
  await expect(page.locator('#menu')).toBeFocused();
});

for (const route of ['settings/general', 'calendar', 'boards', 'record', 'search/explorer']) test(`compact note opens leave ${route.split('/')[0]} cleanly`, async ({ page }) => {
  const note = await createNote(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/#/${route}`);
  await expect(page.locator('#app')).not.toBeEmpty();
  await folders(page);
  await noteRow(page, note.id, '#folders-sheet').click();
  await expect(page.locator('#folders-sheet')).toHaveCount(0);
  await expect(page.locator('#title')).toHaveText(note.title);
  await expect(page).toHaveURL(new RegExp(`#/n/${note.id}$`));
  await expect(page.locator('body')).not.toHaveClass(/on-settings|on-calendar|on-boards|kanban-panel-on/);
  await expect(page.locator('#doc textarea')).toBeFocused();
  await page.keyboard.type(' Still editing the note.');
  await page.keyboard.press('ControlOrMeta+s');
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toContain('Still editing the note.');
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`#/${route}$`));
  await expect(page.locator('#title')).toHaveCount(0);
});

for (const order of ['first-first', 'latest-first']) test(`compact latest note choice wins when responses finish ${order}`, async ({ page }) => {
  const first = await createNote(page);
  const latest = await createNote(page);
  const release = {};
  for (const [key, note] of [['first', first], ['latest', latest]]) await page.route(`**/api/notes/${note.id}`, async route => {
    await new Promise(resolve => { release[key] = resolve; });
    await route.continue();
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await folders(page);
  await noteRow(page, first.id, '#folders-sheet').click();
  await expect.poll(() => Boolean(release.first)).toBe(true);
  await noteRow(page, latest.id, '#folders-sheet').click();
  await expect.poll(() => Boolean(release.latest)).toBe(true);
  const firstResponse = page.waitForResponse(response => response.url().endsWith(`/api/notes/${first.id}`));
  if (order === 'first-first') {
    release.first();
    await (await firstResponse).finished();
    await expect(page.locator('#folders-sheet')).toBeVisible();
    release.latest();
  } else {
    release.latest();
    await expect(page.locator('#title')).toHaveText(latest.title);
    release.first();
    await (await firstResponse).finished();
  }
  await expect(page.locator('#folders-sheet')).toHaveCount(0);
  await expect(page.locator('#title')).toHaveText(latest.title);
  await expect(page).toHaveURL(new RegExp(`#/n/${latest.id}$`));
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expect(page.locator('#title')).toHaveText(latest.title);
});

for (const failure of ['server', 'offline']) test(`legacy folder ${failure} error stays actionable and retries its route`, async ({ page }) => {
  const dir = await createFolder(page);
  await page.route('**/api/folders', route => failure === 'offline' ? route.abort('failed') : route.fulfill({ status: 500, contentType: 'application/json', body: JSON.stringify({ error: 'Folders are unavailable. Try again.' }) }));
  const hash = `#/f/${encodeURIComponent(dir)}`;
  await page.goto(`/${hash}`);
  if (failure === 'offline') await expect(page.getByText("Can't reach leo")).toBeVisible();
  else await expect(page.locator('.toast')).toContainText('Folders are unavailable. Try again.');
  expect(new URL(page.url()).hash).toBe(hash);
  await page.unroute('**/api/folders');
  await page.reload();
  await expect(page.locator('#app .workspace-empty')).toBeVisible();
  await expect(folderRow(page, dir)).toHaveAttribute('aria-current', 'page');
  await expect(page).toHaveURL(/\/#\/$/);
});

test('the explorer sorts notes by name or edit date, reverses the order from the sort menu, shows only open tasks, and remembers the choice', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const dir = await createFolder(page);
  const alpha = await createNote(page, dir, 'Alpha');
  await createNote(page, dir, 'Bravo');
  const charlie = await createNote(page, dir, 'Charlie');
  await new Promise((resolve) => setTimeout(resolve, 1100));
  expect((await page.request.patch(`/api/notes/${alpha.id}`, { data: { body: '- [ ] Still to do' } })).ok()).toBe(true);
  await openNote(page, charlie);
  const rows = page.locator(`#side .side-note[data-from="${dir}"] .side-label:not(.side-count)`);
  await expect(rows).toHaveText(['Alpha', 'Bravo', 'Charlie']);
  const sort = page.getByRole('button', { name: 'Sort notes' });
  const menu = page.getByRole('menu', { name: 'Sort notes' });
  const pick = async (name) => { await sort.click(); await menu.getByRole('menuitemradio', { name, exact: true }).click(); };
  await expect(page.getByRole('button', { name: /^Reverse order/ })).toHaveCount(0);
  await expect(sort).not.toHaveClass(/\bon\b/);
  await expect(sort).toHaveAttribute('title', 'Sorted by name, a to z');
  await pick('Z to A');
  await expect(rows).toHaveText(['Charlie', 'Bravo', 'Alpha']);
  await expect(sort).toHaveClass(/\bon\b/);
  await expect(sort).toHaveAttribute('title', 'Sorted by name, z to a');
  await sort.click();
  await expect(menu.getByRole('menuitemradio')).toHaveText(['Name', 'Last edited', 'Created', 'A to Z', 'Z to A']);
  await expect(menu.getByRole('menuitemradio', { name: 'Name' })).toHaveAttribute('aria-checked', 'true');
  await expect(menu.getByRole('menuitemradio', { name: 'Z to A' })).toHaveAttribute('aria-checked', 'true');
  await menu.getByRole('menuitemradio', { name: 'Last edited' }).click();
  await expect(rows).toHaveText(['Alpha', 'Charlie', 'Bravo']);
  await expect(sort).toHaveAttribute('title', 'Sorted by last edited, newest first');
  await expect(page.locator(`#side .side-note[data-id="${alpha.id}"] .side-count`)).toHaveText('just now');
  await expect(sort).toHaveClass(/\bon\b/);
  await pick('Oldest first');
  await expect(rows).toHaveText(['Bravo', 'Charlie', 'Alpha']);
  await sort.click();
  await menu.getByRole('menuitemcheckbox', { name: 'Only notes with open tasks' }).click();
  await expect(rows).toHaveText(['Alpha']);
  await page.reload();
  await expect(rows).toHaveText(['Alpha']);
  await expect(sort).toHaveAttribute('title', 'Sorted by last edited, oldest first, open tasks only');
  await sort.click();
  await expect(menu.getByRole('menuitemcheckbox', { name: 'Only notes with open tasks' })).toHaveAttribute('aria-checked', 'true');
  await menu.getByRole('menuitemcheckbox', { name: 'Only notes with open tasks' }).click();
  await pick('Created');
  await expect(rows).toHaveText(['Charlie', 'Bravo', 'Alpha']);
  await pick('Oldest first');
  await expect(rows).toHaveText(['Alpha', 'Bravo', 'Charlie']);
  await pick('Name');
  await expect(rows).toHaveText(['Alpha', 'Bravo', 'Charlie']);
  await expect(page.locator(`#side .side-note[data-from="${dir}"] .side-count`)).toHaveCount(0);
  await expect(sort).not.toHaveClass(/\bon\b/);
});
