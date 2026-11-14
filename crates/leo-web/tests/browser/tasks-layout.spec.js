const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss } = require('../page-source');

test('task completion shows its check before hiding and survives refresh, Undo and failure', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const calendar = fs.readFileSync(path.join(web, 'app/calendar.js'), 'utf8');
  const core = fs.readFileSync(path.join(web, 'app/core.js'), 'utf8');
  const actions = fs.readFileSync(path.join(web, 'app/actions.js'), 'utf8');
  const styles = pageCss();
  const completion = calendar.slice(calendar.indexOf('function taskDoneOn'), calendar.indexOf('function pickerBody'));
  const panel = calendar.slice(calendar.indexOf('var tasksWin'), calendar.indexOf('const source ='));
  const action = actions.slice(actions.indexOf("  'task-done':"), actions.indexOf("  'task-date':"));
  const motion = core.slice(core.indexOf('const LEAVE_MS'), core.indexOf('function rel('));
  const fixture = `const S = window.leoSchedule;
    const app = document.querySelector('main');
    let state = {view: 'folder'};
    let seq = 1;
    const CAL_POLL_MS = 150000;
    const cal = {data: {tasks: []}, showDone: true};
    const enc = encodeURIComponent;
    const esc = text => String(text).replace(/[&<>"']/g, ch => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const ICON = {check: '<svg viewBox="0 0 16 16"><path d="M3 8l3 3 7-7" stroke="currentColor" fill="none"/></svg>', plus: '+'};
    class Locked extends Error {}
    const calLocale = () => 'en-US';
    const toneOfTask = () => '--accent';
    const drawCalendar = () => {};
    const loadSchedule = () => {};
    const calFind = id => cal.data.tasks.find(task => task.id === id);
    const closeSheet = () => {};
    let rows = [];
    let failed = false;
    let gate = null;
    let readGate = null;
    let undo = null;
    const toast = (message, options = {}) => { undo = options.run || null; document.querySelector('#message').textContent = message; };
    async function api(url, options = {}) {
      if (options.method === 'PATCH') {
        if (gate) await gate;
        if (failed) { failed = false; throw new Error('Could not save the task. Try again.'); }
        const task = rows.find(task => task.id === decodeURIComponent(url.split('/').pop()));
        if (options.body.on) task.done_on = options.body.done ? [...(task.done_on || []), options.body.on] : (task.done_on || []).filter(day => day !== options.body.on);
        else task.done = options.body.done;
        return task;
      }
      const tasks = structuredClone(rows);
      if (readGate) await readGate;
      return {tasks};
    }
    ${motion}
    ${completion}
    ${panel}
    const actions = {${action}};
    document.addEventListener('click', event => {
      const element = event.target.closest('[data-action]');
      if (!element || !actions[element.dataset.action]) return;
      event.preventDefault();
      Promise.resolve(actions[element.dataset.action](element)).catch(error => toast(error.message));
    });
    window.audit = {
      async seed(tasks) { rows = tasks; await loadTasksWindow(); },
      refresh: loadTasksWindow,
      redraw: drawTasksWindow,
      saved: () => cal.data.tasks,
      undo: () => undo?.(),
      fail() { failed = true; },
      delay() { gate = new Promise(resolve => window.release = () => { gate = null; resolve(); }); },
      staleRefresh() { readGate = new Promise(resolve => window.releaseRead = () => { readGate = null; resolve(); }); window.pendingRead = loadTasksWindow(); },
      async finishRefresh() { releaseRead(); await pendingRead; },
      today: S.dayKey(new Date())
    };`;
  const server = http.createServer((request, response) => {
    if (request.url === '/fixture.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fixture); }
    if (request.url === '/schedule.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fs.readFileSync(path.join(web, 'schedule.js'))); }
    if (request.url === '/style.css') { response.setHeader('Content-Type', 'text/css; charset=utf-8'); return response.end(styles); }
    response.setHeader('Content-Type', 'text/html; charset=utf-8');
    response.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="stylesheet" href="/style.css"></head><body><main></main><p id="message" role="status"></p><script src="/schedule.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({viewport: {width: 1280, height: 900}});
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const task = {id: 'task-panel', title: 'Study the note', done: false, kind: 'other'};
    await page.evaluate(task => audit.seed([task]), task);
    const row = page.locator('.tasks-row');
    const check = row.locator('input');
    const panel = page.locator('#tasks-window');
    await expect(page.locator('.tasks-body'), 'folded until hovered').toBeHidden();
    await panel.hover();
    await page.evaluate(() => audit.delay());
    await check.click();
    await page.evaluate(() => audit.refresh());
    await page.evaluate(() => release());
    await expect(check).toBeChecked({timeout: 500});
    await expect(row).toHaveClass(/done/);
    expect(await page.evaluate(() => audit.saved()[0].done)).toBe(true);
    await page.evaluate(() => audit.refresh());
    await expect(check).toBeChecked();
    await page.waitForTimeout(300);
    await expect(row).toBeVisible();
    await page.evaluate(() => audit.undo());
    await expect(check).not.toBeChecked();
    await page.waitForTimeout(1500);
    await expect(row).toBeVisible();
    await page.evaluate(() => audit.fail());
    await check.click();
    await expect(page.locator('#message')).toHaveText('Could not save the task. Try again.');
    await expect(check).not.toBeChecked();
    await expect(row).not.toHaveClass(/done/);
    await page.evaluate(() => audit.staleRefresh());
    await check.click();
    await expect(check).toBeChecked({timeout: 500});
    await page.evaluate(() => audit.finishRefresh());
    expect(await page.evaluate(() => audit.saved()[0].done)).toBe(true);
    await expect(row).toBeHidden();
    await expect(page.locator('#tasks-window')).toBeHidden();
    await page.emulateMedia({reducedMotion: 'reduce'});
    await page.evaluate(task => audit.seed([{...task, repeat: [0, 1, 2, 3, 4, 5, 6], date: audit.today, done_on: []}]), task);
    const occurrence = await check.getAttribute('data-on');
    await panel.hover();
    await check.click();
    await expect(check).toBeChecked({timeout: 500});
    await page.evaluate(() => audit.refresh());
    await expect(check).toHaveAttribute('data-on', occurrence);
    await expect(check).toBeChecked();
    await expect(check).not.toBeChecked();
    expect(await check.getAttribute('data-on')).not.toBe(occurrence);
    expect(await row.evaluate(el => el.getAnimations().length)).toBe(0);
    await page.setViewportSize({width:1280,height:720});
    await page.evaluate(task => audit.seed(Array.from({length:30}, (_,i) => ({...task,id:"layout-"+i}))), task);
    await panel.hover();
    await expect(page.locator(".tasks-body")).toBeVisible();
    await page.setViewportSize({width:620,height:600});
    expect(errors).toEqual([]);
  } finally {
    await context.close();
    await new Promise(resolve => server.close(resolve));
  }
});
