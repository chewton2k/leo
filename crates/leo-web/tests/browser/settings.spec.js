const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss } = require('../page-source');

test('panel refresh updates controls and keyed rows without resetting native state', async ({ page }) => {
  await page.setContent('<main id="panel"></main>');
  await page.addScriptTag({content:fs.readFileSync(path.resolve(__dirname,'../../src/web/app/refresh.js'),'utf8')});
  const result = await page.evaluate(() => {
    const box=document.querySelector('#panel');
    refreshPanel(null,'');
    refreshPanel(box,'<div data-refresh-key="first"><input id="text" value="before"><textarea id="words">before</textarea><select id="pick"><option value="a" selected>A</option><option value="b">B</option></select><input id="switch" type="checkbox"><input id="file" type="file"><details id="fold"><summary>Details</summary>Keep open</details><button data-action="keep" title="old">Old</button><span id="removed">Remove</span></div><div data-refresh-key="second">Second</div>');
    const row=box.firstChild, second=box.lastChild, button=box.querySelector('button'), file=box.querySelector('#file');
    const data=new DataTransfer();data.items.add(new File(['notes'],'fixture.txt'));file.files=data.files;
    box.querySelector('#fold').open=true;
    button.focus();
    refreshPanel(box,'<div data-refresh-key="second">Second updated</div><div data-refresh-key="first"><input id="text" value="after"><textarea id="words">after</textarea><select id="pick"><option value="b" selected>B</option><option value="a">A</option></select><input id="switch" type="checkbox" checked><input id="file" type="file"><details id="fold"><summary>Details</summary>Still open</details><button data-action="keep" disabled>New</button><span id="added">&lt;img src=x&gt;</span></div>');
    return { rows:box.firstChild===second&&box.lastChild===row,button:box.querySelector('button')===button,file:box.querySelector('#file')===file,fileName:file.files[0].name,text:box.querySelector('#text').value,words:box.querySelector('#words').value,pick:box.querySelector('#pick').value,checked:box.querySelector('#switch').checked,open:box.querySelector('#fold').open,removed:!box.querySelector('#removed'),added:box.querySelector('#added').textContent,image:box.querySelectorAll('img').length,title:button.getAttribute('title'),disabled:button.disabled };
  });
  expect(result).toEqual({rows:true,button:true,file:true,fileName:'fixture.txt',text:'after',words:'after',pick:'b',checked:true,open:true,removed:true,added:'<img src=x>',image:0,title:null,disabled:true});
  await page.evaluate(() => {
    const box=document.querySelector('#panel');
    refreshPanel(box,'<button data-action="a">A</button><button data-action="b">B</button>');
    const active=box.lastChild;active.focus();
    refreshPanel(box,'<button data-action="b">B changed</button><button data-action="a">A</button>');
    window.keptButton=active;
  });
  await expect(page.getByRole('button',{name:'B changed'})).toBeFocused();
  expect(await page.evaluate(()=>document.activeElement===window.keptButton)).toBe(true);
  expect(await page.evaluate(() => {
    const box=document.querySelector('#panel');
    refreshPanel(box,'<details data-area="first"><summary>First</summary></details><details data-area="second"><summary>Second</summary></details>');
    box.firstChild.open=true;
    const kept=box.lastChild;
    refreshPanel(box,'<details data-area="second"><summary>Second</summary></details>');
    return box.firstChild===kept&&!kept.open;
  })).toBe(true);
});

test('coding accounts show readable usage and sign-in states in both themes and narrow windows', async ({ page }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const css = pageCss();
  const source = fs.readFileSync(path.join(web, 'app/settings.js'), 'utf8');
  await page.setContent('<style>' + css + '</style><main class="settings" id="plans"></main>');
  await page.addScriptTag({ content: `const esc = text => String(text).replace(/</g, '&lt;').replace(/"/g, '&quot;'), rel = () => 'just now'; ${source.slice(source.indexOf('const PLAN_STALE_MS'), source.indexOf('const THEME_LABELS'))}` });
  for (const theme of ['light', 'dark']) {
    for (const width of [1200, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.evaluate(theme => {
        document.documentElement.dataset.theme = theme;
        const limits = { five_hour: { used: .25, resets_at: Date.now() / 1000 + 3600 }, seven_day: { used: .93, resets_at: Date.now() / 1000 + 86400 }, seen_at: new Date().toISOString() };
        document.querySelector('#plans').innerHTML = codingPlans({ tasks: [{ task: 'writing', provider: 'codex' }], plans: [{ provider: 'codex', name: 'Codex', installed: true, signed_in: true, limits }, { provider: 'claude_code', name: 'Claude Code', installed: true, signed_in: false, signin: 'claude auth login', limits: null }] });
      }, theme);
      await expect(page.getByRole('meter')).toHaveCount(4);
      await expect(page.getByRole('meter', { name: 'Codex 5 hour remaining' })).toHaveAttribute('aria-valuenow', '75');
      await expect(page.getByRole('meter', { name: 'Codex 7 day remaining' })).toHaveAttribute('aria-valuenow', '7');
      await expect(page.getByText('Signed in', { exact: true })).toBeVisible();
      await expect(page.getByText('Signed out', { exact: true })).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      for (const card of await page.locator('.coding-plan').all()) {
        expect(await card.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
      }
    }
  }
});

test('Usage keeps cached readings through refresh failure and ignores a closed sheet', async ({ page }, testInfo) => {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  const settings = read('app/settings.js');
  const plans = settings.slice(settings.indexOf('const PLAN_STALE_MS'), settings.indexOf('const THEME_LABELS'));
  const usage = settings.slice(settings.indexOf('async function showUsage('));
  const sheets = read('app/sheets.js');
  const focus = sheets.slice(sheets.indexOf('function trapSheetFocus('), sheets.indexOf('const SWIPE_CLOSE'));
  await page.setContent(`<style>${css}</style><body></body>`);
  await page.addScriptTag({ content: `
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const rel=()=> '10 minutes ago'; let seq=1, finishUsage, failUsage, usageCalls=0, writer='claude_code';
    const cached={tasks:[{task:'writing',provider:'claude_code'}],plans:[{provider:'codex',name:'Codex',installed:true,signed_in:true,limits:{five_hour:{used:.39,resets_at:Date.now()/1000+3600},seven_day:{used:.06,resets_at:Date.now()/1000+86400},seen_at:new Date(Date.now()-600000).toISOString()}},{provider:'claude_code',name:'Claude Code',installed:true,signed_in:true,limits:{five_hour:{used:1,resets_at:Date.now()/1000+3600},seven_day:{used:.95,resets_at:Date.now()/1000+86400},seen_at:new Date().toISOString()}}]};
    function sheet(html){const box=document.createElement('div');box.className='scrim';box.innerHTML='<div class="sheet" role="dialog" aria-modal="true">'+html+'</div>';box.addEventListener('click',e=>{if(e.target.closest('[data-action="close"]')) box.classList.add('scrim-gone');});document.body.append(box);trapSheetFocus(box);return box;}
    const bodies=[], ICON={refresh:'<svg></svg>',close:'<svg></svg>'}, cascade=()=>{};
    async function api(url,options){if(url==='/api/settings')return structuredClone(cached);usageCalls++;bodies.push(options&&options.body);return new Promise((resolve,reject)=>{finishUsage=resolve;failUsage=reject;});}
    ${focus}
    ${plans}
    ${usage}
    window.audit={open:showUsage,usageCalls:()=>usageCalls,bodies:()=>bodies,finish:value=>finishUsage(value),fail:()=>failUsage(new Error('offline')),writer:()=>writer};` });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.evaluate(() => audit.open());
  await expect(page.getByRole('heading', { name: 'Usage' })).toBeVisible();
  await expect(page.getByRole('meter', { name: 'Codex 5 hour remaining' })).toHaveAttribute('aria-valuenow', '61');
  await expect(page.getByRole('meter', { name: 'Codex 7 day remaining' })).toHaveAttribute('aria-valuenow', '94');
  await expect(page.getByRole('meter', { name: 'Claude Code 5 hour remaining' })).toHaveAttribute('aria-valuenow', '0');
  await expect(page.getByRole('meter', { name: 'Claude Code 7 day remaining' })).toHaveAttribute('aria-valuenow', '5');
  await expect(page.getByRole('button', { name: /Use|Selected for writing/ })).toHaveCount(0);
  await expect(page.getByText('Saved · Updated 10 minutes ago')).toBeVisible();
  const sheet = page.locator('.usage-sheet');
  expect(await sheet.evaluate(el => el.getBoundingClientRect().width)).toBeGreaterThan(900);
  expect(await sheet.evaluate(el => el.getBoundingClientRect().width)).toBeLessThanOrEqual(960);
  await page.evaluate(() => { document.documentElement.dataset.theme = 'dark'; });
  await page.screenshot({path:testInfo.outputPath('usage-desktop-dark.png'),animations:'disabled'});
  await expect.poll(() => page.evaluate(() => audit.usageCalls())).toBe(1);
  expect(await page.evaluate(() => audit.bodies())).toEqual([{}]);
  await expect(page.getByRole('button', { name: 'Refresh' })).toBeDisabled();
  await page.getByRole('button', { name: 'Refresh' }).dispatchEvent('click');
  expect(await page.evaluate(() => audit.usageCalls())).toBe(1);
  await page.evaluate(() => audit.fail());
  await expect(page.getByText('Refresh failed · showing saved usage')).toBeVisible();
  await expect(page.getByText('Saved · Updated 10 minutes ago')).toBeVisible();
  await expect(page.getByRole('meter', { name: 'Codex 5 hour remaining' })).toHaveAttribute('aria-valuenow', '61');
  expect(await page.evaluate(() => audit.writer())).toBe('claude_code');
  await page.setViewportSize({ width: 390, height: 780 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect(await sheet.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  await page.evaluate(() => { document.documentElement.dataset.theme = 'light'; });
  await page.screenshot({path:testInfo.outputPath('usage-compact-light.png'),animations:'disabled'});
  const refresh = page.getByRole('button', { name: 'Refresh' });
  await refresh.focus();
  await refresh.click();
  await expect(refresh).toBeDisabled();
  expect(await page.evaluate(() => audit.bodies())).toEqual([{}, { claude: true }]);
  await page.evaluate(() => audit.finish({tasks:[{task:'writing',provider:'codex'}],plans:[{provider:'codex',name:'Codex',installed:true,signed_in:true,limits:{five_hour:{used:.2,resets_at:Date.now()/1000+3600},seen_at:new Date().toISOString()}}]}));
  await expect(page.getByRole('meter', { name: 'Codex 5 hour remaining' })).toHaveAttribute('aria-valuenow', '80');
  await expect(refresh).toBeEnabled();
  await expect(refresh).toBeFocused();
  await expect(page.getByText('Refresh failed · showing saved usage')).toHaveCount(0);
  expect(await page.evaluate(() => audit.writer())).toBe('claude_code');
  await page.getByRole('button', { name: 'Close' }).click();
  await page.evaluate(() => audit.open());
  await expect(page.locator('.scrim:not(.scrim-gone)').getByRole('heading', { name: 'Usage' })).toBeVisible();
  await expect.poll(() => page.evaluate(() => audit.usageCalls())).toBe(3);
  const closing = page.locator('.scrim:not(.scrim-gone)');
  const closed = await closing.elementHandle();
  await closing.getByRole('button', { name: 'Close' }).click();
  await page.evaluate(() => audit.finish({ tasks:[], plans:[{provider:'codex',name:'Late',installed:true,signed_in:true}] }));
  expect(await closed.evaluate(el => el.classList.contains('scrim-gone') && !el.textContent.includes('Late'))).toBe(true);
  expect(await page.evaluate(() => audit.writer())).toBe('claude_code');
});

test('note mention picker scrolls within its list and keeps the composer caret', async ({ page }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  const chat = read('chat.js');
  const start = chat.indexOf('  function containChatPickWheel(');
  const end = chat.indexOf('  function felix(', start);
  const reveal = chat.slice(start, end);
  await page.setViewportSize({ width: 390, height: 700 });
  await page.setContent(`<style>${css}</style><main style="height:1800px;padding:500px 20px"><form class="chat-compose"><section id="picker" class="chat-pick"><input id="composer" value="@note text"><div id="list" class="chat-pick-list" role="listbox"></div></section></form></main>`);
  await page.addScriptTag({ content: `${reveal} window.revealChatPick = revealChatPick; window.containChatPickWheel = containChatPickWheel;` });
  await page.evaluate(() => {
    document.querySelector('#list').innerHTML = Array.from({ length: 20 }, (_, i) => `<button class="chat-pick-row${i === 0 ? ' on' : ''}" role="option" aria-selected="${i === 0}"><b>Note ${i}</b><span>Folder</span></button>`).join('');
    document.querySelector('#list').addEventListener('wheel', e => containChatPickWheel(document.querySelector('#list'), e), {passive:false});
    scrollTo(0, 150);
  });
  const list = page.locator('#list');
  expect(await list.evaluate(el => el.scrollHeight > el.clientHeight)).toBe(true);
  const pageTop = await page.evaluate(() => scrollY);
  const listBox = await list.boundingBox();
  await page.mouse.move(listBox.x + listBox.width / 2, listBox.y + 8);
  await page.mouse.wheel(0, -160);
  expect(await list.evaluate(el => el.scrollTop)).toBe(0);
  expect(await page.evaluate(() => scrollY)).toBe(pageTop);
  await page.mouse.move(listBox.x + listBox.width / 2, listBox.y + 8);
  await page.mouse.wheel(0, 160);
  await expect.poll(() => list.evaluate(el => el.scrollTop)).toBeGreaterThan(0);
  await list.evaluate(el => { el.scrollTop = el.scrollHeight; });
  const bottom = await list.evaluate(el => el.scrollTop);
  await page.mouse.move(listBox.x + listBox.width / 2, listBox.y + listBox.height - 8);
  await page.mouse.wheel(0, 180);
  expect(await list.evaluate(el => el.scrollTop)).toBe(bottom);
  expect(await page.evaluate(() => scrollY)).toBe(pageTop);
  const input = page.locator('#composer');
  expect(await list.evaluate(el => el.dispatchEvent(new WheelEvent('wheel', {deltaY:150,ctrlKey:true,cancelable:true})))).toBe(true);
  await input.evaluate(el => { el.focus(); el.setSelectionRange(2, 7); });
  await page.evaluate(() => {
    const rows = [...document.querySelectorAll('.chat-pick-row')];
    rows.forEach((row, i) => { row.classList.toggle('on', i === rows.length - 1); row.setAttribute('aria-selected', String(i === rows.length - 1)); });
    revealChatPick(document.querySelector('#list'));
  });
  await expect(input).toBeFocused();
  expect(await input.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([2, 7]);
  expect(await page.evaluate(() => { const list=document.querySelector('#list'), row=list.lastElementChild; return row.getBoundingClientRect().bottom <= list.getBoundingClientRect().bottom + 1 && scrollY === 150; })).toBe(true);
  await page.evaluate(() => { const rows=[...document.querySelectorAll('.chat-pick-row')]; rows.forEach((row,i)=>row.classList.toggle('on',i===0)); revealChatPick(document.querySelector('#list')); });
  expect(await list.evaluate(el => el.scrollTop)).toBe(0);
  await list.evaluate(el => { el.innerHTML = '<button class="chat-pick-row on"><b>One</b></button><button class="chat-pick-row"><b>Two</b></button>'; });
  const shortBox = await list.boundingBox();
  await page.mouse.move(shortBox.x + shortBox.width / 2, shortBox.y + 8);
  await page.mouse.wheel(0, 150);
  expect(await page.evaluate(() => scrollY)).toBe(150);
  expect(await list.evaluate(el => el.scrollTop)).toBe(0);
});

test('Settings sections keep drafts and selections across redraws in both themes', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  const fixture = `const app=document.querySelector('#app'), $=selector=>document.querySelector(selector), ON_MAC=true;
    let seq=0,state={view:'folder'};
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const ICON=new Proxy({}, {get:()=>'<svg></svg>'}), chrome=()=>{}, skeleton=()=>'', svg=paths=>'<svg>'+paths+'</svg>', felix={felix:()=>''}, rel=()=>'', toast=()=>{}, fail=e=>{window.errors.push(e.message)}, saving={drafts:()=>[]};
    window.errors=[]; window.leoChat={usageKinds:()=>[],usagePeriod:()=>({length:30,totals:{calls:0,input:0,cached:0,output:0,share:0},bars:[],models:[],kinds:[]}),usageRank:()=>({models:[],kinds:[]}),usageFold:()=>[]};
    const page={tasks:[]};
    let appStatusReads=0;
    async function api(url,options={}) {
      if(url==='/api/settings'&&options.method==='POST') return {settings:structuredClone(page),message:'Saved'};
      if(url==='/api/settings') return structuredClone(page);
      if(url==='/api/calendar') return {calendars:[],events:[]};
      if(url==='/api/speech') return {ready:false,models:[],extra_files:[],disk_bytes:0};
      if(url==='/api/dictation') return {settings:{custom_words:[],shortcut:'Option+Space'},recent:[]};
      if(url==='/api/health') return {sections:[{title:'This app',checks:[{name:'Speech model',state:'missing',fix:'Choose a model.',section:'models'}]}]};
      if(url==='/api/app') { appStatusReads++; return {preferences:{onboarding:[true,true,true],automatic_updates:true,install_on_quit:true},version:'1.2.3',updates:{state:'ready',version:'1.2.4'},paths:{}}; }
      if(url==='/api/keep') return {choices:[{days:7,label:'7 days'},{days:30,label:'30 days'},{days:90,label:'90 days'},{days:365,label:'a year'},{days:null,label:'forever'}],trash_days:30,chat_days:null,upload_days:90,transcript_days:null,audio_days:null};
      if(url==='/api/trash') return [];
      throw new Error(url);
    }
    const cascade=()=>{}, pulse=()=>{}, overlapOut=()=>{};
    ${read('app/core.js').slice(read('app/core.js').indexOf('const MOTION_MS'), read('app/core.js').indexOf('function stillMotion'))}
    ${read('app/core.js').slice(read('app/core.js').indexOf('const SHORTCUT_KEYS'), read('app/core.js').indexOf('async function dictateHint'))}
    const unfold=menu=>{menu.hidden=false;}, fold=menu=>{menu.hidden=true;}, floating=document.createElement('div'), plural=(n,w)=>n+' '+w+(n===1?'':'s'), empty=(icon,title,text)=>'<div class="empty"><h2>'+title+'</h2><p>'+text+'</p></div>';
    document.addEventListener('click',e=>{const el=e.target.closest('[data-action="segment-pick"],[data-action="theme-pick"],[data-action="width-pick"],[data-action="text-size-pick"],[data-action="letters-pick"],[data-action="setting-reset"]'); if(!el) return; const act=el.dataset.action; if(act==='segment-pick') pickSegment(el); else if(act==='theme-pick') pickTheme(el); else if(act==='width-pick') pickWidth(el); else if(act==='text-size-pick') pickTextSize(el); else if(act==='letters-pick') pickLetters(el); else resetSetting(el);});
    ${read('theme.js')}
    ${read('app/refresh.js')}
    ${read('app/trash.js')}
    const stillMotion=()=>matchMedia('(prefers-reduced-motion: reduce)').matches;
    ${read('app/settings-navigation.js')}
    ${read('app/select-menu.js')}
    ${read('app/sheets.js').slice(read('app/sheets.js').indexOf('function trapSheetFocus('), read('app/sheets.js').indexOf('const SWIPE_CLOSE'))}
    ${read('app/settings.js')}
    ${read('app/keeping.js')}
    ${read('app/dictation.js')}
    ${read('app/app-settings.js')}
    ${read('app/onboarding.js')}
    const route=()=>showSettings(location.hash.split('/')[2] || 'general').catch(fail);
    window.addEventListener('hashchange',route); route();
    window.audit={redraw:()=>drawSettings(state.settings),status:updateAppStatus,reads:()=>appStatusReads};`;
  const server=http.createServer((req,res)=>{
    if(req.url==='/fixture.js') {res.setHeader('Content-Type','text/javascript; charset=utf-8');return res.end(fixture);}
    if(req.url==='/style.css') {res.setHeader('Content-Type','text/css; charset=utf-8');return res.end(css);}
    res.setHeader('Content-Type','text/html; charset=utf-8');res.end('<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="/style.css"></head><body class="on-settings"><header class="bar"><div class="bar-row"><nav class="crumbs" id="crumbs"></nav></div></header><div id="app-update-banner" hidden></div><main id="app"></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  try {
    for(const theme of ['light','dark']) {
      const page=await browser.newPage({colorScheme:theme,viewport:{width:900,height:850}});
      await page.goto('http://127.0.0.1:'+server.address().port+'/#/settings/general');
      await expect(page.getByRole('heading',{level:1,name:'General'})).toBeVisible();
      await expect(page).toHaveURL(/#\/settings\/general$/);
      await page.clock.install();
      await page.evaluate(()=>{window.__LEO_VISIBLE__=false;window.dispatchEvent(new Event('leo-visibility'));});
      const hiddenReads=await page.evaluate(()=>audit.reads());
      await page.clock.runFor(10000);
      expect(await page.evaluate(()=>audit.reads())).toBe(hiddenReads);
      await page.evaluate(()=>{window.__LEO_VISIBLE__=true;window.dispatchEvent(new Event('leo-visibility'));window.dispatchEvent(new Event('leo-visibility'));});
      expect(await page.evaluate(()=>audit.reads())).toBe(hiddenReads+1);
      await page.clock.resume();
      await expect(page.locator('.settings-nav a')).toHaveCount(11);
      await page.setViewportSize({width:1280,height:850});
      for(const name of ['Leo','Your notes','Data','This app']) await expect(page.locator('.settings-group-title',{hasText:name})).toBeVisible();
      await expect(page.locator('.settings-nav a svg')).toHaveCount(11);
      await expect(page.getByRole('heading',{level:1,name:'General'})).toBeVisible();
      const pinned=async()=>page.locator('.settings-nav').evaluate(el=>{const box=el.getBoundingClientRect();return [Math.round(box.left),Math.round(box.top)];});
      const bar=await page.locator('.bar').evaluate(el=>Math.round(el.getBoundingClientRect().bottom));
      const gap=await page.locator('.settings-layout').evaluate(el=>parseFloat(getComputedStyle(el).paddingTop));
      const [left]=await pinned();
      expect(left).toBeGreaterThanOrEqual(gap);
      for(const width of [390,900,1000,1280,1440]) {
        await page.setViewportSize({width,height:850});
        for(const mini of [false,true]) for(const chat of [false,true]) {
          await page.evaluate(({mini,chat})=>{document.body.classList.toggle('side-mini',mini);document.body.classList.toggle('chat-open',chat);document.body.style.setProperty('--explorer-w','332px');},{mini,chat});
          const [start,end]=await page.evaluate(()=>{const main=document.querySelector('main').getBoundingClientRect();return [document.querySelector('.settings-nav').getBoundingClientRect().left-main.left,main.right-document.querySelector('.settings-section').getBoundingClientRect().right];});
          expect(Math.abs(start-end),`nav and section centered together at ${width}px, mini:${mini}, chat:${chat}`).toBeLessThan(2);
        }
      }
      await page.evaluate(()=>{document.body.classList.remove('side-mini','chat-open');document.body.style.removeProperty('--explorer-w');});
      await page.setViewportSize({width:1280,height:850});
      await page.locator('.settings-section').evaluate(el=>{el.style.minHeight='3000px';scrollTo(0,600);});
      expect(await pinned()).toEqual([left,Math.round(bar+gap)]);
      await page.locator('.settings-section').evaluate(el=>{el.style.minHeight='';scrollTo(0,0);});
      await page.setViewportSize({width:900,height:850});
      const chips=await page.evaluate(()=>{const links=[...document.querySelectorAll('.settings-sections .settings-link')].map(a=>a.getBoundingClientRect());return {visible:links.every(r=>r.width>0&&r.height>0),rows:new Set(links.map(r=>Math.round(r.top))).size,position:getComputedStyle(document.querySelector('.settings-nav')).position,titles:getComputedStyle(document.querySelector('.settings-group-title')).display};});
      expect(chips).toEqual({visible:true,rows:expect.any(Number),position:'static',titles:'none'});
      expect(chips.rows).toBeGreaterThan(1);
      await page.setViewportSize({width:1280,height:850});
      const search = page.locator('#settings-search');
      await search.fill('popup');
      await search.evaluate(el=>{el.focus();el.setSelectionRange(2,4);});
      const keptField = await search.elementHandle();
      const keptNav = await page.locator('.settings-nav').elementHandle();
      await page.evaluate(()=>window.audit.redraw());
      expect(await keptField.evaluate(el=>el.isConnected)).toBe(true);
      expect(await keptNav.evaluate(el=>el.isConnected)).toBe(true);
      await expect(search).toBeFocused();
      expect(await search.evaluate(el=>[el.value,el.selectionStart,el.selectionEnd])).toEqual(['popup',2,4]);
      const pillOn=async()=>page.evaluate(()=>{const pill=document.querySelector('.settings-nav-pill').getBoundingClientRect(),on=document.querySelector('.settings-link.on').getBoundingClientRect();return Math.abs(pill.top-on.top)+Math.abs(pill.height-on.height);});
      expect(await pillOn()).toBeLessThan(2);
      await page.locator('#settings-search').fill('zzzz');
      await expect(page.getByText('No settings match “zzzz”.')).toBeVisible();
      await page.keyboard.press('Escape');
      await expect(page.locator('#settings-search')).toHaveValue('');
      await expect(page.getByRole('listbox',{name:'Settings search results'})).toHaveCount(0);
      await page.locator('#settings-search').fill('custom words');
      await expect(page.getByRole('option',{name:/Custom words/})).toHaveAttribute('aria-selected','true');
      await page.keyboard.press('Enter');
      await expect(page.getByRole('heading',{level:1,name:'Dictation'})).toBeVisible();
      await expect(page.locator('[data-find="dictation-words"]')).toBeFocused();
      const found=await page.locator('[data-find="dictation-words"]').evaluate(el=>{const box=el.getBoundingClientRect();return Math.abs(box.top+box.height/2-innerHeight/2);});
      expect(found).toBeLessThan(60);
      await expect(page.locator('#settings-search')).toHaveValue('');
      expect(await keptNav.evaluate(el=>el.isConnected)).toBe(true);
      await page.getByRole('link',{name:'Dictation',exact:true}).click();
      await expect(page.getByRole('heading',{level:1,name:'Dictation'})).toBeVisible();
      expect(await keptNav.evaluate(el=>el.isConnected)).toBe(true);
      // Choices of a few options are a segmented control whose pill slides to the pick; drafts survive redraws.
      await expect(page.getByRole('heading',{name:'Recent dictations'})).toBeVisible();
      const gaps=await page.locator('.settings .set-card').evaluateAll(cards=>cards.slice(1).map((card,i)=>card.getBoundingClientRect().top-cards[i].getBoundingClientRect().bottom));
      expect(gaps.length).toBeGreaterThan(0);
      for(const gap of gaps) expect(gap).toBeGreaterThanOrEqual(16);
      const activation=page.getByRole('radiogroup',{name:'Activation'});
      await expect(activation.getByRole('radio',{name:'Hold or tap'})).toHaveAttribute('aria-checked','true');
      await activation.getByRole('radio',{name:'Tap',exact:true}).click();
      await expect(page.locator('[data-dictation="mode"]')).toHaveValue('tap');
      await expect(activation.getByRole('radio',{name:'Tap',exact:true})).toHaveAttribute('aria-checked','true');
      await expect(activation).toHaveAttribute('style',/--at: ?2/);
      const pill=await activation.locator('.segment-pill').evaluate(el=>new Promise(done=>setTimeout(()=>done(el.getBoundingClientRect()),400)));
      const tap=await activation.getByRole('radio',{name:'Tap',exact:true}).boundingBox();
      expect(Math.abs(pill.left-tap.x)).toBeLessThan(1.5);
      expect(Math.abs(pill.width-tap.width)).toBeLessThan(1.5);
      await page.keyboard.press('ArrowLeft');
      await expect(page.locator('[data-dictation="mode"]')).toHaveValue('hold');
      await expect(activation.getByRole('radio',{name:'Hold',exact:true})).toBeFocused();
      await page.evaluate(()=>window.audit.redraw());
      await expect(page.locator('[data-dictation="mode"]')).toHaveValue('hold');
      await expect(page.getByRole('radiogroup',{name:'Activation'}).getByRole('radio',{name:'Hold',exact:true})).toHaveAttribute('aria-checked','true');
      // On/off settings are switches, never plain checkboxes.
      const fillers=page.getByRole('switch',{name:'Remove filler words'});
      await expect(fillers).not.toBeChecked();
      await fillers.click();
      await expect(fillers).toBeChecked();
      expect(await page.locator('.settings input[type="checkbox"]:not(.switch)').count()).toBe(0);
      const words=page.locator('[data-dictation="custom_words"]');
      await words.fill('Marie Curie\n文字');
      await words.evaluate(el=>{el.focus();el.setSelectionRange(2,7);});
      await page.evaluate(()=>window.audit.redraw());
      await expect(words).toBeFocused();
      expect(await words.evaluate(el=>[el.selectionStart,el.selectionEnd])).toEqual([2,7]);
      await expect(page.locator('#dictation-shortcut-field')).toHaveCount(0);
      const filler=page.locator('[data-find="dictation-fillers"]');
      await expect(filler.locator('.set-desc')).toHaveText('Take out words like um and uh as you speak.');
      await expect(filler).toHaveCSS('border-top-width','1px');
      await page.getByRole('link',{name:'Shortcuts',exact:true}).click();
      await expect(page.getByRole('heading',{level:1,name:'Shortcuts'})).toBeVisible();
      const dictationKeys=page.locator('[data-find="shortcut-dictation"]');
      await expect(dictationKeys.locator('#dictation-shortcut-field')).toBeVisible();
      await expect(dictationKeys.getByRole('button',{name:'Change',exact:true})).toBeVisible();
      await expect(page.getByRole('link',{name:'Change in Dictation'})).toHaveCount(0);
      await page.getByRole('link',{name:'General',exact:true}).click();
      const themeReset=page.getByRole('button',{name:'Reset Theme to default'});
      await expect(themeReset).toBeHidden();
      await page.getByRole('button',{name:'Dark',exact:true}).click();
      await expect(themeReset).toBeVisible();
      await themeReset.click();
      await expect(themeReset).toBeHidden();
      await expect(page.getByRole('button',{name:'Match device',exact:true})).toHaveAttribute('aria-pressed','true');
      for(const [find,count] of [['theme',3],['felix-look',2],['note-width',3]]) await expect(page.locator(`[data-find="${find}"] .choice-cards button.choice-pick:has(.choice-diagram svg)`),`${find} is a row of picture cards`).toHaveCount(count);
      await page.getByRole('button',{name:'Full width',exact:true}).click();
      await expect(page.locator('html')).toHaveAttribute('data-note-width','full');
      await page.getByRole('button',{name:'Reset Note width to default'}).click();
      await expect(page.getByRole('button',{name:'Wide',exact:true})).toHaveAttribute('aria-pressed','true');
      for(const [find,count] of [['text-size',3],['felix-lettering',2]]) await expect(page.locator(`[data-find="${find}"] .choice-cards button.choice-pick:has(.read-sample)`),`${find} is a row of sample cards`).toHaveCount(count);
      await page.getByRole('button',{name:'Large',exact:true}).click();
      await expect(page.locator('html')).toHaveAttribute('data-text-size','large');
      await page.getByRole('button',{name:'Like your notes',exact:true}).click();
      await expect(page.locator('html')).toHaveAttribute('data-felix-letters','notes');
      await page.getByRole('button',{name:'Reset Text size to default'}).click();
      await page.getByRole('button',{name:"Reset Felix's lettering to default"}).click();
      await expect(page.locator('html')).toHaveAttribute('data-text-size','medium');
      await expect(page.locator('html')).toHaveAttribute('data-felix-letters','own');
      await page.getByRole('link',{name:'Calendar',exact:true}).click();
      await expect.poll(()=>page.evaluate(()=>{const pill=document.querySelector('.settings-nav-pill').getBoundingClientRect(),on=document.querySelector('.settings-link.on').getBoundingClientRect();return Math.abs(pill.top-on.top);})).toBeLessThan(2);
      await expect(page.locator('#calendar-link')).toBeVisible();
      await page.getByRole('link',{name:'Health',exact:true}).click();
      await expect(page.getByText('Choose a model.',{exact:true})).toBeVisible();
      await page.getByRole('link',{name:'Updates & about',exact:true}).click();
      await expect(page.getByRole('heading',{name:'About leo'})).toBeVisible();
      await expect(page.locator('[data-find="updates-check"] h3 .set-version')).toHaveText('1.2.3');
      await expect(page.locator('[data-find="updates-about"] .set-path')).toHaveCount(2);
      const updateCard = await page.locator('#app-updates .set-card').first().elementHandle();
      await page.evaluate(()=>window.audit.status());
      expect(await updateCard.evaluate(el=>el.isConnected)).toBe(true);
      await page.getByRole('link',{name:'Keeping',exact:true}).click();
      await expect(page.locator('.settings-nav a[aria-current="page"]')).toHaveAttribute('href','#/settings/keeping');
      await expect(page.getByRole('heading',{level:1,name:'Keeping'})).toBeVisible();
      await expect(page.locator('#crumbs [aria-current="page"]')).toHaveText('Keeping');
      await expect(page.locator('select[data-keep]')).toHaveCount(5);
      await expect(page.locator('select[data-keep="upload_days"]')).toHaveValue('90');
      await page.getByRole('link',{name:'Trash',exact:true}).click();
      await expect(page.locator('.settings-nav a[aria-current="page"]')).toHaveAttribute('href','#/settings/trash');
      await expect(page.getByText('The trash is empty',{exact:true})).toBeVisible();
      await expect(page.locator('#crumbs [aria-current="page"]')).toHaveText('Trash');
      expect(await keptNav.evaluate(el=>el.isConnected)).toBe(true);
      await page.setViewportSize({width:620,height:850});
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
      expect(await page.evaluate(()=>window.errors)).toEqual([]);
      await page.close();
    }
  } finally {server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
});

test('setup groups permissions with live checkmarks, then offers speech models as cards', async ({ browser }, testInfo) => {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  const sheets = read('app/sheets.js').split('const QUICK_PLACES')[0];
  const settings = read('app/settings.js');
  const speechHelpers = settings.slice(settings.indexOf('function speechBytes('), settings.indexOf('function speechModelRow('));
  const writingHelpers = settings.slice(settings.indexOf('function providerCard('), settings.indexOf('function taskCard(')) + settings.slice(settings.indexOf('function ollamaGB('), settings.indexOf('let ollamaTimer'));
  const core = read('app/core.js');
  const dictateHelper = core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('function wireHomeAsk('));
  const fixture = `const app=document.querySelector('#app'), $=selector=>document.querySelector(selector), ON_MAC=true;
    let seq=0,state={view:'folder'};
    const LEAVE_MS=180, stillMotion=()=>true, ICON={check:'<svg aria-hidden="true" viewBox="0 0 24 24"><path d="m5 12 4 4 10-10" fill="none" stroke="currentColor"/></svg>',mic:'<svg aria-hidden="true" viewBox="0 0 24 24"></svg>',access:'<svg aria-hidden="true" viewBox="0 0 24 24"></svg>'};
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    window.errors=[]; const fail=e=>window.errors.push(e.message);
    let native, pending=null, rejectStep=null;
    const calls=[], speechCalls=[], settingCalls=[];
    const model=(id,name,recommended,extra={})=>({id,name,description:name+' runs on this computer.',languages:['en'],size:200*1048576,recommended,speed:8,accuracy:94,accuracy_measured:true,benchmark_language:'en',streaming:true,...extra});
    let speech;
    ${speechHelpers}
    ${writingHelpers}
    ${dictateHelper}
    async function api(url,options={}) {
      if(url==='/api/writing?probe=signin') return {writing:{state:'not_chosen'},candidates:[{id:'codex',installed:false,signed_in:null},{id:'claude_code',installed:true,signed_in:true},{id:'ollama',installed:false,running:false,models:[]}]};
      if(url==='/api/dictation') return {settings:{shortcut:'⌥Space'}};
      if(url==='/api/settings') { settingCalls.push(options.body); return {}; }
      if(url==='/api/speech') {
        if(options.body) { speechCalls.push(options.body); if(options.body.action==='download') speech.models.find(m=>m.id===options.body.id).progress={state:'downloading',bytes:0,total:200*1048576}; }
        return structuredClone(speech);
      }
      if(url!=='/api/app') throw new Error(url);
      const change=options.body;
      if(change) {
        calls.push(change);
        if(change.action==='finish_step') {
          if(pending) await pending;
          if(change.step===rejectStep) throw new Error('Setup could not be saved. Try again.');
          if(change.step===3) native.preferences.writing_done=true; else native.preferences.onboarding[change.step]=true;
        }
      }
      return structuredClone(native);
    }
    ${sheets}
    ${read('app/refresh.js')}
    ${read('app/settings-navigation.js')}
    ${read('app/app-settings.js')}
    ${read('app/onboarding.js')}
    document.addEventListener('click',event=>{
      const el=event.target.closest('[data-action]'); if(!el) return;
      if(el.dataset.action==='app-command') appChange({action:el.dataset.appAction}).catch(fail);
      const run={'onboarding-next':finishOnboarding,'onboarding-skip':skipOnboarding,'onboarding-back':onboardingBack,'onboarding-pick':pickOnboardingModel,'onboarding-models':showOnboardingModels,'onboarding-model':onboardingModel,'onboarding-writer':pickOnboardingWriter,'onboarding-use':onboardingUse}[el.dataset.action];
      if(run) Promise.resolve(run(el)).catch(fail);
    });
    window.audit={
      start(platform='macos',flags=[false,false,false]) {
        closeSheet(true); calls.length=0; speechCalls.length=0; settingCalls.length=0; rejectStep=null; onboardingChoice=''; onboardingAll=false;
        speech={models:[model('base','Base English',true),model('small','Small English',true,{verified:true,downloaded:true}),model('large','Large multilingual',false,{languages:['en','fr']})]};
        native={platform,preferences:{onboarding:flags,writing_done:flags.every(Boolean)},permissions:{microphone:'not_requested',accessibility:false},updates:{}};
        desktopApp=structuredClone(native); maybeOnboarding(true);
      },
      grant:async (permission,value)=>{native.permissions[permission]=value; await pollAppStatus(appPollGeneration);},
      calls:()=>calls, speechCalls:()=>speechCalls, settingCalls:()=>settingCalls, flags:()=>native.preferences.onboarding, writingDone:()=>native.preferences.writing_done,
      progress:bytes=>{speech.models[0].progress={state:'downloading',bytes,total:200*1048576};},
      reject:step=>{rejectStep=step;},
      hold:()=>{pending=new Promise(resolve=>window.release=()=>{pending=null;resolve();});},
      replace:()=>{closeSheet(true);sheet('<h3>Another sheet</h3>');}
    }; audit.start();`;
  const server = http.createServer((req,res) => {
    if(req.url==='/fixture.js') {res.setHeader('Content-Type','text/javascript; charset=utf-8');return res.end(fixture);}
    if(req.url==='/style.css') {res.setHeader('Content-Type','text/css; charset=utf-8');return res.end(css);}
    res.setHeader('Content-Type','text/html; charset=utf-8');res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body><main id="app"></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.goto('http://127.0.0.1:'+server.address().port);
    for(const [theme,order] of [['light',['microphone','accessibility']],['dark',['accessibility','microphone']]]) {
      await page.evaluate(theme=>{document.documentElement.dataset.theme=theme;audit.start();},theme);
      await expect(page.getByRole('list',{name:/^Setup, step \d of 3$/})).toBeVisible();
      await expect(page.locator('[aria-current="step"]')).toContainText('Access');
      await expect(page.getByRole('button',{name:'Allow microphone',exact:true})).toBeVisible();
      await expect(page.getByRole('button',{name:'Allow Accessibility',exact:true})).toBeVisible();
      const next = page.getByRole('button',{name:'Continue',exact:true});
      await expect(next).toBeDisabled();
      for(const permission of order) {
        const row = page.locator('[data-onboarding-permission="'+permission+'"]');
        const button = row.getByRole('button');
        const descriptionWidth = (await row.locator('.grow').boundingBox()).width;
        expect((await button.boundingBox()).height).toBeLessThanOrEqual(40);
        await button.click();
        await expect(row.locator('[role="status"]')).toBeHidden();
        await page.evaluate(permission=>audit.grant(permission,permission==='microphone'?'allowed':true),permission);
        await expect(row.locator('[role="status"]')).toContainText('Allowed');
        await expect(row.locator('[role="status"] svg')).toHaveCount(1);
        await expect(row).toHaveClass(/allowed/);
        await expect(button).toBeHidden();
        expect(Math.abs((await row.locator('.grow').boundingBox()).width-descriptionWidth)).toBeLessThanOrEqual(1);
        // The microphone is what Continue needs; Accessibility only adds pasting.
        if(permission===order[0]) {
          if(permission==='microphone') await expect(next).toBeEnabled(); else await expect(next).toBeDisabled();
          await page.screenshot({path:testInfo.outputPath('permissions-'+theme+'.png')});
        }
      }
      // A stray click on the backdrop must not dismiss setup.
      await page.mouse.click(4,4);
      await expect(page.locator('.scrim')).toHaveCount(1);
      await expect(next).toBeEnabled();
      await next.click();
      await expect(page.getByRole('heading',{name:'Choose a speech model'})).toBeVisible();
      await expect(page.locator('[aria-current="step"]')).toContainText('Speech model');
      expect(await page.evaluate(()=>audit.flags())).toEqual([true,true,false]);
      // Recommended models arrive as cards; the in-use or recommended one is chosen.
      const cards = page.locator('.onboarding-model');
      await expect(cards).toHaveCount(2);
      await expect(cards.first()).toHaveAttribute('aria-pressed','true');
      await expect(cards.first().locator('meter')).toHaveCount(2);
      await expect(page.getByRole('button',{name:'Download and use · 200 MB'})).toBeVisible();
      await page.getByRole('button',{name:'All 3',exact:true}).click();
      await expect(cards).toHaveCount(3);
      await page.getByRole('button',{name:'Recommended',exact:true}).click();
      await expect(cards).toHaveCount(2);
      await cards.nth(1).click();
      await expect(cards.nth(1)).toHaveAttribute('aria-pressed','true');
      await expect(cards.first()).toHaveAttribute('aria-pressed','false');
      await expect(page.getByRole('button',{name:'Use and finish'})).toBeVisible();
      await cards.first().click();
      await page.getByRole('button',{name:'Download and use · 200 MB'}).click();
      await expect(page.locator('#onboarding-model-progress progress')).toBeVisible();
      const cancel = page.getByRole('button',{name:'Cancel',exact:true});
      const cancelHandle = await cancel.elementHandle();
      await page.evaluate(()=>audit.progress(100*1048576));
      await expect(page.locator('#onboarding-model-progress progress')).toHaveAttribute('value','50');
      await expect(cards.first()).toContainText('Downloading 50%');
      // Polling updates the bar in place, so the Cancel button under the pointer survives.
      expect(await cancelHandle.evaluate(el=>el.isConnected)).toBe(true);
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
      await page.screenshot({path:testInfo.outputPath('models-'+theme+'.png')});
      await page.evaluate(()=>{speech.models[0].progress=null;});
      await page.getByRole('button',{name:'Back',exact:true}).click();
      await expect(page.getByRole('heading',{name:'Allow access'})).toBeVisible();
      await next.click();
      await expect(page.getByRole('heading',{name:'Choose a speech model'})).toBeVisible();
      await page.getByRole('button',{name:'Skip for now'}).click();
      await expect(page.getByRole('heading',{name:'Choose who writes your notes'})).toBeVisible();
      await expect(page.locator('[aria-current="step"]')).toContainText('Writing AI');
      expect(await page.evaluate(()=>audit.flags())).toEqual([true,true,true]);
      const writers = page.locator('#onboarding-writing .choice-pick');
      await expect(writers).toHaveCount(3);
      await expect(writers.first()).toHaveAttribute('aria-pressed','true');
      await expect(page.locator('#onboarding-writer-status')).toContainText('Claude Code is signed in on this Mac.');
      await expect(page.locator('.onboarding-dictate')).toHaveText('Press ⌥Space anywhere to dictate.');
      await writers.nth(1).click();
      await expect(page.locator('#onboarding-writer-status')).toContainText('Not installed');
      await expect(page.getByRole('button',{name:'Use Codex'})).toBeDisabled();
      await writers.first().click();
      if(theme==='light') await page.getByRole('button',{name:'Use Claude Code'}).click();
      else await page.getByRole('button',{name:'Skip for now'}).click();
      await expect(page.locator('.scrim')).toHaveCount(0);
      expect(await page.evaluate(()=>audit.writingDone())).toBe(true);
      expect(await page.evaluate(()=>audit.settingCalls())).toEqual(theme==='light'?[{set:'provider',task:'writing',value:'claude_code'}]:[]);
      expect(await page.evaluate(()=>audit.speechCalls().map(c=>c.action+':'+c.id))).toEqual(['download:base']);
    }
    // A denied microphone cannot be asked again, so the button goes to System Settings.
    await page.evaluate(()=>audit.start());
    await page.evaluate(()=>audit.grant('microphone','denied'));
    await expect(page.getByRole('button',{name:'Open System Settings for the microphone'})).toBeVisible();
    await expect(page.locator('[data-onboarding-permission="microphone"] .onboarding-denied')).toBeVisible();
    await expect(page.getByRole('button',{name:'Continue',exact:true})).toBeDisabled();
    // Skip setup finishes every step at once.
    await page.evaluate(()=>audit.start('macos',[true,false,false]));
    await expect(page.getByRole('button',{name:'Allow Accessibility',exact:true})).toBeVisible();
    await page.getByRole('button',{name:'Skip setup'}).click();
    await expect(page.locator('.scrim')).toHaveCount(0);
    expect(await page.evaluate(()=>audit.flags())).toEqual([true,true,true]);
    await page.evaluate(()=>{audit.start();audit.reject(1);});
    await page.getByRole('button',{name:'Skip setup'}).click();
    await expect.poll(()=>page.evaluate(()=>window.errors.length)).toBe(1);
    await expect(page.getByRole('button',{name:'Allow Accessibility',exact:true})).toBeVisible();
    await page.evaluate(()=>audit.reject(null));
    await page.getByRole('button',{name:'Skip setup'}).click();
    await expect(page.locator('.scrim')).toHaveCount(0);
    await page.evaluate(()=>{audit.start();audit.hold();});
    await page.getByRole('button',{name:'Skip setup'}).click();
    await page.evaluate(()=>{audit.replace();release();});
    await expect(page.getByRole('heading',{name:'Another sheet'})).toBeVisible();
    await page.setViewportSize({width:390,height:844});
    await page.evaluate(()=>audit.start('windows'));
    await expect(page.getByRole('button',{name:'Allow Accessibility',exact:true})).toHaveCount(0);
    // Windows often cannot say whether the microphone is allowed; that must not block setup.
    await page.evaluate(()=>audit.grant('microphone','unknown'));
    await expect(page.getByRole('button',{name:'Continue',exact:true})).toBeEnabled();
    await page.evaluate(()=>audit.grant('microphone','allowed'));
    await expect(page.getByRole('button',{name:'Continue',exact:true})).toBeEnabled();
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
    expect(await page.evaluate(()=>window.errors)).toEqual(['Setup could not be saved. Try again.']);
  } finally {await context.close();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
});

test('page toolbars share geometry', async ({ page }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const css = pageCss();
  await page.setContent('<header class="bar"><div class="bar-row"><button class="icon-btn">‹</button><nav class="crumbs">Settings / Keeping</nav><button class="icon-btn">…</button></div></header><main class="settings-section"></main>');
  await page.addStyleTag({ content: css });
  for (const width of [2000, 1200, 390]) {
    await page.setViewportSize({ width, height: 850 });
    let baseline;
    for (const classes of ['', 'on-settings', 'on-note']) {
      await page.locator('body').evaluate((body, value) => { body.className = value; }, classes);
      const box = await page.locator('.bar-row').boundingBox();
      if (!baseline) baseline = box;
      expect(box).toEqual(baseline);
      expect(box.width).toBeLessThanOrEqual(760);
    }
  }
});

test('Felix activity over 24 hours draws hourly bars and detail for that day', async ({ page }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  await page.setContent(`<style>${pageCss()}</style><main id="app"></main>`);
  await page.addScriptTag({ content: `var app=document.querySelector('#app'), esc=t=>String(t), ICON=new Proxy({}, {get:()=>'<svg></svg>'}), svg=p=>'<svg>'+p+'</svg>', segmented=()=>'', stillMotion=()=>true;` });
  await page.addScriptTag({ content: read('chat.js') });
  await page.addScriptTag({ content: read('app/settings.js').replace(/^(const|let) /gm, 'var ') });
  await page.evaluate(() => {
    felixActivity.span = 1;
    const hour = faToday(true);
    const answer = { purpose: 'answer', calls: 2, failures: 1, median_ms: 1500, p95_ms: 3000 };
    app.innerHTML = felixUsageInner({
      felix_days: { models: [{ day: faToday(), name: 'gpt-6.1-sol', calls: 9, input: 900, cached: 0, output: 90 }], kinds: [] },
      felix_recent: {
        hours: { models: [{ day: hour, name: 'gpt-6.1-sol', calls: 2, input: 200, cached: 0, output: 20 }], kinds: [{ day: hour, name: 'answer', calls: 2 }] },
        spans: { 1: { kinds: [answer], tools: { calls: 4, failures: 0, reused: 1 } }, 30: { kinds: [], tools: {} } },
      },
    });
  });
  await expect(page.locator('.fa-day')).toHaveCount(24);
  await expect(page.locator('.fa-xl span').last()).toHaveText('Now');
  await expect(page.locator('.fa-hero b')).toHaveText('2');
  await expect(page.locator('.fa-more .fa-h')).toHaveText('In the last 24 hours');
  await expect(page.locator('.fa-kc')).toHaveText('2 calls');
  await expect(page.locator('.fa-tools')).toContainText('4 calls');
});

test('Felix activity draws gridlines and colours bars by maker', async ({ page }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  await page.setContent(`<style>${css}</style><main id="app"></main>`);
  await page.addScriptTag({ content: `var app=document.querySelector('#app'), esc=t=>String(t), ICON=new Proxy({}, {get:()=>'<svg></svg>'}), svg=p=>'<svg>'+p+'</svg>', segmented=()=>'', stillMotion=()=>true;` });
  await page.addScriptTag({ content: read('chat.js') });
  await page.addScriptTag({ content: read('app/settings.js').replace(/^(const|let) /gm, 'var ') });
  await page.evaluate(() => {
    const today = faToday();
    const row = (name, input) => ({ day: today, name, calls: 1, input, cached: 0, output: 10 });
    app.innerHTML = felixUsageInner({ felix_days: { models: [row('gpt-6.1-sol', 400), row('claude-sonnet-5-5', 300)], kinds: [] } });
  });
  await expect(page.locator('.fa-grid')).toHaveCount(3);
  const colours = await page.locator('.fa-day:last-child .fa-stack i').evaluateAll(els => els.map(el => getComputedStyle(el).backgroundColor));
  expect(colours).toHaveLength(2);
  expect(colours[0]).not.toBe(colours[1]);
});
