const SIDE_KEY = 'leo-side';
const sideWide = () => Boolean(window.matchMedia && window.matchMedia('(min-width: 1000px)').matches);
const sideNarrow = () => Boolean(window.matchMedia && window.matchMedia('(min-width: 600px) and (max-width: 999px)').matches);
const sideDrawerOpen = () => document.body.classList.contains('side-drawer');
const sideActive = () => sideWide() && !side.mini || sideDrawerOpen() || Boolean(document.querySelector('#folders-sheet'));
const creationDir = (explicit) => {
  const dir = explicit ?? side.target ?? (state.view === 'note' ? state.dir || '' : '');
  return !side.root || dir === side.root || dir.startsWith(`${side.root}/`) ? dir : side.root;
};
const side = { root: '', sort: 'title', flip: false, tasks: false, filter: '', focusKey: '', folders: [], loaded: false, mini: false, open: new Set(), revealed: new Set(), shut: new Set(), here: null, target: null, twisted: null, notes: new Map(), pending: [], reading: 0, visible: [], automatic: new Set(), generation: 0, sig: [], lastSig: '', pill: '', closing: new Set(), growing: null, freshUntil: Infinity, freshTimer: 0, quietTimer: 0, making: null, frame: 0, rev: null, revAt: 0, unopened: new Map() };
// Folders the user opened with their own hands, kept across restarts. Folders
// unfolded only to show the open note or folder (`side.revealed`) are not
// kept, so the explorer reopens exactly as the user left it, closed by default.
// A new key (2026-11-03): the old one also held every folder browsing had
// unfolded, so starting from it would keep those open.
const SIDE_OPEN_KEY = 'leo-side-folders';
const SIDE_FOCUS_KEY = 'leo-focus';
const SIDE_NOTES_PAGE = 100;
const SIDE_NOTES_FOLDERS = 64;
const SIDE_NOTES_AUTOMATIC = 32;
const SIDE_NOTES_READS = 4;
const SIDE_NOTES_TIMEOUT = 10000;
const SIDE_NEW_MS = 60000;
const SIDE_REV_MS = 60000;
const SIDE_FRESH_MS = 30 * 60000;
const SIDE_FRESH_GROUP = 1000;
const SIDE_UNOPENED = '<svg class="side-unopened" viewBox="0 0 13 13" fill="currentColor" role="img" aria-label="New"><title>New, not opened yet</title><path d="M6 2.2c1.7-.5 3.3.3 3.9 1.8.4 1-.1 1.6.3 2.6.6 1.5-.3 2.9-1.8 3.2-1.1.2-1.6-.4-2.7-.1-1.6.4-3.2-.5-3.4-2.2-.1-1.1.6-1.6.5-2.7C2.7 3.4 4.3 2.7 6 2.2z"/><circle cx="11.5" cy="10.6" r="1.3"/></svg>';

try {
  side.open = new Set(JSON.parse(localStorage.getItem(SIDE_OPEN_KEY) || '[]'));
  localStorage.removeItem('leo-side-open');
} catch (e) {}

try {
  side.mini = localStorage.getItem(SIDE_KEY) === 'mini';
  side.root = localStorage.getItem(SIDE_FOCUS_KEY) || '';
} catch (e) {}

const SIDE_ORDER_KEY = 'leo-side-order';
const SIDE_SORTS = { title: { text: 'Name', orders: ['title', 'title-desc'], ways: ['A to Z', 'Z to A'] }, updated: { text: 'Last edited', orders: ['updated', 'oldest'], ways: ['Newest first', 'Oldest first'] }, created: { text: 'Created', orders: ['created', 'created-asc'], ways: ['Newest first', 'Oldest first'] } };

try {
  const kept = JSON.parse(localStorage.getItem(SIDE_ORDER_KEY) || '{}');
  if (Object.hasOwn(SIDE_SORTS, kept.sort)) side.sort = kept.sort;
  side.flip = kept.flip === true;
  side.tasks = kept.tasks === true;
} catch (e) {}
document.body.classList.toggle('side-mini', side.mini || sideNarrow());

const SIDE_LEAST_WIDTH = 220;
const SIDE_MOST_WIDTH = 520;
const SIDE_PAGE_ROOM = 380;
const SPLIT_ROOM = 2 * SIDE_PAGE_ROOM;
const sideGrip = document.createElement('div');
sideGrip.className = 'pane-resize side-resize';
sideGrip.tabIndex = 0;
sideGrip.title = 'Drag to resize folders; double-click to reset';
sideGrip.setAttribute('role', 'separator');
sideGrip.setAttribute('aria-orientation', 'vertical');
sideGrip.setAttribute('aria-label', 'File explorer width');
sideGrip.setAttribute('aria-controls', 'side-explorer');
document.body.appendChild(sideGrip);
const sideResize = window.leoPaneResize.create({
  handle: sideGrip,
  key: 'leo-side-width',
  measure: () => $('#side').getBoundingClientRect().width,
  limits: () => [SIDE_LEAST_WIDTH, Math.max(SIDE_LEAST_WIDTH, Math.min(SIDE_MOST_WIDTH, sideRoom()))],
  apply: (width) => {
    if (width === null) document.body.style.removeProperty('--explorer-w');
    else document.body.style.setProperty('--explorer-w', `${width}px`);
  },
  enabled: () => sideWide() && !side.mini && !document.body.classList.contains('side-folding') && state.view !== 'locked',
  onChange: () => window.dispatchEvent(new Event('resize')),
});

function sideRoom() {
  const body = document.body;
  const chat = body.classList.contains('chat-open') && document.querySelector('.chat:not([hidden])');
  const right = Math.max(parseFloat(getComputedStyle(body).paddingRight) || 0, chat ? window.innerWidth - chat.getBoundingClientRect().left : 0);
  return window.innerWidth - right - (body.classList.contains('split') ? SPLIT_ROOM : SIDE_PAGE_ROOM);
}

function sideRow(action, icon, label, { on = false, attrs = '', count = '', hint = label, aria = label } = {}) {
  const tail = count === '' ? '' : `<span class="side-count side-label">${esc(String(count))}</span>`;
  return `<button class="side-row${on ? ' on' : ''}" data-action="${action}"${attrs} aria-label="${esc(aria)}" title="${esc(hint)}"${on ? ' aria-current="page"' : ''}>${icon}<span class="side-label">${esc(label)}</span>${tail}</button>`;
}

function sidePlace() {
  const view = state.view;
  if (view === 'workspace') return 'home';
  if (view === 'note' || view === 'new') return `dir:${state.dir || ''}`;
  // Keeping and Trash live inside Settings, so the sidebar shows Settings for them.
  if (view === 'keeping' || view === 'trash') return 'settings';
  return view;
}

function drawSide() {
  const box = $('#side');
  if (!box) return;
  box.hidden = state.view === 'locked';
  sideGrip.hidden = box.hidden;
  sideResize.fit();
  if (box.hidden) return;
  const here = sidePlace();
  const dir = sideHere();
  // Unfold to what is open: again whenever the folder or the open note changes,
  // so a note opened after its folder was closed still shows where it is.
  const showing = state.view === 'note' ? `${dir}\n${panes.map((p) => p.s?.note?.id || '').join('\n')}` : 'workspace';
  if (showing !== side.showing) {
    side.showing = showing;
    side.revealed.clear();
    if (state.view === 'note') side.shut.clear();
    const parts = dir.split('/').filter(Boolean);
    for (let i = 1; i <= parts.length; i++) side.revealed.add(parts.slice(0, i).join('/'));
  }
  side.here = dir;
  const actionDir = creationDir();
  let folders = folderTree(dir);
  if (sideActive()) {
    side.automatic = new Set(side.visible.slice(0, SIDE_NOTES_AUTOMATIC));
    let queued = false;
    for (const path of side.automatic) {
      const entry = side.notes.get(path);
      if (!entry || entry.seq !== seq) { loadSideNotes(path, entry?.offset || 0); queued = true; }
    }
    if (queued) folders = folderTree(dir);
  }
  const scrolled = box.querySelector('.side-tree');
  const kept = scrolled ? scrolled.scrollTop : 0;
  const high = side.growing ? sideKids(side.growing.path)?.getBoundingClientRect().height || 0 : 0;
  const railScroll = box.querySelector('.side-rail-main')?.scrollTop || 0;
  const active = box.contains(document.activeElement) ? document.activeElement.closest('button[data-action]') : null;
  const focused = active ? Object.fromEntries(['action', 'dir', 'parent', 'id', 'offset'].filter((key) => key in active.dataset).map((key) => [key, active.dataset[key]])) : null;
  const sig = side.sig.join('\n');
  const listChanged = sig !== side.lastSig;
  side.lastSig = sig;
  const pillBefore = box.querySelector('.side-tree .side-pill');
  const pillFrom = pillBefore ? pillBefore.getBoundingClientRect() : null;
  const paint = () => refreshPanel(box, `
    <div class="side-head" data-tauri-drag-region="deep">
      <button class="icon-btn side-fold" data-action="side-fold" aria-label="${sideShown() ? 'Hide folders' : 'Show folders'}" title="${sideShown() ? 'Hide folders' : 'Show folders'}" aria-expanded="${sideShown()}" aria-controls="side-explorer">${ICON.sidebar}</button>
      <button class="brand side-label" data-action="home"><svg class="brand-mark" aria-hidden="true"><use href="#leo-mark"/></svg>leo</button>
    </div>
    <div class="side-rail">
      <nav class="side-rail-main" aria-label="Leo tools">
        ${sideRow('quick-leo', ICON.search, 'Search', { hint: `Search or ask (${shortcutText(shortcutKeys('quick'))})` })}
        ${sideRow('calendar', ICON.calendar, 'Calendar', { on: here === 'calendar' })}
        ${sideRow('boards', ICON.board, 'Boards', { on: here === 'boards', hint: 'Boards · track tasks from To do to Done' })}
        ${sideRow('record', ICON.mic, 'Record', { on: here === 'record', attrs: ` data-dir="${esc(actionDir)}"` })}
        ${sideRow('upload', ICON.upload, 'Note from a file', { attrs: ` data-dir="${esc(actionDir)}"` })}
      </nav>
      <nav class="side-foot" aria-label="This app">
        ${sideRow('settings', ICON.gear, 'Settings', { on: here === 'settings' })}
      </nav>
    </div>
    <div class="side-explorer" id="side-explorer">
      <div class="side-group" data-drop-dir="" title="Drop a folder or note here to move it ${side.root ? `into ${esc(folderLabel(side.root))}` : 'to the top level'}"><button class="side-row side-new" data-action="new" data-dir="${esc(actionDir)}" aria-label="New note" aria-keyshortcuts="${shortcutAria(shortcutKeys('new'))}" title="New note${actionDir ? ` in ${esc(folderLabel(actionDir))}` : ''} (${shortcutText(shortcutKeys('new'))})">${ICON.filePlus}<span class="side-label">New note</span><kbd aria-hidden="true">${shortcutText(shortcutKeys('new'))}</kbd></button><button class="side-add" data-action="new-folder" data-parent="${esc(actionDir)}" aria-label="New folder" title="New folder${actionDir ? ` in ${esc(folderLabel(actionDir))}` : ''}">${ICON.folderPlus}</button></div>
      <div class="side-filter-row"><label class="field side-filter">${ICON.search}<input id="side-filter" type="search" placeholder="Filter" aria-label="Filter loaded notes" autocomplete="off" spellcheck="false" value="${esc(side.filter || '')}"></label><button class="side-add side-sort${side.sort !== 'title' || side.flip || side.tasks ? ' on' : ''}" data-action="side-sort" aria-label="Sort notes" title="${esc(`Sorted by ${SIDE_SORTS[side.sort].text.toLowerCase()}, ${SIDE_SORTS[side.sort].ways[side.flip ? 1 : 0].toLowerCase()}${side.tasks ? ', open tasks only' : ''}`)}" aria-haspopup="menu" aria-expanded="${Boolean(popup?.owner.matches('[data-action="side-sort"]'))}">${ICON.sort}</button></div>
      ${sideLeave()}<nav class="side-nav side-tree" aria-label="Folders" data-drop-dir="" tabindex="0">
        ${folders || '<p class="side-none">Folders you make show up here.</p>'}
      </nav>
    </div>`);
  if (listChanged && !side.twisted && !side.growing && !side.closing.size && sideWide()) flipList(box, '.side-tree .side-folder[data-refresh-key], .side-tree .side-note[data-id]', (el) => (el.dataset.id ? `n:${el.dataset.id}` : `d:${el.dataset.refreshKey}`), paint);
  else paint();
  inkSettle((id) => box.querySelectorAll(`.side-tree .side-note[data-id="${CSS.escape(id)}"]`));
  applySideFilter(box.querySelector('.side-tree'));
  sideRoving(box.querySelector('.side-tree'));
  const pill = box.querySelector('.side-tree .side-pill');
  const owner = pillKey(pill);
  if (pill && owner !== side.pill && pillFrom && sideWide()) pillGlide(pillFrom, pill);
  side.pill = owner;
  const tree = box.querySelector('.side-tree');
  if (tree) tree.scrollTop = kept;
  revealNoteRow(tree);
  box.querySelector('.side-rail-main').scrollTop = railScroll;
  drawFoldersSheet();
  clearTimeout(side.freshTimer);
  if (side.freshUntil < Infinity) side.freshTimer = setTimeout(sideFreshCheck, side.freshUntil - Date.now() + SIDE_FRESH_GROUP);
  if (side.growing) growFolder(high);
  if (focused) {
    const buttons = [...box.querySelectorAll('button[data-action]')];
    const button = buttons.find((button) => Object.entries(focused).every(([key, value]) => button.dataset[key] === value)) || (focused.action === 'side-notes' && buttons.find((button) => ['side-notes', 'open-folder'].includes(button.dataset.action) && button.dataset.dir === focused.dir));
    if (button) button.focus({ preventScroll: true });
  }
}

function sideHere() {
  return state.view === 'note' ? state.dir || '' : side.target || '';
}

function folderShown(path) {
  return !side.shut.has(path) && (side.open.has(path) || side.revealed.has(path));
}

// Like VS Code's auto reveal: the open note's row is scrolled into the
// explorer's view once, when it first appears for that note; later redraws keep
// whatever scroll the user chose. Only the explorer scrolls, never the page.
function revealNoteRow(tree) {
  const id = panes[0].s?.note?.id;
  if (!id) side.shownNote = null;
  // A hidden explorer has nothing to scroll; try again when it shows.
  if (!tree || !id || side.shownNote === id || !tree.clientHeight) return;
  const row = [...tree.querySelectorAll('.side-note[data-id]')].find((el) => el.dataset.id === id);
  if (!row) return;
  side.shownNote = id;
  const at = row.getBoundingClientRect();
  const view = tree.getBoundingClientRect();
  if (at.top < view.top) tree.scrollTop -= Math.ceil(view.top - at.top);
  else if (at.bottom > view.bottom) tree.scrollTop += Math.ceil(at.bottom - view.bottom);
}

function sideLeave() {
  return side.root ? sideRow('side-unfocus', ICON.close, folderLabel(side.root), { attrs: ` data-dir="${esc(side.root)}"`, aria: `${folderLabel(side.root)}: show all notes`, hint: `Showing only ${folderLabel(side.root)}. Click to show all notes` }) : '';
}

function keepFocus() {
  try {
    if (side.root) localStorage.setItem(SIDE_FOCUS_KEY, side.root);
    else localStorage.removeItem(SIDE_FOCUS_KEY);
  } catch (e) {}
}

function sideFocus(path) {
  side.root = path;
  keepFocus();
  drawSide();
}

function keepOpenFolders() {
  try {
    localStorage.setItem(SIDE_OPEN_KEY, JSON.stringify([...side.open]));
  } catch (e) {}
}

function folderTree(here) {
  side.visible = [side.root];
  side.sig = [];
  side.freshUntil = Infinity;
  const children = (parent) => side.folders.filter((d) => {
    const cut = d.name.lastIndexOf('/');
    return (cut < 0 ? '' : d.name.slice(0, cut)) === parent;
  });
  const rows = [];
  const twisted = side.twisted;
  const unopened = new Set();
  for (const dir of side.unopened.values()) for (let path = dir; path; path = path.slice(0, Math.max(0, path.lastIndexOf('/')))) unopened.add(path);
  const walk = (parent, depth) => {
    if (side.making && side.making.parent === parent) rows.push(`<div class="side-making" data-refresh-key="side-making" style="--depth:${depth}" aria-hidden="true">${ICON.folder}</div>`);
    for (const d of children(parent)) {
      const open = folderShown(d.name);
      const name = d.name.split('/').pop();
      const on = side.target === d.name;
      const current = on && state.view !== 'note';
      const shown = twisted && twisted.path === d.name ? twisted.was : open;
      const toggle = `<button class="side-twist${shown ? ' open' : ''}" data-action="side-twist" data-dir="${esc(d.name)}" aria-expanded="${open}" aria-label="${open ? 'Close' : 'Open'} ${esc(name)}">${ICON.chevron}</button>`;
      side.sig.push(`d:${d.name}`);
      rows.push(`<div data-refresh-key="${esc(d.name)}" class="side-folder${on ? ' on' : ''}" data-drop-dir="${esc(d.name)}" style="--depth:${depth}">${on ? '<span class="side-pill" aria-hidden="true"></span>' : ''}${toggle}<button class="side-row" tabindex="-1" data-action="open-folder" data-dir="${esc(d.name)}" draggable="true" title="${esc(d.notes ? `${d.name} · ${d.notes} ${d.notes === 1 ? 'note' : 'notes'}` : d.name)}" aria-expanded="${open}"${current ? ' aria-current="page"' : ''}>${ICON.folder}<span class="side-label">${esc(name)}</span>${!open && unopened.has(d.name) ? SIDE_UNOPENED : ''}${d.notes ? `<span class="side-count">${d.notes}</span>` : ''}</button><button class="side-add side-sub" tabindex="-1" data-action="explorer-menu" data-kind="folder" data-dir="${esc(d.name)}" aria-label="Options for ${esc(name)}" title="Folder options" aria-haspopup="menu">${ICON.more}</button></div>`);
      if (open || side.closing.has(d.name)) {
        if (open) side.visible.push(d.name);
        rows.push(`<div class="side-kids" data-kids="${esc(d.name)}" data-drop-dir="${esc(d.name)}" style="--depth:${depth}">`);
        walk(d.name, depth + 1);
        rows.push('</div>');
      }
    }
    rows.push(sideNoteRows(parent, depth));
  };
  walk(side.root, 0);
  return rows.join('');
}

function sideKids(path) {
  return [...(document.querySelector('#folders-sheet .side-tree') || document.querySelector('#side .side-tree')).querySelectorAll('.side-kids')].find((k) => k.dataset.kids === path) || null;
}

function sideArrow(path) {
  return [...(document.querySelector('#folders-sheet .side-tree') || document.querySelector('#side .side-tree')).querySelectorAll('.side-twist')].find((b) => b.dataset.dir === path) || null;
}

function drawTwisted(path, was) {
  side.twisted = { path, was };
  drawSide();
  side.twisted = null;
}

function growFolder(high) {
  const { path, to } = side.growing;
  const kids = sideKids(path);
  if (kids && kids.scrollHeight !== to) {
    side.growing.to = kids.scrollHeight;
    slideHeight(kids, high, kids.scrollHeight);
  }
  if (!kids || !side.notes.get(path)?.loading) side.growing = null;
}

function twistFolder(path, control = null, keep = true) {
  if (control) control.focus({ preventScroll: true });
  const was = folderShown(path);
  if (was) {
    side.open.delete(path);
    side.shut.add(path);
  } else {
    (keep ? side.open : side.revealed).add(path);
    side.shut.delete(path);
  }
  if (keep) keepOpenFolders();
  const kids = sideKids(path);
  const from = kids ? kids.getBoundingClientRect().height : 0;
  side.growing = null;
  if (was && kids) {
    const arrow = sideArrow(path);
    if (arrow) arrow.classList.remove('open');
    side.closing.add(path);
    slideHeight(kids, from, 0, () => {
      side.closing.delete(path);
      drawTwisted(path, false);
    });
    return;
  }
  side.closing.delete(path);
  drawTwisted(path, was);
  const grown = sideKids(path);
  if (grown) {
    slideHeight(grown, from, grown.scrollHeight);
    if (side.notes.get(path)?.loading) side.growing = { path, to: grown.scrollHeight };
  }
  requestAnimationFrame(() => {
    const arrow = sideArrow(path);
    if (arrow) arrow.classList.toggle('open', !was);
  });
}

async function loadSideFolders() {
  if (tasksAtHome()) loadHomeRecent();
  if ((!sideWide() && !sideActive()) || state.view === 'locked') return drawSide();
  const mine = seq, generation = ++side.generation;
  const rev = await api('/api/rev').then((answer) => (typeof answer?.rev === 'string' ? answer.rev : null), () => null);
  if (mine !== seq || generation !== side.generation) return;
  if (rev && rev === side.rev && Date.now() - side.revAt < SIDE_REV_MS) {
    const order = sideReadOrder();
    for (const entry of side.notes.values()) if (entry.loaded && !entry.error && !entry.loading && entry.order === order && entry.rev === rev) entry.seq = seq;
    return drawSide();
  }
  try {
    const [folders, unopened] = await Promise.all([api('/api/folders'), api('/api/notes/unopened').catch(() => null)]);
    if (mine !== seq || generation !== side.generation) return;
    side.rev = rev;
    side.revAt = Date.now();
    side.folders = folders;
    if (unopened) side.unopened = new Map(Object.entries(unopened));
    for (const p of panes) side.unopened.delete(p.s?.note?.id);
    if (side.target && !folders.some((folder) => folder.name === side.target)) side.target = null;
    if (side.root && !folders.some((folder) => folder.name === side.root)) {
      sideFocus('');
      toast('The focused folder is gone, so leo shows all your notes again.');
    }
    side.loaded = true;
    for (const [path, entry] of side.notes) {
      if (path && !folders.some((folder) => folder.name === path)) side.notes.delete(path);
      else entry.seq = -1;
    }
  } catch (e) {
    if (mine !== seq || generation !== side.generation) return;
    side.rev = null;
    if (!side.loaded) side.folders = [];
  }
  drawSide();
}

const sideShown = () => (sideNarrow() ? sideDrawerOpen() : !side.mini);

function sideDrawer(open) {
  const body = document.body;
  if (sideDrawerOpen() === open) return;
  body.classList.toggle('side-drawer', open);
  body.classList.toggle('side-mini', !open && (side.mini || sideNarrow()));
  if (open) loadSideFolders().catch(() => {});
  else drawSide();
  if (open) ($('#side-filter') || $('#side .side-fold'))?.focus({ preventScroll: true });
  else if ($('#side')?.contains(document.activeElement)) $('#side .side-fold')?.focus({ preventScroll: true });
}

matchMedia('(min-width: 600px) and (max-width: 999px)').addEventListener('change', () => {
  document.body.classList.remove('side-drawer');
  document.body.classList.toggle('side-mini', side.mini || sideNarrow());
  drawSide();
});
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && sideDrawerOpen() && !document.querySelector('.scrim, .popup-menu')) sideDrawer(false);
});
document.addEventListener('pointerdown', (e) => {
  if (sideDrawerOpen() && !e.target.closest('#side, .popup-menu, .scrim, .sheet')) sideDrawer(false);
}, true);
window.addEventListener('hashchange', () => sideDrawer(false));

function foldSide() {
  if (sideNarrow() || (side.mini && split() && sideRoom() < SIDE_LEAST_WIDTH)) return sideDrawer(!sideDrawerOpen());
  side.mini = !side.mini;
  const generation = side.folding = (side.folding || 0) + 1;
  const body = document.body;
  body.classList.toggle('side-folding', sideWide() && !stillMotion());
  body.classList.toggle('side-mini', side.mini);
  sideResize.fit();
  try {
    localStorage.setItem(SIDE_KEY, side.mini ? 'mini' : 'wide');
  } catch (e) {}
  drawSide();
  window.dispatchEvent(new Event('resize'));
  if (body.classList.contains('side-folding')) {
    const transitions = body.getAnimations().filter((run) => run.transitionProperty === '--side');
    Promise.allSettled(transitions.map((run) => run.finished)).then(() => {
      if (side.folding !== generation) return;
      body.classList.remove('side-folding');
      window.dispatchEvent(new Event('resize'));
    });
  }
}

function moveOpenFolders(from, to) {
  const moved = (path) => (path === from || path.startsWith(`${from}/`) ? to + path.slice(from.length) : path);
  for (const set of [side.open, side.revealed, side.shut]) {
    const paths = [...set];
    set.clear();
    for (const path of paths) set.add(moved(path));
  }
  keepOpenFolders();
  side.root = moved(side.root);
  keepFocus();
}

function revealFolder(path) {
  side.target = path;
  side.revealed.clear();
  const parts = path.split('/').filter(Boolean);
  for (let i = 1; i <= parts.length; i++) {
    const dir = parts.slice(0, i).join('/');
    side.revealed.add(dir);
    side.shut.delete(dir);
  }
  drawSide();
}

function openFolder(el) {
  const path = el.dataset.dir;
  if (el.closest('.side-tree')) {
    side.target = path;
    const open = $('#side .side-note.on');
    if (open) {
      open.classList.replace('on', 'open');
      open.querySelector('.side-pill')?.remove();
    }
    twistFolder(path, el);
    return;
  }
  revealFolder(path);
  if (sideNarrow()) sideDrawer(true);
  else if (!sideWide()) showFolders();
  else if (side.mini) foldSide();
}

function sideNoteRows(path, depth) {
  const entry = side.notes.get(path);
  const action = (label, offset = 0) => `<button class="side-note-page" data-action="side-notes" data-dir="${esc(path)}" data-offset="${offset}">${label}</button>`;
  if (!entry) return `<div class="side-note-status" data-drop-dir="${esc(path)}" style="--depth:${depth}">${action('Show notes')}</div>`;
  const selected = new Set(panes.map((p) => p.s?.note?.id).filter(Boolean));
  const held = Boolean(side.target);
  for (const note of entry.items) side.sig.push(`n:${note.id}`);
  const rows = entry.items.map((note) => `<div class="side-note-wrap" data-refresh-key="n:${esc(note.id)}"><button class="side-row side-note${selected.has(note.id) ? (held ? ' open' : ' on') : ''}" data-action="open-note" tabindex="-1" data-id="${esc(note.id)}" data-from="${esc(path)}" data-drop-dir="${esc(path)}" data-title="${esc(note.title)}" draggable="true" style="--depth:${depth}" title="${esc(note.title)}"${selected.has(note.id) ? ' aria-current="page"' : ''}>${selected.has(note.id) && !held ? '<span class="side-pill" aria-hidden="true"></span>' : ''}<span class="side-label">${esc(note.title || 'Untitled')}</span>${sideNoteMark(note, selected.has(note.id))}${sideNoteWhen(note)}</button><button class="side-add side-sub" tabindex="-1" data-action="explorer-menu" data-kind="note" data-id="${esc(note.id)}" data-dir="${esc(path)}" aria-label="Options for ${esc(note.title || 'Untitled')}" title="Note options" aria-haspopup="menu">${ICON.more}</button></div>`).join('');
  const status = entry.loading && !entry.refreshing ? '<span>Opening notes…</span>' : entry.error ? `Notes could not be opened. ${action('Try again', entry.offset)}` : !entry.items.length && side.tasks ? '<span>No open tasks here</span>' : !entry.items.length && !side.folders.some((folder) => folder.name.startsWith(path ? `${path}/` : '')) ? (path === '' ? `<span>No notes yet.</span> <button class="side-note-page" data-action="new" data-dir="">Write your first note</button>` : '<span>Empty folder</span>') : '';
  const pages = !entry.error ? `${entry.offset ? action('Previous notes', Math.max(0, entry.offset - SIDE_NOTES_PAGE)) : ''}${entry.offset + entry.items.length < entry.total ? action('Next notes', entry.offset + SIDE_NOTES_PAGE) : ''}` : '';
  return rows + (status || pages ? `<div class="side-note-status" data-drop-dir="${esc(path)}" style="--depth:${depth}">${status}${pages}</div>` : '');
}

function noteOpened(id) {
  side.unopened.delete(id);
  api(`/api/notes/${enc(id)}/unopened`, { method: 'DELETE' }).catch(() => {});
}

async function refreshSideTree() {
  side.rev = null;
  await loadSideFolders();
  for (const [path, entry] of [...side.notes]) loadSideNotes(path, entry.offset || 0, true);
}

function loadSideNotes(path, offset = 0, force = false) {
  if (path !== '' && !side.folders.some((folder) => folder.name === path)) return;
  if (!Number.isSafeInteger(offset) || offset < 0) return;
  const previous = side.notes.get(path);
  const order = sideReadOrder();
  if (previous?.seq === seq && previous.order === order && (!force || (previous.loading && previous.offset === offset))) return;
  if (!previous && side.notes.size >= SIDE_NOTES_FOLDERS) {
    const oldest = [...side.notes.keys()].find((key) => !side.automatic.has(key));
    side.notes.delete(oldest);
  }
  const entry = { items: previous?.items || [], total: previous?.total || 0, offset, loading: true, loaded: Boolean(previous?.loaded), refreshing: Boolean(previous?.loaded && previous.offset === offset), error: false, seq, order, reordered: Boolean(previous && previous.order !== order) };
  side.notes.set(path, entry);
  side.pending = side.pending.filter((read) => side.notes.get(read.path) === read.entry);
  side.pending.push({ path, entry });
  pumpSideNotes();
  if (force) drawSide();
}

function sideReadOrder() {
  return `${SIDE_SORTS[side.sort].orders[side.flip ? 1 : 0]} ${side.tasks ? 'tasks' : 'all'}`;
}

function pumpSideNotes() {
  while (side.reading < SIDE_NOTES_READS && side.pending.length) {
    const read = side.pending.shift();
    if (side.notes.get(read.path) !== read.entry || read.entry.seq !== seq) continue;
    side.reading++;
    readSideNotes(read.path, read.entry).finally(() => { side.reading--; pumpSideNotes(); });
  }
}

function sideDrawSoon() {
  if (!side.frame) side.frame = requestAnimationFrame(() => { side.frame = 0; drawSide(); });
}

async function readSideNotes(path, entry) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), SIDE_NOTES_TIMEOUT);
  const rev = side.rev;
  try {
    const [sort, show] = entry.order.split(' ');
    const query = new URLSearchParams({ dir: path, offset: String(entry.offset), limit: String(SIDE_NOTES_PAGE), rows: 'true', sort, show });
    const page = await api(`/api/notes?${query}`, { total: true, signal: controller.signal });
    if (side.notes.get(path) !== entry || entry.seq !== seq) return;
    if (!Array.isArray(page.items)) throw new Error('Notes could not be opened.');
    const had = entry.refreshing && new Set(entry.items.map((note) => note.id));
    const before = entry.refreshing ? JSON.stringify([entry.items, entry.total]) : null;
    entry.items = page.items.slice(0, SIDE_NOTES_PAGE).filter((note) => typeof note.id === 'string' && typeof note.title === 'string');
    entry.total = Number.isSafeInteger(page.total) && page.total >= 0 ? page.total : entry.items.length;
    entry.quiet = before !== null && before === JSON.stringify([entry.items, entry.total]);
    entry.loaded = true;
    entry.rev = rev;
    if (had) for (const note of entry.items) if (!had.has(note.id) && Date.now() - Date.parse(note.created_at) < SIDE_NEW_MS) inkLand(note.id);
    if (!entry.items.length && entry.offset && entry.total) setTimeout(() => loadSideNotes(path, Math.floor((entry.total - 1) / SIDE_NOTES_PAGE) * SIDE_NOTES_PAGE, true));
  } catch (e) {
    if (side.notes.get(path) === entry && entry.seq === seq) {
      entry.error = true;
      if (entry.reordered) entry.items = [];
    }
  } finally {
    clearTimeout(timeout);
    entry.loading = false;
    if (side.notes.get(path) === entry && entry.seq === seq) {
      if (!entry.quiet || entry.error) sideDrawSoon();
      if (path === side.root && tasksAtHome()) drawHomeToday();
    }
  }
}

function sideNoteMark(note, open) {
  const until = Date.parse(note.updated_at) + SIDE_FRESH_MS;
  const fresh = side.sort !== 'updated' && until > Date.now() && until - Date.now() <= SIDE_FRESH_MS + SIDE_NEW_MS;
  if (fresh) side.freshUntil = Math.min(side.freshUntil, until);
  if (open && paneWith(note.id).margin.reading) return '<i class="side-reading" aria-hidden="true" title="Felix is reading this note"></i>';
  if (!open && side.unopened.has(note.id)) return SIDE_UNOPENED;
  return fresh ? '<i class="side-fresh" aria-hidden="true" title="Edited in the last 30 minutes"></i>' : '';
}

function sideRedrawSoon() {
  clearTimeout(side.quietTimer);
  if (popup) side.quietTimer = setTimeout(sideRedrawSoon, 500);
  else drawSide();
}

function sideFreshCheck() {
  const left = side.freshUntil - Date.now();
  clearTimeout(side.freshTimer);
  if (left <= 0) sideRedrawSoon();
  else if (left < Infinity) side.freshTimer = setTimeout(sideFreshCheck, left + SIDE_FRESH_GROUP);
}

function sideNoteWhen(note) {
  const when = side.sort === 'created' ? note.created_at : side.sort === 'updated' ? note.updated_at : '';
  return when ? `<span class="side-count">${esc(rel(when))}</span>` : '';
}

function sideOrder(change) {
  Object.assign(side, change);
  try { localStorage.setItem(SIDE_ORDER_KEY, JSON.stringify({ sort: side.sort, flip: side.flip, tasks: side.tasks })); } catch (e) {}
  for (const path of [...side.notes.keys()]) loadSideNotes(path, 0, true);
  drawSide();
}

function sideSortMenu(el) {
  popupMenu(el, {
    label: 'Sort notes',
    rows: [
      { note: 'Sort by' },
      ...Object.entries(SIDE_SORTS).map(([sort, { text }]) => ({ text, checked: side.sort === sort, run: () => sideOrder({ sort, flip: false }) })),
      { line: true },
      { note: 'Order' },
      ...SIDE_SORTS[side.sort].ways.map((text, i) => ({ text, checked: side.flip === Boolean(i), run: () => sideOrder({ flip: Boolean(i) }) })),
      { line: true },
      { text: 'Only notes with open tasks', checked: side.tasks, toggle: true, run: () => sideOrder({ tasks: !side.tasks }) },
    ],
  });
}

function drawFoldersSheet() {
  const box = $('#folders-sheet');
  if (!box) return;
  const tree = box.querySelector('.side-tree');
  const scroll = tree?.scrollTop || 0;
  const dir = creationDir();
  refreshPanel(box, `<div class="folders-sheet-head"><h3>Folders</h3><button class="icon-btn" data-action="close" aria-label="Close folders">${ICON.close}</button></div>
    <div class="folders-sheet-tools"><button class="btn sm plain" data-action="new" data-dir="${esc(dir)}">${ICON.filePlus}New note</button><button class="btn sm plain" data-action="new-folder" data-parent="${esc(dir)}">${ICON.folderPlus}New folder</button></div>
    ${sideLeave()}<nav class="side-nav side-tree" aria-label="Folders" data-drop-dir="" tabindex="0">${folderTree(sideHere()) || '<p class="side-none">Folders you make show up here.</p>'}</nav>`);
  box.querySelector('.side-tree').scrollTop = scroll;
  sideRoving(box.querySelector('.side-tree'));
  revealNoteRow(box.querySelector('.side-tree'));
}

function showFolders() {
  const scrim = sheet('<section id="folders-sheet" aria-label="Folders"></section>');
  scrim.foldersReturn = false;
  scrim.returnFocus = $('#menu');
  scrim.querySelector('.sheet').setAttribute('aria-label', 'Folders');
  drawSide();
  loadSideFolders().catch(fail);
  const box = $('#folders-sheet');
  box.querySelector('.side-tree').scrollTop = side.sheetScroll || 0;
  (box.querySelector('.side-tree [aria-current="page"]') || box.querySelector('[data-action="new"]')).focus({ preventScroll: true });
}

addEventListener('resize', () => {
  if (sideWide() && $('#folders-sheet')) {
    closeSheet(true);
    $('#side [data-action="side-fold"]')?.focus({ preventScroll: true });
  }
});

let explorerOpenSeq = 0;
async function openExplorerNote(el) {
  const request = ++explorerOpenSeq;
  if (state.session?.note.id === el.dataset.id && side.target) { side.target = null; drawSide(); }
  if (split() && !el.closest('#folders-sheet')) {
    sideDrawer(false);
    return besideOpen(el.dataset.id, here());
  }
  if (!el.closest('#folders-sheet') || state.session?.note.id === el.dataset.id) return go(noteHash(el.dataset.id));
  const box = el.closest('.scrim');
  const mine = seq;
  const current = () => request === explorerOpenSeq && mine === seq && box.isConnected && !sideWide();
  try {
    const note = await api(`/api/notes/${enc(el.dataset.id)}`);
    if (!current()) return;
    await go(noteHash(note.id), { note, isCurrent: current });
  } catch (error) { if (current()) fail(error); }
}

function explorerTarget(el) {
  const note = el.closest('.side-note-wrap')?.querySelector('.side-note');
  if (note) return { kind: 'note', id: note.dataset.id, dir: note.dataset.from || '' };
  const folder = el.closest('.side-folder');
  if (folder) return { kind: 'folder', dir: folder.dataset.dropDir };
  if (el.closest('[data-action="side-unfocus"]')) return { kind: 'folder', dir: side.root };
  if (el.closest('.side-group')) return { kind: 'root', dir: creationDir() };
  return { kind: 'root', dir: el.closest('.side-note-status, .side-kids')?.dataset.dropDir || side.root };
}

function explorerMenu(el, event = null) {
  const target = Object.freeze(explorerTarget(el));
  const owner = el.closest('button') || el.closest('.side-tree, .side-group');
  if (!owner) return;
  const rows = target.kind === 'note'
    ? [['open', 'Open', ICON.note], ['beside', 'Open beside', ICON.sidebar], ...(split() ? [['unsplit', 'Close split view', ICON.close]] : []), ['rename', 'Rename', ICON.pen], ['move', 'Move to…', ICON.move], ['combine', 'Combine into…', ICON.note], null, ['trash', 'Move to trash', ICON.trash, true]]
    : [['new', target.dir ? 'New note here' : 'New note', ICON.filePlus, false, target.dir === creationDir() && !$('.scrim') ? `<kbd aria-hidden="true">${esc(shortcutText(shortcutKeys('new')))}</kbd>` : ''], ['folder', target.dir ? 'New folder here' : 'New folder', ICON.folderPlus], ...(target.kind === 'folder' && target.dir ? [...(target.dir === side.root ? [] : [['focus', 'Focus on this folder', ICON.search]]), null, ['rename', 'Rename', ICON.pen], ['move', 'Move to…', ICON.move], null, ['trash', 'Move to trash', ICON.trash, true]] : [])];
  popupMenu(owner, {
    label: target.kind === 'note' ? 'Note options' : 'Folder options',
    point: event && { x: event.clientX, y: event.clientY },
    rows: rows.map((row) => row ? { text: row[1], icon: row[2], danger: row[3], after: row[4], run: () => explorerAction(row[0], target) } : { line: true }),
  });
}

addEventListener('focus', sideFreshCheck);
document.addEventListener('visibilitychange', sideFreshCheck);
document.addEventListener('contextmenu', (event) => {
  const el = event.target.closest('.side-tree, .side-group, [data-action="side-unfocus"]');
  if (!el) return;
  event.preventDefault();
  explorerMenu(event.target, event);
});
document.addEventListener('keydown', (event) => {
  if (!(event.key === 'ContextMenu' || event.shiftKey && event.key === 'F10') || !event.target.closest('.side-tree, [data-action="side-unfocus"]')) return;
  event.preventDefault();
  explorerMenu(event.target);
});
function explorerFocus(at) {
  const host = $('#folders-sheet') || $('#side');
  if (!host || !at) return null;
  const rows = (selector) => [...host.querySelectorAll(selector)];
  const note = at.kind === 'note' && rows('.side-note[data-id]').find((row) => row.dataset.id === at.id);
  if (note) return note;
  const folders = rows('.side-row[data-action="open-folder"], [data-action="side-unfocus"]');
  for (let dir = at.dir || ''; dir; dir = parentOf(dir)) {
    const row = folders.find((el) => el.dataset.dir === dir);
    if (row) return row;
  }
  return host.querySelector('.side-new, .folders-sheet-tools [data-action="new"]');
}

function refocusExplorer(at) {
  const active = document.activeElement;
  if (active && active !== document.body && !active.closest('#side, #folders-sheet')) return;
  explorerFocus(at)?.focus({ preventScroll: true });
}

async function explorerAction(action, target) {
  const el = { dataset: { dir: target.dir, parent: target.dir } };
  if (action === 'new') return actions.new(el);
  if (action === 'folder') return newFolder(el);
  if (action === 'beside') return besideOpen(target.id);
  if (action === 'unsplit') return besideShut();
  if (action === 'open') {
    const row = [...document.querySelectorAll('#folders-sheet .side-note, #side .side-note')].find((row) => row.dataset.id === target.id);
    return row ? openExplorerNote(row) : go(noteHash(target.id));
  }
  if (target.kind === 'folder') {
    if (!target.dir) return;
    if (action === 'focus') return sideFocus(target.dir);
    if (action === 'rename') return renameFolder(target.dir);
    if (action === 'trash') return trashFolder(target.dir);
  }
  const mine = seq;
  if (target.kind === 'note' && paneWith(target.id)) await ready(paneWith(target.id).s);
  const note = target.kind === 'note' ? await api(`/api/notes/${enc(target.id)}`) : null;
  if (mine !== seq) return;
  if (action === 'rename') return renameExplorerNote(note);
  if (action === 'trash') return trashNote(note, target);
  if (action === 'move') return explorerMoveMenu(target, note, explorerFocus(target) || document.activeElement);
  if (action === 'combine') return explorerCombineMenu(note, explorerFocus(target) || inPane('#title') || document.activeElement);
}

function inlineEdit(where, { id, value, label, hint = '', at, save, end = null }) {
  const input = document.createElement('input');
  input.className = end ? 'inline-edit making' : 'inline-edit';
  input.id = id;
  input.value = value;
  input.maxLength = 120;
  input.placeholder = hint;
  input.autocomplete = 'off';
  input.setAttribute('aria-label', label);
  for (const edge of ['left', 'top', 'width', 'height']) input.style[edge] = `${where[edge]}px`;
  document.body.appendChild(input);
  input.focus();
  input.select();
  let busy = false;
  const finish = (back = at, made = false) => {
    busy = true;
    input.remove();
    end?.(!made);
    explorerFocus(back)?.focus({ preventScroll: true });
  };
  const commit = async () => {
    const text = input.value.trim();
    if (busy || !input.isConnected) return;
    if (!text || text === value) return finish();
    busy = true;
    input.readOnly = true;
    const done = await save(text);
    busy = false;
    input.readOnly = false;
    if (done) finish(done === true ? at : done, true);
    else input.focus();
  };
  input.addEventListener('keydown', (e) => {
    if (e.isComposing) return;
    if (e.key === 'Enter') { e.preventDefault(); commit().catch(fail); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(); }
  });
  input.addEventListener('blur', () => { commit().catch(fail); });
}

function sideMakingOpen(tree, parent, save) {
  const making = { parent };
  side.making = making;
  drawSide();
  drawFoldersSheet();
  const row = tree.querySelector('.side-making');
  if (!row) { side.making = null; return false; }
  for (const run of row.getAnimations()) run.cancel();
  const box = row.getBoundingClientRect();
  const left = row.querySelector('svg').getBoundingClientRect().right + 7;
  if (!stillMotion()) row.animate([{ height: '0px', opacity: 0 }, { height: `${box.height}px`, opacity: 1 }], { duration: MOTION_MS, easing: EASE_OUT });
  inlineEdit({ left, top: box.top + 3, width: Math.max(120, box.right - left - 8), height: box.height - 6 }, {
    id: 'folder-name',
    value: '',
    label: 'Folder name',
    hint: parent ? `New folder in ${folderLabel(parent)}` : 'New folder',
    at: { kind: 'folder', dir: parent },
    save,
    end: (cancelled) => {
      if (side.making !== making) return;
      const shown = tree.querySelector('.side-making');
      const gone = () => { if (side.making !== making) return; side.making = null; drawSide(); drawFoldersSheet(); };
      if (!cancelled || !shown || stillMotion()) return gone();
      shown.animate([{ height: `${shown.offsetHeight}px`, opacity: 1 }, { height: '0px', opacity: 0 }], { duration: LEAVE_MS, easing: EASE_EXIT, fill: 'forwards' }).finished.then(gone, gone);
    },
  });
  const input = $('#folder-name');
  if (!stillMotion()) input?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: MOTION_MS, easing: EASE_OUT });
  return true;
}

function renameExplorerNote(note) {
  const at = { kind: 'note', id: note.id, dir: note.directory };
  const row = explorerFocus(at);
  if (!row) return;
  inlineEdit(row.querySelector('.side-label').getBoundingClientRect(), {
    id: 'explorer-note-name',
    value: note.title,
    label: 'Note name',
    at,
    save: async (title) => {
      const renamed = await api(`/api/notes/${enc(note.id)}`, { method: 'PATCH', body: { title, base: note.version } }).catch((error) => { fail(error); });
      if (!renamed) return false;
      await afterExplorerNote(note.id);
      toast(`Renamed note to “${title}”`, {
        action: 'Undo',
        run: () => api(`/api/notes/${enc(note.id)}`, { method: 'PATCH', body: { title: note.title, base: renamed.version } }).then(() => afterExplorerNote(note.id)).catch(fail),
      });
      return true;
    },
  });
}

async function explorerMoveMenu(target, note, owner) {
  const mine = seq;
  const folders = await api('/api/folders');
  if (mine !== seq) return;
  const from = target.kind === 'note' ? note.directory : parentOf(target.dir);
  const choices = ['', ...folders.map((f) => f.name)].filter((dir) => dir !== from && (target.kind === 'note' || dir !== target.dir && !dir.startsWith(`${target.dir}/`)));
  popupMenu(owner, {
    label: `Move “${note?.title || folderLabel(target.dir)}”`,
    find: 'Find a folder',
    rows: choices.map((dir) => ({
      text: dir || 'Top level',
      icon: ICON.folder,
      run: async () => {
        if (target.kind === 'folder') await dropFolder(target.dir, dir);
        else await dropNote({ id: note.id, title: note.title, dir: note.directory }, dir);
        refocusExplorer(target.kind === 'folder' ? { kind: 'folder', dir: dir ? `${dir}/${folderLabel(target.dir)}` : folderLabel(target.dir) } : { kind: 'folder', dir });
      },
    })),
  });
}

const COMBINE_PAGE = 50;

async function explorerCombineMenu(note, owner) {
  const mine = seq;
  const seen = new Map();
  const rowsOf = (loaded) => {
    const titles = new Map();
    for (const n of loaded) titles.set(n.title, (titles.get(n.title) || 0) + 1);
    return loaded.filter((n) => n.id !== note.id).map((n) => ({
      text: `${n.title || 'Untitled'}${titles.get(n.title) > 1 ? ` · ${n.directory || 'Top level'}` : ''}`,
      icon: ICON.note,
      run: () => combineSheet({ id: note.id, title: note.title || 'Untitled' }, { id: n.id, title: n.title || 'Untitled' }),
    }));
  };
  const page = async (query, offset) => {
    const got = await api(query
      ? `/api/search?q=${enc(query)}&brief=true&limit=${COMBINE_PAGE + 1}&offset=${offset}`
      : `/api/notes?${new URLSearchParams({ limit: String(COMBINE_PAGE + 1), offset: String(offset), rows: 'true', sort: 'title' })}`);
    const loaded = [...(offset ? seen.get(query) : []), ...got.slice(0, COMBINE_PAGE)];
    seen.set(query, loaded);
    return { rows: rowsOf(loaded), more: got.length > COMBINE_PAGE, loaded: loaded.length };
  };
  const first = await page('', 0);
  if (mine !== seq) return;
  if (!first.rows.length) return toast('There is no other note to combine it with.');
  popupMenu(owner, { label: `Combine “${note.title || 'Untitled'}” into`, find: 'Find a note', rows: first.rows, more: first.more, loaded: first.loaded, page });
}

async function afterExplorerNote(id) {
  const open = paneWith(id);
  if (open && !saving.get(open.s.key)) await showNote(id, { pane: open });
  await loadSideFolders();
}

function applySideFilter(tree) {
  if (!tree) return;
  for (const el of tree.querySelectorAll('[data-filtered]')) {
    el.hidden = false;
    el.removeAttribute('data-filtered');
  }
  const text = (side.filter || '').trim().toLowerCase();
  if (!text) return;
  const hide = (el) => {
    el.hidden = true;
    el.dataset.filtered = '';
  };
  for (const wrap of tree.querySelectorAll('.side-note-wrap')) {
    if (!(wrap.querySelector('.side-note')?.dataset.title || '').toLowerCase().includes(text)) hide(wrap);
  }
  const folders = [...tree.querySelectorAll('.side-folder')].reverse();
  for (const folder of folders) {
    const kids = folder.nextElementSibling?.matches('.side-kids') ? folder.nextElementSibling : null;
    const name = (folder.querySelector('.side-label')?.textContent || '').toLowerCase();
    const keep = name.includes(text) || kids?.querySelector('.side-note-wrap:not([hidden]), .side-folder:not([hidden])');
    if (!keep) {
      hide(folder);
      if (kids) hide(kids);
    }
  }
}

const rowKey = (el) => el.dataset.id || el.dataset.dir || '';

function sideRoving(tree) {
  if (!tree) return;
  const rows = [...tree.querySelectorAll('.side-row')];
  for (const row of rows) row.tabIndex = -1;
  const visible = rows.filter((row) => !row.closest('[hidden]'));
  const stop = visible.find((row) => row.getAttribute('aria-current') === 'page') || visible.find((row) => side.focusKey && rowKey(row) === side.focusKey) || visible[0];
  if (stop) stop.tabIndex = 0;
}

document.addEventListener('input', (event) => {
  if (!event.target.matches('#side-filter')) return;
  side.filter = event.target.value;
  applySideFilter(event.target.closest('.side-explorer')?.querySelector('.side-tree'));
  sideRoving(event.target.closest('.side-explorer')?.querySelector('.side-tree'));
});

document.addEventListener('focusin', (event) => {
  if (event.target.matches?.('.side-tree .side-row')) side.focusKey = rowKey(event.target);
});

document.addEventListener('keydown', (event) => {
  const row = event.target.matches?.('.side-tree .side-row') ? event.target : null;
  if (!row || event.defaultPrevented) return;
  const tree = row.closest('.side-tree');
  const rows = [...tree.querySelectorAll('.side-row')].filter((el) => el.offsetParent !== null);
  const folder = row.dataset.action === 'open-folder';
  if (folder && (event.key === 'ArrowRight' || event.key === 'ArrowLeft')) {
    const open = row.getAttribute('aria-expanded') === 'true';
    const twist = row.closest('.side-folder').querySelector('.side-twist');
    event.preventDefault();
    const toggle = () => {
      const dir = row.dataset.dir;
      const refocus = () => {
        if (document.activeElement?.matches?.('.side-tree .side-row') && document.activeElement !== row) return;
        const again = [...document.querySelectorAll('.side-tree .side-row[data-action="open-folder"]')].find((el) => el.dataset.dir === dir && el.offsetParent !== null);
        if (again && document.activeElement !== again) again.focus();
      };
      twist.click();
      refocus();
      requestAnimationFrame(refocus);
    };
    if (event.key === 'ArrowRight') {
      if (!open) toggle();
      else row.closest('.side-folder').nextElementSibling?.querySelector('.side-row')?.focus();
    } else if (open) toggle();
    else {
      const kids = row.closest('.side-kids');
      rows.find((el) => el.dataset.action === 'open-folder' && el.dataset.dir === kids?.dataset.kids)?.focus();
    }
    return;
  }
  const to = listKeyTarget(rows, row, event, (el) => el.querySelector('.side-label')?.textContent || '');
  if (to) {
    event.preventDefault();
    to.focus();
  }
});
