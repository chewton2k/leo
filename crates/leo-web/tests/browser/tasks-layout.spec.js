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
    await expect.poll(() => page.evaluate(() => audit.saved()[0].done)).toBe(true);
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
    await expect.poll(() => page.evaluate(() => audit.saved()[0].done)).toBe(true);
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

test('home opens on the date and one plain sentence, keeps today\'s done rows and stops at six', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const calendar = fs.readFileSync(path.join(web, 'app/calendar.js'), 'utf8');
  const core = fs.readFileSync(path.join(web, 'app/core.js'), 'utf8');
  const actions = fs.readFileSync(path.join(web, 'app/actions.js'), 'utf8');
  const completion = calendar.slice(calendar.indexOf('function taskDoneOn'), calendar.indexOf('function pickerBody'));
  const panel = calendar.slice(calendar.indexOf('var tasksWin'), calendar.indexOf('const source ='));
  const action = actions.slice(actions.indexOf("  'task-done':"), actions.indexOf("  'task-date':"));
  const motion = core.slice(core.indexOf('const LEAVE_MS'), core.indexOf('function rel('));
  const fixture = `const S = window.leoSchedule;
    const $ = (selector, root = document) => root.querySelector(selector);
    const app = document.querySelector('main');
    let state = {view: 'workspace'};
    let seq = 1;
    const CAL_POLL_MS = 150000;
    const cal = {data: null, showDone: true};
    const enc = encodeURIComponent;
    const esc = text => String(text).replace(/[&<>"']/g, ch => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const ICON = {check: '', plus: '+'};
    class Locked extends Error {}
    const calLocale = () => 'en-US';
    const toneOfTask = () => '--accent';
    const drawCalendar = () => {};
    const loadSchedule = () => {};
    const calFind = id => cal.data.tasks.find(task => task.id === id);
    const toast = () => {};
    const side = {loaded: true, folders: [], notes: new Map([['', {loaded: true, loading: false, items: [{id: 'n1'}]}]])};
    const sideActive = () => true;
    let rows = [];
    let events = [];
    window.urls = [];
    async function api(url, options = {}) {
      window.urls.push(url.split('?')[0]);
      if (options.method === 'PATCH') {
        const task = rows.find(task => task.id === decodeURIComponent(url.split('/').pop()));
        task.done = options.body.done;
        return task;
      }
      return url.startsWith('/api/schedule') ? {connected: true, events: structuredClone(events), tasks: structuredClone(rows)} : {tasks: structuredClone(rows)};
    }
    ${motion}
    ${completion}
    ${panel}
    const actions = {${action}};
    document.addEventListener('click', event => {
      const element = event.target.closest('[data-action]');
      if (!element || !actions[element.dataset.action]) return;
      event.preventDefault();
      actions[element.dataset.action](element);
    });
    window.audit = {
      async seed(tasks, seen = [], notes = 1) { rows = tasks; events = seen; side.notes.get('').items = Array.from({length: notes}, (_, i) => ({id: 'n' + i})); await loadTasksWindow(); }
    };`;
  const home = '<section class="workspace-empty"><header class="home-head"><h1 class="home-day" id="home-day"></h1><p class="home-line" id="home-line"></p></header><hr class="home-rule"><section class="home-today" id="home-today" aria-label="Today" tabindex="-1" hidden></section><div class="home-foot"><p id="home-foot"></p></div></section>';
  const server = http.createServer((request, response) => {
    if (request.url === '/fixture.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fixture); }
    if (request.url === '/schedule.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fs.readFileSync(path.join(web, 'schedule.js'))); }
    if (request.url === '/style.css') { response.setHeader('Content-Type', 'text/css; charset=utf-8'); return response.end(pageCss()); }
    response.setHeader('Content-Type', 'text/html; charset=utf-8');
    response.end(`<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="/style.css"></head><body><main id="app">${home}</main><script src="/schedule.js"></script><script src="/fixture.js"></script></body></html>`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({viewport: {width: 1280, height: 900}, locale: 'en-US'});
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.clock.setFixedTime(new Date(2026, 10, 15, 10, 20));
    await page.goto(`http://127.0.0.1:${server.address().port}/#/`);
    const at = (h, m = 0) => new Date(2026, 10, 15, h, m).toISOString();
    const line = page.locator('#home-line');
    const list = page.locator('#home-today');
    await page.evaluate(([a, b]) => audit.seed([
      {id: 'late', title: 'Email the dataset authors', date: '2026-11-12'},
      {id: 'cut', title: 'Haircut', date: '2026-11-15', time: '09:00', done: true},
      {id: 'review', title: 'Problem set review', date: '2026-11-15', time: '14:00'},
      {id: 'draft', title: 'Send draft to advisor', date: '2026-11-15'},
    ], [{id: 'e1', title: 'Seminar', start: a, end: b}]), [at(10), at(11)]);
    expect(await page.evaluate(() => urls)).toContain('/api/schedule');
    await expect(page.locator('#home-day')).toHaveText('Sunday, November 15');
    await expect(line).toHaveText('Seminar until 11:00 AM. Then Problem set review at 2:00 PM.');
    await expect(line.locator('b')).toHaveText(['Seminar', 'Problem set review']);
    await expect(list.locator('.tasks-group')).toHaveText(['Earlier', 'Today1 of 3 done']);
    await expect(list.locator('.tasks-group.late')).toHaveCount(0);
    await expect(list.locator('.tasks-row').first().locator('.tasks-when')).toHaveText('Thu');
    await expect(list.locator('.tasks-row.done')).toHaveText(/Haircut/);
    await expect(page.locator('#home-foot')).toHaveText('Open a note, or start a new one.');
    await list.locator('.tasks-row', {hasText: 'Send draft'}).locator('.sched-check').click();
    await page.waitForTimeout(1500);
    await expect(list.locator('.tasks-row', {hasText: 'Send draft'}), 'a finished row stays until midnight').toHaveClass(/done/);
    await expect(list.locator('.tasks-group').nth(1)).toHaveText('Today2 of 3 done');
    const late = list.locator('.tasks-row', {hasText: 'Email the dataset authors'});
    await late.locator('.sched-check').click();
    await expect(late.locator('.sched-check'), 'an earlier task shows its tick before it leaves').toBeChecked();
    await expect(late).toHaveCount(0);
    await expect(list.locator('.tasks-group')).toHaveText(['Today2 of 3 done']);

    const many = ['Lab meeting', 'Grade quiz 3', 'Rerun ablations', 'Problem set review', 'Read chapter 4', 'Call the bank', 'Book flights', 'Water plants'].map((title, i) => ({id: 'm' + i, title, date: '2026-11-15', time: i < 4 ? `${11 + i}:00` : null}));
    await page.evaluate(tasks => audit.seed(tasks), [{id: 'l1', title: 'Email', date: '2026-11-12'}, {id: 'l2', title: 'Books', date: '2026-11-14'}, ...many]);
    await expect(line).toHaveText('Eight today and two from earlier. First up is Lab meeting at 11:00 AM.');
    await expect(list.locator('.tasks-row')).toHaveCount(6);
    await expect(list.locator('.home-more')).toHaveText('4 more in Calendar');
    await expect(list.locator('.home-more')).toHaveAttribute('href', '#/calendar');

    await page.evaluate(() => audit.seed([{id: 'l1', title: 'Email', date: '2026-11-12'}, {id: 'l2', title: 'Books', date: '2026-11-14'}]));
    await expect(line).toHaveText('Nothing new today. Two from earlier are still open.');
    await expect(list.locator('.tasks-group')).toHaveText(['Earlier']);
    const old = Array.from({length: 8}, (_, i) => ({id: 'o' + i, title: 'Old ' + i, date: '2026-11-0' + (i + 1)}));
    await page.evaluate(tasks => audit.seed(tasks), [...old, {id: 't1', title: 'Today one', date: '2026-11-15'}, {id: 't2', title: 'Today two', date: '2026-11-15'}]);
    await expect(list.locator('.tasks-group'), 'many earlier tasks never push Today out').toHaveText(['Earlier', 'Today0 of 2 done']);
    await expect(list.locator('.tasks-row')).toHaveCount(6);

    await page.evaluate(() => audit.seed([], [], 0));
    await expect(line).toHaveText('Nothing here yet. Type in the bar to jot a note or a task.');
    await expect(list).toBeHidden();
    await expect(page.locator('#home-foot')).toHaveText('Notes you write appear on the left.');
    await expect(page.locator('.home-foot')).toHaveClass(/first/);
    expect(errors).toEqual([]);
  } finally {
    await context.close();
    await new Promise(resolve => server.close(resolve));
  }
});

test('the Undo countdown is a pencil line rubbed out from the right, and Undo keeps a straight underline', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const core = fs.readFileSync(path.join(web, 'app/core.js'), 'utf8');
  const toast = core.slice(core.indexOf('let toastTimer;'), core.indexOf('const MOTION_MS'));
  const fixture = `const $ = (selector, root = document) => root.querySelector(selector);
    const esc = text => String(text).replace(/[&<>"']/g, ch => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    ${toast}
    window.show = () => toast('Moved to the trash', {action: 'Undo', run() { window.undone = true; }});`;
  const server = http.createServer((request, response) => {
    if (request.url === '/fixture.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fixture); }
    if (request.url === '/style.css') { response.setHeader('Content-Type', 'text/css; charset=utf-8'); return response.end(pageCss()); }
    response.setHeader('Content-Type', 'text/html; charset=utf-8');
    response.end('<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="/style.css"></head><body><main id="app"></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({viewport: {width: 1000, height: 700}});
  try {
    const page = await context.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.evaluate(() => show());
    const undo = page.locator('.toast button');
    const time = page.locator('.toast-time');
    expect(await undo.evaluate(el => { const s = getComputedStyle(el); return [s.textDecorationLine, s.maskImage, s.textDecorationStyle]; })).toEqual(['underline', 'none', 'solid']);
    expect(await time.evaluate(el => [getComputedStyle(el).maskImage.startsWith('url('), getComputedStyle(el).animationName, getComputedStyle(el).animationDuration])).toEqual([true, 'toast-time', '10s']);
    await page.waitForTimeout(1000);
    const erased = await time.evaluate(el => Number(/inset\(0px ([\d.]+)%/.exec(getComputedStyle(el).clipPath)?.[1]));
    expect(erased, 'the line is rubbed out from the right as the time runs').toBeGreaterThan(3);
    expect(erased).toBeLessThan(40);
    await undo.click();
    expect(await page.evaluate(() => window.undone)).toBe(true);
    await page.emulateMedia({reducedMotion: 'reduce'});
    await page.evaluate(() => show());
    await expect(page.locator('.toast-time'), 'no countdown line under reduced motion').toBeHidden();
  } finally {
    await context.close();
    await new Promise(resolve => server.close(resolve));
  }
});
