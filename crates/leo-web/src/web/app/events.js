document.addEventListener('click', (e) => {
  const el = e.target.closest('[data-action]');
  if (!el) return;
  const act = actions[el.dataset.action];
  if (!act) return;
  e.preventDefault();
  Promise.resolve(act(el)).catch(fail);
});

window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && !e.isComposing) e.preventDefault();
}, true);

const NOT_TYPED = new Set(['checkbox', 'radio', 'button', 'submit', 'reset', 'range', 'file', 'color', 'image']);
window.addEventListener('keydown', (e) => {
  const field = e.target.isContentEditable || (e.target.matches?.('input, textarea') && !e.target.readOnly && !NOT_TYPED.has(e.target.type));
  if (e.key === 'Backspace' && !field) e.preventDefault();
});

document.addEventListener('keydown', (e) => {
  const typing = e.target.closest('input, textarea, select, [contenteditable]');
  if (pressed(e, 'quick')) {
    e.preventDefault();
    toggleQuickLeo();
    return;
  }
  if (pressed(e, 'new')) {
    if (e.isComposing || $('.scrim') || $('.inline-edit') || state.view === 'locked') return;
    if (typing && !typing.matches('.line-edit, #title, #chat-input, .note-find input') || $('#rec-point-text')?.value.trim()) return;
    e.preventDefault();
    const dir = creationDir();
    go(dir ? `#/new/${enc(dir)}` : '#/new');
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's' && state.view === 'note') {
    e.preventDefault();
    flush().catch(fail);
    return;
  }
  if (e.key === 'Escape') {
    if ($('.scrim')) return closeSheet();
    if (typing && !e.target.matches('input[type="checkbox"], input[type="radio"]')) return e.target.blur();
    if (asideOpen()) {
      e.stopPropagation();
      return closeAside();
    }
    if (e.target === document.body && chat.isOpen()) return;
    if (state.view === 'trash' && state.selecting) {
      state.selecting = false;
      state.picked = new Set();
      return drawTrash();
    }
    if (state.view !== 'workspace') return back();
  }
  if (e.key === 'Enter' && e.target.matches('.card')) {
    e.preventDefault();
    go(noteHash(e.target.dataset.id));
    return;
  }
  if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key === 'n' && state.view === 'workspace' && !e.target.closest('.side-tree')) {
    const dir = creationDir();
    go(dir ? `#/new/${enc(dir)}` : '#/new');
  }
});

const unsaved = () => saving.unsaved() || Boolean(kanban.panel?.dirty()) || pendingAutosaves().length > 0;

function saveWebsiteWork() {
  kanban.panel?.flush().catch(() => {});
  for (const f of pendingAutosaves()) f.flush().catch(() => {});
  saving.retry().catch(() => {});
}

window.addEventListener('pagehide', () => {
  rememberPlace();
  saveWebsiteWork();
});

window.addEventListener('beforeunload', (e) => {
  if (!unsaved()) return;
  saveWebsiteWork();
  e.preventDefault();
});

// Reopen where you left off, like VS Code and Obsidian: the last note, folder
// or page is kept, and a fresh launch of the window (a new tab session) that
// opens on the library goes back there. Reloads and in-app navigation never
// redirect. The place is kept after every page that drew, and again when the
// window hides or quits, since some moves rewrite the address without a
// hashchange (a new note getting its id, a deleted note's page closing). A
// note that turns out to be gone resets it to the library.
const LAST_PLACE_KEY = 'leo-last-place';
const LAST_PLACES = /^#\/(n|f|boards|calendar)(\/|$)/;
function rememberPlace() {
  try {
    const here = location.hash || '#/';
    if (here === '#/' || LAST_PLACES.test(here)) localStorage.setItem(LAST_PLACE_KEY, here);
  } catch (e) {}
}
function reopenLastPlace() {
  try {
    if (sessionStorage.getItem('leo-launched')) return;
    sessionStorage.setItem('leo-launched', '1');
    if (location.hash && location.hash !== '#/') return;
    const last = localStorage.getItem(LAST_PLACE_KEY);
    if (/^#\/interviews(?:\/|$)/.test(last || '')) localStorage.setItem(LAST_PLACE_KEY, '#/');
    if (last && LAST_PLACES.test(last)) history.replaceState(history.state, '', last);
  } catch (e) {}
}

window.addEventListener('hashchange', async (e) => {
  const request = ++navigationRequest;
  const leaving = panes[0].s;
  const mine = seq;
  const hash = location.hash;
  const current = () => request === navigationRequest && mine === seq && panes[0].s === leaving;
  if (leaving) {
    leaving.doc.stop();
    try { await flush(leaving); }
    catch (error) {
      if (!current() || location.hash !== hash) return;
      showSaveFailure(leaving, error, saved => go(hash, { kept: saved }), current);
      return;
    }
    if (!current() || location.hash !== hash) return;
    leaveMain();
  }
  if (kanban.panel) {
    if (!await kanbanSettlePanel()) {
      if (location.hash === hash) history.replaceState(history.state, '', `#/boards/${enc(kanban.board.id)}`);
      return;
    }
    if (request !== navigationRequest || location.hash !== hash) return;
  }
  withPane(panes[0], marginLeave);
  route(true);
});

const busy = () => {
  const active = document.activeElement;
  return unsaved() || Boolean(active && active !== document.body && here().el.contains(active));
};

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden') {
    rememberPlace();
    saveWebsiteWork();
    return;
  }
  if (!['workspace', 'note', 'search', 'trash'].includes(state.view)) return;
  if (state.view === 'note') {
    if (busy()) return;
    for (const p of panes) {
      const s = p.s;
      if (!s?.note.id) continue;
      const shown = JSON.stringify(s.note);
      const again = () => (p === panes[0] ? render() : showNote(s.note.id, { pane: p }));
      api(`/api/notes/${enc(s.note.id)}`)
        .then((note) => {
          if (p.s !== s || busy()) return;
          if (JSON.stringify(note) !== shown) return again();
          if (p === panes[0]) loadSideFolders();
          withPane(p, () => marginLoad(s, () => p.s === s)).catch(fail);
        }, () => {
          if (p.s === s) return again();
        })
        .catch(fail);
    }
    return;
  }
  render().catch(fail);
});

window.addEventListener(
  'scroll',
  () => $('#bar').classList.toggle('scrolled', window.scrollY > 4),
  { passive: true }
);

let toldVisibleAt = Date.now();
const tellVisible = () => {
  try {
    navigator.sendBeacon('/api/visible', new Blob([JSON.stringify({ visible: document.visibilityState !== 'hidden', at: ++toldVisibleAt })], { type: 'application/json' }));
  } catch (e) {}
};
document.addEventListener('visibilitychange', tellVisible);
window.addEventListener('leo-visibility', () => {
  if (window.__LEO_VISIBLE__ === false) saveWebsiteWork();
});
setTimeout(tellVisible, 0);

async function route(arrived = false) {
  if (!arrived || $('.scrim')?.openedOn !== location.hash) closeSheet(true);
  drawn.top = /^#\/(?:f|n|new)(?:\/|$)|^#?\/?$/.test(location.hash);
  if (!drawn.top) window.scrollTo(0, 0);
  await render();
}

const activity = { tasks: [], timer: 0, folded: false, route: '', seen: new Map() };

function hiddenHere(task) {
  if (task.kind === 'recording') return activity.route === 'record';
  if (task.kind === 'map') return activity.route === 'map';
  return Boolean($('.upload-working'));
}

function drawActivity() {
  let box = $('#activity');
  if (!box) {
    box = document.createElement('aside');
    box.id = 'activity';
    box.className = 'activity';
    box.setAttribute('aria-label', 'Working in the background');
    dock().appendChild(box);
  }
  const shown = activity.tasks.filter((t) => !hiddenHere(t));
  const count = shown.length;
  box.hidden = !count;
  if (!count) return refreshPanel(box, '');
  box.classList.toggle('folded', activity.folded);
  const now = Date.now();
  const rows = shown
    .map((t) => {
      const seen = activity.seen.get(t.label) || { since: now, step: `${t.done}/${t.total}`, stepSince: now, steps: [] };
      if (seen.step !== `${t.done}/${t.total}`) {
        seen.steps = [...seen.steps, (now - seen.stepSince) / 1000].slice(-6);
        seen.step = `${t.done}/${t.total}`;
        seen.stepSince = now;
      }
      activity.seen.set(t.label, seen);
      const typical = seen.steps.length ? seen.steps.reduce((a, b) => a + b, 0) / seen.steps.length : 20;
      const creep = 1 - Math.exp(-((now - seen.stepSince) / 1000) / Math.max(3, typical));
      const share = t.total ? Math.max(4, Math.min(99, Math.round(((Math.min(t.done, t.total) + (t.done < t.total ? 0.9 * creep : 0)) / t.total) * 100))) : 0;
      const count = t.ready ? '' : `${t.kind === 'combine' ? ` · ${t.done}%` : t.total ? ` · ${t.done}/${t.total}` : ''} · ${leoChat.tookFor(now - seen.since)}`;
      return `<button class="activity-row" data-action="activity-open" data-href="${esc(t.href)}">
        <span class="activity-label">${esc(t.label)}</span>
        <span class="activity-step">${esc(t.step)}${count}</span>
        ${t.ready ? '' : `<span class="activity-bar${t.total ? '' : ' busy'}"><i style="width:${share}%"></i></span>`}
      </button>`;
    })
    .join('');
  const settled = shown.every((t) => t.ready);
  const said = settled ? (count === 1 ? 'Ready in the background' : `${count} ready in the background`) : count === 1 ? 'Working in the background' : `${count} things in the background`;
  const head = `<button class="activity-head" data-action="activity-fold" aria-expanded="${!activity.folded}" aria-label="${activity.folded ? 'Show' : 'Hide'} background work">${settled ? '' : '<svg class="margin-drop" aria-hidden="true"><use href="#leo-mark"/></svg>'}<span class="grow">${said}</span><span class="activity-fold">${ICON.chevron}</span></button>`;
  refreshPanel(box, `<span class="said" role="status">${esc(said)}</span>${head}${activity.folded ? '' : rows}`);
}

async function checkActivity() {
  clearTimeout(activity.timer);
  if (!appVisible()) return;
  try {
    activity.tasks = (await api('/api/activity')).tasks;
    for (const label of [...activity.seen.keys()]) if (!activity.tasks.some((t) => t.label === label)) activity.seen.delete(label);
  } catch (e) {
    if (e instanceof Locked) return;
  }
  drawActivity();
  const combining = new Set();
  for (const t of activity.tasks) {
    if (t.kind !== 'combine') continue;
    const id = t.href.slice('#combine/'.length);
    combining.add(id);
    if (!t.ready || combinesPaused.delete(id)) watchCombine(id);
  }
  for (const id of combinesPaused) if (!combining.has(id)) combinesPaused.delete(id);
  if (appVisible() && activity.tasks.some((t) => !t.ready)) activity.timer = setTimeout(checkActivity, 1000);
}

function closedHeight(box, summary) {
  const outer = getComputedStyle(box);
  const inner = getComputedStyle(summary);
  return summary.getBoundingClientRect().height
    + parseFloat(inner.marginTop) + parseFloat(inner.marginBottom)
    + parseFloat(outer.paddingTop) + parseFloat(outer.paddingBottom)
    + parseFloat(outer.borderTopWidth) + parseFloat(outer.borderBottomWidth);
}

document.addEventListener('click', (e) => {
  if (e.defaultPrevented || e.button !== 0 || stillMotion()) return;
  const summary = e.target.closest ? e.target.closest('summary') : null;
  const box = summary && summary.parentElement;
  if (!box || box.tagName !== 'DETAILS' || box.querySelector(':scope > summary') !== summary) return;
  const control = e.target.closest('a, button, input, select, textarea, label');
  if (control && control !== summary && summary.contains(control)) return;
  e.preventDefault();
  const from = box.getBoundingClientRect().height;
  stopSlide(box);
  if (box.open && !box.classList.contains('closing')) {
    box.classList.add('closing');
    slideHeight(box, from, closedHeight(box, summary), () => {
      box.classList.remove('closing');
      box.open = false;
    });
  } else {
    box.classList.remove('closing');
    box.open = true;
    slideHeight(box, from, box.getBoundingClientRect().height);
  }
});

let renderGeneration = 0;
async function render() {
  const generation = ++renderGeneration;
  hideAskFloat();
  clearTimeout(cal.timer);
  if (/^#\/interviews(?:\/|$)/.test(location.hash)) {
    history.replaceState(history.state, '', '#/');
    try { localStorage.setItem(LAST_PLACE_KEY, '#/'); } catch (e) {}
  }
  const hash = decodeURI(location.hash || '#/');
  const [, kind, rest = ''] = location.hash.match(/^#\/([a-z]*)\/?(.*)$/) || [null, '', ''];
  const arg = decodeURIComponent(rest);
  const prepared = kind === 'n' && openingNote?.id === arg.replace(/\/edit$/, '') ? openingNote : null;
  openingNote = null;
  stopPageArrival();
  if (kind && !/^(?:f|n|new)$/.test(kind)) document.body.classList.toggle('on-note', false);
  if (kind !== 'search') closeSearch();
  if (kind !== 'n') chat.setContext(null);
  if (kind !== 'record') recorder.leave();
  if (kind !== 'boards') leaveBoards();
  activity.route = kind;
  drawActivity();
  try {
    if (kind === 'f') await showWorkspace(arg);
    else if (kind === 'n') await showNote(arg.replace(/\/edit$/, ''), { fresh: prepared });
    else if (kind === 'new') await newNote(arg);
    else if (kind === 'search') await showSearch(arg);
    else if (kind === 'trash') await showTrash();
    else if (kind === 'calendar') await showCalendar();
    else if (kind === 'boards') await showBoards(arg);
    else if (kind === 'settings') await showSettings(arg);
    else if (kind === 'record') await showRecord(arg);
    else await showWorkspace();
    if (generation !== renderGeneration || (hash !== decodeURI(location.hash || '#/') && !(kind === 'f' && location.hash === '#/'))) return;
    rememberPlace();
    loadSideFolders();
    $('#route-say').textContent = document.title;
    if (prepared && panes[0].s?.note.id === prepared.id) panes[0].s.doc.editStart();
  } catch (e) {
    if (generation !== renderGeneration || (hash !== decodeURI(location.hash || '#/') && !(kind === 'f' && location.hash === '#/'))) return;
    if (e.status === 404 && hash.startsWith('#/n/')) {
      toast('That note is not there any more.', { bad: true });
      try { localStorage.setItem(LAST_PLACE_KEY, '#/'); } catch (err) {}
      go('#/', { replace: true });
    } else if (e instanceof Offline) {
      chrome({ view: kind });
      app.innerHTML = empty(DRAWN.offline, "Can't reach leo", 'Quit leo and open it again.');
    } else {
      for (const room of app.querySelectorAll('.blk.margin-room')) room.classList.remove('margin-room');
      fail(e);
    }
  }
}

window.addEventListener('online', () => saving.retry());
window.addEventListener('beforeprint', () => {
  for (const fold of document.querySelectorAll('details.callout:not([open])')) {
    fold.dataset.printed = '1';
    fold.open = true;
  }
});
window.addEventListener('afterprint', () => {
  for (const fold of document.querySelectorAll('details.callout[data-printed]')) {
    fold.open = false;
    delete fold.dataset.printed;
  }
});
reopenLastPlace();
route();
checkActivity();
warnUnsaved();

document.addEventListener('paste', (e) => {
  if (e.defaultPrevented) return;
  const files = [...((e.clipboardData && e.clipboardData.files) || [])];
  if (!files.length || leoEditing.ownTextCopy(e.clipboardData)) return;
  if ($('#upload-list')) {
    e.preventDefault();
    picked.push(...pastedFiles(files, picked.length));
    drawPicked();
    return;
  }
  if ($('.scrim') || e.target.closest('#chat')) return;
  if (state.view === 'note' && state.session && files.some((f) => /^image\//.test(f.type))) {
    e.preventDefault();
    addPictures(state.session, files);
    return;
  }
  if (!['workspace', 'search'].includes(state.view)) return;
  e.preventDefault();
  uploadSheet(pastedFiles(files));
});

const QUIT_TEXT = 'Press ⌘Q again or choose Quit again within 3 seconds to quit Leo.';
async function quitLoss() {
  try {
    const { job } = await Promise.race([api('/api/record'), new Promise((_, no) => setTimeout(no, 300))]);
    if (job && ['starting', 'recording', 'paused'].includes(job.state)) return 'The recording stops and you can recover it from Record next time.';
    if (job && job.state === 'writing') return 'A note is still being written; you can recover it from Record next time.';
  } catch (e) {}
  return '';
}
async function quitText() {
  const loss = await quitLoss();
  return loss ? `Press ⌘Q again to quit. ${loss}` : QUIT_TEXT;
}
async function confirmQuit() {
  const loss = await quitLoss();
  confirmSheet({ title: 'Quit leo?', text: loss || 'Dictation and reminders stop until you open leo again.', yes: 'Quit leo', action: 'app-command', data: ' data-app-action="quit"', danger: Boolean(loss) });
}
async function warnUnsaved() {
  try {
    const ov = await api('/api/record');
    const n = (ov.pending || []).length;
    if (n && !/^#\/record/.test(location.hash)) toast(window.leoRecording.unsavedLine(n, ov.more), { action: 'Open Record', run: () => go('#/record') });
  } catch (e) {}
}
const QUIT_SETTLE_MS = 2800;
window.leoFromApp = async (what, which) => {
  if (what === 'quit' && which === 'now') {
    saveWebsiteWork();
    try {
      await Promise.race([saving.settle(QUIT_SETTLE_MS), new Promise((r) => setTimeout(r, QUIT_SETTLE_MS))]);
    } catch (e) {}
    return api('/api/app', { method: 'POST', body: { action: 'settled' } }).catch(() => {});
  }
  if (what === 'quit') {
    saveWebsiteWork();
    return toast(await quitText());
  }
  if (what === 'event') {
    const [kind, ...uid] = String(which).split(':');
    return go(`#/record?event=${encodeURIComponent(uid.join(':'))}&kind=${kind === 'microphone' ? 'microphone' : 'call'}`);
  }
};
