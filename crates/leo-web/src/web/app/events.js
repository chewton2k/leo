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

document.addEventListener('keydown', (e) => {
  const typing = e.target.closest('input, textarea, select, [contenteditable]');
  if (pressed(e, 'quick')) {
    e.preventDefault();
    toggleQuickLeo();
    return;
  }
  if (pressed(e, 'new')) {
    if (e.isComposing || $('.scrim') || $('.inline-edit') || state.view === 'locked') return;
    if (typing && !typing.matches('.line-edit, #title, #chat-input') || $('#rec-point-text')?.value.trim()) return;
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
    if (source.note && sourcePane.contains(e.target)) return closeSource();
    if (typing && !e.target.matches('input[type="checkbox"], input[type="radio"]')) return e.target.blur();
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
  const leaving = state.session;
  const mine = seq;
  const hash = location.hash;
  const current = () => request === navigationRequest && mine === seq && state.session === leaving;
  if (leaving) {
    leaving.doc.stop();
    try { await flush(leaving); }
    catch (error) {
      if (!current() || location.hash !== hash) return;
      showSaveFailure(leaving, error, saved => go(hash, { kept: saved }), current);
      return;
    }
    if (!current() || location.hash !== hash) return;
    leaving.doc.destroy();
    state.session = null;
  }
  if (kanban.panel) {
    if (!await kanbanSettlePanel()) {
      if (location.hash === hash) history.replaceState(history.state, '', `#/boards/${enc(kanban.board.id)}`);
      return;
    }
    if (request !== navigationRequest || location.hash !== hash) return;
  }
  marginLeave();
  route(true);
});

const busy = () => {
  const active = document.activeElement;
  return unsaved() || Boolean(active && active !== document.body && app.contains(active));
};

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden') {
    rememberPlace();
    saveWebsiteWork();
    return;
  }
  if (!['workspace', 'note', 'search', 'trash'].includes(state.view)) return;
  if (state.view === 'note') {
    if (busy() || !state.session?.note.id) return;
    const s = state.session;
    const shown = JSON.stringify(s.note);
    api(`/api/notes/${enc(s.note.id)}`)
      .then((note) => {
        if (state.session !== s || busy()) return;
        if (JSON.stringify(note) !== shown) return render();
        loadSideFolders();
        marginLoad(s, () => state.session === s).catch(fail);
      }, () => {
        if (state.session === s) return render();
      })
      .catch(fail);
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
  box.hidden = !shown.length;
  if (!shown.length) return;
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
      const took = Math.round((now - seen.since) / 1000);
      const count = t.ready ? '' : `${t.kind === 'combine' ? ` · ${t.done}%` : t.total ? ` · ${t.done}/${t.total}` : ''} · ${took >= 60 ? `${Math.floor(took / 60)}:${String(took % 60).padStart(2, '0')}` : `${took} s`}`;
      return `<button class="activity-row" data-action="activity-open" data-href="${esc(t.href)}">
        <span class="activity-label">${esc(t.label)}</span>
        <span class="activity-step">${esc(t.step)}${count}</span>
        ${t.ready ? '' : `<span class="activity-bar${t.total ? '' : ' busy'}"><i style="width:${share}%"></i></span>`}
      </button>`;
    })
    .join('');
  const settled = shown.every((t) => t.ready);
  const said = settled ? (shown.length === 1 ? 'Ready in the background' : `${shown.length} ready in the background`) : shown.length === 1 ? 'Working in the background' : `${shown.length} things in the background`;
  const head = `<button class="activity-head" data-action="activity-fold" aria-expanded="${!activity.folded}">${settled ? '' : '<span class="activity-spin"></span>'}<span class="grow">${said}</span><span class="activity-fold">${activity.folded ? 'Show' : 'Hide'}</span></button>`;
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
  if (!/^#\/n\//.test(location.hash)) closeSource();
  // Plain groups: tests/wording.rs flags a colon right before "settings" (an old command).
  document.body.classList.toggle('on-settings', /^#\/(settings|trash)(\/|$)/.test(location.hash));
  document.body.classList.toggle('on-calendar', /^#\/calendar/.test(location.hash));
  document.body.classList.toggle('on-boards', /^#\/boards(?:\/|$)/.test(location.hash));
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
    if (prepared && state.session?.note.id === prepared.id) state.session.doc.editStart();
  } catch (e) {
    if (generation !== renderGeneration || (hash !== decodeURI(location.hash || '#/') && !(kind === 'f' && location.hash === '#/'))) return;
    if (e.status === 404 && hash.startsWith('#/n/')) {
      toast('That note is not there any more.', { bad: true });
      try { localStorage.setItem(LAST_PLACE_KEY, '#/'); } catch (err) {}
      go('#/', { replace: true });
    } else if (e instanceof Offline) {
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
  if (!files.length) return;
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

// The menu bar icon (leo-desktop tray.rs) calls this after showing the window.
// Record starts a lecture (microphone) or a call right away, like pressing
// Start on the Record page; one already running is just shown.
const APP_SETTINGS = ['general', 'updates'];
const QUIT_TEXT = 'Press ⌘Q again or choose Quit again within 3 seconds to quit Leo.';
async function quitText() {
  try {
    const { job } = await Promise.race([api('/api/record'), new Promise((_, no) => setTimeout(no, 300))]);
    if (job && ['starting', 'recording', 'paused'].includes(job.state)) return 'Press ⌘Q again to quit. The recording stops and you can recover it from Record next time.';
    if (job && job.state === 'writing') return 'Press ⌘Q again to quit. A note is still being written; you can recover it from Record next time.';
  } catch (e) {}
  return QUIT_TEXT;
}
async function warnUnsaved() {
  try {
    const n = ((await api('/api/record')).pending || []).length;
    if (n && !/^#\/record/.test(location.hash)) toast(n === 1 ? "1 recording wasn't saved" : `${n} recordings weren't saved`, { action: 'Open Record', run: () => go('#/record') });
  } catch (e) {}
}
window.leoFromApp = async (what, which) => {
  if (what === 'quit') {
    saveWebsiteWork();
    return toast(await quitText());
  }
  if (what === 'settings') return go(`#/settings/${APP_SETTINGS.includes(which) ? which : 'general'}`);
  if (what === 'event') {
    const [kind, ...uid] = String(which).split(':');
    return go(`#/record?event=${encodeURIComponent(uid.join(':'))}&kind=${kind === 'microphone' ? 'microphone' : 'call'}`);
  }
  if (what !== 'record' || !['microphone', 'call'].includes(which)) return;
  if (recorder.active()) return go('#/record');
  if (state.view !== 'record') go('#/record');
  const until = Date.now() + 8000;
  let kind = null;
  while (Date.now() < until && !(kind = $(`#rec-box input[name="rec-kind"][value="${which}"]`))) await new Promise((r) => setTimeout(r, 100));
  if (!kind || kind.disabled || recorder.active()) return;
  kind.checked = true;
  recorder.begin().catch(fail);
};
