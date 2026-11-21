const { test, expect } = require('@playwright/test');
const { signIn } = require('./sign-in');

test.beforeEach(async ({ page }) => {
  await signIn(page, '#/new');
  await expect(page.locator('#title')).toBeFocused();
});

for (const key of ['Enter', 'Tab']) {
  test(`${key} in the title moves to the body`, async ({ page }) => {
    await page.keyboard.type('Plain title');
    await page.keyboard.press(key);
    await expect(page.locator('#doc .line-edit')).toBeFocused();
    await expect(page.locator('#title')).toHaveText('Plain title');
  });
}

test('Shift+Enter never adds a line to the title, and Escape then Shift+Tab walks back through the title to the reading mode buttons', async ({ page, browserName }) => {
  await page.keyboard.type('One line');
  await page.keyboard.press('Shift+Enter');
  await expect(page.locator('#title')).toHaveText('One line');
  await expect(page.locator('#title br, #title div')).toHaveCount(0);
  await page.keyboard.press('Tab');
  await expect(page.locator('#doc .line-edit')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#doc .line-edit')).toHaveCount(0);
  await expect(page.locator('#margin-mode')).toBeEnabled();
  if (browserName === 'webkit') {
    const order = await page.evaluate(() => {
      const doc = document.querySelector('#doc');
      return ['#margin-mode', '#margin-persona'].map((id) => {
        const el = document.querySelector(id);
        return el.tabIndex >= 0 && !el.disabled && Boolean(el.compareDocumentPosition(doc) & Node.DOCUMENT_POSITION_FOLLOWING);
      });
    });
    expect(order).toEqual([true, true]);
    return;
  }
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('#title')).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('#note-more')).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('#margin-mode')).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('#margin-persona')).toBeFocused();
});
