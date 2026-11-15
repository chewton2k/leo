const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { pageCss } = require('../page-source');

async function sidebarFixture(browser, run) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = pageCss();
  const core = read('app/core.js');
  const icons = core.slice(core.indexOf('const svg ='), core.indexOf('const recorder ='));
  const drag = read('app/drag.js');
  const open = read('app/actions.js').split("'open-folder':")[1].split("'open-note':")[0].trim().replace(/,$/, '');
  const fixture = `const $=selector=>document.querySelector(selector);
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    ${icons}
    const chat={button:()=>ICON.chat}, felix={NOTE_DRAG:'application/x-leo-note'};
    let seq=0,state={view:'workspace',dir:''};
    const folders=[{name:'Course',notes:2},{name:'Course/Week 1',notes:1},{name:'Course/Week 1/Lab',notes:0},{name:'Personal',notes:1},...Array.from({length:50},(_,i)=>({name:'Reading '+i,notes:0}))];
    const folderLabel=dir=>dir.split('/').pop()||'Top level', folderHash=dir=>dir?'#/f/'+encodeURIComponent(dir):'#/', noteHash=id=>'#/n/'+encodeURIComponent(id);
    const closeSheet=()=>{}, go=hash=>{location.hash=hash;}, fail=error=>window.errors.push(error.message);
    const stillMotion=()=>matchMedia('(prefers-reduced-motion: reduce)').matches;
    const moves=[], tasksAtHome=()=>false, enc=encodeURIComponent, toast=()=>{}, showLatest=async()=>{}, saving={unsaved:()=>false};
    const notes={
      '':[{id:'root-note',title:'Welcome',directory:''}],
      Course:[{id:'course-note',title:'Course overview',directory:'Course'},{id:'escaped-note',title:'<script> stays text',directory:'Course'}],
      'Course/Week 1':[{id:'week-note',title:'Lecture 1',directory:'Course/Week 1'}],
      Personal:Array.from({length:105},(_,i)=>({id:'personal-'+i,title:'Personal note '+i,directory:'Personal'}))
    };
    let pending=null, broken=null;
    async function api(url,options={}) {
      if(url==='/api/folders')return structuredClone(folders);
      if(options.method==='POST'){moves.push({url,body:options.body});return url==='/api/dirs/move'?{path:(options.body.into?options.body.into+'/':'')+options.body.from.split('/').pop()}:{};}
      const query=new URL(url,location.origin).searchParams, dir=query.get('dir'), offset=Number(query.get('offset')||0);
      const all=notes[dir]||[], result={items:structuredClone(all.slice(offset,offset+100)),total:all.length};
      if(pending && dir==='Course') {
        const wait=pending;pending=null;
        await new Promise((resolve,reject)=>{
          const abort=()=>reject(new Error('Timed out'));
          options.signal?.addEventListener('abort',abort,{once:true});
          wait.then(()=>{options.signal?.removeEventListener('abort',abort);resolve();});
        });
      }
      if(broken===dir)throw new Error('Unavailable');
      return result;
    }
    ${read('app/refresh.js')}
    ${core.slice(core.indexOf('const MOTION_MS'), core.indexOf('function stillMotion'))}
    ${core.slice(core.indexOf('const moving'), core.indexOf('function rel('))}
    ${core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('async function dictateHint'))}
    const overlapOut=()=>{};
    ${read('app/motion.js')}
    ${read('app/popup-menu.js')}
    ${read('app/select-menu.js')}
    ${read('app/side.js')}
    ${drag}
    const folderClick=typeof openFolder==='function'?${open}:el=>twistFolder(el.dataset.dir,el);
    document.addEventListener('click',event=>{
      const el=event.target.closest('[data-action]');if(!el)return;
      const action=el.dataset.action;
      if(action==='open-folder') folderClick(el);
      else if(action==='side-twist') twistFolder(el.dataset.dir,el);
      else if(action==='side-fold') foldSide();
      else if(action==='side-notes') loadSideNotes(el.dataset.dir,Number(el.dataset.offset||0),true);
      else if(action==='open-note') go(noteHash(el.dataset.id));
      else if(action==='home') go('#/');
    });
    window.errors=[];
    window.audit={moves,draw:drawSide,side,refresh:loadSideFolders,note:(id,dir)=>{seq++;state={view:'note',dir,session:{note:{id}}};drawSide();},dragging,drop:el=>dirOf(dropTargetOf(el)),stop:endDrag,rename:title=>{notes.Course[0].title=title;},route:(dir)=>{seq++;state={view:'workspace',dir:''};side.revealed.clear();side.open.clear();drawSide();revealFolder(dir);},mac:on=>{document.documentElement.dataset.app=on?'mac':'';},broken:dir=>{broken=dir;},hold:()=>{pending=new Promise(resolve=>window.release=()=>{pending=null;resolve();});}};
    loadSideFolders().catch(fail);`;
  const server = http.createServer((req,res) => {
    if(req.url==='/fixture.js') {res.setHeader('Content-Type','text/javascript; charset=utf-8');return res.end(fixture);}
    if(req.url==='/pane-resize.js') {res.setHeader('Content-Type','text/javascript; charset=utf-8');return res.end(read('pane-resize.js'));}
    if(req.url==='/chat.js') {res.setHeader('Content-Type','text/javascript; charset=utf-8');return res.end(read('chat.js'));}
    if(req.url.startsWith('/api/')) {res.setHeader('Content-Type','application/json');return res.end('[]');}
    if(req.url==='/style.css') {res.setHeader('Content-Type','text/css; charset=utf-8');return res.end(css);}
    res.setHeader('Content-Type','text/html; charset=utf-8');res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body><aside id="side" class="side"></aside><header class="bar"><div class="bar-row">Leo</div></header><main><textarea id="draft">Keep my draft 文字</textarea></main><script src="/pane-resize.js"></script><script src="/chat.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const context = await browser.newContext();

  try {await run(context,'http://127.0.0.1:'+server.address().port);}
  finally {await context.close();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
}

test('sidebar separates icon actions from the explorer and keeps them reachable in both themes', async ({ browser }, testInfo) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror',error=>errors.push(error.message));
    for(const theme of ['light','dark']) {
      await page.setViewportSize({width:1280,height:640});
      await page.goto(url);
      await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
      const rail = page.locator('.side-rail'), explorer = page.locator('.side-explorer');
      await expect(rail).toBeVisible();
      await expect(explorer).toBeVisible();
      const railBox = await rail.boundingBox(), explorerBox = await explorer.boundingBox();
      expect(railBox.width).toBeLessThanOrEqual(60);
      expect(railBox.x+railBox.width).toBeLessThanOrEqual(explorerBox.x+1);
      await expect(explorer.getByRole('button',{name:'New note',exact:true})).toBeInViewport();
      await expect(explorer.getByRole('button',{name:'New folder',exact:true})).toBeInViewport();
      await expect(rail.getByRole('button',{name:'New note',exact:true})).toHaveCount(0);
      await expect(rail.getByRole('button',{name:'Quick Leo',exact:true}),'search and Quick Leo are one button').toHaveCount(0);
      await expect(rail.locator('kbd')).toHaveCount(0);
      // Trash and Keeping live inside Settings now, so the rail only offers Settings for them.
      await expect(rail.getByRole('button',{name:/^(Trash|Keeping)$/})).toHaveCount(0);
      await expect(rail.getByRole('button',{name:'Ask Felix',exact:true}),'Felix opens from the toolbar, not the rail').toHaveCount(0);
      await expect(page.locator('[data-action="side-collapse"]'),'there is no Collapse all').toHaveCount(0);
      for(const label of ['Search','Record','Note from a file','Calendar','Boards','Settings']) {
        const button = rail.getByRole('button',{name:label,exact:true});
        await expect(button).toBeInViewport();
        await expect(button).toHaveAttribute('title',/.+/);
        await expect(button.locator('.side-label')).toBeHidden();
      }
      const settings = rail.getByRole('button',{name:'Settings',exact:true});
      const before = await settings.boundingBox();
      await page.locator('.side-tree').evaluate(el=>el.scrollTop=el.scrollHeight);
      expect(await settings.boundingBox()).toEqual(before);
      await expect(page.locator('.side-group[data-drop-dir=""]')).toBeInViewport();
      await expect(page.locator('#side [data-action="open-folder"][data-dir=""]')).toHaveCount(0);
      await page.locator('.side-tree').evaluate(el=>el.scrollTop=0);
      const course=page.locator('.side-tree .side-row[data-dir="Course"]');
      if(await course.getAttribute('aria-expanded')==='false') await course.click();
      const week=page.locator('.side-tree .side-row[data-dir="Course/Week 1"]');
      if(await week.getAttribute('aria-expanded')==='false') await week.click();
      await expect(page.locator('.side-note[data-id="week-note"]')).toBeVisible();
      await page.screenshot({path:testInfo.outputPath('sidebar-'+theme+'.png')});
      await page.getByRole('button',{name:'Hide folders',exact:true}).click();
      await expect(explorer).toBeHidden();
      await expect(settings).toBeInViewport();
      await page.reload();
      await expect(explorer).toBeHidden();
      await page.getByRole('button',{name:'Show folders',exact:true}).click();
      await expect(explorer).toBeVisible();
      await page.evaluate(()=>audit.mac(true));
      await page.getByRole('button',{name:'Hide folders',exact:true}).click();
      await page.setViewportSize({width:1000,height:640});
      await page.evaluate(()=>document.body.classList.add('on-note'));
      expect((await page.locator('.side-fold').boundingBox()).x).toBeLessThan(52);
      await page.getByRole('button',{name:'Show folders',exact:true}).click();
      expect((await page.locator('.side-rail-main').boundingBox()).y).toBeGreaterThanOrEqual(50);
      await page.setViewportSize({width:1280,height:320});
      await expect(settings).toBeInViewport();
      const railMain=page.locator('.side-rail-main');
      await railMain.evaluate(el=>el.scrollTop=el.scrollHeight);
      const scroll=await railMain.evaluate(el=>el.scrollTop);
      expect(scroll).toBeGreaterThan(0);
      await page.evaluate(()=>audit.draw());
      expect(await railMain.evaluate(el=>el.scrollTop)).toBe(scroll);
      await page.setViewportSize({width:390,height:844});
      await expect(page.locator('#side')).toBeHidden();
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
    }
    expect(errors).toEqual([]);
  });
});

test('sidebar and page header rows align across desktop layouts', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    await expect(page.locator('.side-head')).toBeVisible();
    await page.locator('main').evaluate(el=>el.insertAdjacentHTML('beforeend','<div class="settings-layout"><nav class="settings-nav">Settings</nav></div>'));
    for(const width of [1000,1280]) for(const theme of ['light','dark']) for(const mode of ['web','mac','mac-full']) for(const top of [0,20]) for(const mini of [false,true]) {
      const mac=mode!=='web', windowed=mode==='mac';
      await page.setViewportSize({width,height:800});
      await page.evaluate(({theme,mac,full,top,mini})=>{
        document.documentElement.dataset.theme=theme;
        document.documentElement.style.setProperty('--top',top+'px');
        audit.mac(mac);
        document.documentElement.toggleAttribute('data-fullscreen',full);
        document.body.classList.toggle('side-mini',mini);
        document.body.classList.add('on-note');
        document.querySelector('.bar').classList.add('scrolled');
      },{theme,mac,full:mode==='mac-full',top,mini});
      const at=JSON.stringify({width,theme,mode,top,mini});
      const head=await page.locator('.side-head').boundingBox(), bar=await page.locator('.bar').boundingBox();
      expect(Math.abs(head.y+head.height-bar.y-bar.height),at).toBeLessThanOrEqual(1);
      const toggle=await page.locator('.side-fold').boundingBox();
      expect(toggle.y).toBeGreaterThanOrEqual(top);
      if(windowed&&mini) {
        expect(toggle.y,'folded, the toggle sits below the macOS window buttons').toBeGreaterThanOrEqual(top+44);
        expect(toggle.x).toBeLessThan(52);
        const first=await page.locator('.side-rail .side-row').first().boundingBox();
        expect(first.y,'the rail starts below the toggle').toBeGreaterThanOrEqual(toggle.y+toggle.height);
      } else {
        expect(toggle.y+toggle.height,at).toBeLessThanOrEqual(head.y+head.height);
        if(windowed) expect(toggle.x,'the header row starts right of the window buttons').toBeGreaterThanOrEqual(84);
        else expect(toggle.x,'without window buttons the header row is flush left').toBeLessThanOrEqual(8);
        if(!mini) {
          const brand=await page.locator('.side-head .brand').boundingBox();
          expect(brand,`${at} shows the brand`).not.toBeNull();
          expect(brand.x).toBeGreaterThanOrEqual(toggle.x+toggle.width);
        }
      }
      const row=await page.locator('.bar-row').boundingBox();
      expect(row.height).toBeGreaterThanOrEqual(40);
      const settings=await page.locator('.settings-nav').evaluate(el=>({top:parseFloat(getComputedStyle(el).top),gap:parseFloat(getComputedStyle(el.parentElement).columnGap)}));
      expect(settings.top).toBe(bar.height+settings.gap);
    }
    await page.setViewportSize({width:390,height:844});
    await expect(page.locator('#side')).toBeHidden();
    expect((await page.locator('.bar').boundingBox()).height).toBe(81);
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  });
});

test('folder chevrons are at least 24 px to press, and open notes line up under their folder', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page = await context.newPage({viewport:{width:1280,height:800}});
    await page.goto(url);
    const twist = page.locator('.side-tree .side-twist').first();
    await expect(twist).toBeVisible();
    const box = await twist.boundingBox();
    expect(box.width).toBeGreaterThanOrEqual(24);
    expect(box.height).toBeGreaterThanOrEqual(24);
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    const folderIcon = await page.locator('.side-tree .side-row[data-dir="Course"] svg').boundingBox();
    const noteIcon = await page.locator('.side-tree .side-note').first().locator('svg').boundingBox();
    expect(Math.abs(noteIcon.x - folderIcon.x - 14)).toBeLessThanOrEqual(14);
    const guide = await page.locator('.side-kids[data-kids="Course"]').evaluate(el => { const s = getComputedStyle(el, '::before'); return { mask: s.maskImage || s.webkitMaskImage, width: s.width }; });
    expect(guide.mask.match(/url\(/g)).toHaveLength(2);
    expect(guide.width).toBe('10px');
  });
});

test('clicking parent folder labels toggles descendants without navigating or disturbing the draft', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page = await context.newPage({viewport:{width:1280,height:800}});
    await page.goto(url);
    const row = dir=>page.locator('.side-tree .side-row[data-dir="'+dir+'"]');
    await expect(row('Course')).toBeVisible();
    await row('Course').click();
    await expect(row('Course/Week 1')).toBeVisible();
    await expect(row('Course')).toHaveAttribute('aria-expanded','true');
    await expect(page.locator('.side-new')).toHaveAttribute('data-dir','Course');
    await expect(page.locator('.side-rail [data-action="record"]')).toHaveAttribute('data-dir','Course');
    await expect(page.locator('.side-rail [data-action="upload"]')).toHaveAttribute('data-dir','Course');
    expect(new URL(page.url()).hash).toBe('');
    await expect(row('Course')).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(row('Course/Week 1')).toHaveCount(0);
    await expect(row('Course')).toHaveAttribute('aria-expanded','false');
    await page.keyboard.press('Space');
    await expect(row('Course/Week 1')).toBeVisible();
    await row('Course/Week 1').click();
    await expect(row('Course/Week 1/Lab')).toBeVisible();
    await page.reload();
    await expect(row('Course/Week 1/Lab')).toBeVisible();
    await page.evaluate(()=>audit.route('Course/Week 1/Lab'));
    await row('Course').click();
    await expect(row('Course/Week 1')).toHaveCount(0);
    await page.locator('[data-action="side-twist"][data-dir="Course"]').click();
    await expect(row('Course/Week 1/Lab')).toBeVisible();
    await row('Personal').click();
    await page.locator('.side-note[data-id="personal-0"]').click();
    await expect(page).toHaveURL(/#\/n\/personal-0$/);
    await expect(page.locator('#draft')).toHaveValue('Keep my draft 文字');
  });
});

test('explorer loads direct notes, escapes titles, pages large folders and recovers errors', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    const row=dir=>page.locator('.side-tree .side-row[data-dir="'+dir+'"]');
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeVisible();
    await row('Course').click();
    await expect(page.locator('.side-note[data-id="course-note"]')).toBeVisible();
    await expect(page.locator('.side-note[data-id="escaped-note"]')).toHaveText('<script> stays text');
    await expect(page.locator('.side-note script')).toHaveCount(0);
    await row('Course/Week 1').click();
    await expect(page.locator('.side-note[data-id="week-note"]')).toBeVisible();
    await row('Personal').click();
    await expect(page.locator('.side-note[data-id^="personal-"]')).toHaveCount(100);
    await page.waitForFunction(()=>!document.getAnimations().some(run=>run.effect?.target?.closest?.('.side-kids')));
    await page.locator('[data-action="side-notes"][data-dir="Personal"]').filter({hasText:'Next'}).click();
    await expect(page.locator('.side-note[data-id="personal-104"]')).toBeVisible();
    await page.evaluate(()=>{seq++;drawSide();});
    await expect(page.locator('.side-note[data-id="personal-104"]')).toBeVisible();
    await expect(page.locator('.side-note[data-id^="personal-"]')).toHaveCount(5);
    await page.evaluate(()=>audit.note('personal-104','Personal'));
    await expect(page.locator('.side-note[data-id="personal-104"]')).toHaveAttribute('aria-current','page');
    await expect(row('Personal')).not.toHaveAttribute('aria-current','page');
    await expect(page.locator('.side-note[data-id="personal-104"]')).toHaveClass(/\bopen\b/);
    expect(await row('Personal').evaluate(el=>{const style=getComputedStyle(el);return style.userSelect==='none'||style.webkitUserSelect==='none';})).toBe(true);
    await page.evaluate(()=>audit.refresh());
    await expect(page.locator('.side-note[data-id="personal-104"]')).toBeVisible();
    await page.locator('[data-action="side-notes"][data-dir="Personal"]').filter({hasText:'Previous'}).click();
    await expect(page.locator('.side-note[data-id="personal-0"]')).toBeVisible();
    await page.evaluate(()=>{audit.broken('Course/Week 1/Lab');});
    await row('Course/Week 1/Lab').click();
    const group=page.locator('[data-kids="Course/Week 1/Lab"]');
    await expect(group).toContainText('Notes could not be opened');
    await page.evaluate(()=>audit.broken(null));
    await group.getByRole('button',{name:'Try again'}).click();
    await expect(group).toContainText('Empty folder');
    await page.evaluate(()=>{audit.side.notes.delete('Course');audit.hold();audit.draw();});
    await row('Course').click();
    await page.evaluate(()=>{audit.route('Personal');release();});
    await expect(row('Course/Week 1')).toHaveCount(0);
    await expect(page.locator('#draft')).toHaveValue('Keep my draft 文字');
  });
});

test('explorer notes keep drag data and refuse stale reads with bounded requests and cache', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    const note=page.locator('.side-note[data-id="course-note"]');
    await expect(note).toBeVisible();
    const transfer=await page.evaluateHandle(()=>new DataTransfer());
    await note.dispatchEvent('dragstart',{dataTransfer:transfer});
    const payload=await transfer.evaluate(data=>data.getData('application/x-leo-note'));
    expect(payload).not.toBe('');
    expect(JSON.parse(payload)).toEqual({id:'course-note',title:'Course overview',dir:'Course'});
    expect(await page.locator('.side-row[data-dir="Personal"]').evaluate(el=>audit.drop(el))).toBe('Personal');
    expect(await page.locator('.side-group').evaluate(el=>audit.drop(el))).toBe('');
    await page.evaluate(()=>audit.stop());
    await page.evaluate(()=>{audit.rename('Old response');audit.side.notes.delete('Course');audit.hold();audit.draw();});
    await page.evaluate(()=>{audit.rename('Fresh response');audit.route('Course');});
    await expect(note).toHaveText('Fresh response');
    await page.evaluate(()=>release());
    await expect(note).toHaveText('Fresh response');
    await note.focus();
    await page.evaluate(()=>{audit.rename('Renamed while focused');return audit.refresh();});
    await expect(note).toHaveText('Renamed while focused');
    await expect(note).toBeFocused();
    const limits=await page.evaluate(()=>{
      for(let i=0;i<100;i++) audit.side.folders.push({name:'Bounded '+i,notes:0});
      for(let i=0;i<100;i++) loadSideNotes('Bounded '+i);
      return {cache:audit.side.notes.size,active:audit.side.reading,queued:audit.side.pending.length};
    });
    expect(limits.cache).toBeLessThanOrEqual(64);
    expect(limits.active).toBeLessThanOrEqual(4);
    expect(limits.queued).toBeLessThanOrEqual(64);
  });
});

test('a stalled explorer read times out and offers a working retry', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeVisible();
    await page.clock.install();
    await page.evaluate(()=>audit.hold());
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    await page.clock.fastForward(15000);
    const group=page.locator('[data-kids="Course"]');
    await expect(group).toContainText('Notes could not be opened');
    await page.evaluate(()=>audit.hold());
    await group.getByRole('button',{name:'Try again'}).click();
    await expect(group).toContainText('Opening notes');
    await page.evaluate(()=>release());
    await expect(page.locator('.side-note[data-id="course-note"]')).toBeVisible();
  });
});

test('explorer refresh keeps file rows and a click in progress intact', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    const note=page.locator('.side-note[data-id="course-note"]');
    await expect(note).toBeVisible();
    await expect.poll(()=>page.locator('.side-kids[data-kids="Course"]').evaluate(el=>el.getAnimations().length)).toBe(0);
    const kept=await note.elementHandle();
    const tree=await page.locator('.side-tree').elementHandle();
    const point=await note.boundingBox();
    await page.mouse.move(point.x+point.width/2,point.y+point.height/2);
    await page.mouse.down();
    await page.evaluate(()=>audit.refresh());
    await expect(page.locator('.side-note-status')).not.toContainText(['Opening notes…']);
    expect(await kept.evaluate(el=>el.isConnected)).toBe(true);
    expect(await tree.evaluate(el=>el.isConnected)).toBe(true);
    await page.mouse.up();
    await expect(page).toHaveURL(/#\/n\/course-note$/);
    await page.evaluate(()=>{audit.rename('Renamed 文字');return audit.refresh();});
    await expect(note).toHaveText('Renamed 文字');
    expect(await kept.evaluate(el=>el.isConnected)).toBe(true);
    expect(await page.evaluate(()=>window.errors)).toEqual([]);
  });
});

test('a note just created lands in the explorer in full ink, and one moved in from elsewhere does not', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    await expect(page.locator('.side-note[data-id="course-note"]')).toBeVisible();
    await page.evaluate(()=>{notes.Course.push({id:'fresh-note',title:'Fresh',directory:'Course',created_at:new Date().toISOString()},{id:'old-note',title:'Old',directory:'Course',created_at:'2020-01-01T00:00:00Z'});return refreshSideTree();});
    const inked=id=>page.locator(`.side-note[data-id="${id}"]`).evaluate(el=>el.getAnimations().filter(run=>run.id==='ink').length);
    await expect(page.locator('.side-note[data-id="fresh-note"]')).toBeVisible();
    expect(await inked('fresh-note')).toBe(1);
    expect(await inked('old-note')).toBe(0);
    expect(await inked('course-note')).toBe(0);
    await page.evaluate(()=>{seq++;drawSide();});
    expect(await inked('fresh-note')).toBeLessThanOrEqual(1);
    expect(await page.evaluate(()=>window.errors)).toEqual([]);
  });
});

test('the explorer filter keeps its text, focus and matches through a redraw', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    await expect(page.locator('.side-note[data-id="course-note"]')).toBeVisible();
    await page.locator('#side-filter').fill('OVERVIEW');
    await expect(page.locator('.side-note-wrap:has(.side-note[data-id="escaped-note"])')).toBeHidden();
    await expect(page.locator('.side-note-wrap:has(.side-note[data-id="course-note"])')).toBeVisible();
    await page.evaluate(()=>{seq++;drawSide();});
    await expect(page.locator('#side-filter')).toBeFocused();
    await expect(page.locator('#side-filter')).toHaveValue('OVERVIEW');
    await expect(page.locator('.side-note-wrap:has(.side-note[data-id="escaped-note"])')).toBeHidden();
    expect(await page.evaluate(()=>window.errors)).toEqual([]);
  });
});

test('loaded folders never flash Opening notes while pages change or folders are clicked', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage({viewport:{width:1280,height:800}});
    await page.goto(url);
    const twist=dir=>page.locator('[data-action="side-twist"][data-dir="'+dir+'"]');
    await twist('Course').click();
    await twist('Course/Week 1').click();
    await twist('Course/Week 1/Lab').click();
    await expect(page.locator('[data-kids="Course/Week 1/Lab"]')).toContainText('Empty folder');
    await expect(page.locator('.side-note[data-id="week-note"]')).toBeVisible();
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeVisible();
    await page.evaluate(()=>{
      window.flashes=[];
      const mentions=node=>(node.textContent||'').includes('Opening notes');
      new MutationObserver(records=>{
        for(const record of records) {
          if(record.type==='characterData'&&(record.oldValue||'').includes('Opening notes')) window.flashes.push('text');
          for(const node of [...record.addedNodes,...record.removedNodes]) if(mentions(node)) window.flashes.push(node.nodeName);
        }
      }).observe(document.body,{childList:true,subtree:true,characterData:true,characterDataOldValue:true});
    });
    for(let round=0;round<6;round++) {
      await page.evaluate(()=>{seq++;drawSide();});
      await page.evaluate(()=>audit.refresh());
      await twist('Course/Week 1').click();
      await twist('Course/Week 1').click();
      await page.locator('.side-tree .side-row[data-dir="Course"]').click();
      await page.locator('.side-tree .side-row[data-dir="Course"]').click();
      await expect(page.locator('[data-kids="Course/Week 1/Lab"]')).toContainText('Empty folder');
      await page.evaluate(()=>new Promise(resolve=>setTimeout(resolve,40)));
    }
    expect(await page.evaluate(()=>window.flashes)).toEqual([]);
    await expect(page.locator('.side-note[data-id="week-note"]')).toBeVisible();
    expect(await page.evaluate(()=>window.errors)).toEqual([]);
  });
});

test('nested folders start closed and the explorer reopens as the user left it, not as browsing unfolded it', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page = await context.newPage({viewport:{width:1280,height:800}});
    await page.goto(url);
    const row = dir=>page.locator('.side-tree .side-row[data-dir="'+dir+'"]');
    const kept = ()=>page.evaluate(()=>JSON.parse(localStorage.getItem('leo-side-folders')||'[]'));
    await expect(row('Course')).toHaveAttribute('aria-expanded','false');
    await expect(row('Course/Week 1'), 'nested folders start closed').toHaveCount(0);
    await page.evaluate(()=>audit.route('Course/Week 1/Lab'));
    await expect(row('Course/Week 1/Lab'), 'the open folder is shown').toBeVisible();
    expect(await kept(), 'unfolding to show where you are is not remembered').toEqual([]);
    await page.reload();
    await expect(row('Course/Week 1')).toHaveCount(0);
    await page.locator('[data-action="side-twist"][data-dir="Personal"]').click();
    await page.locator('[data-action="side-twist"][data-dir="Course"]').click();
    expect((await kept()).sort()).toEqual(['Course', 'Personal']);
    await page.reload();
    await expect(row('Course'), 'a folder the user opened stays open after a restart').toHaveAttribute('aria-expanded','true');
    await expect(row('Course/Week 1')).toHaveAttribute('aria-expanded','false');
    await expect(row('Personal')).toHaveAttribute('aria-expanded','true');
    await page.locator('[data-action="side-twist"][data-dir="Personal"]').click();
    await page.reload();
    await expect(row('Personal'), 'and one they closed stays closed').toHaveAttribute('aria-expanded','false');
    expect(await kept()).toEqual(['Course']);
  });
});

test('the open note is scrolled into the explorer once', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page = await context.newPage({viewport:{width:1280,height:600}});
    await page.goto(url);
    const row = dir=>page.locator('.side-tree .side-row[data-dir="'+dir+'"]');
    await page.locator('[data-action="side-twist"][data-dir="Course"]').click();
    await page.locator('[data-action="side-twist"][data-dir="Course/Week 1"]').click();
    await expect(row('Course/Week 1/Lab')).toBeVisible();

    const tree = page.locator('.side-tree');
    await page.evaluate(()=>audit.note('personal-90','Personal'));
    const note = page.locator('.side-note[data-id="personal-90"]');
    await expect(note).toHaveCount(1);
    const inside = ()=>note.evaluate(el=>{const r=el.getBoundingClientRect(),t=el.closest('.side-tree').getBoundingClientRect();return r.top>=t.top-1&&r.bottom<=t.bottom+1;});
    await expect.poll(inside, {message:'the open note is scrolled into the explorer'}).toBe(true);
    const pageScroll = await page.evaluate(()=>window.scrollY);
    expect(pageScroll, 'only the explorer scrolls').toBe(0);
    await tree.evaluate(el=>{el.scrollTop=0;});
    await page.evaluate(()=>audit.draw());
    expect(await tree.evaluate(el=>el.scrollTop), 'a redraw keeps the scroll the user chose').toBe(0);
  });
});

test('old saved folders are dropped, and after its folder is closed a newly opened note still shows it', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page = await context.newPage({viewport:{width:1280,height:800}});
    await page.goto(url);
    await page.evaluate(()=>localStorage.setItem('leo-side-open',JSON.stringify(['Course','Course/Week 1','Personal'])));
    await page.reload();
    const row = dir=>page.locator('.side-tree .side-row[data-dir="'+dir+'"]');
    await expect(row('Course'), 'folders opened only by browsing before do not come back').toHaveAttribute('aria-expanded','false');
    expect(await page.evaluate(()=>localStorage.getItem('leo-side-open'))).toBeNull();
    await page.evaluate(()=>audit.note('course-note','Course'));
    await expect(page.locator('.side-note[data-id="course-note"]')).toHaveCount(1);
    await page.locator('[data-action="side-twist"][data-dir="Course"]').click();
    await expect(row('Course')).toHaveAttribute('aria-expanded','false');
    await page.evaluate(()=>audit.draw());
    await expect(row('Course'), 'a redraw does not reopen a folder the user closed').toHaveAttribute('aria-expanded','false');
    await page.evaluate(()=>audit.note('escaped-note','Course'));
    await expect(page.locator('.side-note[data-id="escaped-note"]'), 'another note in the same folder unfolds it again').toHaveCount(1);
  });
});

test('the explorer takes drops like a file explorer: rows, open folders and note rows all mean their folder', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage({viewport:{width:1280,height:800}});
    await page.goto(url);
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    await page.locator('.side-tree .side-row[data-dir="Course/Week 1"]').click();
    const lecture=page.locator('.side-note[data-id="week-note"]');
    await expect(lecture).toBeVisible();
    const drop=selector=>page.locator(selector).first().evaluate(el=>audit.drop(el));
    expect(await drop('.side-folder[data-drop-dir="Course"] .side-twist')).toBe('Course');
    expect(await drop('.side-kids[data-kids="Course/Week 1"]')).toBe('Course/Week 1');
    expect(await drop('.side-note[data-id="week-note"]')).toBe('Course/Week 1');
    expect(await drop('.side-note[data-id="root-note"]')).toBe('');
    const transfer=await page.evaluateHandle(()=>new DataTransfer());
    await page.locator('.side-note[data-id="root-note"]').dispatchEvent('dragstart',{dataTransfer:transfer});
    await page.locator('.side-note[data-id="course-note"]').dispatchEvent('dragover',{dataTransfer:transfer});
    await expect(page.locator('.side-folder[data-drop-dir="Course"]')).toHaveClass(/drop-ok/);
    await page.locator('.side-tree').dispatchEvent('dragover',{dataTransfer:transfer});
    await expect(page.locator('.drop-ok'), 'a top-level note is already at the top level').toHaveCount(0);
    await page.locator('.side-note[data-id="root-note"]').dispatchEvent('dragover',{dataTransfer:transfer});
    await expect(page.locator('.drop-ok')).toHaveCount(0);
    await lecture.dispatchEvent('dragover',{dataTransfer:transfer});
    await expect(page.locator('.side-folder[data-drop-dir="Course/Week 1"]')).toHaveClass(/drop-ok/);
    await expect(page.locator('.side-kids[data-kids="Course/Week 1"]')).toHaveClass(/drop-in/);
    await expect(lecture, 'a note row is its folder, not a note to combine with').not.toHaveClass(/drop-ok/);
    await lecture.dispatchEvent('drop',{dataTransfer:transfer});
    await expect.poll(()=>page.evaluate(()=>audit.moves)).toEqual([{url:'/api/notes/root-note/move',body:{directory:'Course/Week 1'}}]);
    await expect(page.locator('.drop-ok, .drop-in')).toHaveCount(0);
    expect(await page.evaluate(()=>audit.dragging.note)).toBe(null);
  });
});

test('a folder moves into a folder and keeps its open subfolders open under the new path', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage({viewport:{width:1280,height:800}});
    await page.goto(url);
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    await page.locator('.side-tree .side-row[data-dir="Course/Week 1"]').click();
    const transfer=await page.evaluateHandle(()=>new DataTransfer());
    await page.locator('.side-tree .side-row[data-dir="Course/Week 1"]').dispatchEvent('dragstart',{dataTransfer:transfer});
    await page.locator('.side-note[data-id="week-note"]').dispatchEvent('dragover',{dataTransfer:transfer});
    await expect(page.locator('.drop-ok'), 'a folder cannot go into itself').toHaveCount(0);
    await page.locator('.side-tree').dispatchEvent('dragover',{dataTransfer:transfer});
    await expect(page.locator('.drop-ok'), 'the top level lights only the Folders heading').toHaveCount(1);
    await expect(page.locator('.side-group')).toHaveClass(/drop-ok/);
    const personal=page.locator('.side-tree .side-row[data-dir="Personal"]');
    await personal.dispatchEvent('dragover',{dataTransfer:transfer});
    await expect(page.locator('.side-folder[data-drop-dir="Personal"]')).toHaveClass(/drop-ok/);
    await personal.dispatchEvent('drop',{dataTransfer:transfer});
    await expect.poll(()=>page.evaluate(()=>audit.moves)).toEqual([{url:'/api/dirs/move',body:{from:'Course/Week 1',into:'Personal'}}]);
    expect(await page.evaluate(()=>JSON.parse(localStorage.getItem('leo-side-folders')))).toContain('Personal/Week 1');
  });
});

test('holding a drag over a closed folder opens it, and a redraw mid-drag cannot leave the drag stuck', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.setViewportSize({width:1280,height:800});
    await page.clock.install({time:new Date('2026-11-03T00:00:00Z')});
    await page.goto(url);
    const course=page.locator('.side-tree .side-row[data-dir="Course"]');
    await expect(course).toHaveAttribute('aria-expanded','false');
    await page.clock.pauseAt(new Date('2026-11-03T00:01:00Z'));
    const transfer=await page.evaluateHandle(()=>new DataTransfer());
    const source=page.locator('.side-note[data-id="root-note"]');
    const original=await source.elementHandle();
    await source.dispatchEvent('dragstart',{dataTransfer:transfer});
    await course.dispatchEvent('dragover',{dataTransfer:transfer});
    await page.clock.runFor(599);
    await course.dispatchEvent('dragover',{dataTransfer:transfer});
    await expect(course, 'a passing drag does not open folders').toHaveAttribute('aria-expanded','false');
    await page.clock.runFor(1);
    await expect(course).toHaveAttribute('aria-expanded','true',{timeout:2000});
    await expect(page.locator('.side-tree .side-row[data-dir="Course/Week 1"]')).toBeVisible();
    expect(await original.evaluate(el=>el.isConnected)).toBe(true);
    await original.dispatchEvent('dragend',{dataTransfer:transfer});
    expect(await page.evaluate(()=>audit.dragging.note)).toBe(null);
    await expect(page.locator('body')).not.toHaveClass(/dragging/);
    await source.dispatchEvent('dragstart',{dataTransfer:transfer});
    await original.evaluate(el=>el.remove());
    await original.dispatchEvent('dragend',{dataTransfer:transfer});
    expect(await page.evaluate(()=>audit.dragging.note)).toBe(null);
    await expect(page.locator('body')).not.toHaveClass(/dragging/);
    expect(await page.evaluate(()=>JSON.parse(localStorage.getItem('leo-side-folders')||'[]')), 'a folder opened by a passing drag is not kept').not.toContain('Course');
    const files=await page.evaluateHandle(()=>{const data=new DataTransfer();data.items.add(new File(['x'],'x.txt',{type:'text/plain'}));return data;});
    const personal=page.locator('.side-tree .side-row[data-dir="Personal"]');
    await personal.dispatchEvent('dragover',{dataTransfer:files});
    await page.clock.runFor(800);
    await expect(personal, 'files dragged in from outside do not open folders').toHaveAttribute('aria-expanded','false');
    await expect(page.locator('.side-group')).not.toHaveClass(/drop-ok/);
  });
});

test('the sidebar toggle keeps its place while the explorer folds, and the fold slides', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    for(const [width,full] of [[1280,284],[1100,240]]) {
      await page.setViewportSize({width,height:800});
      await page.goto(url);
      for(const mode of ['web','mac-full','mac']) {
        const moves=mode==='mac';
        await page.evaluate(mode=>{audit.mac(mode!=='web');document.documentElement.toggleAttribute('data-fullscreen',mode==='mac-full');},mode);
        const toggle=page.locator('.side-fold'), side=page.locator('#side');
        const open=await toggle.boundingBox();
        expect((await side.boundingBox()).width).toBe(full);
        const slid=()=>page.evaluate(()=>{window.slid=new Promise(resolve=>document.body.addEventListener('transitionrun',e=>{if(e.propertyName==='--side')resolve(true);}));});
        await slid();
        await toggle.click();
        expect(await page.evaluate(()=>window.slid)).toBe(true);
        await expect(page.locator('body')).not.toHaveClass(/side-folding/);
        await expect(page.locator('.side-explorer')).toBeHidden();
        expect((await side.boundingBox()).width).toBe(52);
        if(moves) expect((await toggle.boundingBox()).y,'folded beside the window buttons, the toggle drops below them').toBeGreaterThanOrEqual(44);
        else expect(await toggle.boundingBox()).toEqual(open);
        await expect(page.locator('.side-rail')).toBeVisible();
        const clear=await page.locator('.bar').evaluate(el=>el.getBoundingClientRect().left+parseFloat(getComputedStyle(el).paddingLeft));
        const folded=await toggle.boundingBox();
        expect(clear, 'the toolbar starts after the toggle').toBeGreaterThanOrEqual(folded.x+folded.width);
        await slid();
        await toggle.click();
        expect(await page.evaluate(()=>window.slid)).toBe(true);
        await expect(page.locator('body')).not.toHaveClass(/side-folding/);
        await expect(page.locator('.side-explorer')).toBeVisible();
        expect((await side.boundingBox()).width).toBe(full);
        expect(await toggle.boundingBox()).toEqual(open);
      }
      await page.evaluate(()=>{audit.mac(false);document.documentElement.removeAttribute('data-fullscreen');});
    }
    await page.emulateMedia({reducedMotion:'reduce'});
    await page.locator('.side-fold').click();
    await expect(page.locator('body')).not.toHaveClass(/side-folding/);
    await expect(page.locator('.side-explorer')).toBeHidden();
  });
});


async function dragPane(page,grip,dx) {
  const box=await grip.boundingBox(), x=box.x+box.width/2, y=box.y+Math.min(140,box.height/2);
  await page.mouse.move(x,y);
  await page.mouse.down();
  await page.mouse.move(x+dx,y,{steps:5});
  await page.mouse.up();
}

test('file explorer width resizes, survives redraws and reloads, and keeps its bounds', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.setViewportSize({width:1440,height:900});
    await page.goto(url);
    const grip=page.getByRole('separator',{name:'File explorer width',exact:true}), side=page.locator('#side');
    await expect(grip).toBeVisible();
    await page.locator('#draft').fill('Keep my resized draft 文字');
    await dragPane(page,grip,80);
    expect(Math.round((await side.boundingBox()).width)).toBe(364);
    await grip.focus();
    await page.keyboard.press('ArrowLeft');
    expect(Math.round((await side.boundingBox()).width)).toBe(332);
    await page.evaluate(()=>audit.draw());
    await expect(grip).toBeFocused();
    await expect(page.locator('#draft')).toHaveValue('Keep my resized draft 文字');
    await page.reload();
    expect(Math.round((await side.boundingBox()).width)).toBe(332);
    await page.locator('.side-fold').click();
    await expect(grip).toBeHidden();
    await expect(page.locator('body')).not.toHaveClass(/side-folding/);
    expect(Math.round((await side.boundingBox()).width)).toBe(52);
    await page.locator('.side-fold').click();
    await expect(page.locator('body')).not.toHaveClass(/side-folding/);
    expect(Math.round((await side.boundingBox()).width)).toBe(332);
    await grip.focus();
    await page.keyboard.press('End');
    expect(Math.round((await side.boundingBox()).width)).toBe(Number(await grip.getAttribute('aria-valuemax')));
    await page.keyboard.press('Shift+ArrowLeft');
    expect(Math.round((await side.boundingBox()).width)).toBe(Number(await grip.getAttribute('aria-valuemax'))-96);
    await grip.dblclick();
    expect(Math.round((await side.boundingBox()).width)).toBe(284);
    await page.evaluate(()=>localStorage.setItem('leo-side-width','Infinity'));
    await page.reload();
    expect(Math.round((await side.boundingBox()).width)).toBe(284);
    await page.setViewportSize({width:390,height:844});
    await expect(grip).toBeHidden();
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  });
});

async function openFixtureChat(page) {
  await page.emulateMedia({reducedMotion:'reduce'});
  await page.evaluate(()=>{
    window.fixtureChat=window.leoChat.create({render:text=>'<p>'+text+'</p>',escape:text=>String(text).replace(/[&<>]/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;'}[ch]))});
    fixtureChat.toggle(true);
  });
  await expect(page.locator('#chat-input')).toBeVisible();
}

test('Chats divider resizes independently, keeps drafts and restores its width', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.setViewportSize({width:1600,height:900});
    await page.goto(url);
    await openFixtureChat(page);
    const grip=page.getByRole('separator',{name:'Chats list width',exact:true}), history=page.locator('.chat-history');
    await expect(history).toBeHidden();
    await page.locator('.chat-history-toggle').click();
    await expect(grip).toBeVisible();
    await page.locator('#chat-input').fill('Keep this draft 文字');
    const start=Math.round((await history.boundingBox()).width), total=(await page.locator('.chat').boundingBox()).width;
    await dragPane(page,grip,64);
    expect(Math.round((await history.boundingBox()).width)).toBe(start+64);
    expect((await page.locator('.chat').boundingBox()).width).toBe(total);
    await grip.focus();
    await page.keyboard.press('ArrowLeft');
    const kept=Math.round((await history.boundingBox()).width);
    expect(kept).toBe(start+32);
    await expect(page.locator('#chat-input')).toHaveValue('Keep this draft 文字');
    await page.locator('.chat-history-toggle').click();
    await expect(grip).toBeHidden();
    await page.locator('.chat-history-toggle').click();
    expect(Math.round((await history.boundingBox()).width)).toBe(kept);
    await page.reload();
    await openFixtureChat(page);
    expect(Math.round((await history.boundingBox()).width)).toBe(kept);
    await grip.focus();
    await page.keyboard.press('End');
    expect((await page.locator('.chat-main').boundingBox()).width).toBeGreaterThanOrEqual(300);
    await grip.dblclick();
    expect(Math.round((await history.boundingBox()).width)).toBe(start);
    await page.setViewportSize({width:390,height:844});
    await page.locator('.chat-history-toggle').click();
    await expect(history).toBeVisible();
    await expect(grip).toBeHidden();
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  });
});


test('a narrow Felix panel lays Chats over the chat without saving it, and slims the chat column', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.setViewportSize({width:1280,height:800});
    await page.goto(url);
    await openFixtureChat(page);
    await page.evaluate(()=>document.documentElement.style.setProperty('--chat-w','470px'));
    const history=page.locator('.chat-history'), panel=page.locator('.chat');
    await expect(panel).toHaveClass(/source-narrow/);
    await expect(panel).not.toHaveClass(/\bslim\b/);
    await page.locator('.chat-history-toggle').click();
    await expect(history).toBeVisible();
    await expect(page.getByRole('separator',{name:'Chats list width',exact:true})).toBeHidden();
    await page.getByRole('button',{name:'Back to the chat'}).click();
    await expect(panel).not.toHaveClass(/with-history/);
    expect(await page.evaluate(()=>localStorage.getItem('leo-chat-sidebar'))).toBeNull();
    await page.evaluate(()=>document.documentElement.style.setProperty('--chat-w','420px'));
    await expect(panel).toHaveClass(/\bslim\b/);
    expect(await page.evaluate(()=>{const main=document.querySelector('.chat-main'),send=document.querySelector('.chat-send').getBoundingClientRect(),box=main.getBoundingClientRect();return send.right<=box.right+0.5&&main.scrollWidth<=main.clientWidth;})).toBe(true);
  });
});

test('explorer resize handle stays reachable beside Felix in narrow desktop windows', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    for(const width of [1000,1100,1280]) {
      await page.setViewportSize({width,height:800});
      await page.goto(url);
      const grip=page.getByRole('separator',{name:'File explorer width',exact:true});
      await grip.focus();
      await page.keyboard.press('End');
      await openFixtureChat(page);
      await page.evaluate(()=>audit.draw());
      await expect(grip).toBeVisible();
      expect(await grip.evaluate(el=>{const box=el.getBoundingClientRect();return document.elementFromPoint(box.x+box.width/2,box.y+140)===el;})).toBe(true);
      const box=await grip.boundingBox();
      await page.mouse.move(box.x+box.width/2,box.y+140);
      await page.mouse.down();
      await expect(page.locator('body')).toHaveClass(/pane-resizing/);
      await page.mouse.up();
      await expect(page.locator('body')).not.toHaveClass(/pane-resizing/);
    }
  });
});

test('sidebar folding waits for the actual transition rather than a timer', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.setViewportSize({width:1280,height:800});
    await page.clock.install();
    await page.goto(url);
    const body=page.locator('body'), side=page.locator('#side');
    await page.evaluate(()=>document.addEventListener('click',event=>{
      if(!event.target.closest('.side-fold'))return;
      document.body.getAnimations().find(run=>run.transitionProperty==='--side')?.pause();
    }));
    async function fold() {
      await page.locator('.side-fold').click();
      expect(await body.evaluate(el=>el.getAnimations().find(run=>run.transitionProperty==='--side')?.playState)).toBe('paused');
    }
    for(const expected of [52,284]) {
      await fold();
      await page.clock.runFor(300);
      await expect(body).toHaveClass(/side-folding/);
      await body.evaluate(el=>{for(const run of el.getAnimations())if(run.transitionProperty==='--side')run.finish();});
      await expect(body).not.toHaveClass(/side-folding/);
      expect((await side.boundingBox()).width).toBe(expected);
    }
    await fold();
    await body.evaluate(el=>{
      const run=el.getAnimations().find(a=>a.transitionProperty==='--side');
      run.currentTime=90;
      getComputedStyle(el).getPropertyValue('--side');
    });
    await fold();
    await page.clock.runFor(300);
    await expect(body).toHaveClass(/side-folding/);
    await body.evaluate(el=>{for(const run of el.getAnimations())if(run.transitionProperty==='--side')run.finish();});
    await expect(body).not.toHaveClass(/side-folding/);
    expect((await side.boundingBox()).width).toBe(284);
  });
});

test('a folder slide survives a redraw, reverses from where it is, and never animates the list twice', async ({ browser }) => {
  await sidebarFixture(browser, async (context, url) => {
    const page = await context.newPage({ viewport: { width: 1280, height: 800 } });
    await page.goto(url);
    const twist = page.locator('[data-action="side-twist"][data-dir="Course"]');
    await twist.click();
    const kids = page.locator('.side-kids[data-kids="Course"]');
    await expect(kids).toBeVisible();
    await expect.poll(() => kids.evaluate(el => el.getAnimations().length)).toBe(0);
    const full = await kids.evaluate(el => el.scrollHeight);
    expect(full).toBeGreaterThan(20);
    await page.evaluate(() => {
      document.querySelector('[data-action="side-twist"][data-dir="Course"]').click();
      const el = document.querySelector('.side-kids[data-kids="Course"]');
      el.getAnimations()[0].pause();
    });
    const middle = await kids.evaluate(el => {
      const run = el.getAnimations()[0];
      run.currentTime = run.effect.getTiming().duration / 2;
      return el.getBoundingClientRect().height;
    });
    expect(middle).toBeGreaterThan(0);
    expect(middle).toBeLessThan(full);
    await page.evaluate(() => window.audit.draw());
    await expect(kids).toHaveCount(1);
    expect(await kids.evaluate(el => getComputedStyle(el).overflow), 'a redraw keeps the slide clipped').toBe('hidden');

    const reversing = await page.evaluate(() => {
      document.querySelector('[data-action="side-twist"][data-dir="Course"]').click();
      const el = document.querySelector('.side-kids[data-kids="Course"]');
      const run = el.getAnimations()[0];
      run.pause();
      return { start: el.getBoundingClientRect().height, end: run.effect.getKeyframes().at(-1).height };
    });
    expect(Math.abs(reversing.start - middle)).toBeLessThan(2);
    expect(parseFloat(reversing.end)).toBe(full);
    await kids.evaluate(el => el.getAnimations()[0].finish());
    await expect.poll(() => kids.evaluate(el => el.getBoundingClientRect().height)).toBe(full);

    await page.evaluate(() => {
      document.querySelector('[data-action="side-twist"][data-dir="Course"]').click();
      document.querySelector('.side-kids[data-kids="Course"]').getAnimations()[0].finish();
    });
    await expect(kids).toHaveCount(0);
    const moving = await page.evaluate(() => [...document.querySelectorAll('.side-tree .side-folder, .side-tree .side-note')].filter(el => el.getAnimations().length).length);
    expect(moving, 'closing does not slide the rows below a second time').toBe(0);
  });
});

test('a popup menu with find filters as you type, and Enter takes the first match', async ({ browser }) => {
  await sidebarFixture(browser, async (context, url) => {
    const page = await context.newPage({ viewport: { width: 1280, height: 800 } });
    await page.goto(url);
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeVisible();
    await page.evaluate(() => {
      const owner = document.querySelector('.side-note');
      popupMenu(owner, { label: 'Move', find: 'Find a folder', rows: ['Top level', 'Biology', 'History'].map((text) => ({ text, run: () => { window.picked = text; } })) });
    });
    const find = page.getByRole('textbox', { name: 'Find a folder' });
    await expect(find).toBeFocused();
    await find.fill('hist');
    await expect(page.getByRole('menuitem')).toHaveText(['History']);
    await find.press('Enter');
    await expect.poll(() => page.evaluate(() => window.picked)).toBe('History');
    await expect(page.locator('.popup-menu')).toHaveCount(0);
  });
});

test('inline edit keeps the field after a refused save, and Escape never saves', async ({ browser }) => {
  await sidebarFixture(browser, async (context, url) => {
    const page = await context.newPage({ viewport: { width: 1280, height: 800 } });
    await page.goto(url);
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeVisible();
    await page.evaluate(() => {
      window.saves = [];
      inlineEdit({ left: 20, top: 200, width: 200, height: 30 }, { id: 'name', value: 'Old', label: 'Name', at: { kind: 'note', id: 'root-note' }, save: async (text) => { window.saves.push(text); return text !== 'Taken'; } });
    });
    const input = page.getByRole('textbox', { name: 'Name' });
    await input.fill('Taken');
    await input.press('Enter');
    await expect.poll(() => page.evaluate(() => window.saves)).toEqual(['Taken']);
    await expect(input).toHaveValue('Taken');
    await expect(input).toBeFocused();
    await input.fill('Dropped');
    await input.press('Escape');
    await expect(input).toHaveCount(0);
    expect(await page.evaluate(() => window.saves)).toEqual(['Taken']);
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeFocused();
  });
});

test('explorer blank context menu offers creation and stays inside the viewport', async ({ browser }) => {
  await sidebarFixture(browser, async (context, url) => {
    const page = await context.newPage({ viewport: { width: 1280, height: 800 } });
    await page.goto(url);
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeVisible();
    await page.locator('.side-tree').dispatchEvent('contextmenu', { clientX: 1279, clientY: 799 });
    await expect(page.getByRole('menuitem', { name: 'New note', exact: true })).toBeFocused();
    await expect(page.getByRole('menuitem')).toHaveCount(2);
    const box = await page.getByRole('menu', { name: 'Folder options' }).boundingBox();
    expect(box.x + box.width).toBeLessThanOrEqual(1280);
    expect(box.y + box.height).toBeLessThanOrEqual(800);
    await page.keyboard.press('End');
    await expect(page.getByRole('menuitem', { name: 'New folder', exact: true })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(page.locator('.side-tree')).toBeFocused();
  });
});

test('the paper grain is there and never takes faint text below 4.5:1 on a 9x9 grid of background pixels', async ({ browser }) => {
  for (const colorScheme of ['light', 'dark']) {
    for (const [width, lefts] of [[1200, [20, 340]], [400, [20]]]) {
      const context = await browser.newContext({ viewport: { width, height: 600 }, colorScheme });
      const page = await context.newPage();
      await page.setContent(`<style>${pageCss()}</style><body><main style="height:560px"></main></body>`);
      const faint = await page.evaluate(() => { const probe = document.createElement('i'); probe.style.color = 'var(--faint)'; document.body.append(probe); const color = getComputedStyle(probe).color; probe.remove(); return color.match(/\d+/g).slice(0, 3).map(Number); });
      for (const left of lefts) {
        const shot = (await page.screenshot({ clip: { x: left, y: 40, width: 72, height: 72 } })).toString('base64');
        const result = await page.evaluate(async ([shot, faint]) => {
          const img = new Image();
          img.src = 'data:image/png;base64,' + shot;
          await img.decode();
          const canvas = document.createElement('canvas');
          canvas.width = canvas.height = 72;
          const draw = canvas.getContext('2d');
          draw.drawImage(img, 0, 0);
          const lin = v => { v /= 255; return v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4; };
          const lum = c => .2126 * lin(c[0]) + .7152 * lin(c[1]) + .0722 * lin(c[2]);
          const seen = new Set();
          let min = Infinity;
          for (let row = 0; row < 9; row++) for (let col = 0; col < 9; col++) {
            const pixel = [...draw.getImageData(col * 8, row * 8, 1, 1).data].slice(0, 3);
            seen.add(pixel.join());
            const [hi, lo] = [lum(pixel), lum(faint)].sort((a, b) => b - a);
            min = Math.min(min, (hi + .05) / (lo + .05));
          }
          return { min, distinct: seen.size };
        }, [shot, faint]);
        expect(result.min, `${colorScheme} ${width}px at ${left}`).toBeGreaterThanOrEqual(4.5);
        if (width === 1200 && left === 20) expect(result.distinct, `${colorScheme} ${width}px shows grain`).toBeGreaterThan(1);
      }
      await context.close();
    }
  }
});
