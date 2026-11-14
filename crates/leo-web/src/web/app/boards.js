// Boards (routes/boards.rs): Jira-style task boards. Cards have no dates, so they are
// kept apart from calendar tasks. A board has columns (To do, In progress, Done…) that
// people rename, add, delete and drag into order; cards are dragged between columns,
// and onto another board's tab to move them there.
const KANBAN_KEY = 'leo-board';
const BOARD_KINDS = ['add_card', 'update_card', 'move_card', 'delete_card', 'restore_card', 'add_board', 'change_board', 'delete_board', 'restore_board'];
const KANBAN_MOST_COLUMNS = 12;
const KANBAN_HOLD_MS = 300;
const KANBAN_EDGE = 56;
const KANBAN_OTHERS = '.kanban-card:not(.kanban-placeholder)';
const KANBAN_SAVE_MS = 600;
const KANBAN_MOST_DESCRIPTION = 8000;
const KANBAN_PRIORITIES = [['', 'None'], ['low', 'Low'], ['medium', 'Medium'], ['high', 'High']];
const KANBAN_ICON = {
  pencil: svg('<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/>'),
  high: svg('<path d="M6 13l6-6 6 6M6 19l6-6 6 6"/>'),
  medium: svg('<path d="M6 9.5h12M6 14.5h12"/>'),
  low: svg('<path d="M6 9l6 6 6-6"/>'),
};
const KANBAN_TONES = [
  [/\b(done|complete|completed|finished|shipped|closed)\b/i, 'done'],
  [/\b(in progress|doing|working|review|reviewing|testing|started)\b/i, 'doing'],
  [/\b(blocked|stuck|waiting|on hold)\b/i, 'blocked'],
  [/\b(to do|todo|tasks?|backlog|ideas?|next|planned|new)\b/i, 'todo'],
];
const KANBAN_NEAR = ['todo hue0', 'done hue1', 'blocked hue2', 'hue1 hue3'];
const kanban = { boards: [], board: null, cards: [], loaded: false, drag: null, suppressClick: false, redrawing: false, panel: null };

const kanbanClash = (a, b) => a === b || KANBAN_NEAR.includes(`${a} ${b}`) || KANBAN_NEAR.includes(`${b} ${a}`);

// A column's colour follows what its name means, so To do, In progress and Done read at a
// glance; any other column, or one that would look like its neighbour, takes a hue by its
// place on the board, so sections can always be told apart.
function kanbanTone(column) {
  const columns = kanban.board.columns;
  const meant = (c) => c && KANBAN_TONES.find(([test]) => test.test(c.name))?.[1];
  let tone = '';
  for (const [at, c] of columns.entries()) {
    const left = tone;
    const right = meant(columns[at + 1]);
    const named = meant(c);
    tone = named && !kanbanClash(named, left) ? named : [0, 1, 2, 3].map((i) => `hue${(at + i) % 4}`).find((hue) => !kanbanClash(hue, left) && !kanbanClash(hue, right));
    if (c.id === column.id) break;
  }
  return tone;
}

function kanbanRemembered() {
  try { return localStorage.getItem(KANBAN_KEY) || ''; } catch (e) { return ''; }
}

function kanbanRemember(id) {
  try { localStorage.setItem(KANBAN_KEY, id); } catch (e) {}
}

async function showBoards(arg) {
  const mine = ++seq;
  state = { view: 'boards', dir: '' };
  chrome({ showBack: true });
  document.title = 'Boards · leo';
  if (!kanban.loaded || !app.querySelector('.kanban-page')) app.innerHTML = `<div class="kanban-page">${skeleton(3)}</div>`;
  const { boards } = await api('/api/boards');
  if (mine !== seq) return;
  kanban.boards = boards || [];
  kanban.loaded = true;
  if (!kanban.boards.length) {
    kanban.board = null;
    kanban.cards = [];
    drawKanban();
    return;
  }
  const wanted = arg || kanbanRemembered();
  const id = kanban.boards.some((b) => b.id === wanted) ? wanted : kanban.boards[0].id;
  if (arg && arg !== id) history.replaceState(history.state, '', `#/boards/${enc(id)}`);
  const data = await api(`/api/boards/${enc(id)}`);
  if (mine !== seq) return;
  kanbanRemember(id);
  document.title = `${data.board.name} · leo`;
  const opening = kanban.board?.id !== id || !app.querySelector('.kanban-cols');
  if (opening) overlapOut(app.querySelector('.kanban-cols'));
  kanban.board = data.board;
  kanban.cards = data.cards || [];
  drawKanban();
  if (opening) {
    cascade(app.querySelector('.kanban-cols'), 'settling');
    cascade(app.querySelector('.kanban-tabs'), 'tab-wipe');
    for (const list of app.querySelectorAll('.kanban-cards')) cascade(list);
  }
}

let kanbanWrites = 0;

function kanbanWrite(path, options) {
  kanbanWrites++;
  return api(path, options);
}

// Fetch the open board again after a change, so ranks renumbered by leo are known here.
// A write that starts while it is in flight makes the answer old, so it is dropped.
async function kanbanRefresh() {
  const board = kanban.board;
  if (!board || state.view !== 'boards') return;
  const mine = seq;
  const writes = kanbanWrites;
  const data = await api(`/api/boards/${enc(board.id)}`);
  if (mine !== seq || kanban.board?.id !== board.id || kanban.drag || kanbanWrites !== writes) return;
  kanban.board = data.board;
  kanban.cards = data.cards || [];
  drawKanban();
}

const kanbanCardsIn = (column) => kanban.cards.filter((c) => c.column === column);
const kanbanCard = (id) => kanban.cards.find((c) => c.id === id);
const kanbanColumn = (id) => kanban.board?.columns.find((c) => c.id === id);

// A sheet holds the card it opened; the board may have been fetched again since.
function kanbanUpdate(card, saved) {
  Object.assign(card, saved);
  const live = kanbanCard(card.id);
  if (live && live !== card) Object.assign(live, saved);
  kanbanRedrawCard(card);
}

function kanbanPriority(priority) {
  if (!priority) return '';
  const label = KANBAN_PRIORITIES.find(([id]) => id === priority)?.[1] || priority;
  return `<span class="kanban-priority ${esc(priority)}" title="${esc(label)} priority" aria-label="${esc(label)} priority">${KANBAN_ICON[priority] || ''}</span>`;
}

function kanbanCardHtml(card) {
  const preview = card.description ? `<div class="kanban-has-text">${esc(card.description.slice(0, 240))}</div>` : '';
  const open = kanban.panel?.id === card.id;
  return `<article class="kanban-card${open ? ' open' : ''}" tabindex="0" data-card="${esc(card.id)}" data-action="kanban-open-card" aria-roledescription="card" aria-label="${esc(card.title)}"${open ? ' aria-current="true"' : ''}>${kanbanPriority(card.priority)}<div class="kanban-card-title">${esc(card.title)}</div>${card.subheading ? `<div class="kanban-card-sub">${esc(card.subheading)}</div>` : ''}${preview}</article>`;
}

function kanbanColumnHtml(column) {
  const cards = kanbanCardsIn(column.id);
  return `<section class="kanban-col" data-col="${esc(column.id)}" data-tone="${kanbanTone(column)}" aria-label="${esc(column.name)}">
    <header class="kanban-col-head" title="Drag to move this column">
      <input class="kanban-col-name" data-kanban-name="${esc(column.id)}" value="${esc(column.name)}" size="${column.name.length + 1}" maxlength="80" aria-label="Column name" spellcheck="false" autocomplete="off">
      <span class="badge kanban-count" data-count-col="${esc(column.id)}">${cards.length}</span>
      <button class="icon-btn kanban-col-more" data-action="kanban-column-menu" data-col="${esc(column.id)}" aria-haspopup="menu" aria-expanded="false" aria-label="Options for ${esc(column.name)}" title="Column options">${ICON.more}</button>
    </header>
    <div class="kanban-cards" data-col="${esc(column.id)}">${cards.map(kanbanCardHtml).join('')}</div>
    <button class="kanban-add-card" data-action="kanban-add-card" data-col="${esc(column.id)}">${ICON.plus}<span>Add card</span></button>
  </section>`;
}

function kanbanTabsHtml() {
  const here = kanban.board;
  return kanban.boards.map((b) => {
    const on = b.id === here.id;
    const count = `<span class="badge kanban-count" data-count-board="${esc(b.id)}">${on ? kanban.cards.length : b.cards || 0}</span>`;
    if (on) return `<div class="kanban-tab on" data-kanban-drop="${esc(b.id)}" aria-current="page"><input class="kanban-title" data-kanban-name="board" value="${esc(here.name)}" size="${here.name.length + 1}" maxlength="80" aria-label="Board name" spellcheck="false" autocomplete="off">${count}</div>`;
    return `<a class="kanban-tab" href="#/boards/${enc(b.id)}" data-kanban-drop="${esc(b.id)}"><span class="kanban-tab-name">${esc(b.name)}</span>${count}</a>`;
  }).join('');
}

function kanbanHeadHtml() {
  return `<header class="kanban-head">
    <nav class="kanban-tabs" aria-label="Boards">${kanbanTabsHtml()}</nav>
    <label class="select-control kanban-more" hidden><select id="kanban-switch" aria-label="More boards"></select>${ICON.chevron}</label>
    <button class="icon-btn kanban-tab-new" data-action="kanban-new-board" aria-label="New board" title="New board">${ICON.plus}</button>
    ${kanban.board.columns.length < KANBAN_MOST_COLUMNS ? `<button class="btn plain sm kanban-add-col" data-action="kanban-add-column" title="Add column">${ICON.plus}<span>Add column</span></button>` : ''}
    <button class="icon-btn" data-action="kanban-board-menu" aria-haspopup="menu" aria-expanded="false" aria-label="Board options" title="Board options">${ICON.more}</button>
  </header>`;
}

function kanbanFade(el) {
  el.classList.toggle('kanban-fade-left', el.scrollLeft > 1);
  el.classList.toggle('kanban-fade-right', el.scrollLeft + el.clientWidth < el.scrollWidth - 1);
}

function kanbanFit() {
  const row = app.querySelector('.kanban-cols');
  const nav = app.querySelector('.kanban-tabs');
  const more = app.querySelector('.kanban-more');
  const on = nav?.querySelector('.on');
  if (!row || !more || !on) return;
  const tabs = [...nav.children];
  for (const tab of tabs) tab.classList.remove('kanban-over');
  more.hidden = false;
  nav.scrollLeft = 0;
  more.hidden = nav.scrollWidth <= nav.clientWidth + more.offsetWidth + 6;
  if (!more.hidden) {
    let room = nav.clientWidth - on.offsetWidth;
    const over = tabs.filter((tab) => {
      if (tab === on) return false;
      room -= tab.offsetWidth + 4;
      return room < 0;
    });
    for (const tab of over) tab.classList.add('kanban-over');
    const select = more.querySelector('select');
    select.innerHTML = `<option value="" selected disabled hidden>${over.length} more</option>${over.map((tab) => {
      const id = tab.dataset.kanbanDrop;
      return `<option value="${esc(id)}">${esc(kanban.boards.find((b) => b.id === id).name)}</option>`;
    }).join('')}`;
    select.value = '';
  }
  kanbanFade(nav);
  kanbanFade(row);
  for (const list of row.querySelectorAll('.kanban-cards')) list.parentElement.classList.toggle('kanban-full', list.scrollHeight > list.clientHeight);
}

const kanbanSized = typeof ResizeObserver === 'function' ? new ResizeObserver(() => kanbanFit()) : null;

document.addEventListener('scroll', (e) => {
  if (e.target.matches?.('.kanban-cols, .kanban-tabs')) kanbanFade(e.target);
}, true);

function drawKanban() {
  if (state.view !== 'boards') return;
  const board = kanban.board;
  if (!board) {
    kanbanClosePanel(false);
    app.innerHTML = `<div class="kanban-page kanban-none">${empty(ICON.board, 'No boards yet', 'Columns like To do, In progress and Done. Drag a card along as the work moves.')}<button class="btn primary" data-action="kanban-new-board">${ICON.plus}<span>New board</span></button></div>`;
    return;
  }
  const row = app.querySelector('.kanban-cols');
  const scrolled = row?.dataset.board === board.id ? row.scrollLeft : 0;
  const lists = new Map([...app.querySelectorAll('.kanban-cards')].map((list) => [list.dataset.col, list.scrollTop]));
  const active = app.contains(document.activeElement) ? document.activeElement : null;
  const focus = active?.dataset.card ? `[data-card="${CSS.escape(active.dataset.card)}"]` : active?.dataset.kanbanName ? `[data-kanban-name="${CSS.escape(active.dataset.kanbanName)}"]` : null;
  // A redraw keeps what is being typed: a board or column name, and an open Add card box.
  const typing = active?.dataset.kanbanName ? { value: active.value, start: active.selectionStart, end: active.selectionEnd } : null;
  const composers = [...app.querySelectorAll('.kanban-composer')].map((form) => {
    const text = form.querySelector('textarea');
    return { column: form.dataset.composer, text: text.value, start: text.selectionStart, end: text.selectionEnd, focus: text === active, priority: form.querySelector('.segment.on').dataset.priority };
  });
  let host = app.querySelector('.kanban-board');
  if (!host) {
    app.innerHTML = '<div class="kanban-layout"><div class="kanban-board"></div><aside class="kanban-panel" id="kanban-panel" aria-label="Card details" hidden></aside></div>';
    host = app.querySelector('.kanban-board');
    kanbanSized?.disconnect();
    kanbanSized?.observe(host);
    kanban.panel?.end();
    kanban.panel = null;
    document.body.classList.remove('kanban-panel-on');
  }
  kanban.redrawing = true;
  try {
    host.innerHTML = `<div class="kanban-page">${kanbanHeadHtml()}<div class="kanban-cols" data-board="${esc(board.id)}">${board.columns.map(kanbanColumnHtml).join('')}</div></div>`;
  } finally {
    kanban.redrawing = false;
  }
  kanbanRebind();
  const next = app.querySelector('.kanban-cols');
  if (next) next.scrollLeft = scrolled;
  for (const list of app.querySelectorAll('.kanban-cards')) list.scrollTop = lists.get(list.dataset.col) || 0;
  if (kanban.panel && !kanbanCard(kanban.panel.id)) kanbanClosePanel(false);
  else if (kanban.panel) kanbanPanelStatus();
  for (const open of composers) {
    if (board.columns.some((c) => c.id === open.column)) kanbanComposer(open.column, open);
  }
  settleNow('kanban-card', (id) => app.querySelector(`.kanban-card[data-card="${CSS.escape(id)}"]`));
  settleNow('kanban-col', (id) => app.querySelector(`.kanban-col[data-col="${CSS.escape(id)}"]`));
  const again = focus && app.querySelector(focus);
  if (again) {
    if (typing) {
      again.value = typing.value;
      if (typing.start !== null) again.setSelectionRange(typing.start, typing.end);
    }
    again.focus({ preventScroll: true });
  }
  kanbanFit();
}

function kanbanCounts() {
  for (const el of app.querySelectorAll('[data-count-col]')) el.textContent = String(kanbanCardsIn(el.dataset.countCol).length);
  const here = app.querySelector(`[data-count-board="${CSS.escape(kanban.board?.id || '')}"]`);
  if (here) here.textContent = String(kanban.cards.length);
}

function kanbanRedrawCard(card) {
  const el = app.querySelector(`.kanban-card[data-card="${CSS.escape(card.id)}"]`);
  if (!el) return;
  const focused = document.activeElement === el;
  el.outerHTML = kanbanCardHtml(card);
  if (focused) app.querySelector(`.kanban-card[data-card="${CSS.escape(card.id)}"]`)?.focus({ preventScroll: true });
  kanbanRebind();
}

function kanbanRebind() {
  const d = kanban.drag;
  if (!d || d.item.isConnected) return;
  const again = app.querySelector(d.kind === 'card' ? `.kanban-card[data-card="${CSS.escape(d.id)}"]` : `.kanban-col[data-col="${CSS.escape(d.id)}"]`);
  if (!again) return kanbanCancel();
  d.item = again;
  if (d.live && !d.from.parent.isConnected) d.from = { parent: again.parentElement, next: again.nextElementSibling };
  if (d.live) again.classList.add('kanban-placeholder');
}

function kanbanSyncTab() {
  const tab = kanban.boards.find((b) => b.id === kanban.board?.id);
  if (tab) { tab.name = kanban.board.name; tab.columns = kanban.board.columns; tab.cards = kanban.cards.length; }
  const nav = app.querySelector('.kanban-tabs');
  if (nav && !nav.contains(document.activeElement)) {
    nav.innerHTML = kanbanTabsHtml();
    kanbanFit();
  }
}

async function kanbanNewBoard() {
  const box = sheet(`<div class="kanban-start">
    <h3>New board</h3>
    <label class="field">${ICON.board}<input id="kanban-board-name" maxlength="80" placeholder="Name, e.g. Coursework" aria-label="Board name" autocomplete="off" aria-describedby="kanban-start-help"></label>
    <div class="kanban-start-preview" aria-label="Starting columns">
      ${[['todo', 'To do'], ['doing', 'In progress'], ['done', 'Done']].map(([tone, name]) => `<div class="kanban-start-column" data-tone="${tone}"><span><i class="kanban-dot"></i>${name}</span><div class="kanban-start-card" aria-hidden="true"><i></i><i></i></div></div>`).join('')}
    </div>
    <p class="kanban-start-help" id="kanban-start-help">Your starting columns. Rename, add or reorder them any time.</p>
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn primary" id="kanban-board-make">Create board</button></div>
  </div>`);
  const name = $('#kanban-board-name', box);
  const make = $('#kanban-board-make', box);
  const save = async () => {
    if (make.disabled) return;
    make.disabled = true;
    try {
      const board = await kanbanWrite('/api/boards', { method: 'POST', body: { name: name.value.trim() } });
      closeSheet();
      kanbanRemember(board.id);
      go(`#/boards/${enc(board.id)}`);
    } catch (e) {
      make.disabled = false;
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
    }
  };
  make.addEventListener('click', () => save().catch(fail));
  name.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); save().catch(fail); }
  });
  name.focus();
}

async function kanbanSaveBoard(change) {
  const board = kanban.board;
  const saved = await kanbanWrite(`/api/boards/${enc(board.id)}`, { method: 'PATCH', body: change });
  if (kanban.board?.id !== saved.id) return saved;
  kanban.board = saved;
  kanbanSyncTab();
  return saved;
}

async function kanbanRename(input) {
  const key = input.dataset.kanbanName;
  const board = kanban.board;
  if (!board) return;
  const text = input.value.replace(/\s+/g, ' ').trim();
  const column = key === 'board' ? null : kanbanColumn(key);
  const was = column ? column.name : board.name;
  if (!text || text === was) { input.value = was; return; }
  try {
    if (column) {
      await kanbanSaveBoard({ columns: board.columns.map((c) => ({ id: c.id, name: c.id === key ? text : c.name })) });
      input.closest('.kanban-col')?.setAttribute('aria-label', text);
      for (const box of app.querySelectorAll('.kanban-col')) box.dataset.tone = kanbanTone(kanbanColumn(box.dataset.col));
      kanbanPanelStatus();
    } else {
      const saved = await kanbanSaveBoard({ name: text });
      document.title = `${saved.name} · leo`;
    }
  } catch (e) {
    input.value = was;
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
}

document.addEventListener('input', (e) => {
  if (e.target.matches?.('.kanban-title, .kanban-col-name')) e.target.size = e.target.value.length + 1;
});

document.addEventListener('change', (e) => {
  if (e.target.id === 'kanban-switch' && e.target.value) go(`#/boards/${enc(e.target.value)}`);
  const input = e.target.closest?.('[data-kanban-name]');
  // A redraw removing a field being typed in fires its change first; the typing lives on
  // in the new field, so that change saves nothing.
  if (input && !kanban.redrawing && state.view === 'boards') kanbanRename(input).catch(fail);
});

async function kanbanAddColumn() {
  const board = kanban.board;
  if (!board || board.columns.length >= KANBAN_MOST_COLUMNS) return;
  const before = new Set(board.columns.map((c) => c.id));
  await kanbanSaveBoard({ columns: [...board.columns, { name: 'New column' }] });
  drawKanban();
  const made = kanban.board.columns.find((c) => !before.has(c.id));
  const input = made && app.querySelector(`[data-kanban-name="${CSS.escape(made.id)}"]`);
  if (input) {
    input.closest('.kanban-col').scrollIntoView({ block: 'nearest', inline: 'nearest' });
    input.focus();
    input.select();
  }
}

// True when the columns were saved; a failure is shown and the board redrawn as it was.
async function kanbanSaveColumns(columns, moveTo = null) {
  try {
    await kanbanSaveBoard({ columns: columns.map((c) => ({ id: c.id, name: c.name })), ...(moveTo ? { move_to: moveTo } : {}) });
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
    drawKanban();
    return false;
  }
  await kanbanRefresh();
  return true;
}

function kanbanEditName(selector) {
  const input = app.querySelector(selector);
  input?.focus();
  input?.select();
}

function kanbanColumnMenu(owner) {
  const id = owner.dataset.col;
  const board = kanban.board;
  const column = kanbanColumn(id);
  if (!column) return;
  const at = board.columns.indexOf(column);
  const count = kanbanCardsIn(id).length;
  const others = board.columns.filter((c) => c.id !== id);
  const shift = (by) => {
    const columns = [...kanban.board.columns];
    const from = columns.findIndex((c) => c.id === id);
    if (!columns[from] || !columns[from + by]) return;
    [columns[from], columns[from + by]] = [columns[from + by], columns[from]];
    return kanbanSaveColumns(columns);
  };
  const remove = async (target = null) => {
    const saved = await kanbanSaveColumns(kanban.board.columns.filter((c) => c.id !== id), target);
    if (saved) toast(count ? `Deleted “${column.name}” and moved its cards to “${kanbanColumn(target)?.name || 'another column'}”` : `Deleted “${column.name}”`);
  };
  popupMenu(owner, {
    label: `Options for ${column.name}`,
    rows: [
      { text: 'Rename', icon: KANBAN_ICON.pencil, run: () => kanbanEditName(`[data-kanban-name="${CSS.escape(id)}"]`) },
      { text: 'Move left', icon: ICON.back, off: at === 0, run: () => shift(-1) },
      { text: 'Move right', icon: ICON.chevron, off: at === board.columns.length - 1, run: () => shift(1) },
      { line: true },
      {
        text: 'Delete column',
        icon: ICON.trash,
        danger: true,
        off: !others.length,
        run: remove,
        next: count ? [{ note: `Move ${count === 1 ? 'its card' : `its ${count} cards`} to` }, ...others.map((c) => ({ text: c.name, run: () => remove(c.id) }))] : null,
      },
    ],
  });
}

function kanbanBoardMenu(owner) {
  const board = kanban.board;
  if (!board) return;
  const count = kanban.cards.length;
  popupMenu(owner, {
    label: 'Board options',
    rows: [
      { text: 'Rename board', icon: KANBAN_ICON.pencil, run: () => kanbanEditName('[data-kanban-name="board"]') },
      { text: 'Add column', icon: ICON.plus, off: board.columns.length >= KANBAN_MOST_COLUMNS, run: kanbanAddColumn },
      { line: true },
      {
        text: 'Delete board',
        icon: ICON.trash,
        danger: true,
        next: [{ text: count ? `Delete the board and its ${count === 1 ? 'card' : `${count} cards`}` : 'Delete the board', icon: ICON.trash, danger: true, run: () => kanbanDeleteBoard(board) }],
      },
    ],
  });
}

async function kanbanDeleteBoard(board) {
  const gone = await kanbanWrite(`/api/boards/${enc(board.id)}`, { method: 'DELETE' });
  toast(`Deleted “${board.name}”`, {
    action: 'Undo',
    run: () => kanbanWrite('/api/boards/restore', { method: 'POST', body: gone }).then((back) => go(`#/boards/${enc(back.board.id)}`)).catch(fail),
  });
  const left = kanban.boards.filter((b) => b.id !== board.id);
  go(left.length ? `#/boards/${enc(left[0].id)}` : '#/boards', { replace: true });
}

function kanbanPriorityPick(chip) {
  const box = chip.closest('.segmented');
  for (const c of box.querySelectorAll('[data-priority]')) {
    c.classList.toggle('on', c === chip);
    c.setAttribute('aria-pressed', String(c === chip));
  }
  syncSegmented(box);
}

function kanbanComposer(columnId, { text: typed = '', start = null, end = null, focus = true, priority: chosen = '' } = {}) {
  const column = app.querySelector(`.kanban-col[data-col="${CSS.escape(columnId)}"]`);
  const button = column?.querySelector('.kanban-add-card');
  if (!button || !kanban.board) return;
  // Queued cards go to the board they were typed on, even if another is open by then.
  const boardId = kanban.board.id;
  const form = document.createElement('div');
  form.className = 'kanban-composer';
  form.dataset.composer = columnId;
  form.innerHTML = `<div class="kanban-composer-box"><textarea class="set-input" rows="2" maxlength="300" placeholder="What needs doing?" aria-label="New card title"></textarea>${segmented({ label: 'Priority', action: 'kanban-priority', key: 'priority', current: chosen, choices: KANBAN_PRIORITIES })}</div><div class="kanban-composer-row"><button type="button" class="btn sm primary" data-kanban="add">Add card</button><button type="button" class="btn sm plain" data-kanban="cancel">Cancel</button></div>`;
  button.replaceWith(form);
  const text = form.querySelector('textarea');
  // Cards save one after another, so typing the next one while the last saves loses nothing.
  let queue = Promise.resolve();
  let waiting = 0;
  const close = () => {
    if (!form.isConnected) return;
    form.replaceWith(button);
    button.focus({ preventScroll: true });
  };
  const post = async (title, priority) => {
    try {
      const card = await kanbanWrite(`/api/boards/${enc(boardId)}/cards`, { method: 'POST', body: { title, column: columnId, priority } });
      const tab = kanban.boards.find((b) => b.id === boardId);
      if (kanban.board?.id !== boardId) { if (tab) tab.cards = (tab.cards || 0) + 1; return; }
      if (kanbanCard(card.id)) return;
      kanban.cards.push(card);
      // Look the list up now: a redraw since the card was typed replaces it.
      const list = app.querySelector(`.kanban-cards[data-col="${CSS.escape(columnId)}"]`);
      list?.insertAdjacentHTML('beforeend', kanbanCardHtml(card));
      if (list) list.scrollTop = list.scrollHeight;
      kanbanCounts();
      kanbanFit();
      pulse(list?.lastElementChild, { lift: 1.02 });
    } catch (e) {
      if (e instanceof Locked) throw e;
      const open = app.querySelector(`.kanban-composer[data-composer="${CSS.escape(columnId)}"] textarea`);
      if (kanban.board?.id === boardId && open && !open.value.trim()) {
        open.value = title;
        kanbanPriorityPick(open.parentElement.querySelector(`[data-priority="${priority}"]`));
      }
      toast(e.message, { bad: true });
    }
  };
  const add = () => {
    const title = text.value.replace(/\s+/g, ' ').trim();
    if (!title) { text.focus(); return Promise.resolve(); }
    const priority = form.querySelector('.segment.on').dataset.priority;
    text.value = '';
    kanbanPriorityPick(form.querySelector('[data-priority=""]'));
    text.focus();
    waiting++;
    queue = queue.then(() => post(title, priority)).finally(() => { waiting--; });
    return queue;
  };
  form.addEventListener('mousedown', (e) => {
    if (e.target !== text) e.preventDefault();
  });
  form.addEventListener('click', (e) => {
    const chip = e.target.closest('[data-priority]');
    if (chip) kanbanPriorityPick(chip);
    const what = e.target.closest('[data-kanban]')?.dataset.kanban;
    if (what === 'add') add().catch(fail);
    else if (what === 'cancel') close();
  });
  form.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
  });
  text.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); add().catch(fail); }
  });
  form.addEventListener('focusout', (e) => {
    if (!form.contains(e.relatedTarget) && !text.value.trim() && !waiting) setTimeout(() => { if (form.isConnected && !form.contains(document.activeElement)) close(); }, 0);
  });
  text.value = typed;
  if (focus) {
    text.focus();
    if (start !== null) text.setSelectionRange(start, end);
  }
}

function kanbanStatusOptions(card) {
  return kanban.board.columns.map((c) => `<option value="${esc(c.id)}"${c.id === card.column ? ' selected' : ''}>${esc(c.name)}</option>`).join('');
}

// Keep the open card's Status in step with moves made on the board itself.
function kanbanPanelStatus() {
  const open = kanban.panel;
  const card = open && kanbanCard(open.id);
  const box = $('#kanban-panel');
  const status = box && $('#kanban-card-column', box);
  if (!card || !status) return;
  if (document.activeElement !== status) {
    status.innerHTML = kanbanStatusOptions(card);
    status.value = card.column;
  }
  const column = kanbanColumn(card.column);
  box.dataset.tone = column ? kanbanTone(column) : '';
}

async function kanbanSettlePanel() {
  try {
    return !kanban.panel || await kanban.panel.settle();
  } catch (e) {
    fail(e);
    return false;
  }
}

async function kanbanLeavePanel(refocus = true) {
  const open = kanban.panel;
  if (!await kanbanSettlePanel()) return false;
  if (kanban.panel === open) kanbanClosePanel(refocus);
  return true;
}

function kanbanClosePanel(refocus = true) {
  const open = kanban.panel;
  if (!open) return;
  open.end();
  kanban.panel = null;
  const box = $('#kanban-panel');
  if (box) { box.hidden = true; box.innerHTML = ''; }
  document.body.classList.remove('kanban-panel-on');
  const el = app.querySelector(`.kanban-card[data-card="${CSS.escape(open.id)}"]`);
  if (el) {
    el.classList.remove('open');
    el.removeAttribute('aria-current');
    if (refocus) el.focus({ preventScroll: true });
  }
}

// A card opens beside the board, like a note: edits save as you type and the board stays
// in view, so the next card is one click away.
async function kanbanCardPanel(id) {
  while (kanban.panel) {
    if (kanban.panel.id === id) { $('#kanban-card-title')?.focus(); return; }
    if (!await kanbanLeavePanel(false)) return;
  }
  const box = $('#kanban-panel');
  const card = kanbanCard(id);
  const board = kanban.board;
  if (!card || !board || !box?.isConnected) return;
  const others = kanban.boards.filter((b) => b.id !== board.id);
  const column = kanbanColumn(card.column);
  box.dataset.tone = column ? kanbanTone(column) : '';
  box.innerHTML = `<div class="kanban-sheet">
    <div class="kanban-panel-bar"><span class="kanban-panel-where"><span class="kanban-dot" aria-hidden="true"></span><span>${esc(board.name)}</span></span><span class="hint grow" id="kanban-card-saved" role="status"></span><button type="button" class="icon-btn danger" data-kanban="delete" aria-label="Delete card" title="Delete card">${ICON.trash}</button><button type="button" class="icon-btn" data-kanban="close" aria-label="Close" title="Close (Esc)">${ICON.close}</button></div>
    <textarea class="kanban-sheet-title" id="kanban-card-title" rows="1" maxlength="300" aria-label="Card title" spellcheck="true">${esc(card.title)}</textarea>
    <input class="kanban-sheet-sub" id="kanban-card-sub" maxlength="200" placeholder="Add a subheading" aria-label="Subheading" autocomplete="off" value="${esc(card.subheading || '')}">
    <div class="kanban-props">
      <div class="set-row"><span class="set-label">Status</span><label class="select-control kanban-status"><span class="kanban-dot" aria-hidden="true"></span><select id="kanban-card-column" aria-label="Status">${kanbanStatusOptions(card)}</select>${ICON.chevron}</label></div>
      <div class="set-row kanban-priorities"><span class="set-label">Priority</span>${segmented({ label: 'Priority', action: 'kanban-priority', key: 'priority', current: card.priority || '', choices: KANBAN_PRIORITIES })}</div>
      ${others.length ? `<div class="set-row"><span class="set-label">Board</span><label class="select-control"><select id="kanban-card-board" aria-label="Board"><option value="${esc(board.id)}" selected>${esc(board.name)}</option>${others.map((b) => `<option value="${esc(b.id)}">${esc(b.name)}</option>`).join('')}</select>${ICON.chevron}</label></div>` : ''}
    </div>
    <div class="prose doc kanban-description" id="kanban-card-description" role="group" aria-label="Description"></div>
    <p class="kanban-limit" id="kanban-card-problem" role="alert" hidden></p><button type="button" class="btn plain sm" id="kanban-card-discard" hidden>Discard these changes</button>
  </div>`;
  box.hidden = false;
  document.body.classList.add('kanban-panel-on');
  for (const el of app.querySelectorAll('.kanban-card.open')) { el.classList.remove('open'); el.removeAttribute('aria-current'); }
  const el = app.querySelector(`.kanban-card[data-card="${CSS.escape(id)}"]`);
  if (el) { el.classList.add('open'); el.setAttribute('aria-current', 'true'); }
  const title = $('#kanban-card-title', box);
  const sub = $('#kanban-card-sub', box);
  const description = $('#kanban-card-description', box);
  const status = $('#kanban-card-column', box);
  const saved = $('#kanban-card-saved', box);
  const problem = $('#kanban-card-problem', box);
  const tell = (text) => { problem.textContent = text; problem.hidden = !text; };
  const discard = $('#kanban-card-discard', box);
  const fit = (area) => { area.style.height = 'auto'; area.style.height = `${area.scrollHeight}px`; };
  fit(title);
  const fields = { title, subheading: sub };
  const timers = {};
  const queue = (name) => {
    if (saved.isConnected) saved.textContent = '';
    clearTimeout(timers[name]);
    timers[name] = setTimeout(() => save(name).catch(fail), KANBAN_SAVE_MS);
  };
  const over = () => Array.from(doc.source().trim()).length - KANBAN_MOST_DESCRIPTION;
  const tellOver = () => {
    const extra = over();
    tell(extra > 0 ? `Keep the description within ${KANBAN_MOST_DESCRIPTION.toLocaleString('en')} characters. It is ${extra.toLocaleString('en')} over, so this text is not saved yet.` : '');
    discard.hidden = extra <= 0;
    return extra > 0;
  };
  const doc = leoDoc.mount(description, { source: card.description || '', placeholder: 'Add details, steps or links…', onChange: () => { tellOver(); queue('description'); } });
  const valueOf = (name) => name === 'description' ? doc.source().trim() : fields[name].value.replace(/\s+/g, ' ').trim();
  const dirty = () => names.some((name) => valueOf(name) !== (card[name] || '') && (name !== 'title' || valueOf(name)));
  const running = {};
  const save = (name) => {
    clearTimeout(timers[name]);
    timers[name] = 0;
    return (running[name] = (running[name] || Promise.resolve()).catch(() => {}).then(() => write(name)));
  };
  const write = async (name) => {
    const value = valueOf(name);
    if (value === (card[name] || '')) return true;
    if (name === 'title' && !value) { title.value = card.title; fit(title); return true; }
    if (name === 'description' && tellOver()) return false;
    try {
      const done = await kanbanWrite(`/api/cards/${enc(card.id)}`, { method: 'PATCH', body: { [name]: value } });
      kanbanUpdate(card, done);
      if (saved.isConnected) saved.textContent = 'Saved';
      if (problem.isConnected) tellOver();
      return true;
    } catch (e) {
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
      if (problem.isConnected) tell(e.message);
      return false;
    }
  };
  const names = [...Object.keys(fields), 'description'];
  const flush = () => Promise.all(names.map(save));
  const settle = async () => {
    doc.stop();
    const done = await flush();
    return done.every(Boolean) && !names.some((name) => timers[name]);
  };
  const end = () => {
    doc.stop();
    for (const name of names) if (timers[name]) save(name).catch(fail);
    doc.destroy();
  };
  kanban.panel = { id, end, settle, flush, dirty };
  discard.addEventListener('click', () => {
    doc.stop();
    clearTimeout(timers.description);
    timers.description = 0;
    kanbanClosePanel();
  });
  for (const [name, field] of Object.entries(fields)) {
    field.addEventListener('input', () => {
      if (field === title) fit(field);
      queue(name);
    });
    field.addEventListener('blur', () => { if (timers[name]) save(name).catch(fail); });
  }
  description.addEventListener('focusout', () => { if (timers.description) save('description').catch(fail); });
  title.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); sub.focus(); }
  });
  sub.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); doc.editStart(); }
  });
  box.onkeydown = (e) => {
    if (e.key !== 'Escape' || e.isComposing || e.target.closest('.line-edit')) return;
    e.preventDefault();
    e.stopPropagation();
    kanbanLeavePanel();
  };
  status.addEventListener('change', () => {
    const column = kanbanColumn(status.value);
    if (column) box.dataset.tone = kanbanTone(column);
    kanbanMove(card, { column: status.value }).catch(fail);
  });
  $('.kanban-priorities', box).addEventListener('click', async (e) => {
    const chip = e.target.closest('[data-priority]');
    if (!chip || chip.classList.contains('on')) return;
    try {
      const done = await kanbanWrite(`/api/cards/${enc(card.id)}`, { method: 'PATCH', body: { priority: chip.dataset.priority } });
      kanbanUpdate(card, done);
      kanbanPriorityPick(chip);
    } catch (err) {
      if (err instanceof Locked) throw err;
      toast(err.message, { bad: true });
    }
  });
  $('#kanban-card-board', box)?.addEventListener('change', async (e) => {
    if (await kanbanLeavePanel(false)) return kanbanMoveToBoard(card, e.target.value).catch(fail);
    e.target.value = board.id;
    syncSelectMenu(e.target);
  });
  box.querySelector('[data-kanban="delete"]').addEventListener('click', () => {
    kanbanClosePanel(false);
    kanbanDeleteCard(card).catch(fail);
  });
  box.querySelector('[data-kanban="close"]').addEventListener('click', () => kanbanLeavePanel());
  title.focus();
  title.setSelectionRange(title.value.length, title.value.length);
}

// The card just before this one in its column, or null at the top.
function kanbanAbove(card) {
  const list = kanbanCardsIn(card.column);
  const at = list.findIndex((c) => c.id === card.id);
  return at > 0 ? list[at - 1].id : null;
}

async function kanbanMove(card, to) {
  try {
    const saved = await kanbanWrite(`/api/cards/${enc(card.id)}/move`, { method: 'POST', body: to });
    Object.assign(card, saved);
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
  await kanbanRefresh();
}

async function kanbanMoveToBoard(card, boardId) {
  const target = kanban.boards.find((b) => b.id === boardId);
  if (!target || boardId === card.board) return;
  const back = { board: card.board, column: card.column, after: kanbanAbove(card) };
  try {
    const saved = await kanbanWrite(`/api/cards/${enc(card.id)}/move`, { method: 'POST', body: { board: boardId } });
    kanban.cards = kanban.cards.filter((c) => c.id !== card.id);
    target.cards = (target.cards || 0) + 1;
    toast(`Moved “${card.title}” to ${target.name}`, {
      action: 'Undo',
      run: () => kanbanWrite(`/api/cards/${enc(saved.id)}/move`, { method: 'POST', body: back }).then(() => {
        target.cards = Math.max(0, (target.cards || 1) - 1);
        return kanbanRefresh();
      }).catch(fail),
    });
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
  await kanbanRefresh();
  kanbanSyncTab();
}

async function kanbanDeleteCard(card) {
  await kanbanWrite(`/api/cards/${enc(card.id)}`, { method: 'DELETE' });
  kanban.cards = kanban.cards.filter((c) => c.id !== card.id);
  drawKanban();
  toast(`Deleted “${card.title}”`, {
    action: 'Undo',
    run: () => kanbanWrite(`/api/boards/${enc(card.board)}/cards`, { method: 'POST', body: { id: card.id, column: card.column, title: card.title, subheading: card.subheading, description: card.description, priority: card.priority, rank: card.rank } }).then(kanbanRefresh).catch(fail),
  });
}

// Dragging. Mouse and pen lift past a few pixels; touch lifts after a short hold, so a
// swipe still scrolls the board. The real card (or column) moves through the page as
// the placeholder while a copy follows the pointer.
function kanbanLift() {
  const d = kanban.drag;
  if (!d || d.live || !d.item.isConnected) return;
  d.live = true;
  clearTimeout(d.timer);
  if (d.item.contains(document.activeElement) && document.activeElement.matches('input')) document.activeElement.blur();
  getSelection()?.removeAllRanges();
  document.addEventListener('touchmove', kanbanHoldStill, { passive: false });
  const rect = d.item.getBoundingClientRect();
  d.dx = d.startX - rect.left;
  d.dy = d.startY - rect.top;
  const ghost = d.item.cloneNode(true);
  for (const el of [ghost, ...ghost.querySelectorAll('*')]) {
    for (const { name } of [...el.attributes]) if (name === 'id' || name.startsWith('data-') || name === 'tabindex') el.removeAttribute(name);
  }
  ghost.classList.remove('kanban-placeholder');
  ghost.classList.add('kanban-ghost');
  ghost.dataset.tone = d.item.dataset.tone || '';
  ghost.setAttribute('aria-hidden', 'true');
  ghost.inert = true;
  ghost.style.width = `${rect.width}px`;
  if (d.kind === 'col') ghost.style.height = `${Math.min(rect.height, innerHeight * 0.7)}px`;
  document.body.appendChild(ghost);
  d.ghost = ghost;
  lift(ghost);
  d.from = { parent: d.item.parentElement, next: d.item.nextElementSibling };
  d.item.classList.add('kanban-placeholder');
  document.body.classList.add('kanban-dragging');
  kanbanFade(app.querySelector('.kanban-tabs'));
  try { app.setPointerCapture(d.pointer); } catch (e) {}
  kanbanFollow();
  d.frame = requestAnimationFrame(kanbanEdges);
}

function kanbanFollow() {
  const d = kanban.drag;
  if (!d || !d.live) return;
  const page = app.getBoundingClientRect();
  d.ghost.style.transform = `translate(${pastEdge(d.x, page.left, page.right) - d.dx}px, ${pastEdge(d.y, page.top, page.bottom) - d.dy}px)`;
  const under = document.elementFromPoint(d.x, d.y);
  for (const tab of document.querySelectorAll('.kanban-tab.drop-on')) tab.classList.remove('drop-on');
  d.tab = null;
  if (d.kind === 'card') {
    const tab = under?.closest('[data-kanban-drop]');
    if (tab && tab.dataset.kanbanDrop !== kanban.board?.id) {
      tab.classList.add('drop-on');
      d.tab = tab.dataset.kanbanDrop;
      return;
    }
    const list = under?.closest('.kanban-col')?.querySelector('.kanban-cards');
    if (!list) return;
    const before = [...list.querySelectorAll(KANBAN_OTHERS)].find((c) => {
      const r = c.getBoundingClientRect();
      return d.y < r.top - new DOMMatrix(getComputedStyle(c).transform).m42 + r.height / 2;
    }) || null;
    if (d.item.parentElement !== list || d.item.nextElementSibling !== before) flipList(app.querySelector('.kanban-cols'), KANBAN_OTHERS, (c) => c.dataset.card, () => list.insertBefore(d.item, before));
  } else {
    const row = app.querySelector('.kanban-cols');
    if (!row) return;
    const before = [...row.querySelectorAll('.kanban-col:not(.kanban-placeholder)')].find((c) => {
      const r = c.getBoundingClientRect();
      return d.x < r.left + r.width / 2;
    }) || null;
    if (d.item.nextElementSibling !== before) row.insertBefore(d.item, before);
  }
}

// Near an edge the row under the pointer, lanes or tabs, scrolls sideways and a lane up or
// down, so far columns and other boards' tabs are reachable.
function kanbanEdges() {
  const d = kanban.drag;
  if (!d || !d.live) return;
  let moved = false;
  for (const row of app.querySelectorAll('.kanban-cols, .kanban-tabs')) {
    const r = row.getBoundingClientRect();
    if (d.y < r.top || d.y > r.bottom) continue;
    const step = d.x < r.left + KANBAN_EDGE ? -14 : d.x > r.right - KANBAN_EDGE ? 14 : 0;
    const before = row.scrollLeft;
    if (step) row.scrollLeft += step;
    moved = moved || row.scrollLeft !== before;
  }
  const list = document.elementFromPoint(d.x, d.y)?.closest('.kanban-col')?.querySelector('.kanban-cards');
  if (list && d.kind === 'card') {
    const r = list.getBoundingClientRect();
    const step = d.y < r.top + 32 ? -10 : d.y > r.bottom - 32 ? 10 : 0;
    const before = list.scrollTop;
    if (step) list.scrollTop += step;
    moved = moved || list.scrollTop !== before;
  }
  if (moved) kanbanFollow();
  d.frame = requestAnimationFrame(kanbanEdges);
}

function kanbanEnd(keepGhost = false) {
  const d = kanban.drag;
  if (!d) return null;
  kanban.drag = null;
  clearTimeout(d.timer);
  cancelAnimationFrame(d.frame);
  document.removeEventListener('touchmove', kanbanHoldStill);
  if (!d.live) return null;
  d.settled = liftedRect(d.ghost);
  if (!keepGhost) d.ghost?.remove();
  d.item.classList.remove('kanban-placeholder');
  document.body.classList.remove('kanban-dragging');
  kanbanFit();
  for (const tab of document.querySelectorAll('.kanban-tab.drop-on')) tab.classList.remove('drop-on');
  try { if (app.hasPointerCapture(d.pointer)) app.releasePointerCapture(d.pointer); } catch (e) {}
  kanban.suppressClick = true;
  setTimeout(() => { kanban.suppressClick = false; }, 0);
  return d;
}

function kanbanCancel() {
  const d = kanbanEnd();
  if (d && d.from.parent.isConnected) d.from.parent.insertBefore(d.item, d.from.next && d.from.next.parentElement === d.from.parent ? d.from.next : null);
}

async function kanbanDrop() {
  const d = kanbanEnd(true);
  if (!d) return;
  let flying = false;
  try {
    if (d.tab && kanban.panel?.id === d.id && !await kanbanLeavePanel(false)) return;
    const home = d.tab ? app.querySelector(`[data-kanban-drop="${CSS.escape(d.tab)}"]`) : null;
    if (home) {
      flyInto(d.ghost, home.matches('.kanban-over') ? app.querySelector('.kanban-more') : home);
      flying = true;
    }
  } finally {
    if (!flying) d.ghost?.remove();
  }
  if (!kanban.board) return;
  if (!d.tab) {
    settleLater(`kanban-${d.kind}`, d.id, d.settled);
    settleNow(`kanban-${d.kind}`, () => d.item);
  }
  if (d.kind === 'col') {
    const order = [...app.querySelectorAll('.kanban-col')].map((c) => c.dataset.col);
    if (order.join('\n') === kanban.board.columns.map((c) => c.id).join('\n')) return;
    return kanbanSaveColumns(order.map(kanbanColumn).filter(Boolean));
  }
  const card = kanbanCard(d.id);
  if (!card) return;
  if (d.tab) {
    d.item.remove();
    return kanbanMoveToBoard(card, d.tab);
  }
  const column = d.item.parentElement?.dataset.col;
  if (!column) return kanbanRefresh();
  const after = d.item.previousElementSibling?.dataset.card || null;
  if (column === card.column && after === kanbanAbove(card)) return;
  card.column = column;
  kanbanCounts();
  d.item.focus({ preventScroll: true });
  return kanbanMove(card, { column, after });
}

// Keep the moved card focused after keyboard moves and redraws.
async function kanbanKeyMove(card, key) {
  const columns = kanban.board.columns;
  const list = kanbanCardsIn(card.column);
  const at = list.findIndex((c) => c.id === card.id);
  let to = null;
  if (key === 'ArrowUp' && at > 0) to = { column: card.column, after: at > 1 ? list[at - 2].id : null };
  else if (key === 'ArrowDown' && at < list.length - 1) to = { column: card.column, after: list[at + 1].id };
  else if (key === 'ArrowLeft' || key === 'ArrowRight') {
    const c = columns.findIndex((col) => col.id === card.column) + (key === 'ArrowLeft' ? -1 : 1);
    if (c >= 0 && c < columns.length) to = { column: columns[c].id };
  }
  if (!to) return;
  await kanbanMove(card, to);
  app.querySelector(`.kanban-card[data-card="${CSS.escape(card.id)}"]`)?.focus();
}

document.addEventListener('pointerdown', (e) => {
  if (state.view !== 'boards' || e.button !== 0 || kanban.drag || !e.isPrimary) return;
  const card = e.target.closest('.kanban-card');
  const head = card ? null : e.target.closest('.kanban-col-head');
  if (!card && !head) return;
  // A column also drags by its name, unless that name is being edited; a click still edits it.
  const onName = Boolean(head) && e.target.matches('.kanban-col-name') && document.activeElement !== e.target;
  if (!onName && e.target.closest('input, textarea, select, button, a')) return;
  const item = card || head.closest('.kanban-col');
  const touch = e.pointerType === 'touch';
  kanban.drag = { kind: card ? 'card' : 'col', item, id: card ? card.dataset.card : item.dataset.col, pointer: e.pointerId, startX: e.clientX, startY: e.clientY, x: e.clientX, y: e.clientY, live: false, timer: 0, touch };
  if (touch) kanban.drag.timer = setTimeout(kanbanLift, KANBAN_HOLD_MS);
  else if (!onName) {
    // No text selection while dragging; the card still takes focus.
    e.preventDefault();
    card?.focus({ preventScroll: true });
  }
});

document.addEventListener('pointermove', (e) => {
  const d = kanban.drag;
  if (!d || e.pointerId !== d.pointer) return;
  d.x = e.clientX;
  d.y = e.clientY;
  if (!d.live) {
    const far = Math.hypot(d.x - d.startX, d.y - d.startY);
    if (d.touch) { if (far > 8) kanbanEnd(); return; }
    if (far < 5) return;
    if (!e.buttons) { kanbanEnd(); return; }
    kanbanLift();
    return;
  }
  if (!e.buttons && e.pointerType !== 'touch') { kanbanDrop().catch(fail); return; }
  kanbanFollow();
});

document.addEventListener('pointerup', (e) => {
  const d = kanban.drag;
  if (!d || e.pointerId !== d.pointer) return;
  if (d.live) kanbanDrop().catch(fail);
  else kanbanEnd();
});

document.addEventListener('pointercancel', (e) => {
  const d = kanban.drag;
  if (!d || e.pointerId !== d.pointer) return;
  if (d.live) kanbanCancel();
  else kanbanEnd();
});

app.addEventListener('lostpointercapture', (e) => {
  if (kanban.drag?.live && e.pointerId === kanban.drag.pointer) kanbanDrop().catch(fail);
});

// A held touch is a drag: stop the page from scrolling under it. Added only while a drag
// is live, so scrolling everywhere else stays on the fast path.
function kanbanHoldStill(e) {
  if (kanban.drag?.live && e.cancelable) e.preventDefault();
}

document.addEventListener('contextmenu', (e) => {
  if (kanban.drag) e.preventDefault();
});

// A drop ends with a click on the card; it must not open it.
document.addEventListener('click', (e) => {
  if (!kanban.suppressClick) return;
  e.preventDefault();
  e.stopImmediatePropagation();
}, true);

document.addEventListener('keydown', (e) => {
  if (state.view !== 'boards') return;
  if (kanban.drag?.live && e.key === 'Escape') {
    e.preventDefault();
    e.stopImmediatePropagation();
    kanbanCancel();
    return;
  }
  if (kanban.panel && e.key === 'Escape' && !e.target.closest?.('input, textarea, select, [contenteditable], .scrim')) {
    e.preventDefault();
    e.stopImmediatePropagation();
    kanbanLeavePanel();
    return;
  }
  const name = e.target.closest?.('[data-kanban-name]');
  if (name) {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); name.blur(); }
    else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopImmediatePropagation();
      const column = kanbanColumn(name.dataset.kanbanName);
      name.value = column ? column.name : kanban.board?.name || '';
      name.blur();
    }
    return;
  }
  const el = e.target.closest?.('.kanban-card');
  if (!el || e.target !== el) return;
  const card = kanbanCard(el.dataset.card);
  if (!card) return;
  if ((e.key === 'Enter' || e.key === ' ') && !e.altKey && !e.metaKey && !e.ctrlKey) {
    e.preventDefault();
    kanbanCardPanel(card.id).catch(fail);
  } else if (e.altKey && !e.metaKey && !e.ctrlKey && ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key)) {
    e.preventDefault();
    kanbanKeyMove(card, e.key).catch(fail);
  }
});

// Leaving the page mid-drag drops nothing and leaves nothing behind.
function leaveBoards() {
  if (kanban.drag?.live) kanbanCancel();
  else kanbanEnd();
  for (const stray of document.querySelectorAll('body > .kanban-ghost, body > .kanban-card, body > .kanban-col')) stray.remove();
  kanbanClosePanel(false);
}
