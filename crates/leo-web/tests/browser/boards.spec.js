const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss } = require('../page-source');

// The Boards page with its real CSS, icons, sheets and boards.js over an in-memory API
// that behaves like routes/boards.rs (order by position, same-name columns across boards).
async function boardsFixture(browser, run, { width = 1280, height = 800 } = {}) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  const core = read('app/core.js');
  const icons = core.slice(core.indexOf('const svg ='), core.indexOf('const recorder ='));
  const sheets = read('app/sheets.js');
  const sheetParts = sheets.slice(sheets.indexOf('function sheet('), sheets.indexOf('const QUICK_PLACES'));
  const events = read('app/events.js');
  const escapeCapture = events.slice(events.indexOf("window.addEventListener('keydown'"), events.indexOf("document.addEventListener('keydown'"));
  if (!escapeCapture.includes('preventDefault')) throw new Error('The capture listener moved out of events.js.');
  const fixture = `const $=(selector,root=document)=>root.querySelector(selector);
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const enc=encodeURIComponent, app=$('#app'), LEAVE_MS=150;
    ${icons}
    class Locked extends Error {}
    class Offline extends Error {}
    let seq=0,state={view:null};
    const chrome=()=>{}, skeleton=n=>'<div class="skeleton"></div>'.repeat(n);
    const empty=(icon,title,text)=>'<div class="empty">'+icon+'<h3>'+esc(title)+'</h3><p>'+esc(text)+'</p></div>';
    const stillMotion=()=>false;
    const requestId=()=>'r'+Math.random().toString(36).slice(2);
    window.toasts=[];
    function toast(message,{action,run,bad}={}){window.toasts.push({message,bad:Boolean(bad)});document.querySelector('.toast')?.remove();const el=document.createElement('div');el.className='toast';el.innerHTML='<span>'+esc(message)+'</span>'+(action?'<button>'+esc(action)+'</button>':'');if(action)el.querySelector('button').onclick=()=>{el.remove();run();};document.body.appendChild(el);}
    const fail=error=>window.errors.push(error.message);
    function go(hash,{replace=false}={}){if(location.hash===hash)return route();if(replace){history.replaceState(history.state,'',hash);route();}else location.hash=hash;}
    ${sheetParts}
    const db={boards:[
      {id:'bwork00001',name:'Work',columns:[{id:'ktodo00001',name:'To do'},{id:'kdoing0001',name:'In progress'},{id:'kdone00001',name:'Done'}],created_at:'2026-01-01T00:00:00Z'},
      {id:'bhome00001',name:'Home',columns:[{id:'khtodo0001',name:'To do'},{id:'khdoing001',name:'In progress'}],created_at:'2026-01-02T00:00:00Z'}],
      cards:[
        {id:'cessay0001',board:'bwork00001',column:'ktodo00001',title:'Write essay',subheading:'History · due soon',description:'Outline first',priority:'high'},
        {id:'cread00001',board:'bwork00001',column:'ktodo00001',title:'Read chapter 3',subheading:'',description:'',priority:''},
        {id:'cemail0001',board:'bwork00001',column:'kdoing0001',title:'Email tutor <b>now</b>',subheading:'',description:'',priority:'low'}],
      notes:[{id:'ncalc00001',title:'Calculus notes',directory:''},{id:'nhist00001',title:'History essay',directory:'History'},{id:'nodd000001',title:'Odd [title]',directory:''}]};
    let made=0;
    const gone={};
    const ranked=board=>db.cards.filter(c=>c.board===board).map((c,i)=>{const text=Array.from(c.description);return {...c,description:text.slice(0,240).join(''),description_more:text.length>240,rank:i+1};});
    const place=(card,after)=>{db.cards=db.cards.filter(c=>c.id!==card.id);const same=db.cards.filter(c=>c.board===card.board&&c.column===card.column);let at;if(after===undefined)at=same.length?db.cards.indexOf(same[same.length-1])+1:db.cards.length;else if(after===null)at=same.length?db.cards.indexOf(same[0]):db.cards.length;else at=db.cards.findIndex(c=>c.id===after)+1;db.cards.splice(at,0,card);};
    window.calls=[];
    window.pending=0;
    async function api(url,options={}){window.pending++;try{return await fakeApi(url,options);}finally{window.pending--;}}
    window.failNext=null;window.slow=0;
    async function fakeApi(url,{method='GET',body}={}){
      window.calls.push(method+' '+url);
      if(url==='/api/boards'&&method==='POST')(window.boardBodies||(window.boardBodies=[])).push(structuredClone(body));
      let early=null;
      if(window.early&&method==='GET'){const found=url.match(/^\\/api\\/boards\\/([^/]+)$/);const b=found&&db.boards.find(x=>x.id===found[1]);if(b)early={board:structuredClone(b),cards:ranked(b.id)};}
      await new Promise(r=>setTimeout(r,5+window.slow));
      const held=method!=='GET'&&window.holds&&window.holds.shift();if(held)await held;
      if(early)return early;
      if(window.failNext&&method!=='GET'){const e=new Error(window.failNext);window.failNext=null;throw e;}
      let m;
      if(url==='/api/boards'&&method==='GET')return {boards:db.boards.map(b=>({...b,cards:db.cards.filter(c=>c.board===b.id).length}))};
      if(url==='/api/boards'&&method==='POST'){const b={id:'bnew'+(++made)+'00000',name:body.name||'New board',columns:[{id:'kn'+made+'a000000',name:'To do'},{id:'kn'+made+'b000000',name:'In progress'},{id:'kn'+made+'c000000',name:'Done'}]};db.boards.push(b);return structuredClone(b);}
      if((m=url.match(/^\\/api\\/boards\\/([^/]+)\\/undelete$/))&&method==='POST'){const g=gone[m[1]];if(!g){const e=new Error("That board is no longer in leo's trash.");e.status=404;throw e;}delete gone[m[1]];db.boards.push(g.board);db.cards.push(...g.cards);return {board:g.board};}
      if(url==='/api/boards/restore'){db.boards.push(body.board);db.cards.push(...body.cards);return {board:body.board,cards:body.cards};}
      if((m=url.match(/^\\/api\\/boards\\/([^/]+)$/))){
        const b=db.boards.find(x=>x.id===m[1]);if(!b){const e=new Error('That board is not there any more.');e.status=404;throw e;}
        if(method==='GET')return {board:structuredClone(b),cards:ranked(b.id)};
        if(method==='PATCH'){if(body.name!==undefined)b.name=body.name;if(body.columns){const before=b.columns;b.columns=body.columns.map((c,i)=>({id:c.id||'kadd'+(++made)+'0000000',name:c.name}));const kept=new Set(b.columns.map(c=>c.id));for(const c of db.cards)if(c.board===b.id&&!kept.has(c.column))c.column=body.move_to||b.columns[0].id;}return structuredClone(b);}
        if(method==='DELETE'){gone[b.id]={board:b,cards:db.cards.filter(c=>c.board===b.id)};db.boards=db.boards.filter(x=>x!==b);db.cards=db.cards.filter(c=>c.board!==b.id);return {board:b,undo:{board:b.id}};}
      }
      if((m=url.match(/^\\/api\\/boards\\/([^/]+)\\/cards$/))){const card={id:body.id||'cmade'+(++made)+'00000',board:m[1],column:body.column||db.boards.find(b=>b.id===m[1]).columns[0].id,title:body.title,subheading:body.subheading||'',description:body.description||'',priority:body.priority||''};place(card,undefined);return {...card,rank:0};}
      if((m=url.match(/^\\/api\\/cards\\/([^/]+)\\/move$/))){const card=db.cards.find(c=>c.id===m[1]);const from=db.boards.find(b=>b.id===card.board);if(body.board&&body.board!==card.board){const to=db.boards.find(b=>b.id===body.board);const name=from.columns.find(c=>c.id===card.column)?.name;card.board=to.id;card.column=body.column||(to.columns.find(c=>c.name===name)||to.columns[0]).id;}else if(body.column)card.column=body.column;place(card,'after' in body?body.after:undefined);return {...card,rank:0};}
      if((m=url.match(/^\\/api\\/cards\\/([^/]+)\\/undelete$/))){const card=window.goneCards[m[1]];place(card,undefined);return {...card,rank:0};}
      if((m=url.match(/^\\/api\\/cards\\/([^/]+)$/))){const card=db.cards.find(c=>c.id===m[1]);if(method==='GET'){if(window.failCardRead){const e=new Error('Leo could not open the card.');e.status=500;throw e;}return {...card,rank:0};}if(method==='DELETE'){(window.goneCards||(window.goneCards={}))[card.id]=card;db.cards=db.cards.filter(c=>c!==card);return {...card,rank:0};}Object.assign(card,body);return {...card,rank:0};}
      if(url.startsWith('/api/notes?')||url.startsWith('/api/search?')){const q=(new URLSearchParams(url.split('?')[1]).get('q')||'').toLowerCase();return db.notes.filter(n=>n.title.toLowerCase().includes(q)).map(n=>({...n}));}
      throw new Error('No fake for '+method+' '+url);
    }
    ${core.slice(core.indexOf('const MOTION_MS'), core.indexOf('function stillMotion'))}
    ${core.slice(core.indexOf('const moving'), core.indexOf('function rel('))}
    ${read('app/motion.js')}
    ${read('app/popup-menu.js')}
    ${read('app/settings-navigation.js')}
    ${read('markdown.js')}
    ${read('editing.js')}
    ${read('doc.js')}
    ${read('chat.js')}
    const leoDoc=window.leoDoc;
    const SEARCH_STEP=60;const noteHash=id=>'#/n/'+enc(id);
    ${sheets.slice(sheets.indexOf('async function openTitle('), sheets.indexOf('function openSearch('))}
    ${read('app/boards.js')}
    const actions={'kanban-new-board':()=>kanbanNewBoard(),'kanban-board-menu':kanbanBoardMenu,'kanban-column-menu':kanbanColumnMenu,'kanban-add-card':el=>kanbanComposer(el.dataset.col),'kanban-add-column':()=>kanbanAddColumn(),'kanban-open-card':el=>kanbanCardPanel(el.dataset.card),close:()=>closeSheet(),'open-title':el=>openTitle(el.dataset.title||'')};
    document.addEventListener('click',e=>{const el=e.target.closest('[data-action]');if(!el||!actions[el.dataset.action])return;e.preventDefault();Promise.resolve(actions[el.dataset.action](el)).catch(fail);});
    ${escapeCapture}
    document.addEventListener('keydown',e=>{if(e.key==='Escape'&&$('.scrim'))closeSheet();});
    async function route(){closeSheet(true);const m=location.hash.match(/^#\\/boards\\/?(.*)$/);if(!m){leaveBoards();state={view:'other'};app.innerHTML='<p id="elsewhere">Elsewhere</p>';return;}await showBoards(decodeURIComponent(m[1]));}
    window.addEventListener('hashchange',()=>route().catch(fail));
    window.errors=[];window.db=db;
    if(!location.hash)history.replaceState(null,'','#/boards');
    route().catch(fail);`;
  const server = http.createServer((req, res) => {
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(fixture); }
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); return res.end(css); }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body class="on-boards"><header class="bar" id="bar"><div class="bar-row">Leo</div></header><main id="app"></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({ viewport: { width, height } });
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto('http://127.0.0.1:' + server.address().port + '/#/boards');
    await expect(page.locator('.kanban-col')).toHaveCount(3);
    await run(page);
    expect(errors).toEqual([]);
    expect(await page.evaluate(() => window.errors)).toEqual([]);
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
}

async function settled(page) {
  await expect.poll(() => page.evaluate(() => window.pending)).toBe(0);
}
const titles = (page, column) => page.locator(`.kanban-cards[data-col="${column}"] .kanban-card-title`).allTextContents();
const saved = (page, board) => page.evaluate(id => window.db.cards.filter(c => c.board === id).map(c => `${c.column}:${c.title}`), board);

async function drag(page, from, to, { below = false, left = false } = {}) {
  const a = await from.boundingBox();
  const b = await to.boundingBox();
  await page.mouse.move(a.x + a.width / 2, a.y + Math.min(14, a.height / 2));
  await page.mouse.down();
  await page.mouse.move(a.x + a.width / 2 + 10, a.y + 20, { steps: 3 });
  const y = below ? b.y + b.height - 4 : b.y + Math.min(8, b.height / 4);
  const x = left ? b.x + 8 : b.x + b.width / 2;
  await page.mouse.move(x, y, { steps: 8 });
  await page.mouse.move(x, y);
  await page.mouse.up();
  await settled(page);
}

test('a board shows columns of cards, and cards drag between columns and onto another board', async ({ browser }, testInfo) => {
  await boardsFixture(browser, async page => {
    for (const theme of ['light', 'dark']) {
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      await page.screenshot({ path: testInfo.outputPath(`boards-${theme}.png`) });
    }
    await page.evaluate(() => { document.documentElement.dataset.theme = 'light'; });
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Work');
    await expect(page.locator('.kanban-col-name').first()).toHaveValue('To do');
    expect(await titles(page, 'ktodo00001')).toEqual(['Write essay', 'Read chapter 3']);
    await expect(page.locator('.kanban-card-sub').first()).toHaveText('History · due soon');
    await expect(page.locator('[data-card="cemail0001"] .kanban-card-title')).toHaveText('Email tutor <b>now</b>');
    await expect(page.locator('[data-card="cessay0001"] .kanban-priority.high')).toBeVisible();
    await expect(page.locator('[data-count-col="ktodo00001"]')).toHaveText('2');
    await expect(page.locator('.kanban-head > .kanban-tabs'), 'the tabs are the one header row').toHaveCount(1);
    await expect(page.locator('.kanban-head [data-action="kanban-board-menu"]')).toHaveCount(1);
    await expect(page.locator('.kanban-head [data-action="kanban-new-board"]')).toHaveCount(1);
    await expect(page.locator('text=Drag a card here'), 'an empty lane holds no drop target').toHaveCount(0);
    await expect(page.locator('.kanban-title'), 'the board name shows once, as its tab').toHaveCount(1);
    const tall = async (id) => (await page.locator(`[data-card="${id}"]`).boundingBox()).height;
    expect(await tall('cread00001'), 'a title-only card hugs its title').toBeLessThan(await tall('cessay0001') - 30);

    // Into another column, below the card already there.
    await drag(page, page.locator('[data-card="cread00001"]'), page.locator('[data-card="cemail0001"]'), { below: true });
    await expect.poll(() => saved(page, 'bwork00001')).toEqual(['ktodo00001:Write essay', 'kdoing0001:Email tutor <b>now</b>', 'kdoing0001:Read chapter 3']);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Read chapter 3']);
    await expect(page.locator('[data-count-col="kdoing0001"]')).toHaveText('2');
    await expect(page.locator('.kanban-ghost')).toHaveCount(0);
    await expect(page.locator('.scrim')).toHaveCount(0);

    // Into an empty column.
    await drag(page, page.locator('[data-card="cessay0001"]'), page.locator('.kanban-cards[data-col="kdone00001"]'));
    await expect.poll(() => titles(page, 'kdone00001')).toEqual(['Write essay']);

    // Escape puts a lifted card back where it was.
    await expect(page.locator('[data-card="cread00001"]')).toBeVisible();
    const card = await page.locator('[data-card="cread00001"]').boundingBox();
    await page.mouse.move(card.x + 20, card.y + 10);
    await page.mouse.down();
    await page.mouse.move(card.x - 300, card.y + 10, { steps: 6 });
    await expect(page.locator('.kanban-ghost')).toHaveCount(1);
    await page.keyboard.press('Escape');
    await page.mouse.up();
    await expect(page.locator('.kanban-ghost')).toHaveCount(0);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Read chapter 3']);

    // Onto the Home tab: the card keeps its In progress column there.
    await drag(page, page.locator('[data-card="cread00001"]'), page.locator('.kanban-tab[data-kanban-drop="bhome00001"]'));
    await expect(page.locator('.toast')).toContainText('Moved “Read chapter 3” to Home');
    await expect.poll(() => saved(page, 'bhome00001')).toEqual(['khdoing001:Read chapter 3']);
    await expect(page.locator('[data-card="cread00001"]')).toHaveCount(0);
    await expect(page.locator('.kanban-tabs .kanban-count'), 'board tabs show names only').toHaveCount(0);
    await page.locator('.toast button').click();
    await expect(page.locator('[data-card="cread00001"]')).toHaveCount(1);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Read chapter 3']);
  });
});

test('priority is one to three coloured pencil strokes before the title, with the word kept as hidden text', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const mark = id => page.locator(`[data-card="${id}"] .kanban-priority`);
    await expect(mark('cessay0001').locator('path')).toHaveCount(3);
    await expect(mark('cemail0001').locator('path')).toHaveCount(1);
    await expect(mark('cread00001')).toHaveCount(0);
    await expect(mark('cessay0001')).toHaveAttribute('title', 'High priority');
    await expect(mark('cessay0001').locator('.kanban-priority-word')).toHaveText('High priority');
    await expect(page.locator('[data-card="cessay0001"]')).toContainText('High priority');
    expect(await titles(page, 'ktodo00001')).toEqual(['Write essay', 'Read chapter 3']);
    const look = await page.evaluate(() => {
      const card = document.querySelector('[data-card="cessay0001"]');
      const strokes = card.querySelector('.kanban-priority svg').getBoundingClientRect();
      const title = card.querySelector('.kanban-card-title');
      return { indent: getComputedStyle(title).textIndent, plain: getComputedStyle(document.querySelector('[data-card="cread00001"] .kanban-card-title')).textIndent, strokesRight: strokes.right, titleLeft: title.getBoundingClientRect().left, height: strokes.height, color: getComputedStyle(card.querySelector('.kanban-priority')).color, ink: getComputedStyle(title).color, low: getComputedStyle(document.querySelector('[data-card="cemail0001"] .kanban-priority')).color };
    });
    const levels = await page.evaluate(() => {
      const probe = document.createElement('span');
      document.body.append(probe);
      const named = name => { probe.style.color = `var(--${name})`; return getComputedStyle(probe).color; };
      const level = (name) => {
        const mark = document.querySelector('[data-card="cemail0001"] .kanban-priority').cloneNode(true);
        mark.className = `kanban-priority ${name}`;
        probe.append(mark);
        return getComputedStyle(mark).color;
      };
      const result = { low: level('low'), medium: level('medium'), high: level('high'), hueLow: named('hue-0'), hueMedium: named('hue-amber'), hueHigh: named('hue-red') };
      probe.remove();
      return result;
    });
    expect(levels.low, 'low is blue').toBe(levels.hueLow);
    expect(levels.medium, 'medium is amber').toBe(levels.hueMedium);
    expect(levels.high, 'high is red').toBe(levels.hueHigh);
    expect(new Set([levels.low, levels.medium, levels.high]).size, 'each level has its own colour').toBe(3);
    expect(look.indent).toBe('24px');
    expect(look.plain).toBe('0px');
    expect(look.strokesRight).toBeLessThan(look.titleLeft + 24);
    expect(look.height).toBe(14);
    expect(look.color, 'high priority is a hue, not ink').not.toBe(look.ink);
    expect(look.color).toBe(levels.high);
    expect(look.low).toBe(levels.low);
  });
});

test('a dragged card tilts while lifted and lands flat, never under Reduced Motion', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const lifted = async () => {
      const card = await page.locator('[data-card="cread00001"]').boundingBox();
      await page.mouse.move(card.x + 20, card.y + 10);
      await page.mouse.down();
      await page.mouse.move(card.x + 120, card.y + 60, { steps: 6 });
      await expect(page.locator('.kanban-ghost')).toHaveCount(1);
    };
    const tilt = () => page.evaluate(() => getComputedStyle(document.querySelector('.kanban-ghost')).getPropertyValue('--tilt'));
    await lifted();
    await expect.poll(tilt).toBe('1.5deg');
    await page.mouse.up();
    await expect(page.locator('.kanban-ghost')).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => document.getAnimations().length)).toBe(0);
    expect(await page.evaluate(() => getComputedStyle(document.querySelector('[data-card="cread00001"]')).transform)).toBe('none');
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await lifted();
    expect(await tilt()).toBe('0deg');
    await page.mouse.up();
    await expect(page.locator('.kanban-ghost')).toHaveCount(0);
  });
});

test('cards are added, opened, edited, moved by keyboard and deleted with undo', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-action="kanban-add-card"][data-col="ktodo00001"]').click();
    const composer = page.locator('.kanban-composer textarea');
    await expect(composer).toBeFocused();
    await composer.fill('Plan the trip');
    await composer.press('Enter');
    await composer.fill('Pack bags');
    await composer.press('Enter');
    await expect.poll(() => titles(page, 'ktodo00001')).toEqual(['Write essay', 'Read chapter 3', 'Plan the trip', 'Pack bags']);
    await expect(composer).toBeFocused();
    await composer.press('Escape');
    await expect(page.locator('.kanban-composer')).toHaveCount(0);
    await expect(page.locator('[data-count-col="ktodo00001"]')).toHaveText('4');

    await page.locator('[data-card="cessay0001"]').click();
    const sheet = page.locator('#kanban-panel .kanban-sheet');
    await expect(page.locator('.scrim')).toHaveCount(0);
    await expect(page.locator('[data-card="cessay0001"]')).toHaveClass(/\bopen\b/);
    await expect(sheet.locator('#kanban-card-title')).toHaveValue('Write essay');
    await expect(sheet.locator('#kanban-card-description')).toHaveText('Outline first');
    await sheet.locator('#kanban-card-sub').fill('History · draft 2');
    await sheet.locator('#kanban-card-description .blk').click();
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await page.keyboard.type('Then write');
    await sheet.locator('[data-priority="medium"]').click();
    await expect(sheet.locator('[data-priority="medium"]')).toHaveAttribute('aria-pressed', 'true');
    await sheet.locator('#kanban-card-column').selectOption({ label: 'In progress' });
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001'))).toMatchObject({ column: 'kdoing0001', subheading: 'History · draft 2', description: 'Outline first\nThen write', priority: 'medium' });
    await page.keyboard.press('Escape');
    await expect(page.locator('#kanban-panel')).toBeHidden();
    await expect(page.locator('[data-card="cessay0001"]')).toBeFocused();
    await expect(page.locator('[data-card="cessay0001"] .kanban-card-sub')).toHaveText('History · draft 2');
    await expect(page.locator('[data-card="cessay0001"] .kanban-priority.medium')).toBeVisible();

    // Alt + arrows move the focused card.
    await page.locator('[data-card="cessay0001"]').focus();
    await page.keyboard.press('Alt+ArrowRight');
    await expect.poll(() => titles(page, 'kdone00001')).toEqual(['Write essay']);
    await expect(page.locator('[data-card="cessay0001"]')).toBeFocused();
    await page.keyboard.press('Alt+ArrowLeft');
    await expect.poll(() => titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Write essay']);
    await page.keyboard.press('Alt+ArrowUp');
    await expect.poll(() => titles(page, 'kdoing0001')).toEqual(['Write essay', 'Email tutor <b>now</b>']);
    await page.keyboard.press('Enter');
    await expect(page.locator('#kanban-panel .kanban-sheet')).toBeVisible();

    await page.locator('#kanban-panel [data-kanban="delete"]').click();
    await expect(page.locator('#kanban-panel')).toBeHidden();
    await expect(page.locator('[data-card="cessay0001"]')).toHaveCount(0);
    await expect(page.locator('.toast')).toContainText('Deleted “Write essay”');
    await page.locator('.toast button').click();
    await expect(page.locator('[data-card="cessay0001"]')).toHaveCount(1);
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001').description)).toBe('Outline first\nThen write');
  });
});

test('a new card lands in full ink and settles by colour alone, without restarting on a redraw', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-action="kanban-add-card"][data-col="ktodo00001"]').click();
    const composer = page.locator('.kanban-composer textarea');
    await composer.fill('Fresh ink');
    await composer.press('Enter');
    const card = page.locator('.kanban-card', { hasText: 'Fresh ink' });
    await expect(card).toHaveCount(1);
    const ink = () => card.evaluate(el => {
      const run = el.getAnimations().find(a => a.id === 'ink');
      if (!run) return null;
      const props = Object.keys(run.effect.getKeyframes()[0]).filter(k => !['offset', 'computedOffset', 'easing', 'composite'].includes(k)).sort();
      return { at: run.effect.getComputedTiming().localTime - run.effect.getTiming().delay, props, ms: run.effect.getTiming().duration };
    });
    const first = await ink();
    expect(first).toMatchObject({ props: ['backgroundColor', 'color'], ms: 600 });
    await page.evaluate(() => drawKanban());
    const again = await ink();
    if (again) expect(again.at).toBeGreaterThanOrEqual(first.at);
    await expect.poll(ink).toBeNull();
  });
});

test('a card Felix made arrives in his blue and settles to ink by colour alone', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const card = page.locator('.kanban-card').first();
    await expect(card).toBeVisible();
    const id = await card.getAttribute('data-card');
    await page.evaluate((made) => { inkLand(made, true); drawKanban(); }, id);
    const ink = () => card.evaluate(el => {
      const run = el.getAnimations().find(a => a.id === 'ink');
      if (!run) return null;
      const from = run.effect.getKeyframes()[0];
      const probe = document.createElement('i');
      probe.style.color = 'var(--felix)';
      el.append(probe);
      const felix = getComputedStyle(probe).color;
      probe.remove();
      const props = Object.keys(from).filter(k => !['offset', 'computedOffset', 'easing', 'composite'].includes(k)).sort();
      return { props, felix: from.color === 'var(--felix)' || from.color === felix, ms: run.effect.getTiming().duration };
    });
    expect(await ink()).toEqual({ props: ['color'], felix: true, ms: 600 });
    await expect.poll(ink).toBeNull();
  });
});

test('a card\'s details render as Markdown like a note, toggle checkboxes, edit by line and stay plain Markdown', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const markdown = '## Plan\n\nSome **bold** and [a link](https://example.com)\n\n- [ ] first\n\n`code`\n\n<img src=x onerror="window.ran=1"> [bad](javascript:window.ran=2)';
    await page.evaluate(text => { window.db.cards.find(c => c.id === 'cessay0001').description = text; }, markdown);
    await redraw(page);
    await page.locator('[data-card="cessay0001"]').click();
    const details = page.locator('#kanban-card-description');
    await expect(details.locator('h3')).toHaveText('Plan');
    await expect(details.locator('strong')).toHaveText('bold');
    await expect(details.locator('a[href="https://example.com"]')).toHaveText('a link');
    await expect(details.locator('code')).toHaveText('code');
    await expect(details.locator('img, script, a[href^="javascript"]')).toHaveCount(0);
    expect(await page.evaluate(() => window.ran)).toBeUndefined();

    await details.locator('input[type=checkbox]').check();
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001').description)).toContain('- [x] first');

    await details.locator('.blk').first().click();
    await expect(details.locator('textarea.line-edit')).toHaveValue('## Plan');
    await page.keyboard.press('End');
    await page.keyboard.type(' today');
    await page.keyboard.press('Escape');
    await expect(page.locator('#kanban-panel')).toBeVisible();
    await expect(details.locator('h3')).toHaveText('Plan today');
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001').description)).toMatch(/^## Plan today\n/);

    await page.keyboard.press('Escape');
    await expect(page.locator('#kanban-panel')).toBeHidden();
    await page.locator('[data-card="cessay0001"]').click();
    await expect(details.locator('h3')).toHaveText('Plan today');
    await expect(details.locator('input[type=checkbox]')).toBeChecked();

    await page.locator('[data-card="cread00001"]').click();
    await expect(details).toHaveText('Add details, steps or links…');
    await details.click();
    await page.keyboard.type('Notes');
    await page.locator('#kanban-panel [data-kanban="close"]').click();
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description)).toBe('Notes');
  });
});

test('typing @ in a card description offers notes, Escape closes the list first, and the picked note is a link that opens it', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-card="cread00001"]').click();
    const details = page.locator('#kanban-card-description');
    const menu = page.locator('.popup-menu[aria-label="Link a note"]');
    await details.click();
    await page.keyboard.type('See @');
    await expect(menu.getByRole('option')).toHaveText(['Calculus notes', 'History essay']);
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(details.locator('textarea.line-edit')).toBeFocused();
    await expect(details.locator('textarea.line-edit')).toHaveValue('See @');
    await page.keyboard.press('Backspace');
    await page.keyboard.type('@');
    await expect(menu.getByRole('option')).toHaveCount(2);
    await page.keyboard.press('ArrowDown');
    await expect(menu.getByRole('option').nth(1)).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Enter');
    await expect(menu).toHaveCount(0);
    await expect(details.locator('textarea.line-edit')).toHaveValue('See [[History essay]]');
    await page.keyboard.press('Escape');
    await expect(details.locator('a.wiki-link')).toHaveText('History essay');
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description)).toBe('See [[History essay]]');
    await details.locator('a.wiki-link').click();
    await expect(page.locator('#elsewhere')).toBeVisible();
    expect(await page.evaluate(() => location.hash)).toBe('#/n/nhist00001');
  });
});

test('no boards shows the drawn empty state with its facts and the New board button', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.evaluate(() => { window.db.boards = []; window.db.cards = []; location.hash = '#/elsewhere'; });
    await expect(page.locator('#elsewhere')).toBeVisible();
    await page.evaluate(() => { location.hash = '#/boards'; });
    const none = page.locator('.kanban-none');
    await expect(none.locator('svg.drawn-board')).toBeVisible();
    await expect(none.locator('h3')).toHaveText('No boards yet');
    await expect(none.locator('[data-action="kanban-new-board"]')).toBeVisible();
    expect(await none.locator('svg.drawn-board').evaluate(el => [el.getAttribute('fill'), el.getAttribute('stroke')])).toEqual(['none', 'currentColor']);
  });
});

test('a board create that is retried sends the same request id', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-action="kanban-new-board"]').click();
    await page.locator('#kanban-board-name').fill('Job search');
    await page.evaluate(() => { window.failNext = 'Leo could not make the board.'; });
    await page.locator('#kanban-board-make').click();
    await expect(page.locator('.toast')).toContainText('Leo could not make the board.');
    await expect(page.locator('#kanban-board-make')).toBeEnabled();
    await page.locator('#kanban-board-make').click();
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Job search');
    const bodies = await page.evaluate(() => window.boardBodies);
    expect(bodies).toHaveLength(2);
    expect(bodies[0].request).toBeTruthy();
    expect(bodies[1].request).toBe(bodies[0].request);
    expect(bodies[1].name).toBe('Job search');
  });
});

test('Escape and Close each leave a card at one press', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const panel = page.locator('#kanban-panel');
    await page.locator('[data-card="cessay0001"]').click();
    await expect(page.locator('#kanban-card-title')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(panel).toBeHidden();
    await expect(page.locator('[data-card="cessay0001"]')).toBeFocused();

    await page.locator('[data-card="cessay0001"]').click();
    await page.locator('#kanban-card-sub').click();
    await page.keyboard.press('Escape');
    await expect(panel).toBeHidden();

    await page.locator('[data-card="cessay0001"]').click();
    await page.locator('#kanban-card-description .blk').click();
    await expect(page.locator('#kanban-card-description textarea.line-edit')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(panel).toBeVisible();
    await expect(page.locator('#kanban-card-description textarea.line-edit')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await expect(panel).toBeHidden();

    await page.locator('[data-card="cessay0001"]').click();
    await page.locator('#kanban-panel [data-kanban="close"]').click();
    await expect(panel).toBeHidden();
  });
});

test('a long description is saved whole', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const panel = page.locator('#kanban-panel');
    const description = () => page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description);
    await page.locator('[data-card="cread00001"]').click();
    await page.locator('#kanban-card-description').click();
    await page.keyboard.insertText('x'.repeat(8010));
    await expect(page.locator('#kanban-card-problem')).toBeHidden();
    await page.locator('#kanban-panel [data-kanban="close"]').click();
    await expect(panel).toBeHidden();
    expect(await description()).toBe('x'.repeat(8010));
  });
});

test('a failed save keeps the card open with its text', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const panel = page.locator('#kanban-panel');
    await page.locator('[data-card="cread00001"]').click();
    await page.locator('#kanban-card-description').click();
    await page.keyboard.type('Keep me');
    await page.evaluate(() => { window.failNext = 'Leo could not save the card.'; });
    await page.keyboard.press('Escape');
    await expect(page.locator('#kanban-card-problem')).toHaveText('Leo could not save the card.');
    await page.evaluate(() => { window.failNext = 'Leo still could not save the card.'; });
    await page.locator('#kanban-panel [data-kanban="close"]').click();
    await expect(page.locator('#kanban-card-problem')).toHaveText('Leo still could not save the card.');
    await expect(panel).toBeVisible();
    await expect(page.locator('#kanban-card-description')).toHaveText('Keep me');
    await page.keyboard.press('Escape');
    await expect(panel).toBeHidden();
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description)).toBe('Keep me');
  });
});

test('a keyboard move shows at once and goes back with the error when the save fails', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.evaluate(() => { window.slow = 600; });
    await page.locator('[data-card="cessay0001"]').focus();
    await page.keyboard.press('Alt+ArrowRight');
    await expect.poll(() => titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Write essay']);
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001').column)).toBe('ktodo00001');
    await expect(page.locator('[data-card="cessay0001"]')).toBeFocused();
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001').column)).toBe('kdoing0001');
    await page.evaluate(() => { window.slow = 600; window.failNext = 'Leo could not move the card.'; });
    await page.keyboard.press('Alt+ArrowRight');
    await expect.poll(() => titles(page, 'kdone00001')).toEqual(['Write essay']);
    await expect.poll(() => titles(page, 'kdone00001')).toEqual([]);
    await expect.poll(() => titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Write essay']);
    expect(await page.evaluate(() => window.toasts.at(-1))).toEqual({ message: 'Leo could not move the card.', bad: true });
  });
});

test('a keyboard move the server agrees with redraws the board once', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.evaluate(() => {
      const draw = window.drawKanban;
      window.drawn = 0;
      window.drawKanban = function () { window.drawn++; return draw.apply(this, arguments); };
      window.fetched = window.calls.filter(c => c === 'GET /api/boards/bwork00001').length;
    });
    await page.locator('[data-card="cessay0001"]').focus();
    await page.keyboard.press('Alt+ArrowRight');
    await expect.poll(() => page.evaluate(() => window.calls.filter(c => c === 'GET /api/boards/bwork00001').length > window.fetched)).toBe(true);
    await settled(page);
    await page.waitForTimeout(200);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Write essay']);
    expect(await page.evaluate(() => window.drawn)).toBe(1);
  });
});

test('a move whose answer renames the card shows the new title', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.evaluate(() => {
      const write = window.kanbanWrite;
      window.kanbanWrite = async function (path, options) {
        const answer = await write.call(this, path, options);
        if (!/\/move$/.test(path)) return answer;
        window.db.cards.find(c => c.id === 'cessay0001').title = 'Write the essay';
        return { ...answer, title: 'Write the essay' };
      };
    });
    await page.locator('[data-card="cessay0001"]').focus();
    await page.keyboard.press('Alt+ArrowRight');
    await expect.poll(() => titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Write the essay']);
    await settled(page);
  });
});

test('a priority and a delete show before leo answers and come back when the save fails', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-card="cread00001"]').click();
    const chips = page.locator('#kanban-panel .kanban-priorities');
    await expect(chips.locator('[data-priority=""]')).toHaveAttribute('aria-pressed', 'true');
    await page.evaluate(() => { window.slow = 800; window.failNext = 'Leo could not save the card.'; });
    await chips.locator('[data-priority="high"]').click();
    expect(await page.evaluate(() => [document.querySelector('#kanban-panel [data-priority="high"]').getAttribute('aria-pressed'), window.pending > 0])).toEqual(['true', true]);
    await expect(chips.locator('[data-priority=""]')).toHaveAttribute('aria-pressed', 'true');
    await expect(chips.locator('[data-priority="high"]')).toHaveAttribute('aria-pressed', 'false');
    expect(await page.evaluate(() => window.toasts.at(-1))).toEqual({ message: 'Leo could not save the card.', bad: true });
    await settled(page);

    await page.evaluate(() => { window.failNext = 'Leo could not delete the card.'; });
    await page.locator('#kanban-panel [data-kanban="delete"]').click();
    expect(await page.evaluate(() => [document.querySelectorAll('[data-card="cread00001"]').length, window.pending > 0])).toEqual([0, true]);
    await expect(page.locator('[data-card="cread00001"]')).toHaveCount(1);
    expect(await titles(page, 'ktodo00001')).toEqual(['Write essay', 'Read chapter 3']);
    await expect.poll(() => page.evaluate(() => window.errors)).toContain('Leo could not delete the card.');
    await settled(page);
    expect(await page.evaluate(() => window.db.cards.some(c => c.id === 'cread00001'))).toBe(true);
    await page.evaluate(() => { window.errors.length = 0; });
  });
});

test('two quick moves keep the second when the first fails after it was sent, and the board ends as the server has it', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.evaluate(() => {
      window.failNext = 'Leo could not move the card.';
      window.release = [];
      window.holds = [0, 1].map(() => new Promise(done => window.release.push(done)));
    });
    const card = page.locator('[data-card="cessay0001"]');
    await card.focus();
    await page.keyboard.press('Alt+ArrowRight');
    await expect.poll(() => titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Write essay']);
    await page.keyboard.press('Alt+ArrowRight');
    await expect.poll(() => titles(page, 'kdone00001')).toEqual(['Write essay']);
    await expect.poll(() => page.evaluate(() => window.holds.length)).toBe(0);
    await page.evaluate(() => window.release[0]());
    await expect.poll(() => page.evaluate(() => window.toasts.at(-1))).toEqual({ message: 'Leo could not move the card.', bad: true });
    expect(await titles(page, 'kdone00001')).toEqual(['Write essay']);
    expect(await page.evaluate(() => window.pending)).toBe(1);
    await page.evaluate(() => window.release[1]());
    await expect.poll(() => page.evaluate(() => window.pending)).toBe(0);
    await page.waitForTimeout(400);
    expect(await titles(page, 'kdone00001')).toEqual(['Write essay']);
    expect(await titles(page, 'ktodo00001')).toEqual(['Read chapter 3']);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>']);
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001').column)).toBe('kdone00001');
    expect(await page.evaluate(() => window.calls.filter(c => c === 'GET /api/boards/bwork00001').length)).toBeGreaterThan(0);
  });
});

test('columns are renamed, added, reordered and deleted, and boards are made and deleted', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const name = page.locator('[data-kanban-name="kdoing0001"]');
    await name.fill('Doing');
    await name.press('Enter');
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns[1].name)).toBe('Doing');

    // Drag the Done column by its header to the front.
    await drag(page, page.locator('.kanban-col[data-col="kdone00001"] .kanban-col-name'), page.locator('.kanban-col[data-col="ktodo00001"] .kanban-col-head'), { left: true });
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns.map(c => c.name))).toEqual(['Done', 'To do', 'Doing']);

    await page.locator('[data-action="kanban-add-column"]').click();
    await expect(page.locator('.kanban-col')).toHaveCount(4);
    const fresh = page.locator('.kanban-col').last().locator('.kanban-col-name');
    await expect(fresh).toBeFocused();
    await fresh.fill('Waiting');
    await fresh.press('Enter');
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns.map(c => c.name))).toEqual(['Done', 'To do', 'Doing', 'Waiting']);

    // Deleting To do moves its two cards to the column picked.
    await page.locator('[data-action="kanban-column-menu"][data-col="ktodo00001"]').click();
    await page.getByRole('menuitem', { name: 'Delete column' }).click();
    await expect(page.locator('.popup-menu p')).toHaveText('Move its 2 cards to');
    await page.getByRole('menuitem', { name: 'Waiting' }).click();
    await expect(page.locator('.kanban-col')).toHaveCount(3);
    await expect.poll(() => page.evaluate(() => window.db.cards.filter(c => c.board === 'bwork00001' && c.column !== 'kdoing0001').map(c => c.title))).toEqual(['Write essay', 'Read chapter 3']);

    await page.locator('[data-action="kanban-new-board"]').click();
    await page.locator('#kanban-board-name').fill('Job search');
    await page.locator('#kanban-board-name').press('Enter');
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Job search');
    await expect(page.locator('.kanban-col-name')).toHaveCount(3);
    await expect(page.locator('.kanban-card')).toHaveCount(0);

    await page.locator('[data-action="kanban-board-menu"]').click();
    await page.getByRole('menuitem', { name: 'Delete board' }).click();
    await page.getByRole('menuitem', { name: 'Delete the board' }).click();
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Work');
    await expect(page.locator('.kanban-tab')).toHaveCount(2);
    await page.locator('.toast button').click();
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Job search');
  });
});

test('a board fetched before a later change cannot overwrite it', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.evaluate(() => { window.slow = 120; window.early = true; });
    await page.locator('.kanban-col').nth(1).hover();
    await page.locator('[data-action="kanban-column-menu"][data-col="kdoing0001"]').click();
    await page.getByRole('menuitem', { name: 'Move left' }).click();
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns[0].id), { intervals: [10] }).toBe('kdoing0001');
    await page.evaluate(() => { window.slow = 0; });
    await page.locator('[data-action="kanban-add-column"]').click();
    await settled(page);
    await expect(page.locator('.kanban-col')).toHaveCount(4);
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns.length)).toBe(4);
    await expect(page.locator('.kanban-col').first().locator('.kanban-col-name')).toHaveValue('In progress');
  });
});

test('column and board options open as a small menu by their button, not a sheet', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const button = page.locator('[data-action="kanban-column-menu"][data-col="kdoing0001"]');
    await page.locator('.kanban-col').nth(1).hover();
    await button.click();
    const menu = page.getByRole('menu', { name: 'Options for In progress' });
    await expect(menu).toBeVisible();
    await expect(page.locator('.scrim')).toHaveCount(0);
    await expect(button).toHaveAttribute('aria-expanded', 'true');
    const [box, anchor] = [await menu.boundingBox(), await button.boundingBox()];
    expect(Math.abs(box.y - anchor.y - anchor.height)).toBeLessThan(8);
    await page.keyboard.press('ArrowDown');
    await expect(page.getByRole('menuitem', { name: 'Move left' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns[0].id)).toBe('kdoing0001');
    await expect(menu).toHaveCount(0);

    await page.locator('[data-action="kanban-board-menu"]').click();
    await expect(page.getByRole('menu', { name: 'Board options' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.locator('.popup-menu')).toHaveCount(0);
    await expect(page.locator('[data-action="kanban-board-menu"]')).toBeFocused();
    await page.locator('[data-action="kanban-board-menu"]').click();
    await page.mouse.click(5, 5);
    await expect(page.locator('.popup-menu')).toHaveCount(0);
  });
});

test('a new card can take a priority, which resets for the next card and survives a redraw', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-action="kanban-add-card"][data-col="ktodo00001"]').click();
    const composer = page.locator('.kanban-composer');
    const text = composer.locator('textarea');
    const box = await composer.boundingBox();
    const chips = await composer.locator('.segment').evaluateAll(els => els.map(el => el.scrollWidth <= el.clientWidth));
    expect(chips, `every label fits in a ${box.width}px composer`).toEqual([true, true, true, true]);
    await composer.getByRole('button', { name: 'High' }).click();
    await expect(composer).toBeVisible();
    await expect(text).toBeFocused();
    await text.fill('Urgent one');
    await page.evaluate(() => kanbanRefresh());
    await settled(page);
    await expect(composer.getByRole('button', { name: 'High' })).toHaveAttribute('aria-pressed', 'true');
    await text.press('Enter');
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.title === 'Urgent one')?.priority)).toBe('high');
    await expect(page.locator('.kanban-card', { hasText: 'Urgent one' }).locator('.kanban-priority.high')).toHaveCount(1);
    await expect(composer.getByRole('button', { name: 'None' })).toHaveAttribute('aria-pressed', 'true');
    await text.fill('Plain one');
    await text.press('Enter');
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.title === 'Plain one')?.priority)).toBe('');
  });
});

test('a redraw keeps half-typed names and new cards, a failed delete says so, and queued cards keep their board', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const name = page.locator('[data-kanban-name="kdoing0001"]');
    // End does not move the caret in a field on macOS (WebKit), so put it at the end directly.
    const typeAtEnd = async (text) => {
      await name.click();
      await name.evaluate(el => el.setSelectionRange(el.value.length, el.value.length));
      await name.pressSequentially(text);
    };
    await typeAtEnd(' soon');
    await page.locator('[data-action="kanban-add-card"][data-col="ktodo00001"]').click();
    await page.locator('.kanban-composer textarea').fill('Half typed');
    await page.evaluate(() => kanbanRefresh());
    await settled(page);
    await expect(page.locator('.kanban-composer textarea')).toHaveValue('Half typed');
    await expect(page.locator('.kanban-composer textarea')).toBeFocused();
    await typeAtEnd(' now');
    await page.evaluate(() => kanbanRefresh());
    await settled(page);
    await expect(name).toHaveValue('In progress soon now');
    await expect(name).toBeFocused();
    expect(await name.evaluate(el => [el.selectionStart, el.value.length])).toEqual([20, 20]);
    // Leaving the field saved " soon"; Escape drops only what was typed since.
    await name.press('Escape');
    await expect(name).toHaveValue('In progress soon');
    expect(await page.evaluate(() => window.db.boards[0].columns[1].name)).toBe('In progress soon');

    // A column delete that fails keeps the column and does not claim it was deleted.
    await page.locator('[data-action="kanban-column-menu"][data-col="kdone00001"]').click();
    await page.evaluate(() => { window.failNext = 'Leo could not save the board.'; });
    await page.getByRole('menuitem', { name: 'Delete column' }).click();
    await settled(page);
    await expect(page.locator('.kanban-col')).toHaveCount(3);
    const toasts = await page.evaluate(() => window.toasts.map(t => t.message));
    expect(toasts).toContain('Leo could not save the board.');
    expect(toasts.some(t => t.startsWith('Deleted'))).toBe(false);

    // A card still saving when another board opens lands on the board it was typed on.
    await page.evaluate(() => { window.slow = 300; });
    await page.locator('.kanban-composer textarea').fill('Stays on Work');
    await page.locator('.kanban-composer textarea').press('Enter');
    await page.evaluate(() => { location.hash = '#/boards/bhome00001'; });
    await page.evaluate(() => { window.slow = 0; });
    await settled(page);
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Home');
    expect(await page.evaluate(() => window.db.cards.find(c => c.title === 'Stays on Work')?.board)).toBe('bwork00001');
    await expect(page.locator('.kanban-card', { hasText: 'Stays on Work' })).toHaveCount(0);
  });
});

test('on a phone the columns scroll sideways inside the page, which never scrolls sideways itself', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const width = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(width, 'the page itself never scrolls sideways').toBeLessThanOrEqual(390);
    const row = page.locator('.kanban-cols');
    expect(await row.evaluate(el => el.scrollWidth > el.clientWidth)).toBe(true);
    const col = await page.locator('.kanban-col').first().boundingBox();
    expect(col.width).toBeLessThanOrEqual(330);
  }, { width: 390, height: 780 });
});


const lanes = page => page.locator('.kanban-col').evaluateAll(els => els.map(el => { const r = el.getBoundingClientRect(); return { left: r.left, right: r.right, width: r.width, top: r.top, bottom: r.bottom, height: r.height, color: getComputedStyle(el).backgroundColor }; }));
const addColumns = (page, count) => page.evaluate(count => { const board = window.db.boards[0]; for (let i = 0; i < count; i++) board.columns.push({ id: 'kextra' + i + '00000', name: 'Extra ' + i }); }, count);
const addCards = (page, column, count) => page.evaluate(([column, count]) => { for (let i = 0; i < count; i++) window.db.cards.push({ id: 'cfill' + column + i, board: 'bwork00001', column, title: 'Filler card ' + i, subheading: '', description: '', priority: '' }); }, [column, count]);
const redraw = async page => { await page.evaluate(() => kanbanRefresh()); await settled(page); };

test('every lane has one fixed height, down to the window, and only a full lane scrolls', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const first = await lanes(page);
    expect(new Set(first.map(l => l.height)).size, 'lanes with 2, 1 and 0 cards match').toBe(1);
    expect(800 - first[0].bottom, 'lanes stop a small margin above the window').toBe(20);
    await addCards(page, 'ktodo00001', 50);
    await redraw(page);
    expect((await lanes(page)).map(l => l.height), 'fifty cards change no height').toEqual(first.map(l => l.height));
    const page_ = await page.evaluate(() => [document.documentElement.scrollHeight, innerHeight]);
    expect(page_[0], 'the page itself does not scroll').toBeLessThanOrEqual(page_[1]);

    const list = page.locator('.kanban-cards[data-col="ktodo00001"]');
    expect(await list.evaluate(el => el.scrollHeight > el.clientHeight), 'the full lane scrolls inside itself').toBe(true);
    expect(await page.locator('.kanban-cards[data-col="kdoing0001"]').evaluate(el => el.scrollHeight > el.clientHeight)).toBe(false);

    const add = page.locator('[data-action="kanban-add-card"][data-col="ktodo00001"]');
    expect(Math.round(first[0].bottom - (await add.boundingBox()).y - (await add.boundingBox()).height)).toBe(8);
    await expect(add).toHaveCSS('opacity', '1');
    const near = page.locator('[data-action="kanban-add-card"][data-col="kdoing0001"]');
    const last = await page.locator('[data-card="cemail0001"]').boundingBox();
    expect((await near.boundingBox()).y - last.y - last.height).toBeLessThan(12);
    const none = page.locator('[data-action="kanban-add-card"][data-col="kdone00001"]');
    expect((await none.boundingBox()).y - first[2].top, 'an empty lane keeps Add card near its top').toBeLessThan(70);
    await expect(none).toHaveCSS('opacity', '1');

    await add.click();
    await page.locator('.kanban-composer textarea').fill('Newest');
    await page.locator('.kanban-composer textarea').press('Enter');
    await expect(page.locator('.kanban-card', { hasText: 'Newest' })).toBeInViewport();
    await page.locator('.kanban-composer textarea').press('Escape');
    const top = await list.evaluate(el => { el.scrollTop = 300; return el.scrollTop; });
    await redraw(page);
    expect(await list.evaluate(el => el.scrollTop)).toBe(top);

    await list.evaluate(el => { el.scrollTop = 0; });
    const card = await page.locator('[data-card="cessay0001"]').boundingBox();
    const box = await list.boundingBox();
    await page.mouse.move(card.x + 20, card.y + 10);
    await page.mouse.down();
    await page.mouse.move(card.x + 24, box.y + box.height - 10, { steps: 6 });
    await expect.poll(() => list.evaluate(el => el.scrollTop)).toBeGreaterThan(0);
    await page.keyboard.press('Escape');
    await page.mouse.up();
  });
});

test('each column has its own tone dot, cards sit solid, and text stays readable', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const ratio = page.evaluate(() => {
      const rgb = text => { const box = document.createElement('i'); box.style.color = text; document.body.append(box); const c = getComputedStyle(box).color; box.remove(); const canvas = document.createElement('canvas').getContext('2d'); canvas.fillStyle = c; canvas.fillRect(0, 0, 1, 1); return [...canvas.getImageData(0, 0, 1, 1).data].slice(0, 3); };
      const lin = v => { v /= 255; return v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4; };
      const lum = c => .2126 * lin(c[0]) + .7152 * lin(c[1]) + .0722 * lin(c[2]);
      window.contrast = (a, b) => { const [hi, lo] = [lum(rgb(a)), lum(rgb(b))].sort((x, y) => y - x); return (hi + .05) / (lo + .05); };
    });
    await ratio;
    await addColumns(page, 9);
    for (const theme of ['light', 'dark']) {
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      await redraw(page);
      const row = await lanes(page);
      expect(new Set(row.map(l => l.color)).size, 'every lane sits on the shelf tone').toBe(1);
      const edge = await page.locator('.kanban-col').first().evaluate(el => ({ shadow: getComputedStyle(el).boxShadow, lane: getComputedStyle(el).backgroundColor, board: getComputedStyle(document.body).backgroundColor }));
      expect(edge.shadow, theme + ' lane keeps an outline').not.toBe('none');
      expect(edge.lane, theme + ' lane tone differs from the board').not.toBe(edge.board);
      const dots = await page.locator('.kanban-col-head').evaluateAll(els => els.map(el => { const dot = getComputedStyle(el, '::before'); return { color: dot.backgroundColor, width: dot.width }; }));
      for (let i = 0; i < dots.length; i++) {
        expect(dots[i].width, 'the tone dot is 8px').toBe('8px');
        if (i) expect(dots[i].color, `columns ${i - 1} and ${i} have different tone dots`).not.toBe(dots[i - 1].color);
      }
      expect(await page.locator('.kanban-card').first().evaluate(el => getComputedStyle(el).backgroundColor), 'cards sit solid, off the lane').not.toBe(row[0].color);
      const checks = await page.evaluate(() => {
        const out = [];
        for (const lane of document.querySelectorAll('.kanban-col')) {
          const ground = getComputedStyle(lane).backgroundColor;
          out.push(contrast(getComputedStyle(lane.querySelector('.kanban-col-name')).color, ground), contrast(getComputedStyle(lane.querySelector('.kanban-count')).color, ground));
        }
        const card = document.querySelector('.kanban-card');
        out.push(contrast(getComputedStyle(card).color, getComputedStyle(card).backgroundColor), contrast(getComputedStyle(card.querySelector('.kanban-card-sub')).color, getComputedStyle(card).backgroundColor));
        return out;
      });
      for (const value of checks) expect(value, theme + ' contrast').toBeGreaterThanOrEqual(4.5);
    }
    await page.evaluate(() => { window.db.boards[0].columns = ['Backlog', 'Research', 'Done', 'Later', 'Blocked', 'Review', 'Doing'].map((name, i) => ({ id: 'knamed' + i + '00000', name })); });
    await redraw(page);
    const tones = await page.locator('.kanban-col').evaluateAll(els => els.map(el => el.dataset.tone));
    expect(tones.slice(0, 3), 'a section beside Done is not green-teal').toEqual(['todo', 'hue2', 'done']);
    expect(tones.at(-1), 'two doing columns side by side differ').toMatch(/^hue/);
  });
});

test('controls not needed at rest show on hover and keyboard focus', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.mouse.move(2, 2);
    const menu = page.locator('[data-action="kanban-column-menu"][data-col="ktodo00001"]');
    const add = page.locator('[data-action="kanban-add-card"][data-col="ktodo00001"]');
    await expect(menu).toHaveCSS('opacity', '0');
    await expect(add).toHaveCSS('opacity', '0.7');
    await page.locator('.kanban-col').first().hover();
    await expect(menu).toHaveCSS('opacity', '1');
    await expect(add).toHaveCSS('opacity', '1');
    await page.mouse.move(2, 2);
    await page.locator('[data-card="cessay0001"]').focus();
    await expect(menu).toHaveCSS('opacity', '1');
  });
});

test('lanes share the whole width equally, narrow to a minimum, then the row scrolls', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const fits = async () => {
      const row = await page.locator('.kanban-cols').boundingBox();
      const all = await lanes(page);
      expect(new Set(all.map(l => Math.round(l.width))).size, 'equal widths').toBe(1);
      expect(Math.abs(all.at(-1).right - (row.x + row.width)), 'no leftover gap at the right').toBeLessThan(1);
      expect(await page.locator('.kanban-cols').evaluate(el => el.scrollWidth > el.clientWidth + 1)).toBe(false);
      return all[0].width;
    };
    const three = await fits();
    await page.locator('[data-action="kanban-add-column"]').click();
    await page.locator('.kanban-col').last().locator('.kanban-col-name').press('Enter');
    await settled(page);
    expect(await fits(), 'a new column narrows the others').toBeLessThan(three);

    await addColumns(page, 8);
    await redraw(page);
    const all = await lanes(page);
    expect(Math.round(all[0].width), 'lanes stop narrowing at the minimum').toBe(224);
    const row = page.locator('.kanban-cols');
    expect(await row.evaluate(el => el.scrollWidth > el.clientWidth), 'past the minimum the row scrolls').toBe(true);
    await expect(row, 'and shows more lies to the right').toHaveClass(/kanban-fade-right/);
    await row.evaluate(el => { el.scrollLeft = el.scrollWidth; });
    await expect(row).not.toHaveClass(/kanban-fade-right/);
    await expect(row).toHaveClass(/kanban-fade-left/);
    await expect(page.locator('[data-action="kanban-add-column"]'), 'twelve columns is the most').toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  }, { width: 1400, height: 800 });
});

test('a board is one click away; tabs that do not fit go under More and still take drops', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await expect(page.locator('.kanban-more')).toBeHidden();
    await page.locator('.kanban-tab[data-kanban-drop="bhome00001"]').click();
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Home');
    await expect(page.locator('.kanban-tab.on')).toHaveAttribute('aria-current', 'page');

    await page.locator('.kanban-title').fill('Home jobs');
    await page.locator('.kanban-title').press('Enter');
    await expect.poll(() => page.evaluate(() => window.db.boards[1].name)).toBe('Home jobs');

    await page.evaluate(() => { for (let i = 0; i < 10; i++) window.db.boards.push({ id: 'blong' + i + '00000', name: 'A much longer board name number ' + i, columns: [{ id: 'klong' + i + '000000', name: 'To do' }] }); });
    await page.evaluate(() => { location.hash = '#/boards/blong900000'; });
    await expect(page.locator('.kanban-more')).toBeVisible();
    const head = await page.locator('.kanban-head').boundingBox();
    const shown = page.locator('.kanban-tab:visible');
    for (const tab of await shown.all()) {
      const box = await tab.boundingBox();
      expect(box.x + box.width, 'whole tabs only').toBeLessThan(head.x + head.width);
    }
    await expect(page.locator('.kanban-tab.on .kanban-title'), 'the open board stays among them').toHaveValue(/number 9/);
    await page.locator('#kanban-switch').selectOption('blong500000');
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue(/number 5/);

    await page.evaluate(() => { location.hash = '#/boards/bwork00001'; });
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Work');
    const far = page.locator('.kanban-tab[data-kanban-drop="blong900000"]');
    await expect(far).toBeHidden();
    const card = await page.locator('[data-card="cread00001"]').boundingBox();
    await page.mouse.move(card.x + 20, card.y + 10);
    await page.mouse.down();
    await page.mouse.move(card.x + 30, card.y + 20, { steps: 3 });
    await expect(far).toBeAttached();
    const nav = await page.locator('.kanban-tabs').boundingBox();
    await page.mouse.move(nav.x + nav.width - 10, nav.y + nav.height / 2, { steps: 6 });
    await expect.poll(async () => { const box = await far.boundingBox(); return box && box.x + box.width <= nav.x + nav.width + 1; }, { timeout: 10000 }).toBe(true);
    const box = await far.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 3 });
    await page.mouse.up();
    await settled(page);
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').board)).toBe('blong900000');
    await expect(far).toBeHidden();
  });
});

test('new board setup has a full-width named field and a readable column preview', async ({ browser },testInfo) => {
  await boardsFixture(browser,async page=>{
    for(const theme of ['light','dark']) for(const width of [1280,390]) {
      await page.setViewportSize({width,height:844});
      await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
      await page.locator('[data-action="kanban-new-board"]').click();
      const sheet=page.locator('.sheet'), field=page.locator('.sheet').getByRole('textbox',{name:'Board name',exact:true});
      await expect(field).toBeFocused();
      const panel=await sheet.boundingBox(), input=await field.locator('xpath=ancestor::label[1]').boundingBox();
      expect(input.width).toBeGreaterThan(panel.width-64);
      await expect(sheet.locator('.kanban-start-column')).toHaveCount(3);
      for(const name of ['To do','In progress','Done'])await expect(sheet.getByText(name,{exact:true})).toBeVisible();
      for(const tone of ['todo','doing','done']) {
        const color=await page.locator('.kanban-col[data-tone="'+tone+'"]').evaluate(el=>getComputedStyle(el).getPropertyValue('--tone'));
        expect(await sheet.locator('.kanban-start-column[data-tone="'+tone+'"]').evaluate(el=>getComputedStyle(el).getPropertyValue('--tone'))).toBe(color);
      }
      await page.screenshot({path:testInfo.outputPath('new-board-'+theme+'-'+width+'.png'),animations:'disabled'});
      await field.fill('Coursework 文字');
      await page.getByRole('button',{name:'Cancel',exact:true}).click();
      await expect(page.locator('.scrim')).toHaveCount(0);
      await expect(page.locator('.scrim-gone')).toHaveCount(0);
      expect(await page.evaluate(()=>db.boards.length)).toBe(2);
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
    }
    await page.locator('[data-action="kanban-new-board"]').click();
    await page.locator('.sheet').getByRole('textbox',{name:'Board name',exact:true}).fill('Coursework 文字');
    await page.keyboard.press('Enter');
    await expect(page.locator('.kanban-tab.on .kanban-title')).toHaveValue('Coursework 文字');
    expect(await page.evaluate(()=>db.boards.at(-1).columns.map(c=>c.name))).toEqual(['To do','In progress','Done']);
  });
});

test('clicking anywhere in an empty card description shows it as a field with the caret in it', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-card="cread00001"]').click();
    const description = page.locator('#kanban-panel #kanban-card-description');
    await expect(description).toHaveText('Add details, steps or links…');
    const editing = () => page.evaluate(() => {
      const el = document.activeElement;
      return Boolean(el && el.matches('#kanban-card-description textarea') && el.selectionStart === el.value.length);
    });
    for (const spot of [{ x: 40, y: 12 }, { x: 40, y: 160 }]) {
      await page.locator('#kanban-card-title').click();
      await description.click({ position: spot });
      await expect.poll(editing).toBe(true);
      expect(await description.evaluate(el => { const probe = document.createElement('i'); probe.style.color = 'var(--focus)'; el.append(probe); const focus = getComputedStyle(probe).color; probe.remove(); return getComputedStyle(el).borderTopColor === focus; })).toBe(true);
    }
    await page.keyboard.type('Bring the reading list');
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description)).toBe('Bring the reading list');
  });
});

test('a dropped card leaves no copy behind when its panel is open, edited and cannot close', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const strays = page.locator('body > .kanban-ghost, body > .kanban-card');
    await drag(page, page.locator('[data-card="cread00001"]'), page.locator('[data-card="cemail0001"]'), { below: true });
    await expect(strays).toHaveCount(0);
    await page.locator('[data-card="cread00001"]').click();
    await page.locator('#kanban-card-title').fill('Read chapter four');
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').title)).toBe('Read chapter four');
    await drag(page, page.locator('[data-card="cread00001"]'), page.locator('.kanban-cards[data-col="kdone00001"]'));
    await expect(strays).toHaveCount(0);
    await page.evaluate(() => { window.realClose = kanbanClosePanel; kanbanClosePanel = () => { throw new Error('close failed'); }; window.slow = 100; });
    await drag(page, page.locator('[data-card="cread00001"]'), page.locator('.kanban-tab[data-kanban-drop="bhome00001"]'));
    await expect(strays).toHaveCount(0);
    await page.evaluate(() => { kanbanClosePanel = window.realClose; window.slow = 0; window.errors.length = 0; });
    await page.evaluate(() => { location.hash = '#/elsewhere'; });
    await expect(page.locator('#elsewhere')).toBeVisible();
    await expect(strays).toHaveCount(0);
  });
});

test('a card saved from its panel while it is being dragged leaves no copy behind', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-card="cread00001"]').click();
    await page.locator('#kanban-card-sub').click();
    await page.keyboard.type("Jonathan's work");
    await page.evaluate(() => { window.slow = 150; });
    const a = await page.locator('[data-card="cread00001"]').boundingBox();
    const away = await page.locator('.kanban-cards[data-col="kdone00001"]').boundingBox();
    await page.mouse.move(a.x + 20, a.y + 10);
    await page.mouse.down();
    await page.mouse.move(a.x + 30, a.y + 30, { steps: 3 });
    await expect(page.locator('.kanban-ghost')).toHaveCount(1);
    await expect.poll(() => page.evaluate(() => window.db.cards.find((c) => c.id === 'cread00001').subheading)).toBe("Jonathan's work");
    await page.mouse.move(away.x + 40, away.y + 20, { steps: 6 });
    await page.mouse.move(a.x + 20, a.y + 10, { steps: 6 });
    await page.mouse.up();
    await settled(page);
    await expect(page.locator('.kanban-card-title', { hasText: 'Read chapter 3' })).toHaveCount(1);
    await expect(page.locator('[data-card="cread00001"] .kanban-card-sub')).toHaveText("Jonathan's work");
    expect(await titles(page, 'ktodo00001')).toEqual(['Write essay', 'Read chapter 3']);

    await page.locator('.kanban-cols').evaluate((el) => { el.scrollLeft = 0; });
    const card = await page.locator('[data-card="cread00001"]').boundingBox();
    await page.mouse.move(card.x + 20, card.y + 10);
    await page.mouse.down();
    const doing = await page.locator('.kanban-cards[data-col="kdoing0001"]').boundingBox();
    await page.mouse.move(doing.x + 40, doing.y + doing.height - 20, { steps: 6 });
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Read chapter 3']);
    await page.evaluate(() => kanbanRedrawCard(kanbanCard('cread00001')));
    await page.keyboard.press('Escape');
    await page.mouse.up();
    await expect(page.locator('.kanban-ghost')).toHaveCount(0);
    expect(await titles(page, 'ktodo00001')).toEqual(['Write essay', 'Read chapter 3']);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>']);
  });
});

test('a title typed while a long description is still loading is saved, and the description keeps its whole text', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const whole = 'long words '.repeat(40);
    await page.evaluate(async text => { window.db.cards.find(c => c.id === 'cread00001').description = text; await kanbanRefresh(); window.slow = 400; }, whole);
    await page.locator('[data-card="cread00001"]').click();
    await page.locator('#kanban-card-title').fill('Read chapter four');
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').title)).toBe('Read chapter four');
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description)).toBe(whole);
    await expect(page.locator('#kanban-card-description')).toContainText('long words long words');
    await expect.poll(() => page.evaluate(() => document.querySelector('#kanban-card-description').textContent.length)).toBeGreaterThan(300);
  });
});

test('a card whose whole description cannot be read says so and is left as it was', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const whole = 'long words '.repeat(40);
    await page.evaluate(async text => { window.db.cards.find(c => c.id === 'cread00001').description = text; await kanbanRefresh(); window.failCardRead = true; }, whole);
    await page.locator('[data-card="cread00001"]').click();
    await expect(page.locator('#kanban-card-problem')).toContainText('could not be loaded');
    await expect.poll(() => page.evaluate(() => window.errors)).toContain('Leo could not open the card.');
    await page.evaluate(() => { window.errors.length = 0; });
    await expect(page.locator('#kanban-card-description')).toHaveText('');
    await page.locator('#kanban-card-title').fill('Read chapter four');
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').title)).toBe('Read chapter four');
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description)).toBe(whole);
  });
});

test('a click outside the card panel closes it, a click on another card switches it, and menus inside keep it', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const panel = page.locator('#kanban-panel');
    await page.locator('[data-card="cessay0001"]').click();
    await expect(panel).toBeVisible();
    await page.mouse.click(5, 5);
    await expect(panel).toBeHidden();
    await expect(page.locator('[data-card="cessay0001"]')).toBeFocused();

    await page.locator('[data-card="cessay0001"]').click();
    await page.locator('[data-card="cread00001"]').click();
    await expect(panel).toBeVisible();
    await expect(page.locator('#kanban-card-title')).toHaveValue('Read chapter 3');

    const details = page.locator('#kanban-card-description');
    const menu = page.locator('.popup-menu[aria-label="Link a note"]');
    await details.click();
    await page.keyboard.type('See @');
    await expect(menu.getByRole('option')).toHaveCount(2);
    await menu.getByRole('option').first().click();
    await expect(panel).toBeVisible();
    await expect(details.locator('textarea.line-edit')).toHaveValue('See [[Calculus notes]]');
    await page.locator('#kanban-card-sub').click();
    await expect(panel).toBeVisible();
    await page.mouse.click(5, 5);
    await expect(panel).toBeHidden();
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description)).toBe('See [[Calculus notes]]');
  });
});

test('a click outside keeps the card panel and its text when the save fails', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const panel = page.locator('#kanban-panel');
    await page.locator('[data-card="cread00001"]').click();
    await page.locator('#kanban-card-description').click();
    await page.keyboard.type('Keep me');
    await page.evaluate(() => { window.rearm = setInterval(() => { if (!window.failNext) window.failNext = 'Leo could not save the card.'; }, 1); });
    await page.mouse.click(5, 5);
    await expect(page.locator('#kanban-card-problem')).toHaveText('Leo could not save the card.');
    await expect(panel).toBeVisible();
    await expect(page.locator('#kanban-card-description')).toHaveText('Keep me');
    await page.evaluate(() => { clearInterval(window.rearm); window.failNext = null; });
    await page.mouse.click(5, 5);
    await expect(panel).toBeHidden();
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cread00001').description)).toBe('Keep me');
  });
});
