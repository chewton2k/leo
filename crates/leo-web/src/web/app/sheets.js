function showLocked() {
  ++seq;
  state = { view: 'locked' };
  chrome();
  app.innerHTML = empty(
    ICON.lock,
    'leo needs to sign in again',
    'Quit leo and open it again.'
  );
}

// `modal` sheets ignore a click on the backdrop (first-launch setup must not
// vanish from a stray click); Escape and their own buttons still close them.
function sheet(html, { modal = false } = {}) {
  const opener = document.activeElement;
  const foldersReturn = Boolean($('#folders-sheet') || $('.scrim')?.foldersReturn);
  closeSheet(true);
  const scrim = document.createElement('div');
  scrim.foldersReturn = foldersReturn;
  scrim.openedOn = location.hash;
  scrim.returnFocus = opener;
  scrim.className = 'scrim';
  scrim.innerHTML = `<div class="sheet" role="dialog" aria-modal="true"><div class="grip"></div>${html}</div>`;
  const heading = scrim.querySelector('.sheet > h3, .sheet > * > h3');
  if (heading) {
    heading.id = heading.id || 'sheet-title';
    scrim.querySelector('.sheet').setAttribute('aria-labelledby', heading.id);
  }
  scrim.addEventListener('click', (e) => {
    if (e.target === scrim && !modal) closeSheet();
  });
  document.body.appendChild(scrim);
  if (!modal) swipeToClose(scrim.querySelector('.sheet'));
  trapSheetFocus(scrim);
  sheetControls(scrim)[0]?.focus({ preventScroll: true });
  return scrim;
}

function ghostOf(el) {
  const ghost = el.cloneNode(true);
  const typed = el.querySelectorAll('input, textarea, select');
  ghost.querySelectorAll('input, textarea, select').forEach((field, i) => {
    if (typed[i] && field.type !== 'file') field.value = typed[i].value;
  });
  ghost.setAttribute('aria-hidden', 'true');
  ghost.inert = true;
  for (const part of [ghost, ...ghost.querySelectorAll('*')]) {
    const kept = part.closest('.viewer-diagram');
    for (const { name } of [...part.attributes]) if ((name === 'id' && !kept) || name.startsWith('data-')) part.removeAttribute(name);
  }
  return ghost;
}

function overlapOut(el) {
  if (!el?.isConnected || stillMotion() || typeof el.animate !== 'function') return;
  const { left, top, width, height } = el.getBoundingClientRect();
  if (!width || !height) return;
  const ghost = ghostOf(el);
  ghost.classList.add('overlap-gone');
  Object.assign(ghost.style, { left: `${left}px`, top: `${top}px`, width: `${width}px`, height: `${height}px` });
  document.body.appendChild(ghost);
  ghost.animate({ opacity: [1, 0] }, { duration: ICON_MS, easing: EASE_OUT, fill: 'forwards' }).onfinish = () => ghost.remove();
}

function sheetControls(scrim) {
  return [...scrim.querySelectorAll('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])')].filter((el) => el.getClientRects().length);
}

function trapSheetFocus(scrim) {
  scrim.addEventListener('keydown', (event) => {
    if (event.key !== 'Tab') return;
    const controls = sheetControls(scrim);
    const at = controls.indexOf(document.activeElement);
    if (event.shiftKey && at <= 0 || !event.shiftKey && at === controls.length - 1) {
      event.preventDefault();
      controls[event.shiftKey ? controls.length - 1 : 0]?.focus();
    }
  });
}

function sheetButtons(primary, { cancel = 'Cancel', action = 'close', middle = '', kind = 'primary' } = {}) {
  const [label, attrs] = primary || [];
  return `<div class="buttons"><button type="button" class="btn plain" data-action="${action}">${cancel}</button>${middle}${primary ? `<button type="button" class="btn ${kind}" ${attrs}>${label}</button>` : ''}</div>`;
}

function confirmSheet({ title, text, yes, action, data = '', danger = true, cancel = 'close' }) {
  const lines = (Array.isArray(text) ? text : [text]).filter(Boolean).map((line) => `<p>${line}</p>`).join('');
  return sheet(`<h3>${title}</h3>${lines}${sheetButtons([yes, `data-action="${action}"${data}`], { action: cancel, kind: danger ? 'danger' : 'primary' })}`);
}

const SWIPE_CLOSE = 90;
const SWIPE_FLICK = 0.6;

function swipeToClose(panel) {
  let start = null;
  const reset = () => {
    panel.style.transition = '';
    panel.style.transform = '';
    start = null;
  };
  panel.addEventListener('touchstart', (e) => {
    if (e.touches.length !== 1 || window.matchMedia('(min-width: 600px)').matches) return;
    const inside = e.target.closest('textarea, input, select, .viewer-pan, .viewer-frame');
    if (inside || panel.scrollTop > 0) return;
    start = { y: e.touches[0].clientY, at: performance.now(), moved: 0 };
  }, { passive: true });
  panel.addEventListener('touchmove', (e) => {
    if (!start) return;
    const moved = e.touches[0].clientY - start.y;
    if (moved <= 0) {
      panel.style.transition = 'none';
      panel.style.transform = stillMotion() ? '' : `translateY(${rubber(moved)}px)`;
      start.moved = 0;
      return;
    }
    start.moved = moved;
    panel.style.transition = 'none';
    panel.style.transform = `translateY(${moved}px)`;
    if (e.cancelable) e.preventDefault();
  }, { passive: false });
  const finish = () => {
    if (!start) return;
    const { moved, at } = start;
    const speed = moved / Math.max(1, performance.now() - at);
    if (moved > SWIPE_CLOSE || (moved > 24 && speed > SWIPE_FLICK)) {
      panel.style.transition = `transform ${LEAVE_MS}ms ${EASE_EXIT}`;
      panel.style.transform = 'translateY(110%)';
      start = null;
      setTimeout(closeSheet, LEAVE_MS - 10);
      return;
    }
    panel.style.transition = `transform ${OPEN_MS}ms ${EASE_OUT}`;
    panel.style.transform = '';
    setTimeout(reset, OPEN_MS + 10);
  };
  panel.addEventListener('touchend', finish);
  panel.addEventListener('touchcancel', reset);
}

function closeSheet(now = false) {
  const open = $('.scrim');
  const returnFocus = open && open.returnFocus;
  const focusBack = typeof returnFocus === 'function' ? (now ? null : returnFocus) : () => returnFocus;
  const foldersReturn = open?.foldersReturn && !now;
  const tree = open?.querySelector('#folders-sheet .side-tree');
  if (tree) side.sheetScroll = tree.scrollTop;
  if (open && now) open.remove();
  else if (open) {
    const ghost = ghostOf(open);
    ghost.classList.replace('scrim', 'scrim-gone');
    const inner = ghost.querySelector('.sheet');
    if (inner) inner.classList.replace('sheet', 'sheet-gone');
    const scrolled = open.querySelector('.sheet');
    const panned = open.querySelector('.viewer-pan');
    open.replaceWith(ghost);
    if (inner && scrolled) inner.scrollTop = scrolled.scrollTop;
    const pan = ghost.querySelector('.viewer-pan');
    if (pan && panned) {
      pan.scrollTop = panned.scrollTop;
      pan.scrollLeft = panned.scrollLeft;
    }
    setTimeout(() => ghost.remove(), LEAVE_MS);
  }
  if (state.keepPending) {
    state.keepPending = null;
    if (state.view === 'keeping') drawKeeping();
  }
  if (foldersReturn && !sideWide()) {
    showFolders();
    if (typeof returnFocus === 'function') returnFocus()?.focus({ preventScroll: true });
  } else {
    const back = focusBack?.();
    if (back?.isConnected) back.focus({ preventScroll: true });
  }
}

const QUICK_PLACES = [
  { name: 'Notes', hash: '#/', icon: 'note' },
  { name: 'Calendar', hash: '#/calendar', icon: 'calendar' },
  { name: 'Boards', hash: '#/boards', icon: 'board' },
  { name: 'Record', hash: '#/record', icon: 'mic' },
  { name: 'Trash', hash: '#/trash', icon: 'trash' },
  { name: 'Keeping', hash: '#/settings/keeping', icon: 'storage' },
  { name: 'Settings', hash: '#/settings', icon: 'gear' },
];
const QUICK_DOES = new Set(['ask', 'search', 'task', 'note', 'combine']);
const QUICK_SEARCH_MS = 120;
const QUICK_WAIT_MS = 800;
const quick = { due: [], rows: [], at: 0, picked: null, notes: [], chats: [], folders: [], recent: [], asked: '', timer: 0, seq: 0, busy: false, root: null };
const inQuick = (sel) => (quick.root && !quick.root.classList.contains('scrim-gone') ? quick.root.querySelector(sel) : null);

function quickLeo() {
  Object.assign(quick, { due: [], rows: [], at: 0, picked: null, notes: [], chats: [], folders: [], recent: [], asked: '' });
  quick.root = sheet(`<div class="quick-leo"><span class="quick-perch" aria-hidden="true">${chat.button(50)}</span>
    <label class="quick-bar">${ICON.search}<input id="quick-input" placeholder="Search notes, or ask Felix" autocomplete="off" spellcheck="false" enterkeyhint="go" maxlength="2000" aria-label="Quick Leo" role="combobox" aria-controls="quick-results" aria-expanded="true"><kbd>${shortcutText(shortcutKeys('quick'))}</kbd></label>
    <div class="quick-results" id="quick-results" role="listbox" aria-label="Results"></div>
    <div class="quick-foot"><span><kbd>↑</kbd><kbd>↓</kbd> move</span><span><kbd>↵</kbd> open</span><span><kbd>esc</kbd> close</span></div>
  </div>`);
  const input = inQuick('#quick-input');
  input.addEventListener('input', () => {
    inQuick('#quick-results')?.classList.remove('rows-in');
    quick.at = 0;
    quick.picked = null;
    drawQuick();
    clearTimeout(quick.timer);
    quick.timer = setTimeout(() => quickSearch().catch(() => {}), QUICK_SEARCH_MS);
  });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      quickGo(quick.at).catch(fail);
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      if (!quick.rows.length) return;
      e.preventDefault();
      quick.at = (quick.at + (e.key === 'ArrowDown' ? 1 : quick.rows.length - 1)) % quick.rows.length;
      quick.picked = JSON.stringify(quick.rows[quick.at]);
      drawQuick();
    }
  });
  drawQuick();
  cascade(inQuick('#quick-results'));
  input.focus();
  loadQuickDue().catch(() => {});
  recentNotes().then((notes) => { quick.recent = notes; drawQuick(); }).catch(() => {});
  api('/api/folders').then((all) => { quick.folders = (all || []).map((f) => f.name); drawQuick(); }).catch(() => {});
}

function toggleQuickLeo() {
  if ($('.scrim:not(.scrim-gone) .quick-leo')) closeSheet();
  else if (!$('.scrim') && !$('.inline-edit') && state.view !== 'locked') quickLeo();
}

async function quickSearch() {
  const text = (inQuick('#quick-input')?.value || '').trim();
  const mine = ++quick.seq;
  const [found, talks] = text.length >= 2
    ? await Promise.all([api(`/api/search?q=${enc(text)}&brief=true&limit=4`), api(`/api/chats?q=${enc(text)}`).catch(() => [])])
    : [[], []];
  if (mine !== quick.seq || !inQuick('#quick-input')) return;
  quick.notes = (found || []).map((n) => ({ id: n.id, title: n.title, directory: n.directory }));
  quick.chats = (talks || []).map((c) => ({ id: c.id, title: c.title, updated_at: c.updated_at }));
  quick.asked = text;
  drawQuick();
}

async function quickSettled() {
  const text = (inQuick('#quick-input')?.value || '').trim();
  if (text.length < 2 || quick.asked === text) return;
  clearTimeout(quick.timer);
  await Promise.race([quickSearch().catch(() => {}), new Promise((done) => setTimeout(done, QUICK_WAIT_MS))]);
  drawQuick();
}

function quickWhen(date, time) {
  if (!date) return 'no date';
  const today = S.dayKey(new Date());
  const day = date === today ? 'Today' : date === S.dayKey(S.addDays(new Date(), 1)) ? 'Tomorrow' : S.fromKey(date).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric' });
  return time ? `${day}, ${S.timeLabel(time, calLocale())}` : day;
}

const quickSaid = (...parts) => parts.map((part, i) => (i % 2 ? `<span class="quick-words">“${esc(part)}”</span>` : part && `<span class="quick-verb">${esc(part)}</span>`)).join(' ');

function quickTaskWhen(row) {
  if (!row.date) return 'to tasks';
  if (row.date === S.dayKey(new Date())) return `to today’s tasks${row.time ? ` at ${S.timeLabel(row.time, calLocale())}` : ''}`;
  return `to tasks for ${quickWhen(row.date, row.time).replace(/^Tomorrow/, 'tomorrow')}`;
}

function quickLook(row) {
  if (row.kind === 'combine') return { icon: ICON.note, label: row.id ? `Combine “${row.title || 'Untitled'}” into…` : 'Open a note to combine it', hint: 'Combine notes' };
  if (row.kind === 'ask') return { icon: chat.button(20), said: quickSaid('Ask Felix about', row.text) };
  if (row.kind === 'task') return { icon: ICON.check, said: quickSaid('Add', row.title, quickTaskWhen(row)) };
  if (row.kind === 'open') return { icon: ICON.note, label: row.title, hint: (row.dir ?? quick.notes.find((n) => n.id === row.id)?.directory ?? '').replaceAll('/', ' / ') };
  if (row.kind === 'start') return { icon: row.what === 'felix' ? chat.button(20) : ICON[row.icon], label: row.label, hint: row.hint || '', keys: row.keys };
  if (row.kind === 'folder') return { icon: ICON.folder, label: row.path.split('/').pop(), hint: row.path.includes('/') ? row.path.split('/').slice(0, -1).join(' / ') : 'Folder' };
  if (row.kind === 'chat') {
    const when = quick.chats.find((c) => c.id === row.id)?.updated_at;
    return { icon: ICON.chat, label: row.title, hint: when ? `Chat · ${rel(when)}` : 'Chat' };
  }
  if (row.kind === 'search') return { icon: ICON.search, said: quickSaid('Search every note for', row.text) };
  if (row.kind === 'place') return { icon: ICON[QUICK_PLACES.find((p) => p.hash === row.hash)?.icon] || ICON.folder, label: row.name, hint: 'Go to' };
  return { icon: ICON.plus, said: quickSaid('Start a note called', row.title) };
}

function quickRow(row, i, head) {
  const look = quickLook(row);
  const on = i === quick.at;
  return `${head ? `<div class="quick-due-head" role="presentation">${head}</div>` : ''}<button type="button" class="quick-row${on ? ' on' : ''}" id="quick-row-${i}" role="option" aria-selected="${on}" data-action="quick-run" data-at="${i}" tabindex="-1"><span class="quick-icon">${look.icon}</span><span class="quick-label${look.said ? ' quick-said' : ''}">${look.said || esc(look.label)}</span>${look.hint ? `<span class="quick-hint">${esc(look.hint)}</span>` : ''}${look.keys ? `<kbd>${esc(look.keys)}</kbd>` : on ? '<kbd>↵</kbd>' : ''}</button>`;
}

function drawQuick() {
  const box = inQuick('#quick-results');
  if (!box) return;
  const text = (inQuick('#quick-input')?.value || '').trim();
  const fresh = quick.asked && text.toLowerCase().startsWith(quick.asked.toLowerCase());
  if (!text) return drawQuickStart(box);
  const ranked = S.quickRank(text, { notes: fresh ? quick.notes : [], chats: fresh ? quick.chats : [], folders: quick.folders, places: QUICK_PLACES });
  const open = state.view === 'note' && state.session?.note;
  if (/^(merge|combine)(\s+(this|this note|note|notes|it|into|with))?$/i.test(text)) ranked.unshift(open ? { kind: 'combine', id: open.id, title: open.title, dir: open.directory } : { kind: 'combine' });
  const doing = (row) => QUICK_DOES.has(row.kind);
  const lead = ranked.length && doing(ranked[0]);
  quick.rows = [...ranked.filter((row) => doing(row) === lead), ...ranked.filter((row) => doing(row) !== lead)];
  const headed = ranked.some(doing) && !ranked.every(doing);
  const picked = quick.picked ? quick.rows.findIndex((row) => JSON.stringify(row) === quick.picked) : -1;
  if (picked >= 0) quick.at = picked;
  quick.at = Math.min(quick.at, Math.max(0, quick.rows.length - 1));
  if (!quick.rows.length) {
    quick.keys = [];
    box.innerHTML = '';
    inQuick('#quick-input')?.removeAttribute('aria-activedescendant');
    drawQuickDue();
    return;
  }
  const keys = quick.rows.map((row) => JSON.stringify(row));
  flipList(box, '.quick-row', (el) => (quick.keys || [])[Number(el.dataset.at)], () => {
    quick.keys = keys;
    box.innerHTML = quick.rows.map((row, i) => quickRow(row, i, headed && (i === 0 || doing(row) !== doing(quick.rows[i - 1])) ? (doing(row) ? 'Do' : 'Notes') : '')).join('');
  }, { ms: QUICK_FLIP_MS });
  inQuick('#quick-input')?.setAttribute('aria-activedescendant', `quick-row-${quick.at}`);
}

const QUICK_DUE_ROWS = 6;
const QUICK_DUE_PAGE = 50;

function quickDueRows(tasks) {
  return tasksGroups(tasks).filter(([name]) => name === 'Overdue' || name === 'Today').flatMap(([name, list]) => list.map((t) => ({ ...t, late: name === 'Overdue' }))).slice(0, QUICK_DUE_ROWS);
}

async function loadQuickDue() {
  const today = S.dayKey(new Date());
  const tasks = [];
  for (;;) {
    const answer = await api(`/api/tasks?offset=${tasks.length}&limit=${QUICK_DUE_PAGE}`);
    const page = answer.tasks || [];
    tasks.push(...page);
    const last = page[page.length - 1];
    if (!last || tasks.length >= (answer.total ?? 0) || last.done || !last.date || last.date > today) break;
    const due = quickDueRows(tasks);
    if (due.length >= QUICK_DUE_ROWS && (due[QUICK_DUE_ROWS - 1].late ? true : last.date >= today)) break;
  }
  quick.due = quickDueRows(tasks);
  drawQuickDue();
}

function quickStart() {
  return [
    ...quick.recent.map((n) => ({ kind: 'open', id: n.id, title: n.title || 'Untitled', dir: n.directory || '', group: 'Recent' })),
    { kind: 'start', what: 'new', icon: 'filePlus', label: 'New note', keys: shortcutText(shortcutKeys('new')), group: 'Do' },
    { kind: 'start', what: 'record', icon: 'mic', label: 'Start recording', group: 'Do' },
    { kind: 'start', what: 'task', icon: 'check', label: 'New task', group: 'Do' },
    { kind: 'start', what: 'felix', label: 'Ask Felix', hint: 'or type a question above', group: 'Do' },
  ];
}

function drawQuickStart(box) {
  quick.rows = quickStart();
  const picked = quick.picked ? quick.rows.findIndex((row) => JSON.stringify(row) === quick.picked) : -1;
  if (picked >= 0) quick.at = picked;
  quick.at = Math.min(quick.at, quick.rows.length - 1);
  quick.keys = quick.rows.map((row) => JSON.stringify(row));
  box.innerHTML = quick.rows.map((row, i) => quickRow(row, i, i === 0 || row.group !== quick.rows[i - 1].group ? row.group : '')).join('') + quickDueHtml();
  inQuick('#quick-input')?.setAttribute('aria-activedescendant', `quick-row-${quick.at}`);
}

function quickDueHtml() {
  return quick.due.length
    ? `<div class="quick-due-head">Due today</div>${quick.due.map((t) => `<label class="quick-task${t.late ? ' late' : ''}"><input type="checkbox" class="sched-check" data-action="quick-done" data-id="${esc(t.id)}"${t.on ? ` data-on="${esc(t.on)}"` : ''} aria-label="Done: ${esc(t.title)}"><span class="grow">${esc(t.title)}</span><span class="hint">${esc(t.late ? 'Earlier' : t.time ? S.timeLabel(t.time, calLocale()) : 'Today')}</span></label>`).join('')}`
    : '';
}

function drawQuickDue() {
  const box = inQuick('#quick-results');
  if (!box) return;
  if (!(inQuick('#quick-input')?.value || '').trim()) return drawQuick();
  if (!quick.rows.length) box.innerHTML = quickDueHtml();
}

async function quickDone(el) {
  quick.due = quick.due.filter((t) => !(t.id === el.dataset.id && (t.on || null) === (el.dataset.on || null)));
  drawQuickDue();
  await setTaskDone(el.dataset.id, true, el.dataset.on || null);
  loadTasksWindow().catch(() => {});
}

async function quickGo(at) {
  if (quick.busy) return;
  if (at === 0) {
    quick.busy = true;
    try {
      await quickSettled();
    } finally {
      quick.busy = false;
    }
    if (!inQuick('#quick-input')) return;
  }
  const row = quick.rows[at];
  if (!row) return;
  if (row.kind === 'start') {
    closeSheet();
    if (row.what === 'felix') chat.toggle(true);
    else if (row.what === 'new') actions.new({ dataset: {} });
    else if (row.what === 'record') go('#/record');
    else setTimeout(() => newTaskSheet(), LEAVE_MS);
    return;
  }
  if (row.kind === 'combine') {
    if (!row.id) return;
    closeSheet();
    setTimeout(() => explorerAction('combine', { kind: 'note', id: row.id, dir: row.dir || '' }).catch(fail), LEAVE_MS);
    return;
  }
  if (row.kind === 'ask') {
    closeSheet();
    if (!chat.askNew(row.text)) toast('Felix is still answering. Try again in a moment.', { bad: true });
    return;
  }
  if (row.kind === 'open' || row.kind === 'place' || row.kind === 'folder' || row.kind === 'search') {
    const search = inQuick('#quick-input')?.value || '';
    closeSheet();
    go(row.kind === 'open' ? noteHash(row.id) : row.kind === 'folder' ? folderHash(row.path) : row.kind === 'search' ? `#/search/${enc(row.text)}` : row.hash, { search });
    return;
  }
  if (row.kind === 'chat') {
    if (chat.isBusy()) {
      toast('Felix is still answering. Try again in a moment.', { bad: true });
      return;
    }
    closeSheet();
    await chat.openSaved(row.id);
    return;
  }
  quick.busy = true;
  try {
    if (row.kind === 'task') {
      row.request = row.request || requestId();
      const made = await api('/api/tasks', { method: 'POST', body: { title: row.title, date: row.date, time: row.time, request: row.request } });
      closeSheet();
      loadTasksWindow().catch(() => {});
      toast(`Added “${made.title}” · ${quickWhen(row.date, row.time)}`, {
        action: 'Undo',
        run: async () => {
          await api(`/api/tasks/${enc(made.id)}`, { method: 'DELETE' });
          loadTasksWindow().catch(() => {});
        },
      });
      return;
    }
    const [first, ...rest] = row.title.split('\n');
    const note = await api('/api/notes', { method: 'POST', body: { title: first.trim() || 'Quick note', body: rest.join('\n').trim() } });
    closeSheet();
    toast(`Saved “${note.title}”`, { action: 'Open', run: () => go(noteHash(note.id)) });
    if (state.view === 'workspace') showLatest().catch(() => {});
  } finally {
    quick.busy = false;
  }
}

function menu(el) {
  const here = creationDir();
  const rows = [
    !sideWide() && { action: 'folders', icon: ICON.folder, text: 'Folders' },
    { action: 'quick-leo', icon: ICON.search, text: 'Search or ask' },
    { action: 'new', dir: here, icon: ICON.plus, text: 'New note' },
    { action: 'record', dir: here, icon: ICON.mic, text: 'Record a lecture or meeting' },
    { action: 'upload', icon: ICON.upload, text: 'Make a note from a file' },
    { action: 'new-folder', icon: ICON.folderPlus, text: `New folder${here ? ` in ${folderLabel(here)}` : ''}` },
    { action: 'calendar', icon: ICON.calendar, text: 'Calendar', after: tasksDueBadge() },
    { action: 'boards', icon: ICON.board, text: 'Boards' },
    { action: 'settings', icon: ICON.gear, text: 'Settings' },
    { action: 'trash', icon: ICON.trash, text: 'Trash' },
    { action: 'keeping', icon: ICON.storage, text: 'Keeping' },
    { action: 'refresh', icon: ICON.refresh, text: 'Refresh' },
  ].filter(Boolean);
  if (window.matchMedia('(min-width: 600px)').matches) return popupMenu(el, { label: 'More', rows });
  const box = sheet(rows.map(({ action, dir, icon, text, after = '' }) => `<button class="list-row" data-action="${action}"${dir === undefined ? '' : ` data-dir="${esc(dir)}"`}>${icon}<span class="grow">${esc(text)}</span>${after}</button>`).join(''));
  box.returnFocus = $('#menu');
}

function newFolder(el) {
  const parent = creationDir(el && el.dataset && el.dataset.parent);
  const host = [$('#folders-sheet'), $('#side')].find((box) => box?.querySelector('.side-tree')?.clientHeight);
  const tree = host?.querySelector('.side-tree');
  const row = parent && [...(tree?.querySelectorAll('.side-folder') || [])].find((folder) => folder.dataset.dropDir === parent);
  if (tree && (row || !parent)) {
    if (parent) revealFolder(parent);
    if (sideMakingOpen(tree, parent, (name) => saveFolder(name, parent))) return;
  }
  const scrim = sheet(`<h3>New folder${parent ? ` in ${esc(folderLabel(parent))}` : ''}</h3>
    <label class="field">${ICON.folder}<input id="folder-name" data-parent="${esc(parent)}" placeholder="${parent ? 'Name, e.g. week 3' : 'Name, e.g. cs130'}" maxlength="120" autocomplete="off"></label>
    ${sheetButtons(['Create', 'data-action="create-folder"'])}`);
  const input = $('#folder-name', scrim);
  input.focus();
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') createFolder();
  });
}

function renameFolder(from) {
  const row = explorerFocus({ kind: 'folder', dir: from });
  if (!row) return;
  inlineEdit(row.querySelector('.side-label').getBoundingClientRect(), {
    id: 'folder-rename',
    value: folderLabel(from),
    label: 'Folder name',
    at: { kind: 'folder', dir: from },
    save: async (name) => {
      if (!validFolderName(name)) return false;
      try {
        for (const p of panesIn(from)) await ready(p.s);
        const moved = await api('/api/dirs/rename', { method: 'POST', body: { from, name } });
        await afterMove(from, moved.path);
        toast(`Renamed folder to “${name}”`, {
          action: 'Undo',
          run: () => api('/api/dirs/rename', { method: 'POST', body: { from: moved.path, name: folderLabel(from) } }).then(() => afterMove(moved.path, from)).catch(fail),
        });
        return { kind: 'folder', dir: moved.path };
      } catch (error) {
        if (error.status === 409) toast('That folder name is already in use. Choose another name.', { bad: true });
        else fail(error);
        return false;
      }
    },
  });
}

async function trashFolder(from) {
  if (!from) return;
  const mine = seq;
  for (const p of panesIn(from)) await ready(p.s);
  const done = await api('/api/trash/move', { method: 'POST', body: { notes: [], dirs: [from] } });
  if (mine === seq) for (const p of panesIn(from).reverse()) paneGone(p);
  if (inFolder(side.target || '', from)) side.target = parentOf(from);
  if (inFolder(side.root, from)) sideFocus('');
  loadSideFolders();
  refocusExplorer({ kind: 'folder', dir: parentOf(from) });
  toast(`Moved “${folderLabel(from)}” to the trash`, { action: 'Undo', run: () => undoTrashMove(done, from) });
}

async function saveFolder(name, parent) {
  if (!validFolderName(name)) return false;
  const path = parent ? `${parent}/${name}` : name;
  try {
    await api('/api/dirs', { method: 'POST', body: { path } });
    revealFolder(path);
    await loadSideFolders();
    return { kind: 'folder', dir: path };
  } catch (e) {
    if (e.status === 409) toast('That folder name is already in use. Choose another name.', { bad: true });
    else fail(e);
    return false;
  }
}

async function createFolder() {
  const input = $('#folder-name');
  const box = input.closest('.scrim');
  const button = box.querySelector('[data-action="create-folder"]');
  if (button.disabled) return;
  button.disabled = true;
  const made = await saveFolder(input.value.trim(), input.dataset.parent || '');
  if (button.isConnected) button.disabled = false;
  if (!made) return;
  const reopen = !sideWide() && !box.foldersReturn;
  if (box.isConnected) closeSheet();
  if (reopen) showFolders();
}

function validFolderName(name) {
  if (!name) { toast('Enter a folder name.', { bad: true }); return false; }
  if ([...name].length > 120 || new TextEncoder().encode(name).length > 240) { toast('That folder name is too long. Use 120 characters or fewer.', { bad: true }); return false; }
  if (/[\/\\]/.test(name)) { toast('A folder name cannot contain / or \\. To nest folders, make one, then make another inside it.', { bad: true }); return false; }
  if (name === '.' || name === '..') { toast('Choose a name other than . or ..', { bad: true }); return false; }
  return true;
}

async function moveSheet(el) {
  const s = await ready();
  if (!s) return toast('Write something first.');
  return explorerMoveMenu({ kind: 'note', id: s.note.id, dir: s.note.directory }, s.note, el.isConnected ? el : $('[data-action="move"]'));
}

let trashing = null;
async function trashNote(note = null, at = null) {
  if (!note) {
    const s = await ready();
    if (!s) return back();
    note = s.note;
  }
  if (trashing === note.id) return;
  trashing = note.id;
  try {
    await api(`/api/notes/${enc(note.id)}`, { method: 'DELETE' });
  } finally { trashing = null; }
  if (paneWith(note.id)) paneGone(paneWith(note.id));
  loadSideFolders();
  if (at) refocusExplorer({ kind: 'folder', dir: at.dir });
  toast(`Moved “${note.title || 'Untitled'}” to the trash`, { action: 'Undo', run: () => restore(note.id, false) });
}

async function restore(id, open) {
  try {
    const note = await api(`/api/trash/${enc(id)}/restore`, { method: 'POST' });
    await refreshSideTree();
    if (open) return go(noteHash(note.id));
    toast(`Restored “${note.title}”`, { action: 'Open', run: () => go(noteHash(note.id)) });
    pulse($('.toast'));
    if (state.view === 'trash') showTrash();
  } catch (e) {
    fail(e);
  }
}

async function pinCard(el) {
  const on = !el.classList.contains('on');
  el.classList.toggle('on', on);
  el.setAttribute('aria-pressed', String(on));
  try {
    await api(`/api/notes/${enc(el.dataset.id)}`, { method: 'PATCH', body: { pinned: on } });
    toast(on ? 'Pinned to the top of its folder' : `Unpinned “${el.dataset.title || 'Untitled'}”`);
    await render();
  } catch (e) {
    el.classList.toggle('on', !on);
    el.setAttribute('aria-pressed', String(!on));
    fail(e);
  }
}

async function noteMarkdown() {
  const s = await ready();
  if (!s) {
    toast('Write something first.');
    return null;
  }
  const { title, body } = snapshot(s);
  return { title, text: `# ${title}\n\n${body}`.trimEnd() + '\n' };
}

async function copyMarkdown() {
  const note = await noteMarkdown();
  if (!note) return;
  try {
    await navigator.clipboard.writeText(note.text);
    toast(`Copied “${note.title || 'Untitled'}” as Markdown`);
  } catch (e) {
    toast(`Could not copy: ${e.message || 'the clipboard is not available.'}`, { bad: true });
  }
}

async function saveMarkdown() {
  const note = await noteMarkdown();
  if (!note) return;
  const link = document.createElement('a');
  link.href = URL.createObjectURL(new Blob([note.text], { type: 'text/markdown' }));
  link.download = `${note.title.replace(/[\\/:*?"<>|]+/g, '-') || 'Untitled'}.md`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(link.href), 1000);
}

function shareRows() {
  return [
    { text: 'Copy as Markdown', icon: ICON.note, run: copyMarkdown },
    { text: 'Save .md file', icon: ICON.download, run: saveMarkdown },
    { text: 'Export as PDF', icon: ICON.share, run: share },
    ...(marginPrintable().length ? [{ text: 'Export as PDF with Felix’s notes', icon: ICON.share, run: () => share(true) }] : []),
  ];
}

function shareMenu(owner) {
  popupMenu(owner, { label: 'Share', rows: shareRows() });
}

function throughRow() {
  if (marginPage.through) return [{ text: 'Stop reading through', icon: ICON.close, run: () => marginThroughClick() }];
  const personas = marginPrefs?.personas || [];
  const off = !marginPage.s?.note.id || marginPage.changing || !personas.length;
  if (!marginPrefs) marginLoadPrefs();
  const why = personas.length ? [] : [{ note: marginPrefs ? 'No readers to choose from' : 'Readers did not load, open this menu again' }];
  return [{ text: 'Read through', icon: ICON.readThrough, off, next: personas.map((p) => ({ text: p.name, icon: PERSONA_ICON[p.icon] || PERSONA_ICON.book, checked: p.id === marginPage.persona, run: () => marginThroughClick(p.id) })) }, ...why];
}

function noteMenu(owner) {
  const note = state.view === 'note' ? state.session?.note || {} : {};
  const words = note.id && state.session.doc ? md.wordCount(state.session.doc.source()) : 0;
  popupMenu(owner, {
    label: 'This note',
    rows: [
      ...(note.id && note.recorded ? [{ text: 'Show transcript', icon: ICON.mic, run: () => showRecordingSources(window.getSelection().toString().trim().slice(0, 200)) }] : []),
      ...(note.id ? [{ text: 'Tasks', icon: ICON.calendar, run: () => noteTasksSheet() }] : []),
      { text: 'Add picture', icon: ICON.image, run: () => choosePictures() },
      { text: 'Move to folder', icon: ICON.move, run: () => moveSheet(owner) },
      ...throughRow(),
      { text: 'Share', icon: ICON.share, next: shareRows() },
      ...(split() ? [{ text: 'Close this pane', icon: ICON.close, run: () => paneClose(here()) }] : []),
      { line: true },
      { text: 'Move to trash', icon: ICON.trash, danger: true, run: () => trashNote() },
      ...(words ? [{ line: true }, { note: `${words.toLocaleString()} ${words === 1 ? 'word' : 'words'}` }] : []),
    ],
  });
}

async function share(notes = false) {
  printNotesClear();
  const mine = seq;
  const s = await ready();
  if (mine !== seq) return;
  if (!s) return toast('Write something first.');
  const before = document.title;
  document.title = s.note.title;
  const restore = () => {
    if (mine === seq) document.title = before;
    window.removeEventListener('afterprint', restore);
  };
  window.addEventListener('afterprint', restore);
  try {
    if (notes) printNotes(s);
    await window.print();
    if (printingNotes) for (const name of ['pointerdown', 'keydown']) window.addEventListener(name, printNotesClear, true);
  } catch (e) {
    restore();
    printNotesClear();
    toast('Could not open the print dialog. Try PDF export again.', { bad: true });
  }
}

let printingNotes = false;

function printNotesClear() {
  if (!printingNotes) return;
  printingNotes = false;
  for (const name of ['afterprint', 'pointerdown', 'keydown']) window.removeEventListener(name, printNotesClear, true);
  for (const el of document.querySelectorAll('[data-print]')) {
    const up = el.parentNode;
    el.remove();
    up?.normalize();
  }
}

function printNotes(s) {
  printingNotes = true;
  window.addEventListener('afterprint', printNotesClear, true);
  const found = marginPage.s === s ? marginPrintable() : [];
  const list = document.createElement('ol');
  for (const { at, text } of found) {
    const point = s.doc.domPoint(at.to);
    const sup = document.createElement('sup');
    sup.hidden = true;
    sup.dataset.print = String(list.childElementCount + 1);
    if (point.node.nodeType === Node.TEXT_NODE) leoDoc.insertAt(point, sup);
    else {
      const blk = s.doc.blockAt(Math.max(at.from, at.to - 1));
      if (!blk) continue;
      const range = document.createRange();
      range.selectNodeContents(blk);
      range.collapse(false);
      range.insertNode(sup);
    }
    const item = document.createElement('li');
    marginAnswer(item, text);
    list.append(item);
  }
  if (!list.childElementCount) return;
  const notes = document.createElement('section');
  notes.className = 'print-notes';
  notes.hidden = true;
  notes.dataset.print = '';
  notes.append(list);
  $('#doc', marginPage.root).after(notes);
  const title = (s.note.title || '').trim().replace(/["\\]/g, '\\$&').replace(/[\n\r\f]/g, ' ');
  if (title) {
    const footer = document.createElement('style');
    footer.dataset.print = '';
    footer.textContent = `@media print { @page { @bottom-center { content: "${title} · " counter(page) " / " counter(pages); } } }`;
    document.head.append(footer);
  }
}

async function openTitle(title) {
  const want = title.trim().toLowerCase();
  if (!want) return;
  let hit = null;
  for (let offset = 0; !hit; offset += SEARCH_STEP) {
    const hits = await api(`/api/search?q=${enc(title)}&brief=true&limit=${SEARCH_STEP}&offset=${offset}`);
    hit = hits.find((n) => n.title.trim().toLowerCase() === want);
    if (hits.length < SEARCH_STEP) break;
  }
  if (hit && split()) return besideOpen(hit.id, here());
  go(hit ? noteHash(hit.id) : `#/search/${enc(title)}`);
}

function openSearch(value) {
  const box = $('#search');
  box.hidden = false;
  const input = $('#search-input');
  if (value !== undefined && document.activeElement !== input) input.value = value;
}

function closeSearch() {
  $('#search').hidden = true;
  $('#search-input').value = '';
}

let searchTimer;
$('#search-input').addEventListener('input', (e) => {
  clearTimeout(searchTimer);
  const query = e.target.value;
  searchTimer = setTimeout(() => {
    if (state.view === 'search') history.replaceState(history.state, '', `#/search/${enc(query)}`);
    else history.pushState(null, '', `#/search/${enc(query)}`);
    showSearch(query).catch(fail);
  }, 160);
});

function discardRecordingAsk(el) {
  const box = confirmSheet({ title: 'Delete this recording?', text: 'Its saved audio will be permanently deleted from this device and your computer. Notes already written stay.', yes: 'Delete recording', action: 'rec-discard-now', data: ` data-id="${esc(el.dataset.id)}"` });
  box.returnFocus = el;
  box.querySelector('[data-action="close"]').focus();
}

async function discardRecordingNow(el) {
  if (el.disabled) return;
  el.disabled = true;
  const box = el.closest('.scrim');
  try {
    await recorder.discard(el.dataset.id);
    if (box && box.isConnected) closeSheet();
  } finally {
    if (el.isConnected) el.disabled = false;
  }
}
