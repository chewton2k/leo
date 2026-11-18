const { test, expect } = require('@playwright/test');
const { signIn } = require('./sign-in');

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

const paragraph = 'A long paragraph of ordinary words that fills the note. '.repeat(18).trim();
const bodyOf = (bytes) => Array.from({ length: Math.ceil(bytes / (paragraph.length + 2)) }, (_, i) => `${i} ${paragraph}`).join('\n\n');

async function bigNote(page, bytes) {
  return (await page.request.post('/api/notes', { data: { title: 'A large draft', body: bodyOf(bytes) } })).json();
}

async function typeAtTop(page, words) {
  await page.locator('#doc .blk').first().click();
  const area = page.locator('#doc textarea.line-edit');
  await area.press('Home');
  await area.pressSequentially(words);
}

const hide = (page, hidden) => page.evaluate((h) => { window.__LEO_VISIBLE__ = !h; window.dispatchEvent(new Event('leo-visibility')); }, hidden);

test('a large draft survives hide, show, edit, quit and reload', async ({ page }) => {
  const item = await bigNote(page, 1000000);
  await page.route('**/api/notes/*', (route) => (route.request().method() === 'PATCH' ? route.abort() : route.continue()));
  await page.goto('/#/n/' + item.id);
  await expect(page.locator('#title')).toHaveText('A large draft');
  await typeAtTop(page, 'first words ');
  await hide(page, true);
  await hide(page, false);
  await typeAtTop(page, 'later words ');
  await page.evaluate(() => window.leoFromApp('quit', ''));
  await page.waitForFunction(() => new Promise((resolve) => {
    const open = indexedDB.open('leo-drafts', 1);
    open.onsuccess = () => {
      const read = open.result.transaction('drafts').objectStore('drafts').getAll();
      read.onsuccess = () => resolve(read.result.some((d) => d.edit.body.includes('later words')));
      read.onerror = () => resolve(false);
    };
    open.onerror = () => resolve(false);
  }));
  expect(await page.evaluate(() => Object.keys(localStorage).filter((key) => key.startsWith('leo-draft-v1:')))).toEqual([]);
  await page.reload();
  await expect(page.locator('#doc')).toContainText('later words');
  await expect(page.locator('#doc')).toContainText('first words');

  await page.locator('#doc .blk').first().click();
  const pasted = 'pasted '.repeat(70000);
  await page.evaluate((text) => {
    const area = document.querySelector('#doc textarea.line-edit');
    area.setRangeText(text, 0, 0, 'end');
    area.dispatchEvent(new Event('input', { bubbles: true }));
  }, pasted);
  await page.evaluate(() => window.leoFromApp('quit', ''));
  await page.reload();
  await expect(page.locator('#doc')).toContainText('pasted pasted');
  expect(await page.evaluate(() => document.querySelector('#doc').textContent.includes('later words'))).toBe(true);
});

test('when the browser cannot store a large draft the note is saved to leo and the page says so', async ({ page }) => {
  await page.addInitScript(() => {
    indexedDB.open = () => {
      const request = {};
      setTimeout(() => { request.error = new Error('refused'); if (request.onerror) request.onerror(); }, 0);
      return request;
    };
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (String(key).startsWith('leo-draft-v1:') && String(value).length > 262144) throw new Error('QuotaExceededError');
      return set.call(this, key, value);
    };
  });
  const item = await bigNote(page, 600000);
  await page.goto('/#/n/' + item.id);
  await typeAtTop(page, 'kept by leo ');
  await expect(page.locator('.toast')).toContainText('cannot keep a draft');
  await expect(page.locator('#save-state')).toHaveText('Saved');
  const saved = await (await page.request.get('/api/notes/' + item.id)).json();
  expect(saved.body.startsWith('kept by leo ')).toBe(true);
});
