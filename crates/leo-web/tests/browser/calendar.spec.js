const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss } = require('../page-source');

const at = (d, h, m = 0) => new Date(2026, 10, d, h, m).toISOString();

// The Calendar page with its real CSS, icons, sheets, schedule.js and calendar.js over an
// in-memory API that behaves like routes/tasks.rs (occurrence moves included). A fixed
// 284 px explorer sits on the left like the desktop sidebar.
async function calendarFixture(browser, run, { width = 1360, height = 900, view = 'week' } = {}) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  const core = read('app/core.js');
  const icons = core.slice(core.indexOf('const svg ='), core.indexOf('const recorder ='));
  const sheets = read('app/sheets.js');
  const sheetParts = sheets.slice(sheets.indexOf('function sheet('), sheets.indexOf('const QUICK_PLACES'));
  const calendar = read('app/calendar.js');
  const page = calendar.slice(0, calendar.indexOf('var tasksWin'));
  const actions = read('app/actions.js');
  const calActions = actions.slice(actions.indexOf("  'cal-pick'"), actions.indexOf("  'task-date'"));
  const db = {
    tasks: [
      { id: 'task-lab', source: 'leo', title: 'Lab report', description: '', kind: 'deadline', done: false, date: '2026-11-05', time: '11:00:00', end: null, note: null, found: true, promoted: false },
      { id: 'task-study', source: 'leo', title: 'Study group', description: '', kind: 'meeting', done: false, date: '2026-11-03', time: '13:00:00', end: '14:00:00', note: null, found: true, promoted: false },
      { id: 'task-gym', source: 'leo', title: 'Gym', description: '', kind: 'other', done: false, date: '2026-11-02', time: '16:00:00', end: '17:00:00', repeat: [0, 2], done_on: [], note: null, found: true, promoted: false },
      { id: 'task-essay', source: 'leo', title: 'Draft essay outline', description: '', kind: 'assignment', done: false, date: null, time: null, end: null, note: null, found: true, promoted: false },
      { id: 'task-rent', source: 'leo', title: 'Pay rent', description: '', kind: 'other', done: false, date: '2026-11-01', time: null, end: null, note: null, found: true, promoted: false },
      { id: 'task-note', source: 'note', title: 'Read chapter 5', description: '', kind: 'other', done: false, date: '2026-11-04', time: '12:00:00', end: null, note: { id: 'n1', title: 'Biology' }, found: true, promoted: false },
    ],
    events: [
      { id: 'e-lecture', title: 'CS 241 lecture', start: at(3, 10), end: at(3, 11, 30), all_day: false, calendar: 'School', context: '', location: 'DC 1351' },
      { id: 'e-week', title: 'Reading week', all_day: true, date: '2026-11-04', end_date: '2026-11-07', start: at(4, 0), end: at(7, 0), calendar: 'School', context: '' },
      { id: 'e-maya', title: '1:1 Maya', start: at(4, 14), end: at(4, 15, 30), all_day: false, calendar: 'Work', context: '' },
      { id: 'e-ta', title: 'TA hours', start: at(4, 14, 30), end: at(4, 16), all_day: false, calendar: 'Work', context: '' },
    ],
  };
  const fixture = `const $=(selector,root=document)=>root.querySelector(selector);
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const enc=encodeURIComponent, app=$('#app'), LEAVE_MS=150;
    ${icons}
    class Locked extends Error {}
    let seq=0,state={view:null};
    const chrome=()=>{}, stillMotion=()=>true, rel=()=>'just now', noteHash=id=>'#/n/'+id;
    var tasksWin={revision:0,tasks:null};
    const drawTasksWindow=()=>{}, loadTasksWindow=async()=>{}, finishTaskInWindow=()=>{};
    window.toasts=[];
    function toast(message,{action,run,bad}={}){window.toasts.push({message,bad:Boolean(bad)});document.querySelector('.toast')?.remove();const el=document.createElement('div');el.className='toast';el.innerHTML='<span>'+esc(message)+'</span>'+(action?'<button>'+esc(action)+'</button>':'');if(action)el.querySelector('button').onclick=()=>{el.remove();run();};document.body.appendChild(el);}
    const fail=error=>window.errors.push(error.message);
    ${sheetParts}
    const db=${JSON.stringify(db)};
    const hms=v=>v?String(v).slice(0,5)+':00':null;
    let made=0;
    window.calls=[];window.pending=0;window.failNext=null;window.failSchedule=false;
    async function api(url,options={}){window.pending++;try{return await fakeApi(url,options);}finally{window.pending--;}}
    async function fakeApi(url,{method='GET',body}={}){
      window.calls.push({method,url:url.split('?')[0],body:body?structuredClone(body):null});
      await new Promise(r=>setTimeout(r,5));
      if(window.failNext&&method!=='GET'){const e=new Error(window.failNext);window.failNext=null;throw e;}
      let m;
      if(url.startsWith('/api/schedule')&&window.failSchedule){window.failSchedule=false;throw new Error('offline');}
      if(url.startsWith('/api/schedule'))return {connected:true,calendars:[{id:'c1',name:'School'}],synced:new Date().toISOString(),refreshing:false,events:structuredClone(db.events),tasks:structuredClone(db.tasks)};
      if(url==='/api/calendar/refresh')return null;
      if(url==='/api/tasks'&&method==='POST'){const t={id:'task-made'+(++made),source:'leo',title:body.title,description:'',kind:body.kind||'other',done:false,date:body.date||null,time:hms(body.time),end:body.time?hms(body.end):null,note:null,found:true,promoted:false};db.tasks.push(t);return structuredClone(t);}
      if((m=url.match(/^\\/api\\/tasks\\/([^/]+)$/))){
        const t=db.tasks.find(x=>x.id===m[1]);
        if(!t){const e=new Error('That task is not there any more.');e.status=404;throw e;}
        if(method==='DELETE'){db.tasks=db.tasks.filter(x=>x!==t);return structuredClone(t);}
        if(body.occurrence){if(!body.date){const e=new Error('Say which day it moves to.');e.status=400;throw e;}const slot={date:body.date,time:hms(body.time),end:body.time?hms(body.end):null};t.moved={...(t.moved||{})};if(slot.date===body.occurrence&&slot.time===t.time&&slot.end===t.end)delete t.moved[body.occurrence];else t.moved[body.occurrence]=slot;}
        else{
          for(const k of ['title','description','kind','repeat'])if(k in body)t[k]=body[k];
          if('done' in body){if(t.repeat&&t.repeat.length){t.done_on=(t.done_on||[]).filter(d=>d!==body.on);if(body.done)t.done_on.push(body.on);}else t.done=body.done;}
          if('date' in body){t.date=body.date;t.time=body.date?hms(body.time):null;t.end=t.time&&t.source==='leo'?hms(body.end):null;}
        }
        return structuredClone(t);
      }
      throw new Error('No fake for '+method+' '+url);
    }
    ${core.slice(core.indexOf('const MOTION_MS'), core.indexOf('function stillMotion'))}
    ${core.slice(core.indexOf('const moving'), core.indexOf('function rel('))}
    ${read('app/motion.js')}
    ${read('app/settings-navigation.js')}
    ${page}
    const actions={${calActions}'task-done':el=>{const id=el.dataset.id,on=el.dataset.on||null;const task=calFind(id);const done=!(task&&taskDoneOn(task,on));return setTaskDone(id,done,on);},'task-delete':el=>deleteTask(el.dataset.id),'task-new':el=>newTaskSheet(el.dataset.day||null),close:()=>closeSheet()};
    document.addEventListener('click',e=>{const el=e.target.closest('[data-action]');if(!el||!actions[el.dataset.action])return;e.preventDefault();Promise.resolve(actions[el.dataset.action](el)).catch(fail);});
    document.addEventListener('keydown',e=>{if(e.key==='Escape'&&$('.scrim'))closeSheet();});
    window.errors=[];window.db=db;
    try{localStorage.setItem('leo-calendar-view',${JSON.stringify(view)});}catch(e){}
    showCalendar().catch(fail);`;
  const server = http.createServer((req, res) => {
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(fixture); }
    if (req.url === '/schedule.js') { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(read('schedule.js')); }
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); return res.end(css); }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"><style>body{--side:284px;padding-left:var(--side)}#explorer{position:fixed;left:0;top:0;bottom:0;width:var(--side);background:var(--surface-2)}</style></head><body class="on-calendar"><nav id="explorer">Folders</nav><header class="bar" id="bar"><div class="bar-row"><div class="crumbs" id="crumbs"></div></div></header><main id="app"></main><script src="/schedule.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({ viewport: { width, height }, locale: 'en-US' });
  try {
    const tab = await context.newPage();
    await tab.clock.install({ time: new Date(2026, 10, 3, 11, 40) });
    const errors = [];
    tab.on('pageerror', error => errors.push(error.message));
    await tab.goto('http://127.0.0.1:' + server.address().port + '/');
    await expect(tab.locator('.cal-item').first()).toBeVisible();
    await settled(tab);
    await run(tab);
    expect(errors).toEqual([]);
    expect(await tab.evaluate(() => window.errors)).toEqual([]);
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
}

async function settled(page) {
  await expect.poll(() => page.evaluate(() => window.pending)).toBe(0);
}

const patches = (page, id) => page.evaluate(id => window.calls.filter(c => c.method === 'PATCH' && c.url === '/api/tasks/' + id).map(c => c.body), id);
const task = (page, id) => page.evaluate(id => window.db.tasks.find(t => t.id === id), id);

async function point(page, day, minutes) {
  const box = await page.locator(`.cal-col[data-day="${day}"]`).boundingBox();
  return { x: box.x + box.width / 2, y: box.y + (minutes / 60) * 48 };
}

async function dragTo(page, from, to) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 6, from.y + 6, { steps: 2 });
  await page.mouse.move(to.x, to.y, { steps: 8 });
  await page.mouse.move(to.x, to.y);
  await page.mouse.up();
  await settled(page);
}

test('the calendar fills the room beside the explorer and shows a week of events and tasks', async ({ browser }, testInfo) => {
  await calendarFixture(browser, async page => {
    for (const theme of ['light', 'dark']) {
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      await page.screenshot({ path: testInfo.outputPath(`calendar-week-${theme}.png`) });
    }
    await page.evaluate(() => { document.documentElement.dataset.theme = 'light'; });
    const explorer = await page.locator('#explorer').boundingBox();
    const cal = await page.locator('.cal-page').boundingBox();
    expect(cal.x).toBeGreaterThanOrEqual(explorer.x + explorer.width - 1);
    expect(cal.x + cal.width).toBeLessThanOrEqual(1900 + 1);
    await expect(page.locator('.cal-dayhead')).toHaveCount(7);
    expect(await page.locator('.cal-hours').evaluate(el => getComputedStyle(el).backgroundImage)).not.toBe('none');
    const boundaries = await page.evaluate(() => {
      const lefts = selector => [...document.querySelectorAll(selector)].map(el => el.getBoundingClientRect().left);
      return [lefts('.cal-dayhead'), lefts('.cal-allday-cell'), lefts('.cal-col')];
    });
    for (let day = 0; day < 7; day++) {
      expect(Math.abs(boundaries[0][day] - boundaries[2][day])).toBeLessThan(1);
      expect(Math.abs(boundaries[1][day] - boundaries[2][day])).toBeLessThan(1);
    }

    await expect(page.locator('.cal-dayhead.today')).toContainText('3');
    await expect(page.locator('.cal-col[data-day="2026-11-03"] .cal-now')).toHaveCount(1);
    await expect(page.locator('.cal-allday .cal-item', { hasText: 'Reading week' })).toHaveCount(1);
    const maya = await page.locator('.cal-item', { hasText: '1:1 Maya' }).boundingBox();
    const ta = await page.locator('.cal-item', { hasText: 'TA hours' }).boundingBox();
    expect(ta.x).toBeGreaterThan(maya.x + 4);
    await expect(page.locator('.cal-col[data-day="2026-11-02"] .cal-item', { hasText: 'Gym' })).toHaveCount(1);
    await expect(page.locator('.cal-col[data-day="2026-11-04"] .cal-item', { hasText: 'Gym' })).toHaveCount(1);
    await expect(page.locator('.cal-item[data-id="task-note"] .cal-grip')).toHaveCount(0);
    await expect(page.locator('.cal-tray-list h2.late')).toContainText('Overdue');
    await expect(page.locator('.cal-tray-row', { hasText: 'Draft essay outline' })).toBeVisible();
  }, { width: 1900 });
});

test('a narrow week column shows one whole tile and a count beside it for overlapping events', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    const col = page.locator('.cal-col[data-day="2026-11-04"]');
    await expect(col.locator('.cal-item', { hasText: '1:1 Maya' })).toHaveCount(1);
    await expect(col.locator('.cal-item', { hasText: 'TA hours' })).toHaveCount(0);
    const more = col.locator('.cal-stack-more');
    await expect(more).toHaveText('+1');
    const maya = await col.locator('.cal-item', { hasText: '1:1 Maya' }).boundingBox();
    expect(maya.x + maya.width).toBeLessThanOrEqual((await more.boundingBox()).x + 0.5);
  });
});

test('tasks follow the mini calendar and stay visible above a narrow calendar', async ({ browser }) => {
  for (const width of [1360, 1000, 674]) {
    await calendarFixture(browser, async page => {
      const side = page.locator('#cal-side');
      await expect(side.getByRole('heading', { name: 'Tasks', exact: true })).toBeVisible();
      const row = side.locator('.cal-tray-row', { hasText: 'Draft essay outline' });
      await expect(row).toBeVisible();
      const tasks = await row.boundingBox();
      const mini = side.locator('.cal-mini');
      if (width === 1360) {
        const month = await mini.boundingBox();
        expect(month.y + month.height).toBeLessThan(tasks.y);
      } else {
        await expect(mini).toBeHidden();
        const main = await page.locator('#cal-main').boundingBox();
        expect(tasks.y + tasks.height).toBeLessThanOrEqual(main.y);
      }
      await row.locator('.cal-check').click();
      await settled(page);
      await expect(side.locator('.cal-tray-row', { hasText: 'Draft essay outline' })).toHaveCount(0);
    }, { width });
  }
});

test('the calendar status refreshes on click and offers Retry only after a failure', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    const status = page.locator('.cal-status');
    await expect(status.locator('.cal-refresh')).toContainText('Updated');
    await expect(status.getByRole('button', { name: 'Retry' })).toHaveCount(0);
    await page.evaluate(() => { window.failSchedule = true; });
    await status.locator('.cal-refresh').click();
    await expect(status).toContainText("Couldn't update");
    await status.getByRole('button', { name: 'Retry' }).click();
    await expect(status.locator('.cal-refresh')).toContainText('Updated');
  });
});

test('the task sidebar creates undated tasks from its empty state', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await page.evaluate(async () => { window.db.tasks = []; await loadSchedule(); });
    const side = page.locator('#cal-side');
    await expect(side).toContainText('Nothing overdue. Tasks without a date land here, ready to drag onto the calendar.');
    await side.getByRole('button', { name: 'New task', exact: true }).click();
    await expect(page.locator('#task-title')).toBeFocused();
    await page.locator('#task-title').fill('Read the next chapter');
    await page.locator('#task-add').click();
    await settled(page);
    await expect(side.locator('.cal-tray-row', { hasText: 'Read the next chapter' })).toBeVisible();
    expect(await page.evaluate(() => window.db.tasks[0].date)).toBeNull();
  });
});

test('a task drags to a new time, its bottom edge sets how long it takes, and Undo puts it back', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    const study = page.locator('.cal-item[data-id="task-study"]');
    const box = await study.boundingBox();
    await dragTo(page, { x: box.x + box.width / 2, y: box.y + 10 }, await point(page, '2026-11-05', 15 * 60 + 12.5));
    await expect.poll(() => patches(page, 'task-study')).toEqual([{ date: '2026-11-05', time: '15:00', end: '16:00' }]);
    await expect(page.locator('.cal-col[data-day="2026-11-05"] .cal-item[data-id="task-study"]')).toHaveCount(1);
    await page.locator('.toast button', { hasText: 'Undo' }).click();
    await settled(page);
    expect(await task(page, 'task-study')).toMatchObject({ date: '2026-11-03', time: '13:00:00', end: '14:00:00' });
    await expect(page.locator('.cal-col[data-day="2026-11-03"] .cal-item[data-id="task-study"]')).toHaveCount(1);

    const grip = await page.locator('.cal-item[data-id="task-study"] .cal-grip').boundingBox();
    const from = { x: grip.x + grip.width / 2, y: grip.y + grip.height / 2 };
    const delta = (await point(page, '2026-11-03', 15 * 60 + 30)).y - (await point(page, '2026-11-03', 14 * 60)).y;
    await dragTo(page, from, { x: from.x, y: from.y + delta });
    expect(await task(page, 'task-study')).toMatchObject({ date: '2026-11-03', time: '13:00:00', end: '15:30:00' });

    await page.evaluate(() => { window.failNext = 'The disk is full.'; });
    const lab = await page.locator('.cal-item[data-id="task-lab"]').boundingBox();
    await dragTo(page, { x: lab.x + lab.width / 2, y: lab.y + 8 }, await point(page, '2026-11-06', 12 * 60 + 10));
    expect(await task(page, 'task-lab')).toMatchObject({ date: '2026-11-05', time: '11:00:00' });
    await expect(page.locator('.cal-col[data-day="2026-11-05"] .cal-item[data-id="task-lab"]')).toHaveCount(1);
    expect(await page.evaluate(() => window.toasts.at(-1))).toEqual({ message: 'The disk is full.', bad: true });
  });
});

test('clicking or dragging over empty time writes a task in place, and Escape lets it go', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    const at = await point(page, '2026-11-06', 14 * 60 + 5);
    await page.mouse.click(at.x, at.y);
    const input = page.locator('.cal-compose-input');
    await expect(input).toBeFocused();
    await input.fill('Call the bank');
    await input.press('Enter');
    await settled(page);
    expect(await page.evaluate(() => window.calls.filter(c => c.method === 'POST').map(c => c.body))).toEqual([{ title: 'Call the bank', date: '2026-11-06', time: '14:00', end: '15:00', kind: 'other' }]);
    await expect(page.locator('.cal-col[data-day="2026-11-06"] .cal-item', { hasText: 'Call the bank' })).toHaveCount(1);

    await dragTo(page, await point(page, '2026-11-07', 12 * 60), await point(page, '2026-11-07', 13 * 60 + 30));
    await input.fill('Picnic');
    await input.press('Enter');
    await settled(page);
    expect(await page.evaluate(() => window.calls.filter(c => c.method === 'POST').at(-1).body)).toMatchObject({ date: '2026-11-07', time: '12:00', end: '13:30' });

    const quarter = await point(page, '2026-11-05', 14 * 60 + 13.5);
    await page.mouse.click(quarter.x, quarter.y);
    await expect(input).toHaveAttribute('aria-label', 'New task, 2 – 3 PM');
    await input.press('Escape');

    const later = await point(page, '2026-11-02', 18 * 60);
    await page.mouse.click(later.x, later.y);
    await page.locator('.cal-compose-input').press('Escape');
    await expect(page.locator('.cal-compose')).toHaveCount(0);
    expect(await page.evaluate(() => window.calls.filter(c => c.method === 'POST').length)).toBe(2);
  });
});

test('a task without a date drags from the tray onto the week', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    const row = await page.locator('.cal-tray-row', { hasText: 'Draft essay outline' }).boundingBox();
    await dragTo(page, { x: row.x + row.width / 2, y: row.y + row.height / 2 }, await point(page, '2026-11-05', 15 * 60));
    expect(await task(page, 'task-essay')).toMatchObject({ date: '2026-11-05', time: '15:00:00', end: '16:00:00' });
    await expect(page.locator('.cal-tray-row', { hasText: 'Draft essay outline' })).toHaveCount(0);
  });
});

test('one day of a repeating task moves alone, or every day moves together', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    const gym = page.locator('.cal-col[data-day="2026-11-04"] .cal-item', { hasText: 'Gym' });
    let box = await gym.boundingBox();
    await dragTo(page, { x: box.x + box.width / 2, y: box.y + 10 }, await point(page, '2026-11-05', 17 * 60 + 12.5));
    await page.locator('#cal-move-one').click();
    await settled(page);
    expect(await patches(page, 'task-gym')).toEqual([{ occurrence: '2026-11-04', date: '2026-11-05', time: '17:00', end: '18:00' }]);
    await expect(page.locator('.cal-col[data-day="2026-11-04"] .cal-item', { hasText: 'Gym' })).toHaveCount(0);
    await expect(page.locator('.cal-col[data-day="2026-11-05"] .cal-item', { hasText: 'Gym' })).toHaveCount(1);
    await expect(page.locator('.cal-col[data-day="2026-11-02"] .cal-item', { hasText: 'Gym' })).toHaveCount(1);

    box = await page.locator('.cal-col[data-day="2026-11-02"] .cal-item', { hasText: 'Gym' }).boundingBox();
    await dragTo(page, { x: box.x + box.width / 2, y: box.y + 10 }, await point(page, '2026-11-03', 16 * 60 + 12.5));
    await page.locator('#cal-move-all').click();
    await settled(page);
    expect((await patches(page, 'task-gym')).at(-1)).toEqual({ date: '2026-11-03', time: '16:00', end: '17:00', repeat: [1, 3] });
  });
});

test('a task opens beside the calendar, edits save, and Escape closes it', async ({ browser }, testInfo) => {
  await calendarFixture(browser, async page => {
    await page.locator('.cal-item[data-id="task-lab"] .cal-open').click();
    const side = page.locator('#cal-side.peek');
    await expect(side.locator('#cal-peek-title')).toHaveValue('Lab report');
    await expect(page.locator('#explorer')).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('calendar-peek.png') });
    await side.locator('#cal-peek-title').fill('Lab report v2');
    await side.locator('#cal-peek-title').press('Enter');
    await settled(page);
    expect((await task(page, 'task-lab')).title).toBe('Lab report v2');
    await expect(page.locator('.cal-item[data-id="task-lab"] .cal-title')).toHaveText('Lab report v2');
    await side.locator('#cal-peek-about').fill('Bring the data sheet');
    await side.locator('#cal-peek-about').blur();
    await settled(page);
    expect((await task(page, 'task-lab')).description).toBe('Bring the data sheet');
    await side.locator('#cal-peek-start').fill('09:30');
    await side.locator('#cal-peek-start').dispatchEvent('change');
    await settled(page);
    expect(await task(page, 'task-lab')).toMatchObject({ time: '09:30:00' });
    await page.locator('.cal-name').click();
    await page.keyboard.press('Escape');
    await expect(page.locator('#cal-side.peek')).toHaveCount(0);
    await expect(page.locator('.cal-mini')).toBeVisible();

    await page.locator('.cal-item', { hasText: 'CS 241 lecture' }).locator('.cal-open').click();
    await expect(page.locator('#cal-side.peek .cal-peek-title')).toHaveText('CS 241 lecture');
    await expect(page.locator('#cal-side.peek')).toContainText('DC 1351');
    await expect(page.locator('#cal-side.peek .cal-item, #cal-side.peek input')).toHaveCount(0);

    await page.locator('.cal-name').click();
    await page.keyboard.press('Escape');
    await page.locator('.cal-tray-row .cal-open', { hasText: 'Draft essay outline' }).click();
    await expect(page.locator('#cal-side [data-action="task-google"]')).toHaveCount(0);
    await page.locator('#cal-peek-date').fill('2026-11-06');
    await page.locator('#cal-peek-date').dispatchEvent('change');
    await expect(page.locator('#cal-peek-start')).toBeEnabled();
    await expect(page.locator('#cal-side [data-action="task-google"]')).toBeVisible();
    await expect(page.locator('#cal-side a[download]')).toBeVisible();
  });
});

test('Day, Week and Month switch by the control or the keyboard, and the choice is kept', async ({ browser }, testInfo) => {
  await calendarFixture(browser, async page => {
    await page.locator('.cal-views [data-view="month"]').click();
    await settled(page);
    await expect(page.locator('.cal-cell')).toHaveCount(42);
    await expect(page.locator('.cal-cell.today .cal-num')).toHaveText('3');
    await page.screenshot({ path: testInfo.outputPath('calendar-month.png') });
    expect(await page.evaluate(() => localStorage.getItem('leo-calendar-view'))).toBe('month');
    const lab = await page.locator('.cal-cell[data-day="2026-11-05"] .cal-item[data-id="task-lab"]').boundingBox();
    const cell = await page.locator('.cal-cell[data-day="2026-11-12"]').boundingBox();
    await dragTo(page, { x: lab.x + lab.width / 2, y: lab.y + lab.height / 2 }, { x: cell.x + cell.width / 2, y: cell.y + cell.height - 10 });
    expect(await task(page, 'task-lab')).toMatchObject({ date: '2026-11-12', time: '11:00:00' });

    await page.keyboard.press('d');
    await settled(page);
    await expect(page.locator('.cal-dayhead')).toHaveCount(1);
    await page.keyboard.press('j');
    await settled(page);
    await expect(page.locator('.cal-dayhead')).toContainText('4');
    await page.keyboard.press('t');
    await page.keyboard.press('w');
    await settled(page);
    await expect(page.locator('.cal-dayhead')).toHaveCount(7);
    await page.locator('.cal-dayhead', { hasText: '5' }).click();
    await expect(page.locator('.cal-dayhead')).toHaveCount(1);
    expect(await page.evaluate(() => localStorage.getItem('leo-calendar-view'))).toBe('day');
  });
});

test('on a narrow window the calendar keeps the explorer and the peek covers only the calendar', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await expect(page.locator('.cal-side')).toBeVisible();
    await page.locator('.cal-item[data-id="task-lab"] .cal-open').click();
    const side = await page.locator('#cal-side.peek').boundingBox();
    const explorer = await page.locator('#explorer').boundingBox();
    expect(side.x).toBeGreaterThanOrEqual(explorer.x + explorer.width - 1);
    await page.locator('[data-action="cal-peek-close"]').click();
    await expect(page.locator('#cal-side .cal-tray-row').first()).toBeVisible();
  }, { width: 1000, view: 'day' });
});

test('dating and clearing a task updates export actions without replacing focused fields', async ({ browser }) => {
  await calendarFixture(browser,async page=>{
    await page.locator('.cal-tray-row[data-id="task-essay"] .cal-open').click();
    const date=page.locator('#cal-peek-date'), google=page.locator('#cal-side [data-action="task-google"]'), ics=page.locator('#cal-side a[download]');
    await expect(google).toBeHidden();
    await page.evaluate(()=>window.keptDate=document.querySelector('#cal-peek-date'));
    await date.fill('2026-11-06');
    await date.dispatchEvent('change');
    await settled(page);
    await expect(google).toBeVisible();
    await expect(ics).toBeVisible();
    await expect(date).toBeFocused();
    expect(await page.evaluate(()=>keptDate===document.querySelector('#cal-peek-date'))).toBe(true);
    await date.fill('');
    await date.dispatchEvent('change');
    await settled(page);
    await expect(google).toBeHidden();
    await expect(ics).toBeHidden();
    await expect(page.locator('#cal-peek-start')).toBeDisabled();
    await expect(date).toBeFocused();
  });
});

test('creating a time range respects grid boundaries and either drag direction', async ({ browser }) => {
  await calendarFixture(browser,async page=>{
    for(const [start,end] of [[720,810],[810,720],[0,15],[1425,1440]]) {
      await page.locator('#cal-scroll').evaluate((el,minute)=>{el.scrollTop=Math.max(0,(minute/60-1)*48);},Math.min(start,end));
      const a=await point(page,'2026-11-07',start), b=await point(page,'2026-11-07',end);
      await dragTo(page,{x:a.x,y:a.y+(start>0&&start%60===0?-0.4:0.4)},{x:b.x,y:b.y-0.4});
      const input=page.locator('.cal-compose-input');
      await expect(input).toBeFocused();
      await input.fill('Range '+start);
      await input.press('Enter');
      await settled(page);
      const request=await page.evaluate(()=>window.calls.filter(c=>c.method==='POST').at(-1).body);
      const hm=m=>{m=Math.min(1439,m);return String(Math.floor(m/60)).padStart(2,'0')+':'+String(m%60).padStart(2,'0');};
      expect(request).toMatchObject({time:hm(Math.min(start,end)),end:hm(Math.max(start,end))});
    }
  });
});

test('calendar tasks have readable blocks and compact accessible action icons', async ({ browser }, testInfo) => {
  for (const view of ['week', 'day', 'month', 'all-day']) {
    await calendarFixture(browser, async page => {
      const item = page.locator('.cal-item[data-id="task-lab"]').first();
      await page.evaluate(allDay => {
        const task = cal.data.tasks.find(task => task.id === 'task-lab');
        task.title = 'Math 164 homework — finish exercises 1–20 and submit the worked solutions before the deadline';
        if (allDay) { task.time = null; task.end = null; }
        Object.assign(window.db.tasks.find(saved => saved.id === task.id), task);
        drawCalendar();
      }, view === 'all-day');

      if (view === 'day') {
        await page.locator('.cal-dayhead').first().click();
        await page.evaluate(() => { cal.selected = '2026-11-05'; drawCalendar(); });
      }
      if (view === 'month') await page.locator('[data-action="cal-view"][data-view="month"]').click();
      await settled(page);
      await expect(item).toBeVisible();
      const height = (await item.boundingBox()).height;
      expect(height).toBeGreaterThanOrEqual(view === 'all-day' ? 44 : 60);
      expect(await item.locator('.cal-title').evaluate(el => parseFloat(getComputedStyle(el).fontSize))).toBeGreaterThanOrEqual(13);
      if (view !== 'day') expect((await item.locator('.cal-title').boundingBox()).height).toBeGreaterThan(25);
      if (view === 'week') await item.screenshot({ path: testInfo.outputPath('readable-task.png') });
      await item.locator('.cal-open').click();
      const actions = page.locator('#cal-peek-acts');
      for (const name of ['Add to Google Calendar', 'Download .ics', 'Delete']) {
        const control = actions.getByRole(name === 'Download .ics' ? 'link' : 'button', { name, exact: true });
        await expect(control).toBeVisible();
        await expect(control.locator('svg')).toHaveCount(1);
        expect((await control.textContent()).trim()).toBe('');
        await expect(control).toHaveAttribute('title', name);
      }
      const boxes = await actions.locator('.icon-btn').evaluateAll(controls => controls.map(control => control.getBoundingClientRect().y));
      expect(Math.max(...boxes) - Math.min(...boxes)).toBeLessThan(1);
      await page.screenshot({ path: testInfo.outputPath('task-icons-' + view + '.png') });
      if (view === 'week') await actions.screenshot({ path: testInfo.outputPath('task-action-icons.png') });
    });
  }
});

test('readable minimum task heights participate in calendar overlap layout', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await page.evaluate(() => {
      const task = cal.data.tasks.find(task => task.id === 'task-lab');
      cal.data.tasks.push({ ...task, id: 'task-nearby', title: 'Nearby deadline', time: '11:45:00' });
      drawCalendar();
    });
    const first = await page.locator('.cal-item[data-id="task-lab"]').boundingBox();
    const second = await page.locator('.cal-item[data-id="task-nearby"]').boundingBox();
    expect(second.x).toBeGreaterThan(first.x + 5);
  }, { width: 1900 });
});

for (const [width, tiles] of [[1360, 1], [1900, 2]]) test(`three tasks at the end of a week day show ${tiles} readable tile${tiles > 1 ? 's' : ''} at ${width} px, a count beside them, and the day shows all three`, async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await page.evaluate(() => {
      const task = cal.data.tasks.find(task => task.id === 'task-lab');
      for (const [id, time] of [['task-a', '23:00:00'], ['task-b', '23:30:00'], ['task-c', '23:45:00']]) {
        const late = { ...task, id, title: 'Late ' + id, time, end: null };
        cal.data.tasks.push(late);
        window.db.tasks.push(late);
      }
      drawCalendar();
    });
    const col = page.locator('.cal-col[data-day="2026-11-05"]');
    await expect(col.locator('.cal-item[data-id^="task-"]').filter({ hasText: 'Late' })).toHaveCount(tiles);
    const more = col.locator('.cal-stack-more');
    await expect(more).toHaveText(`+${3 - tiles}`);
    const edge = (await more.boundingBox()).x;
    for (const box of await col.locator('.cal-item', { hasText: 'Late' }).evaluateAll(els => els.map(el => el.getBoundingClientRect().toJSON()))) {
      expect(box.width).toBeGreaterThan(55);
      expect(box.right, 'the count sits beside the tiles, not on them').toBeLessThanOrEqual(edge + 0.5);
    }
    await more.click();
    await expect(page.locator('.cal-item', { hasText: 'Late' })).toHaveCount(3);
    await expect(page.locator('.cal-item', { hasText: 'Late task-c' })).toContainText('11:45');
  }, { width });
});

test('a task at the end of the day stays inside the grid and keeps its real time', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await page.evaluate(() => {
      const task = cal.data.tasks.find(task => task.id === 'task-lab');
      cal.data.tasks.push({ ...task, id: 'task-late', title: 'Late deadline', time: '23:59:00', end: null });
      drawCalendar();
    });
    const column = await page.locator('.cal-col[data-day="2026-11-05"]').boundingBox();
    const late = await page.locator('.cal-item[data-id="task-late"]').boundingBox();
    expect(late.y + late.height).toBeLessThanOrEqual(column.y + column.height);
    expect(await page.locator('#cal-scroll').evaluate(el => el.scrollHeight)).toBeLessThanOrEqual(24 * 48 + 2);
  });
});

test('a task with an end is as tall as its time: 2 to 3 PM stops at the 3 PM line, half an hour shows title and time on one line', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await page.evaluate(() => {
      const task = cal.data.tasks.find(task => task.id === 'task-lab');
      for (const [id, time, end] of [['task-hour', '14:00:00', '15:00:00'], ['task-half', '16:00:00', '16:30:00']]) {
        const made = { ...task, id, title: 'Block ' + id, time, end };
        cal.data.tasks.push(made);
        window.db.tasks.push(made);
      }
      drawCalendar();
    });
    const column = await page.locator('.cal-col[data-day="2026-11-05"]').boundingBox();
    const hour = await page.locator('.cal-item[data-id="task-hour"]').boundingBox();
    expect(hour.height).toBeCloseTo(46, 0);
    expect(hour.y + hour.height).toBeLessThanOrEqual(column.y + 15 * 48);
    const half = page.locator('.cal-item[data-id="task-half"]');
    expect((await half.boundingBox()).height).toBeCloseTo(22, 0);
    await expect(half).toHaveClass(/inline/);
    const title = await half.locator('.cal-title').boundingBox();
    const sub = await half.locator('.cal-sub').boundingBox();
    expect(sub.width).toBeGreaterThan(0);
    expect(Math.abs((sub.y + sub.height / 2) - (title.y + title.height / 2))).toBeLessThan(8);
    await expect(page.locator('.cal-item[data-id="task-lab"]')).toHaveCount(1);
    expect((await page.locator('.cal-item[data-id="task-lab"]').boundingBox()).height).toBeGreaterThanOrEqual(60);
  });
});

test('resizing a task smaller shrinks it while the pointer is still down', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    const study = page.locator('.cal-item[data-id="task-study"]');
    expect((await study.boundingBox()).height).toBeCloseTo(46, 0);
    const grip = await page.locator('.cal-item[data-id="task-study"] .cal-grip').boundingBox();
    const from = { x: grip.x + grip.width / 2, y: grip.y + grip.height / 2 };
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(from.x, from.y - 6, { steps: 2 });
    await page.mouse.move(from.x, from.y - 24, { steps: 6 });
    await expect.poll(async () => (await study.boundingBox()).height).toBeCloseTo(22, 0);
    await expect(study).toHaveClass(/inline/);
    await expect(study.locator('.cal-sub')).toHaveText(/1:00.*1:30/);
    expect(await patches(page, 'task-study')).toHaveLength(0);
    await page.mouse.up();
    await settled(page);
    expect(await task(page, 'task-study')).toMatchObject({ time: '13:00:00', end: '13:30:00' });
  });
});

test('today is a filled accent circle in the mini calendar, the week header and the month', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    const mini = page.locator('.cal-mini-day.today');
    await expect(mini).toHaveText('3');
    const fill = await page.locator('.cal-dayhead.today b').evaluate(el => getComputedStyle(el).backgroundColor);
    const look = await mini.evaluate(el => { const style = getComputedStyle(el); const box = el.getBoundingClientRect(); return { fill: style.backgroundColor, radius: style.borderRadius, square: Math.abs(box.width - box.height) < 1 }; });
    expect(look).toEqual({ fill, radius: '50%', square: true });
    await page.locator('.cal-views [data-view="month"]').click();
    await settled(page);
    expect(await page.locator('.cal-cell.today .cal-num').evaluate(el => getComputedStyle(el).backgroundColor)).toBe(fill);
    expect(await page.locator('.cal-mini-day.today').evaluate(el => getComputedStyle(el).backgroundColor)).toBe(fill);
  });
});

test('the hour grid reaches the bottom of the calendar in a tall window', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await page.evaluate(() => {
      const task = cal.data.tasks.find(task => task.id === 'task-lab');
      cal.data.tasks.push({ ...task, id: 'task-late', title: 'Late deadline', time: '23:59:00', end: null });
      drawCalendar();
    });
    const gap = await page.evaluate(() => {
      const scroll = document.querySelector('#cal-scroll');
      const col = document.querySelector('.cal-col');
      return { band: scroll.getBoundingClientRect().bottom - col.getBoundingClientRect().bottom };
    });
    expect(gap.band).toBeLessThanOrEqual(1);
  }, { height: 1500 });
});

test('enlarged task blocks do not change duration on a horizontal resize drag', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    for (const end of [null, '11:01:00', '11:05:00', '11:15:00', '11:17:00', '12:00:00']) {
      await page.evaluate(end => {
        for (const tasks of [cal.data.tasks, window.db.tasks]) tasks.find(task => task.id === 'task-lab').end = end;
        drawCalendar();
      }, end);
      const before = (await patches(page, 'task-lab')).length;
      const grip = await page.locator('.cal-item[data-id="task-lab"] .cal-grip').boundingBox();
      const from = { x: grip.x + grip.width / 2, y: grip.y + grip.height / 2 };
      await dragTo(page, from, { x: from.x + 12, y: from.y });
      expect((await task(page, 'task-lab')).end).toBe(end);
      expect((await patches(page, 'task-lab')).length).toBe(before);
    }
  });
});

test('larger month tasks keep every preview and the more action inside their day', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await page.evaluate(() => {
      const task = window.db.tasks.find(task => task.id === 'task-lab');
      for (let index = 0; index < 4; index++) window.db.tasks.push({ ...task, id: 'month-task-' + index, title: 'Additional homework ' + index });
      for (let week = 1; week < 6; week++) {
        for (let index = 0; index < 3; index++) window.db.tasks.push({ ...task, id: `busy-${week}-${index}`, date: S.dayKey(S.addDays(S.fromKey(task.date), week * 7)) });
      }

    });
    await page.locator('.cal-views [data-view="month"]').click();
    await settled(page);
    const cell = page.locator('.cal-cell[data-day="2026-11-05"]');
    await expect(cell.locator('.cal-item')).toHaveCount(3);
    const bounds = await cell.boundingBox();
    const more = await cell.getByRole('button', { name: '3 more' }).boundingBox();
    expect(more.y + more.height).toBeLessThanOrEqual(bounds.y + bounds.height);
    await page.locator('.cal-month-grid').evaluate(grid => { grid.scrollTop = 40; });
    await page.evaluate(() => drawCalendar());
    expect(await page.locator('.cal-month-grid').evaluate(grid => grid.scrollTop)).toBe(40);

    await cell.getByRole('button', { name: '3 more' }).click();
    await expect(page.locator('.cal-dayhead')).toHaveCount(1);
    await expect(page.locator('.cal-item[data-kind="task"]')).toHaveCount(5);
  });
});

test('a sideways deadline resize at the scroll edge still preserves its absent end', async ({ browser }) => {
  await calendarFixture(browser, async page => {
    await page.evaluate(() => {
      for (const tasks of [cal.data.tasks, window.db.tasks]) Object.assign(tasks.find(task => task.id === 'task-lab'), { time: '20:00:00', end: null });
      drawCalendar();
      const grip = document.querySelector('.cal-item[data-id="task-lab"] .cal-grip');
      const scroll = document.querySelector('#cal-scroll');
      scroll.scrollTop += grip.getBoundingClientRect().bottom - (scroll.getBoundingClientRect().bottom - 10);
    });
    const grip = await page.locator('.cal-item[data-id="task-lab"] .cal-grip').boundingBox();
    const from = { x: grip.x + grip.width / 2, y: grip.y + grip.height / 2 };
    await dragTo(page, from, { x: from.x + 12, y: from.y });
    expect((await task(page, 'task-lab')).end).toBe(null);
    expect(await patches(page, 'task-lab')).toHaveLength(0);
  });
});
