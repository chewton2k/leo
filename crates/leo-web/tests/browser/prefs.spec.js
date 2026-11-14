const { test, expect } = require('@playwright/test');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const binary = process.env.LEO_TEST_BIN || path.resolve(__dirname, '../../../../target/debug/leo-test-server');

async function start(home, port) {
  const env = { ...process.env, LEO_HOME: home, LEO_INSTALL_NO_MODEL: '1', LEO_NO_UPDATE_CHECK: '1', LEO_NO_MICROPHONE: '1', LEO_NO_OPEN: '1' };
  delete env.LEO_NO_UI_PREFS;
  const child = spawn(binary, ['--port', String(port)], { env, stdio: 'ignore' });
  const exited = new Promise((resolve) => child.once('exit', resolve));
  const url = `http://127.0.0.1:${port}`;
  for (let tries = 0; tries < 150; tries++) {
    if (await fetch(url).then(() => true, () => false)) {
      const token = fs.readFileSync(path.join(home, 'serve-token'), 'utf8').trim();
      return { url: `${url}/?token=${token}`, stop: () => { child.kill('SIGINT'); return exited; } };
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  child.kill('SIGKILL');
  throw new Error('the engine did not start');
}

test('the explorer sort, order and open-tasks choices survive the engine moving to another port', async ({ page }) => {
  test.setTimeout(90000);
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'leo-prefs-'));
  let server;
  try {
    await page.setViewportSize({ width: 1280, height: 800 });
    server = await start(home, 31842);
    await page.goto(server.url);
    const sort = page.getByRole('button', { name: 'Sort notes' });
    const flip = page.getByRole('button', { name: 'Reverse order' });
    const menu = page.getByRole('menu', { name: 'Sort notes' });
    await sort.click();
    await menu.getByRole('menuitemradio', { name: 'Last edited' }).click();
    await sort.click();
    await menu.getByRole('menuitemcheckbox', { name: 'Only notes with open tasks' }).click();
    await flip.click();
    await expect.poll(() => {
      try {
        return JSON.parse(JSON.parse(fs.readFileSync(path.join(home, 'ui-prefs.json'), 'utf8'))['leo-side-order']);
      } catch (e) {
        return null;
      }
    }).toEqual({ sort: 'updated', flip: true, tasks: true });
    await server.stop();
    server = await start(home, 31843);
    await page.goto(server.url);
    await expect(sort).toHaveClass(/\bon\b/);
    await expect(sort).toHaveAttribute('title', 'Sorted by last edited, open tasks only');
    await expect(flip).toHaveAttribute('aria-pressed', 'true');
    await expect(flip).toHaveAttribute('title', 'Oldest first');
  } finally {
    if (server) await server.stop();
    fs.rmSync(home, { recursive: true, force: true });
  }
});
