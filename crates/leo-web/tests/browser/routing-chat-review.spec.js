const { test, expect } = require('@playwright/test');
const { signIn } = require('./sign-in');
test.beforeEach(async ({ page }) => {
  await signIn(page);
});
async function note(page, title) {
  return (await page.request.post('/api/notes', { data: { title, body: 'Original paragraph.' } })).json();
}
test('a deleted note keeps its draft and offers an explicit path to Settings and Trash', async ({ page }) => {
  const item = await note(page, 'Deleted while editing');
  await page.goto('/#/n/' + item.id);
  await page.locator('#doc .blk').first().click();
  await page.request.delete('/api/notes/' + item.id);
  await page.locator('#doc textarea.line-edit').fill('Locally retained edits after deletion.');
  await expect(page.locator('#save-state')).toContainText('draft kept');
  await page.locator('#side [data-action="settings"]').click();
  await expect(page.locator('#title')).toHaveText(item.title);
  await page.getByRole('button', { name: 'Leave, draft kept', exact: true }).click();
  await expect(page.locator('.settings-layout')).toBeVisible();
  expect(await page.evaluate(() => Object.keys(localStorage).some(key => key.startsWith('leo-draft-v1:')))).toBe(true);
  await page.goto('/#/settings/trash');
  await expect(page.locator('#app')).toContainText(item.title);
  await page.request.post('/api/trash/' + item.id + '/restore', { data: {} });
  await page.goto('/#/n/' + item.id);
  await expect(page.locator('#doc')).toContainText('Locally retained edits after deletion.');
  await expect(page.locator('#save-state')).toHaveText('Saved');
});
test('a draft that could not be persisted cannot use the leave escape', async ({ page }) => {
  const refuseDrafts = () => {
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (key.startsWith('leo-draft-v1:')) throw new DOMException('Full', 'QuotaExceededError');
      return set.call(this, key, value);
    };
  };
  await page.addInitScript(refuseDrafts);
  await page.evaluate(refuseDrafts);
  const item = await note(page, 'Memory-only draft');
  await page.goto('/#/n/' + item.id);
  await page.route('**/api/notes/' + item.id, route => route.request().method() === 'PATCH' ? route.fulfill({ status: 503, json: { error: 'Save refused. Try again.' } }) : route.continue());
  await page.locator('#doc .blk').first().click();
  await page.locator('#doc textarea.line-edit').fill('Keep this editor open.');
  await expect(page.locator('#save-state')).toContainText('keep this page open');
  await page.locator('#side [data-action="settings"]').click();
  await expect(page.locator('#title')).toHaveText(item.title);
  await expect(page.getByRole('button', { name: 'Leave, draft kept', exact: true })).toHaveCount(0);
});
test('rapid note choices during an in-flight save honor the latest destination', async ({ page }) => {
  const current = await note(page, 'A current draft'), first = await note(page, 'B first target'), latest = await note(page, 'C latest target');
  await page.goto('/#/n/' + current.id);
  let release; const waiting = new Promise(resolve => { release = resolve; });
  await page.route('**/api/notes/' + current.id, async route => { if (route.request().method() === 'PATCH') await waiting; await route.continue(); });
  await page.locator('#doc .blk').first().click();
  await page.locator('#doc textarea.line-edit').fill('The latest saved body.');
  await page.locator('#side .side-note[data-id="' + first.id + '"]').click();
  await page.locator('#side .side-note[data-id="' + latest.id + '"]').click();
  release();
  await expect(page.locator('#title')).toHaveText(latest.title);
  expect((await (await page.request.get('/api/notes/' + current.id)).json()).body).toBe('The latest saved body.');
});
test('chat history retains its delete control through confirmation and handles failure', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  const title = 'A longer conversation title with useful distinguishing words';
  await page.request.put('/api/chats/review-history', { data: { title, mode: 'chat', messages: [{ role: 'user', text: title }, { role: 'assistant', text: 'A saved answer.' }] } });
  await page.reload();
  await page.locator('#chat-toggle').click();
  const history = page.locator('#chat-history');
  if (!(await history.isVisible())) await page.locator('.chat-head [data-chat="history"]').click();
  await expect(history).toBeVisible();
  await expect(history.locator('[data-chat="new"]')).toBeVisible();
  await expect(page.locator('.chat-head [data-chat="new"]')).toBeHidden();
  const row = history.locator('.chat-history-row').filter({ has: page.locator('[data-id="review-history"]') });
  const button = row.locator('.chat-history-x');
  await button.evaluate(el => {
    window.retainedDelete = el;
    el.addEventListener('click', () => {
      window.deleteWidths = [];
      const started = performance.now();
      const sample = () => {
        window.deleteWidths.push(el.getBoundingClientRect().width);
        if (performance.now() - started < 400) requestAnimationFrame(sample);
        else window.deleteSampled = true;
      };
      sample();
    }, { once: true });
  });
  await button.click();
  expect(await button.evaluate(el => el === window.retainedDelete)).toBe(true);
  await expect(button).toHaveClass(/sure/);
  await expect(button).toBeFocused();
  await page.waitForFunction(() => window.deleteSampled);
  const widths = await page.evaluate(() => window.deleteWidths);
  expect(new Set(widths.map(value => Math.round(value))).size).toBeGreaterThan(2);
  expect(Math.max(...widths) - Math.min(...widths)).toBeGreaterThan(8);
  const font = await row.locator('.chat-history-title').evaluate(el => parseFloat(getComputedStyle(el).fontSize));
  expect(font).toBeLessThanOrEqual(13);
  await page.keyboard.press('Escape');
  await expect(button).not.toHaveClass(/sure/);
  await page.route('**/api/chats/review-history', route => route.request().method() === 'DELETE' ? route.fulfill({ status: 503 }) : route.continue());
  await button.click(); await button.click();
  await expect(row).toBeVisible();
  await expect(page.locator('.toast')).toContainText('could not delete');
  await expect(button).toBeEnabled();
  await page.unroute('**/api/chats/review-history');
  await button.click();
  await expect(row).toHaveCount(0);
});

for (const layout of ['desktop', 'compact', 'source']) test(`Chats history ${layout} slides without replacing content and reverses from its current frame`, async ({ page }, testInfo) => {
  await page.setViewportSize(layout === 'compact' ? { width: 390, height: 844 } : { width: 1600, height: 900 });
  const chats = Array.from({ length: 40 }, (_, i) => ({ id: `motion-${i}`, title: `Saved conversation ${i}`, count: 2, updated_at: new Date().toISOString() }));
  await page.route('**/api/chats', route => route.fulfill({ json: chats }));
  await page.locator('#chat-toggle').click();
  if (layout === 'source') {
    await page.evaluate(() => { document.body.classList.add('with-source'); document.querySelector('#chat').style.width = '420px'; });
    await expect(page.locator('#chat')).toHaveClass(/source-narrow/);
  }
  const toggle = page.locator('.chat-head .chat-history-toggle');
  const history = page.locator('#chat-history');
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(toggle).toHaveAttribute('aria-controls', 'chat-history');
  await expect(history).toBeHidden();
  expect(await history.evaluate(el => el.inert)).toBe(true);
  await page.locator('#chat-input').fill('Keep this question and its caret.');
  await page.locator('#chat-input').evaluate(el => el.setSelectionRange(5, 13));
  await page.evaluate(() => {
    const panel = document.querySelector('#chat');
    panel.addEventListener('click', event => {
      if (!event.target.closest('[data-chat="history"]')) return;
      const history = document.querySelector('#chat-history');
      getComputedStyle(history).opacity;
      window.historyMotion = history.getAnimations().filter(run => run.effect.getTiming().duration > 0);
      for (const run of window.historyMotion) run.pause();
    });
  });
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(history.locator('.chat-history-row')).toHaveCount(40);
  const frames = await page.evaluate(() => {
    const history = document.querySelector('#chat-history');
    const main = document.querySelector('.chat-main');
    return [0, .25, .5, .75, 1].map(part => {
      for (const run of window.historyMotion) run.currentTime = run.effect.getTiming().duration * part;
      const rect = history.getBoundingClientRect();
      return { x: rect.x, width: rect.width, main: main.getBoundingClientRect().x, opacity: Number(getComputedStyle(history).opacity) };
    });
  });
  expect(await page.evaluate(() => historyMotion.length), 'the toggle starts native motion').toBeGreaterThan(0);
  expect(new Set(frames.map(frame => Math.round(frame.x))).size, 'history travels through intermediate positions').toBeGreaterThan(2);
  expect(Math.max(...frames.map(frame => frame.width)) - Math.min(...frames.map(frame => frame.width)), 'text keeps its own width during the reveal').toBeLessThan(1);
  if (layout === 'desktop') expect(Math.abs(frames.at(-1).main - frames[0].main)).toBeGreaterThan(100);
  else expect(Math.abs(frames.at(-1).main - frames[0].main)).toBeLessThan(1);
  expect(await page.locator('.chat-main').evaluate(el => el.inert)).toBe(layout !== 'desktop');
  await page.evaluate(() => {
    const list = document.querySelector('#chat-history-list');
    list.scrollTop = 180;
    window.historyRow = list.firstElementChild;
    window.historyScroll = list.scrollTop;
  });
  const close = layout === 'desktop' ? toggle : history.locator('.chat-history-back');
  const reversal = await close.evaluate(el => {
    const history = document.querySelector('#chat-history');
    el.focus({ preventScroll: true });
    for (const run of historyMotion) run.currentTime = run.effect.getTiming().duration * .5;
    const before = history.getBoundingClientRect().x;
    el.click();
    const after = history.getBoundingClientRect().x;
    const runs = history.getAnimations().map(run => ({ what: run.transitionProperty, at: run.currentTime, state: run.playState, pending: run.pending }));
    for (const run of historyMotion) {
      run.pause();
      run.currentTime = 0;
    }
    return { before, after, first: history.getBoundingClientRect().x, runs };
  });
  expect(Math.abs(reversal.first - reversal.before), `the reversed slide starts from the frame it was closed on ${JSON.stringify(reversal)}`).toBeLessThan(1);
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await page.evaluate(() => { for (const run of historyMotion) run.finish(); });
  await expect(history).toBeHidden();
  expect(await history.evaluate(el => el.inert)).toBe(true);
  expect(await page.locator('.chat-main').evaluate(el => el.inert)).toBe(false);
  await expect(toggle).toBeFocused();
  await toggle.click();
  await page.evaluate(() => { for (const run of historyMotion) run.finish(); });
  await expect(history).toBeVisible();
  expect(await history.evaluate(el => document.querySelector('#chat-history-list').firstElementChild === window.historyRow)).toBe(true);
  expect(await history.locator('.chat-history-list').evaluate(el => el.scrollTop)).toBe(await page.evaluate(() => historyScroll));
  await expect(page.locator('#chat-input')).toHaveValue('Keep this question and its caret.');
  expect(await page.locator('#chat-input').evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([5, 13]);
  await page.screenshot({ path: testInfo.outputPath(`chats-${layout}-open.png`) });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await close.evaluate(el => { el.focus(); el.click(); });
  await expect(history).toBeHidden();
  await toggle.click();
  await expect(history).toBeVisible();
  expect(await history.evaluate(el => el.getAnimations().filter(run => run.effect.getTiming().duration > 0).length)).toBe(0);
});

test('narrowing Felix below the overlay width closes Chats without animating over the chat', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.route('**/api/chats', route => route.fulfill({ json: [{ id: 'narrow-history', title: 'Saved conversation', count: 2, updated_at: new Date().toISOString() }] }));
  await page.locator('#chat-toggle').click();
  const panel = page.locator('#chat');
  const history = page.locator('#chat-history');
  await panel.evaluate(el => { el.style.width = '640px'; });
  await page.locator('.chat-head .chat-history-toggle').click();
  await expect(history).toBeVisible();
  await expect(panel).not.toHaveClass(/source-narrow/);
  await panel.evaluate(el => { el.style.width = '440px'; });
  await expect(panel).toHaveClass(/source-narrow/);
  expect(await history.evaluate(el => el.getAnimations().length)).toBe(0);
  await expect(history).toBeHidden();
});

test('Escape on a pending history deletion leaves the streaming answer running', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.request.put('/api/chats/escape-history', { data: { title: 'Keep this saved conversation', mode: 'chat', messages: [{ role: 'user', text: 'Question' }, { role: 'assistant', text: 'Answer' }] } });
  let release;
  const waiting = new Promise(done => { release = done; });
  await page.route('**/api/chat', async route => {
    await waiting;
    await route.fulfill({ status: 200, contentType: 'application/x-ndjson', body: '{"t":"Finished answer."}\n{"done":true}\n' });
  });
  try {
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('Hold this answer while I inspect history.');
    await page.locator('#chat-input').press('Enter');
    await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
    await page.locator('.chat-head .chat-history-toggle').click();
    const button = page.locator('#chat-history .chat-history-x[data-id="escape-history"]');
    await button.click();
    await expect(button).toHaveClass(/sure/);
    await page.keyboard.press('Escape');
    await expect(button).not.toHaveClass(/sure/);
    await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
    await expect(page.locator('.chat-body')).not.toContainText('Stopped.');
    await page.keyboard.press('Escape');
    await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Send');
    await expect(page.locator('.chat-body')).toContainText('Stopped.');
  } finally { release(); }
});

test('Escape closes an open Felix menu or the Chats list and leaves the note open', async ({ page }) => {
  const item = await note(page, 'Escape keeps the note');
  await page.goto('/#/n/' + item.id);
  await page.locator('#chat-toggle').click();
  await expect(page.locator('#chat')).toBeVisible();
  const open = '/#/n/' + item.id;
  const here = () => page.evaluate(() => location.hash);
  for (const [trigger, menu] of [['#chat-access', '#chat-access-menu'], ['#chat [data-chat="attach"]', '#chat-attach-menu']]) {
    await page.locator(trigger).click();
    await expect(page.locator(menu)).toBeVisible();
    await page.evaluate(() => document.activeElement.blur());
    await page.keyboard.press('Escape');
    await expect(page.locator(menu)).toBeHidden();
    expect(await here()).toBe(open.slice(1));
    await expect(page.locator('#chat')).toBeVisible();
  }
  await page.locator('.chat-head .chat-history-toggle').click();
  await expect(page.locator('#chat.with-history')).toBeVisible();
  await page.evaluate(() => document.activeElement.blur());
  await page.keyboard.press('Escape');
  await expect(page.locator('#chat.with-history')).toHaveCount(0);
  expect(await here()).toBe(open.slice(1));
  await expect(page.locator('#chat')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#chat')).toBeHidden();
  expect(await here()).toBe(open.slice(1));
});
test('pressing Enter on a quiz answer keeps the chat where it was while the answer is checked', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const long = Array.from({ length: 40 }, (_, i) => `Paragraph ${i + 1} about stack canaries and return addresses.`).join('\n\n');
  await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { t: long }, { quiz: { kind: 'free_response', question: 'Will the function return to the attacker address?', answer: 'No, the canary check aborts first.' } }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' }));
  let release;
  const marked = new Promise((resolve) => { release = resolve; });
  await page.route('**/api/quiz/mark', async (route) => { await marked; await route.fulfill({ json: { correct: true, feedback: 'Right: the check runs before ret.' } }); });
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('quiz me');
  await page.locator('#chat-input').press('Enter');
  const answer = page.locator('#chat .quiz-input');
  await answer.click();
  await page.keyboard.type('No, the canary check fails first');
  const scrolled = () => page.locator('#chat-body').evaluate((el) => el.scrollTop);
  const before = await scrolled();
  expect(before).toBeGreaterThan(200);
  await page.keyboard.press('Enter');
  await expect(page.locator('#chat .quiz-result')).toContainText('Checking your answer');
  expect(await scrolled(), 'the chat does not jump to the top while the answer is checked').toBeGreaterThan(before - 40);
  release();
  await expect(page.locator('#chat .quiz-verdict')).toHaveText('Right');
  expect(await scrolled()).toBeGreaterThan(before - 40);
});
test("a chat that leo no longer has is not brought back from the browser's copy", async ({ page }) => {
  const asked = [];
  page.on('request', (request) => { if (request.url().includes('/api/chats/gone-chat')) asked.push(request.method()); });
  await page.addInitScript(() => {
    if (sessionStorage.getItem('seeded-gone-chat')) return;
    sessionStorage.setItem('seeded-gone-chat', '1');
    localStorage.setItem('leo-chat-v1', JSON.stringify({ id: 'gone-chat', messages: [{ role: 'user', text: 'What is a heap?' }, { role: 'assistant', text: 'A tree kept in order.' }], refs: [], synced: true }));
  });
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('leo-chat-v1') || '{}').id)).not.toBe('gone-chat');
  await expect(page.locator('#chat')).not.toContainText('A tree kept in order.');
  expect(asked).toContain('GET');
  expect(asked).not.toContain('PUT');
});
test('a chat removed while a reply is running is not re-created', async ({ page }) => {
  const tag = Date.now().toString(36);
  const id = `removed-${tag}`;
  await note(page, `Removed chat note one ${tag}`);
  await note(page, `Removed chat note two ${tag}`);
  const earlier = [{ role: 'user', text: 'Earlier question' }, { role: 'assistant', text: 'Earlier answer' }];
  await page.request.put(`/api/chats/${id}`, { data: { title: 'Removed', messages: earlier } });
  await page.addInitScript(([chat, messages]) => {
    if (sessionStorage.getItem('seeded-' + chat)) return;
    sessionStorage.setItem('seeded-' + chat, '1');
    localStorage.setItem('leo-chat-v1', JSON.stringify({ id: chat, messages, refs: [], synced: true }));
  }, [id, earlier]);
  let puts = 0;
  await page.route(`**/api/chats/${id}`, (route) => {
    if (route.request().method() !== 'PUT') return route.continue();
    puts += 1;
    return route.fulfill({ status: 410, body: '' });
  });
  let release;
  const waiting = new Promise((done) => { release = done; });
  await page.route('**/api/chat', async (route) => {
    await waiting;
    await route.fulfill({ status: 200, contentType: 'application/x-ndjson', body: '{"t":"An answer for the removed chat."}\n{"done":true}\n' });
  });
  try {
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await expect(chat).toContainText('Earlier answer');
    await chat.locator('#chat-input').fill('Hold this reply');
    await chat.locator('#chat-input').press('Enter');
    await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
    for (const which of ['one', 'two']) {
      await chat.locator('[data-chat="attach"]').click();
      await chat.locator('[data-chat="attach-note"]').click();
      await chat.locator('#chat-pick-search').fill(`Removed chat note ${which} ${tag}`);
      await chat.locator('.chat-pick-row', { hasText: `Removed chat note ${which} ${tag}` }).click();
      await expect.poll(() => puts).toBe(1);
    }
  } finally { release(); }
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Send');
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('leo-chat-v1') || '{}').id)).not.toBe(id);
  await expect(page.locator('#chat-input')).toHaveValue('Hold this reply');
  await page.waitForTimeout(300);
  expect(puts).toBe(1);
});
test('a streamed reply leaves the finished bubbles exactly as they were', async ({ page }) => {
  test.setTimeout(120000);
  await page.setViewportSize({ width: 1600, height: 900 });
  const id = `streamed-${Date.now().toString(36)}`;
  const messages = Array.from({ length: 100 }, (_, i) => (i % 2
    ? { role: 'assistant', text: `Answer ${i} with **bold words** in it.\n\n- first point ${i}\n- second point ${i}\n\nA closing line for answer ${i}.` }
    : { role: 'user', text: `Question ${i} about heaps and stacks` }));
  messages[95].session = { ideas: ['Heaps'], outcomes: [{ state: 'open', misses: 0, right: 0 }], asked: 0, state: 'active' };
  messages[97].quizzes = [{ kind: 'multiple_choice', question: 'Which keeps the smallest on top?', options: ['A min-heap', 'A stack'], answer: 'A min-heap', given: 'A min-heap', correct: true, held: true, told: true }];
  messages[99].spent = { by: 'Ollama', model: 'llama' };
  await page.addInitScript(([chat, seeded]) => {
    if (sessionStorage.getItem('seeded-' + chat)) return;
    sessionStorage.setItem('seeded-' + chat, '1');
    localStorage.setItem('leo-chat-v1', JSON.stringify({ id: chat, messages: seeded, refs: [] }));
  }, [id, messages]);
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  const chat = page.locator('#chat');
  await expect(chat.locator('.chat-body > .msg')).toHaveCount(100);
  await page.evaluate(() => {
    const real = window.fetch;
    const enc = new TextEncoder();
    window.fetch = (url, init) => {
      if (!String(url).endsWith('/api/chat')) return real(url, init);
      let feed;
      const body = new ReadableStream({ start(c) { feed = c; } });
      window.__push = (event) => feed.enqueue(enc.encode(JSON.stringify(event) + '\n'));
      window.__end = () => { feed.enqueue(enc.encode('{"done":true}\n')); feed.close(); };
      return Promise.resolve(new Response(body, { status: 200, headers: { 'content-type': 'application/x-ndjson' } }));
    };
  });
  const pending = chat.locator('.chat-body > .msg.pending');
  const finished = () => page.evaluate(() => [...document.querySelectorAll('#chat .chat-body > .msg:not(.pending)')].map((m) => m.outerHTML));
  const frame = async (event, marker) => {
    await page.evaluate(([e, t]) => window.__push({ ...e, t: ` ${t}` }), [event, marker]);
    await expect(pending).toContainText(marker);
    return finished();
  };
  const ask = async (text) => {
    await chat.locator('#chat-input').fill(text);
    await chat.locator('#chat-input').press('Enter');
    await expect(pending).toHaveCount(1);
    return finished();
  };

  let before = await ask('one more question');
  expect(before).toHaveLength(101);
  expect(await chat.locator('.quiz-next').count()).toBe(1);
  for (const [event, marker] of [[{}, 'Alpha'], [{ step: 'Looked in your notes', tool: 'search_notes' }, 'Bravo'], [{ sources: [{ id: 'n1', title: 'Heap note' }] }, 'Charlie'], [{ proposal: { kind: 'create', title: 'Heap summary', body: 'Heaps keep order.', folder: '' } }, 'Delta']]) {
    expect(await frame(event, marker), `finished bubbles after ${marker}`).toEqual(before);
  }
  before = await frame({ quiz: { kind: 'multiple_choice', question: 'Which is a heap?', options: ['A tree', 'A list'], answer: 'A tree' } }, 'Echo');
  await expect(chat.locator('.quiz-next'), 'the older card loses Next question at once').toHaveCount(0);
  expect(await frame({}, 'Foxtrot')).toEqual(before);
  await page.evaluate(() => window.__end());
  await expect(pending).toHaveCount(0);

  await ask('second question');
  await chat.locator('#chat-input').fill('Please also cover heaps');
  await chat.locator('#chat-input').press('Enter');
  await expect(chat.locator('.chat-body > .msg.user.queued')).toHaveCount(1);
  before = await finished();
  expect(await frame({}, 'Golf')).toEqual(before);
  before = await frame({ steered: ['Please also cover heaps'] }, 'Hotel');
  const above = pending.locator('xpath=preceding-sibling::div[contains(@class, "msg")][1]');
  await expect(above).toContainText('Please also cover heaps');
  await expect(above).toContainText('Sent while Felix worked');
  expect(await frame({}, 'India')).toEqual(before);
  await expect(chat.locator('.session-recap')).toHaveCount(0);
  before = await frame({ session_end: true }, 'Juliet');
  await expect(chat.locator('.session-recap'), 'the recap shows while the answer is still written').toHaveCount(1);
  expect(await frame({}, 'Kilo')).toEqual(before);
  before = await frame({ spent: { by: 'Claude Code', model: 'opus' } }, 'Lima');
  await expect(chat.locator('.chat-switch', { hasText: 'Now answered by Claude Code' })).toHaveCount(1);
  expect(await frame({}, 'Mike')).toEqual(before);
  await page.evaluate(() => window.__end());
  await expect(pending).toHaveCount(0);
});
