const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');
const { pageCss } = require('../page-source');
const http = require('node:http');

test('dictation preferences keep drafts and caret while shortcut and history update', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const source = fs.readFileSync(path.join(web, 'app/refresh.js'), 'utf8') + fs.readFileSync(path.join(web, 'app/settings-navigation.js'), 'utf8') + fs.readFileSync(path.join(web, 'app/dictation.js'), 'utf8');
  const core = fs.readFileSync(path.join(web, 'app/core.js'), 'utf8');
  const keys = core.slice(core.indexOf('const SHORTCUT_KEYS'), core.indexOf('async function dictateHint'));
  const css = pageCss();
  const initial = () => ({ settings: { shortcut: 'option+space', mode: 'both', custom_words: ['Initial'], fillers: true, popup: 'live', top: false }, microphones: ['External microphone'], shortcut: {}, recent: [{ id: 1, text: '<img src=x onerror=bad()>' }] });
  let view = initial();
  const script = `const app=document.querySelector('#app'),$=s=>document.querySelector(s); let seq=1,state={view:'settings',section:'dictation',dictation:${JSON.stringify(view)}};
    const pulse=()=>{},cascade=()=>{},overlapOut=()=>{},ICON={chevron:''}, svg=p=>'<svg>'+p+'</svg>', fail=e=>console.error(e); const esc=s=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
    function settingsOption(value,label,current){return '<option value="'+esc(value)+'"'+(value===current?' selected':'')+'>'+esc(label)+'</option>';}
    async function api(url,options={}){const response=await fetch(url,{method:options.method||'GET',headers:{'Content-Type':'application/json'},body:options.body?JSON.stringify(options.body):undefined});return response.json();}
    const ON_MAC=true;
    ${keys}
    ${source}
    app.innerHTML=dictationShortcutRow(state.dictation)+dictationCards(state.dictation);
    app.addEventListener('click',e=>{const b=e.target.closest('[data-action]');if(!b)return;const actions={'segment-pick':()=>pickSegment(b),'dictation-record':()=>dictationChange({action:'record_shortcut'}),'dictation-clear':()=>dictationChange({action:'clear'}),'dictation-clear-now':()=>dictationChange({action:'clear'})};if(actions[b.dataset.action])actions[b.dataset.action]();});
    pollDictation(seq,++dictationGeneration);`;
  const server = http.createServer(async (request, response) => {
    if (request.url === '/api/dictation') {
      response.setHeader('Content-Type', 'application/json');
      if (request.method === 'POST') {
        let body = ''; for await (const part of request) body += part;
        const change = JSON.parse(body);
        if (change.action === 'save') { view.settings = change.settings; view.shortcut = {}; }
        if (change.action === 'record_shortcut') view.shortcut = { recording: true, held: '', captured: '' };
        if (change.action === 'clear') view.recent = [];
      }
      response.end(JSON.stringify(view)); return;
    }
    if (request.url === '/fixture.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); response.end(script); return; }
    response.setHeader('Content-Type', 'text/html; charset=utf-8'); response.end(`<html><head><style>${css}</style></head><body><main id="app"></main><script src="/fixture.js"></script></body></html>`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    for (const colorScheme of ['light', 'dark']) {
      view = initial();
      const context = await browser.newContext({ colorScheme });
      const page = await context.newPage();
      await page.goto(`http://127.0.0.1:${server.address().port}`);
      const words = page.locator('[data-dictation="custom_words"]');
      await words.fill('Charlton\n東京');
      await words.evaluate(el => { el.focus(); el.setSelectionRange(3, 3); });
      await page.waitForTimeout(1400);
      await expect(words).toBeFocused();
      await expect(words).toHaveValue('Charlton\n東京');
      expect(await words.evaluate(el => el.selectionStart)).toBe(3);
      await expect.poll(() => view.settings.custom_words).toEqual(['Charlton', '東京']);
      await expect(page.locator('#dictation-result')).toHaveText('Saved');
      await expect(page.getByRole('button', { name: /^Save/ })).toHaveCount(0);
      const shortcut = await page.locator('.set-line:has([data-dictation="shortcut"])').evaluate(line => [...line.querySelectorAll('.set-label, .shortcut-field, button:not([hidden])')].map(el => Math.round(el.getBoundingClientRect().top + el.getBoundingClientRect().height / 2)));
      expect(Math.max(...shortcut) - Math.min(...shortcut)).toBeLessThan(3);
      const copy=page.locator('[data-action="dictation-copy"][data-id="1"]');
      const keptCopy=await copy.elementHandle();
      await copy.focus();
      view.recent.unshift({id:2,text:'Newest dictation'});
      await expect(page.locator('[data-action="dictation-copy"][data-id="2"]')).toBeVisible();
      expect(await keptCopy.evaluate(el=>el.isConnected)).toBe(true);
      await expect(copy).toBeFocused();
      await expect(page.locator('#dictation-recent img')).toHaveCount(0);
      await page.getByRole('button', { name: 'Change', exact: true }).click();
      await expect(page.locator('#dictation-shortcut-status')).toHaveText('Esc to cancel');
      await expect(page.locator('#dictation-shortcut-field')).toHaveClass(/recording/);
      view.shortcut = { recording: true, held: 'option', captured: '' };
      await expect(page.locator('#dictation-shortcut-field kbd')).toHaveText('\u2325');
      view.shortcut = { recording: false, held: '', captured: 'fn' };
      await expect(page.locator('#dictation-shortcut-status')).toHaveText('Saved. Hold or press Fn to dictate.');
      await expect(page.locator('[data-dictation="shortcut"]')).toHaveValue('fn');
      await expect(words).toHaveValue('Charlton\n東京');
      await page.getByRole('radiogroup', { name: 'Popup position' }).getByRole('radio', { name: 'Top' }).click();
      await page.getByRole('radiogroup', { name: 'Activation' }).getByRole('radio', { name: 'Tap', exact: true }).click();
      await page.getByRole('switch', { name: 'Remove filler words' }).uncheck();
      await expect.poll(() => [view.settings.top, view.settings.mode, view.settings.fillers]).toEqual([true, 'tap', false]);
      await expect(page.getByRole('radiogroup', { name: 'Popup position' }).getByRole('radio', { name: 'Top' })).toHaveAttribute('aria-checked', 'true');
      await page.getByRole('button', { name: 'Clear history', exact: true }).click();
      await expect(page.locator('#dictation-recent')).toContainText('Your dictations will appear here.');
      await context.close();
    }
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
});
