const { test, expect } = require('@playwright/test');
const { signIn } = require('./sign-in');

let counter = 0;
const name = () => `Review ${Date.now()} ${counter++}`;
const noteRow = (page, id, host = '#side') => page.locator(`${host} .side-note[data-id="${id}"]`);
const folderRow = (page, dir, host = '#side') => page.locator(`${host} .side-row[data-action="open-folder"][data-dir="${dir}"]`);
const twist = (page, dir) => page.locator(`#side [data-action="side-twist"][data-dir="${dir}"]`);
const newHash = (dir) => (dir ? `#/new/${encodeURIComponent(dir)}` : '#/new');
async function createNote(page, directory = '', title = `! ${name()}`) {
  const response = await page.request.post('/api/notes', { data: { title, body: 'A saved note for the review.', directory } });
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
async function moveChoices(page, row) {
  await options(page, row, 'Move to…');
  await expect(page.getByRole('menu', { name: /^Move/ })).toBeVisible();
  return (await page.getByRole('menuitem').allTextContents()).map(text => (text === 'Top level' ? '' : text));
}
async function menuFor(page, locator) {
  await locator.dispatchEvent('contextmenu', { clientX: 200, clientY: 300 });
  await expect(page.locator('.popup-menu')).toBeVisible();
}
const items = (page) => page.getByRole('menuitem').evaluateAll((els) => els.map((el) => [...el.childNodes].filter((n) => !(n.nodeType === 1 && n.getAttribute('aria-hidden') === 'true')).map((n) => n.textContent).join('')));
async function folders(page) {
  await page.locator('#menu').click();
  await page.locator(':is(.sheet, .popup-menu) [data-action="folders"]').click();
  await expect(page.locator('#folders-sheet')).toBeVisible();
}
const noteDir = async (page, id) => (await (await page.request.get(`/api/notes/${id}`)).json()).directory;

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

test('a passive folder reveal keeps the explorer hidden and opens no sheet', async ({ page }) => {
  const dir = await createFolder(page);
  await page.evaluate(() => localStorage.setItem('leo-side', 'mini'));
  await page.goto(`/#/f/${encodeURIComponent(dir)}`);
  await page.reload();
  await expect(page.getByText('Open a note from Folders, or start a new one.')).toBeVisible();
  await expect(page.locator('body')).toHaveClass(/side-mini/);
  await expect(page.locator('.home-foot svg.drawn-arrow')).toBeHidden();
  await page.evaluate(() => localStorage.setItem('leo-side', 'wide'));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/#/f/${encodeURIComponent(dir)}`);
  await expect(page.getByText('Open a note from Folders, or start a new one.')).toBeVisible();
  await expect(page.locator('#folders-sheet')).toHaveCount(0);
  await expect(page.locator('.scrim')).toHaveCount(0);
});

for (const home of ['root', 'folder']) test(`folder then note: opening a note in the ${home} replaces the folder scope for New, Record and Upload`, async ({ page }) => {
  const picked = await createFolder(page);
  const other = home === 'folder' ? await createFolder(page) : '';
  const note = await createNote(page, other, `! Scope ${name()}`);
  await page.goto('/#/');
  if (other) await twist(page, other).click();
  await folderRow(page, picked).click();
  await expect(page.locator('#side .side-new')).toHaveAttribute('data-dir', picked);
  await expect(page.locator('#side .side-new')).toHaveAttribute('title', /\((⌘N|Ctrl\+N)\)$/);
  await noteRow(page, note.id).click();
  await expect(page.locator('#title')).toHaveText(note.title);
  await expect(page.locator('#side [data-action="record"]')).toHaveAttribute('data-dir', other);
  await expect(page.locator('#side [data-action="upload"]')).toHaveAttribute('data-dir', other);
  await page.locator('#side .side-new').click();
  await expect.poll(() => new URL(page.url()).hash).toBe(newHash(other));
});

for (const home of ['root', 'folder']) test(`note then folder: a folder click after opening a ${home} note wins for New, Record and Upload`, async ({ page }) => {
  const picked = await createFolder(page);
  const other = home === 'folder' ? await createFolder(page) : '';
  const note = await createNote(page, other);
  await openNote(page, note);
  await folderRow(page, picked).click();
  await expect(page.locator('#title')).toHaveText(note.title);
  await expect(page.locator('#side [data-action="record"]')).toHaveAttribute('data-dir', picked);
  await expect(page.locator('#side [data-action="upload"]')).toHaveAttribute('data-dir', picked);
  await page.locator('#side .side-new').click();
  await expect.poll(() => new URL(page.url()).hash).toBe(newHash(picked));
});

test('context menu regions: blank tree is root, heading follows the scope, status and kids use their folder', async ({ page }) => {
  const dir = await createFolder(page);
  const empty = await createFolder(page);
  const note = await createNote(page, dir);
  const current = await createNote(page);
  await openNote(page, current);
  await menuFor(page, page.locator('#side .side-tree'));
  expect(await items(page)).toEqual(['New note', 'New folder']);
  await page.keyboard.press('Escape');
  await menuFor(page, page.locator('#side .side-group'));
  expect(await items(page)).toEqual(['New note', 'New folder']);
  await page.keyboard.press('Escape');
  await folderRow(page, dir).click();
  await expect(noteRow(page, note.id)).toBeVisible();
  await menuFor(page, page.locator(`#side .side-group`));
  expect(await items(page)).toEqual(['New note here', 'New folder here']);
  await page.keyboard.press('Escape');
  await menuFor(page, page.locator(`#side .side-kids[data-kids="${dir}"]`));
  expect(await items(page)).toEqual(['New note here', 'New folder here']);
  await page.getByRole('menuitem', { name: 'New note here', exact: true }).click();
  await expect.poll(() => new URL(page.url()).hash).toBe(newHash(dir));
  await openNote(page, current);
  await folderRow(page, empty).click();
  const status = page.locator(`#side .side-note-status[data-drop-dir="${empty}"]`);
  await expect(status).toContainText('Empty folder');
  await menuFor(page, status);
  expect(await items(page)).toEqual(['New note here', 'New folder here']);
  await page.getByRole('menuitem', { name: 'New folder here', exact: true }).click();
  await page.locator('#folder-name').fill('Inner');
  await page.locator('#folder-name').press('Enter');
  await expect(folderRow(page, `${empty}/Inner`)).toBeVisible();
});

test('opening a context menu never selects a folder or changes the open note', async ({ page }) => {
  const dir = await createFolder(page);
  const current = await createNote(page);
  await openNote(page, current);
  const before = page.url();
  await folderRow(page, dir).click({ button: 'right' });
  await expect(page.locator('.popup-menu')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(folderRow(page, dir)).not.toHaveAttribute('aria-current', 'page');
  await expect(page.locator('#side .side-new')).toHaveAttribute('data-dir', '');
  await expect(page.locator('#title')).toHaveText(current.title);
  expect(page.url()).toBe(before);
});

test('note rename, move and trash sheets return focus to the logical row on desktop', async ({ page }) => {
  const dir = await createFolder(page);
  const destination = await createFolder(page);
  const note = await createNote(page, dir);
  await page.goto('/#/');
  await folderRow(page, dir).click();
  const row = noteRow(page, note.id);
  await options(page, row, 'Rename');
  await page.getByRole('textbox', { name: 'Note name' }).press('Escape');
  await expect(row).toBeFocused();
  await options(page, row, 'Rename');
  await page.getByRole('textbox', { name: 'Note name' }).fill(name());
  await page.getByRole('textbox', { name: 'Note name' }).press('Enter');
  await expect(row).toBeFocused();
  await moveChoices(page, row);
  await page.keyboard.press('Escape');
  await expect(row).toBeFocused();
  await moveChoices(page, row);
  await page.getByRole('menuitem', { name: destination, exact: true }).click();
  await expect(page.locator('.toast')).toContainText('Moved');
  await expect(row).toHaveCount(0);
  await expect(folderRow(page, destination)).toBeFocused();
  await folderRow(page, destination).click();
  await options(page, noteRow(page, note.id), 'Move to trash');
  await expect(noteRow(page, note.id)).toHaveCount(0);
  await expect(folderRow(page, destination)).toBeFocused();
});

test('folder rename, move and trash sheets return focus to the logical row on desktop', async ({ page }) => {
  const dir = await createFolder(page);
  const destination = await createFolder(page);
  await page.goto('/#/');
  await options(page, folderRow(page, dir), 'Rename');
  await page.getByRole('textbox', { name: 'Folder name' }).press('Escape');
  await expect(folderRow(page, dir)).toBeFocused();
  await options(page, folderRow(page, dir), 'Rename');
  const renamed = name();
  await page.getByRole('textbox', { name: 'Folder name' }).fill(renamed);
  await page.getByRole('textbox', { name: 'Folder name' }).press('Enter');
  await expect(folderRow(page, renamed)).toBeFocused();
  await moveChoices(page, folderRow(page, renamed));
  await page.getByRole('menuitem', { name: destination, exact: true }).click();
  await expect(page.locator('.toast')).toContainText('Moved');
  await expect(folderRow(page, destination)).toBeFocused();
  await folderRow(page, destination).click();
  const inside = `${destination}/${renamed}`;
  await expect(folderRow(page, inside)).toBeVisible();
  await options(page, folderRow(page, inside), 'Move to trash');
  await expect(folderRow(page, inside)).toHaveCount(0);
  await expect(folderRow(page, destination)).toBeFocused();
});

test('compact nested sheets return to Folders with focus on the logical row', async ({ page }) => {
  const dir = await createFolder(page);
  const note = await createNote(page, dir);
  await page.setViewportSize({ width: 390, height: 844 });
  await folders(page);
  const host = '#folders-sheet';
  await folderRow(page, dir, host).click();
  await expect(noteRow(page, note.id, host)).toBeVisible();
  await options(page, noteRow(page, note.id, host), 'Rename');
  await page.getByRole('textbox', { name: 'Note name' }).press('Escape');
  await expect(page.locator(host)).toBeVisible();
  await expect(noteRow(page, note.id, host)).toBeFocused();
  await options(page, noteRow(page, note.id, host), 'Rename');
  await page.getByRole('textbox', { name: 'Note name' }).fill(name());
  await page.getByRole('textbox', { name: 'Note name' }).press('Enter');
  await expect(page.locator(host)).toBeVisible();
  await expect(noteRow(page, note.id, host)).toBeFocused();
  await options(page, folderRow(page, dir, host), 'Rename');
  const renamed = name();
  await page.getByRole('textbox', { name: 'Folder name' }).fill(renamed);
  await page.getByRole('textbox', { name: 'Folder name' }).press('Enter');
  await expect(page.locator(host)).toBeVisible();
  await expect(folderRow(page, renamed, host)).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#menu')).toBeFocused();
});

test('move choices never offer the folder a note or folder is already in', async ({ page }) => {
  const parent = await createFolder(page);
  const child = await createFolder(page, `${parent}/Child`);
  const sibling = await createFolder(page);
  const inChild = await createNote(page, child);
  const atRoot = await createNote(page);
  await page.goto('/#/');
  await twist(page, parent).click();
  await folderRow(page, child).click();
  const noteChoices = await moveChoices(page, noteRow(page, inChild.id));
  expect(noteChoices).not.toContain(child);
  expect(noteChoices).toEqual(expect.arrayContaining(['', parent, sibling]));
  await page.keyboard.press('Escape');
  const rootChoices = await moveChoices(page, noteRow(page, atRoot.id));
  expect(rootChoices).not.toContain('');
  expect(rootChoices).toEqual(expect.arrayContaining([parent, child, sibling]));
  await page.keyboard.press('Escape');
  const nested = await moveChoices(page, folderRow(page, child));
  expect(nested).not.toContain(parent);
  expect(nested).not.toContain(child);
  expect(nested).toEqual(expect.arrayContaining(['', sibling]));
  await page.keyboard.press('Escape');
  const top = await moveChoices(page, folderRow(page, sibling));
  expect(top).not.toContain('');
  expect(top).not.toContain(sibling);
  expect(top).toEqual(expect.arrayContaining([parent, child]));
  expect(await noteDir(page, inChild.id)).toBe(child);
});

test('note titles keep slashes as text while folder names stay strict', async ({ page }) => {
  const dir = await createFolder(page);
  const note = await createNote(page, dir);
  const current = await createNote(page);
  await openNote(page, current);
  await folderRow(page, dir).click();
  const before = (await (await page.request.get('/api/folders')).json()).map(folder => folder.name).sort();
  const title = 'Q3/Q4 plan \\ draft';
  await options(page, noteRow(page, note.id), 'Rename');
  await page.getByRole('textbox', { name: 'Note name' }).fill(title);
  await page.getByRole('textbox', { name: 'Note name' }).press('Enter');
  await expect(page.locator('.toast')).toContainText('Renamed note');
  await expect(noteRow(page, note.id)).toHaveText(title);
  const saved = await (await page.request.get(`/api/notes/${note.id}`)).json();
  expect(saved.title).toBe(title);
  expect(saved.directory).toBe(dir);
  expect((await (await page.request.get('/api/folders')).json()).map(folder => folder.name).sort()).toEqual(before);
  await expect(page.locator('#title')).toHaveText(current.title);
  await options(page, folderRow(page, dir), 'Rename');
  const input = page.getByRole('textbox', { name: 'Folder name' });
  await input.fill('a/b');
  await page.getByRole('textbox', { name: 'Folder name' }).press('Enter');
  await expect(page.locator('.toast')).toContainText('cannot contain /');
  await expect(input).toHaveValue('a/b');
});

test('compact More new folder shows the new folder revealed in Folders', async ({ page }) => {
  const dir = await createFolder(page);
  await page.goto('/#/');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator('#menu').click();
  await page.locator(':is(.sheet, .popup-menu) [data-action="new-folder"]').click();
  const made = name();
  await page.locator('#folder-name').fill(made);
  await page.locator('#folder-name').press('Enter');
  await expect(page.locator('#folders-sheet')).toBeVisible();
  await expect(folderRow(page, made, '#folders-sheet')).toBeVisible();
  await expect(folderRow(page, made, '#folders-sheet')).toHaveAttribute('aria-current', 'page');
  await expect(folderRow(page, dir, '#folders-sheet')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#menu')).toBeFocused();
});

test('the rail has no Felix or Collapse all control and loaded folders never flash Opening notes', async ({ page }) => {
  const dir = await createFolder(page);
  const empty = await createFolder(page, `${dir}/Empty`);
  await createNote(page, dir);
  await page.goto('/#/');
  await expect(page.locator('#side').getByRole('button', { name: 'Ask Felix' })).toHaveCount(0);
  await expect(page.locator('#chat-toggle')).toBeVisible();
  await expect(page.locator('[data-action="side-collapse"]')).toHaveCount(0);
  await folderRow(page, dir).click();
  await folderRow(page, empty).click();
  const status = page.locator(`#side .side-note-status[data-drop-dir="${empty}"]`);
  await expect(status).toContainText('Empty folder');
  await page.evaluate(() => {
    window.flashes = [];
    new MutationObserver(records => {
      for (const record of records) {
        if (record.type === 'characterData' && (record.oldValue || '').includes('Opening notes')) window.flashes.push('text');
        for (const node of [...record.addedNodes, ...record.removedNodes]) if ((node.textContent || '').includes('Opening notes')) window.flashes.push(node.nodeName);
        if (record.type === 'attributes' && record.attributeName === 'aria-busy') window.flashes.push('busy');
      }
    }).observe(document.body, { childList: true, subtree: true, characterData: true, characterDataOldValue: true, attributes: true, attributeFilter: ['aria-busy'] });
  });
  for (const section of ['general', 'felix', 'keeping', 'general']) {
    await page.locator('#side [data-action="settings"]').click();
    await page.goto(`/#/settings/${section}`);
    await expect(page.locator('#app')).not.toBeEmpty();
    await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 120)));
    await expect(status).toContainText('Empty folder');
    await folderRow(page, dir).click();
    await folderRow(page, dir).click();
    await expect(status).toContainText('Empty folder');
  }
  await page.goto('/#/');
  await expect(status).toContainText('Empty folder');
  expect(await page.evaluate(() => window.flashes)).toEqual([]);
});

test('explorer keys move, open and filter', async ({ page }) => {
  const dir = await createFolder(page);
  await createNote(page, dir, 'Graph');
  await createNote(page, dir, 'Heap');
  await page.goto('/#/');
  const folder = folderRow(page, dir);
  await expect(folder).toBeVisible();
  await folder.focus();
  await page.keyboard.press('ArrowRight');
  await expect(folder).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#side .side-note[data-title="Heap"]')).toBeVisible();
  await folder.focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('#side .side-note[data-title="Graph"]')).toBeFocused();
  await folder.focus();
  await page.keyboard.press('ArrowLeft');
  await expect(folder).toHaveAttribute('aria-expanded', 'false');
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('#side .side-note[data-title="Heap"]')).toBeVisible();
  await folder.focus();
  await page.keyboard.press('h');
  await expect(page.locator('#side .side-note[data-title="Heap"]')).toBeFocused();
  for (const sub of await page.locator('#side .side-sub').all()) await expect(sub).toHaveAttribute('tabindex', '-1');
  await expect(page.locator('#side .side-tree .side-row[tabindex="0"]')).toHaveCount(1);
  await page.locator('#side-filter').fill('GRA');
  await expect(page.locator('#side .side-note-wrap:has(.side-note[data-title="Heap"])')).toBeHidden();
  await expect(page.locator('#side .side-note-wrap:has(.side-note[data-title="Graph"])')).toBeVisible();
});
