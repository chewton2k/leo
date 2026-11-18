const NOTE_DRAG = felix.NOTE_DRAG;
const FOLDER_DRAG = 'application/x-leo-folder';
const FILE_VIEWS = ['workspace', 'search', 'note'];
const SPRING_MS = 600;
const EDGE_PX = 32;
const dragging = { note: null, folder: null, filesTimer: 0, spring: null, armed: null };

const carries = (e, type) => Boolean(e.dataTransfer && [...e.dataTransfer.types].includes(type));
const parentOf = (path) => path.split('/').slice(0, -1).join('/');
const dropTargetOf = (el) => (el && el.closest ? el.closest('[data-action="open-folder"][data-dir], [data-action="home"], [data-drop-dir]') : null);
const dirOf = (target) => (Object.hasOwn(target.dataset, 'dropDir') ? target.dataset.dropDir : target.dataset.action === 'home' ? '' : target.dataset.dir);
const noteCardOf = (el) => (el && el.closest ? el.closest('.card[data-id][data-action="open-note"]') : null);
const armedRowOf = (el) => {
  const row = el && el.closest ? el.closest('.side-tree .side-note[data-id]') : null;
  return row && row.dataset.id === dragging.armed ? row : null;
};
const combinesWith = (e) => {
  const card = dragging.note && carries(e, NOTE_DRAG) ? noteCardOf(e.target) || armedRowOf(e.target) : null;
  return card && card.dataset.id !== dragging.note.id ? card : null;
};

function accepts(e, target) {
  const into = dirOf(target);
  if (carries(e, NOTE_DRAG)) return !dragging.note || dragging.note.dir !== into;
  if (carries(e, FOLDER_DRAG)) {
    const from = dragging.folder;
    return from !== null && into !== from && !into.startsWith(`${from}/`) && parentOf(from) !== into;
  }
  return carries(e, 'Files');
}

function marksOf(target) {
  const tree = target.closest('.side-tree');
  const dir = dirOf(target);
  if (target.closest('.side-explorer, #folders-sheet') && !dir) return { row: target.closest('.side-explorer, #folders-sheet')?.querySelector('.side-group') || target, kids: null };
  if (!tree || !dir) return { row: target, kids: null };
  const row = [...tree.querySelectorAll('.side-folder[data-drop-dir]')].find((el) => el.dataset.dropDir === dir) || target;
  return { row, kids: sideKids(dir) };
}

function markDrop(target) {
  const { row, kids } = target ? marksOf(target) : {};
  for (const el of document.querySelectorAll('.drop-ok')) if (el !== row) el.classList.remove('drop-ok');
  for (const el of document.querySelectorAll('.drop-in')) if (el !== kids) el.classList.remove('drop-in');
  if (row) row.classList.add('drop-ok');
  if (kids) kids.classList.add('drop-in');
}

function markCombine(row) {
  const armed = Boolean(row) && row.dataset.id === dragging.armed;
  for (const el of document.querySelectorAll('.combine-wait, .combine-armed')) if (el !== row) el.classList.remove('combine-wait', 'combine-armed');
  if (!row) return;
  row.style.setProperty('--spring', `${SPRING_MS}ms`);
  row.classList.toggle('combine-wait', !armed);
  row.classList.toggle('combine-armed', armed);
}

function springFolder(e) {
  const row = e.target.closest ? e.target.closest('.side-tree .side-folder[data-drop-dir]') : null;
  const note = !row && e.target.closest && dragging.note && carries(e, NOTE_DRAG) ? e.target.closest('.side-tree .side-note[data-id]') : null;
  const kind = row && (carries(e, NOTE_DRAG) || carries(e, FOLDER_DRAG)) ? 'folder' : note && note.dataset.id !== dragging.note.id ? 'note' : null;
  const key = kind === 'folder' ? row.dataset.dropDir : kind === 'note' ? note.dataset.id : null;
  if (dragging.spring && dragging.spring.kind === kind && dragging.spring.key === key) {
    markCombine(note);
    return;
  }
  clearTimeout(dragging.spring?.timer);
  dragging.spring = null;
  dragging.armed = null;
  markCombine(kind === 'note' ? note : null);
  if (kind === 'note') {
    dragging.spring = { key, kind, timer: setTimeout(() => { dragging.armed = key; markCombine(note); }, SPRING_MS) };
    return;
  }
  const dir = key;
  const from = dragging.folder;
  if (dir === null || folderShown(dir) || (from !== null && (dir === from || dir.startsWith(`${from}/`)))) return;
  dragging.spring = { key, kind, timer: setTimeout(() => { if (!folderShown(dir)) twistFolder(dir, null, false); }, SPRING_MS) };
}

function scrollTreeNear(e) {
  const tree = e.target.closest ? e.target.closest('.side-tree') : null;
  if (!tree) return;
  const box = tree.getBoundingClientRect();
  if (e.clientY < box.top + EDGE_PX) tree.scrollTop -= 10;
  else if (e.clientY > box.bottom - EDGE_PX) tree.scrollTop += 10;
}

function filesOver(on) {
  clearTimeout(dragging.filesTimer);
  document.body.classList.toggle('files-over', on);
  if (on) dragging.filesTimer = setTimeout(() => document.body.classList.remove('files-over'), 160);
}

function endDrag() {
  dragging.note = null;
  dragging.folder = null;
  clearTimeout(dragging.spring?.timer);
  dragging.spring = null;
  dragging.armed = null;
  markCombine(null);
  document.body.classList.remove('dragging');
  markDrop(null);
  filesOver(false);
}

document.addEventListener('dragstart', (e) => {
  endDrag();
  const from = e.target.closest ? e.target : null;
  const card = from && from.closest('.card[data-id][draggable="true"], .side-note[data-id][draggable="true"]');
  const folder = !card && from && from.closest('[data-action="open-folder"][data-dir][draggable="true"]');
  if (card) {
    dragging.note = { id: card.dataset.id, title: card.dataset.title || 'Untitled', dir: card.dataset.from || '' };
    e.dataTransfer.setData(NOTE_DRAG, JSON.stringify(dragging.note));
    e.dataTransfer.setData('text/plain', dragging.note.title);
    e.dataTransfer.effectAllowed = 'copyMove';
  } else if (folder) {
    dragging.folder = folder.dataset.dir;
    e.dataTransfer.setData(FOLDER_DRAG, dragging.folder);
    e.dataTransfer.setData('text/plain', folderLabel(dragging.folder));
    e.dataTransfer.effectAllowed = 'move';
  } else {
    e.preventDefault();
    return;
  }
  (card || folder).addEventListener('dragend', endDrag, { once: true });
  document.body.classList.add('dragging');
});

document.addEventListener('dragend', endDrag);
document.addEventListener('pointermove', (e) => {
  if (!e.buttons && (dragging.note || dragging.folder !== null)) endDrag();
});

document.addEventListener('dragover', (e) => {
  if (e.defaultPrevented) return;
  springFolder(e);
  scrollTreeNear(e);
  const onto = combinesWith(e);
  if (onto) {
    markDrop(onto.closest('.side-tree') ? null : onto);
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    return;
  }
  const target = dropTargetOf(e.target);
  const ok = target && accepts(e, target);
  markDrop(ok ? target : null);
  if (ok) {
    e.preventDefault();
    e.dataTransfer.dropEffect = carries(e, 'Files') ? 'copy' : 'move';
    return;
  }
  if (carries(e, 'Files') && FILE_VIEWS.includes(state.view) && !$('.scrim')) {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
    filesOver(state.view !== 'note');
  }
});

document.addEventListener('drop', (e) => {
  const onto = combinesWith(e);
  const target = dropTargetOf(e.target);
  const ok = target && accepts(e, target);
  const note = carries(e, NOTE_DRAG) ? dragging.note : null;
  const folder = carries(e, FOLDER_DRAG) ? dragging.folder : null;
  endDrag();
  if (e.defaultPrevented) return;
  if (note && onto) {
    e.preventDefault();
    combineSheet(note, { id: onto.dataset.id, title: onto.dataset.title || 'Untitled' });
    return;
  }
  if (note || folder) {
    if (!ok) return;
    e.preventDefault();
    const into = dirOf(target);
    if (note && note.dir !== into) dropNote(note, into).catch(fail);
    else if (folder !== null && folder !== into) dropFolder(folder, into).catch(fail);
    return;
  }
  const files = e.dataTransfer && e.dataTransfer.files;
  if (!files || !files.length || $('.scrim')) return;
  if (target) {
    e.preventDefault();
    uploadSheet(files, dirOf(target)).catch(fail);
  } else if (FILE_VIEWS.includes(state.view)) {
    e.preventDefault();
    if (state.view === 'note') uploadSheet(files, undefined, { add: true }).catch(fail);
    else dropChoice(files);
  }
});

function combineSheet(from, into) {
  const box = sheet(`<div class="combine">
    <h3>Combine these notes?</h3>
    <p>“${esc(from.title)}” will be merged into “${esc(into.title)}”.</p>
    <p class="hint">Heads up: the AI writes the combined note, so it may not be fully accurate.</p>
    ${sheetButtons(['Combine', 'id="combine-go"'])}
  </div>`);
  $('#combine-go', box).addEventListener('click', () => combineNow(from, into).catch(fail));
}

async function combineNow(from, into) {
  const go = $('#combine-go');
  if (go) go.disabled = true;
  let job;
  try {
    job = await api(`/api/notes/${enc(into.id)}/combine`, { method: 'POST', body: { with: from.id } });
  } catch (e) {
    const open = $('.scrim .combine');
    if (open) open.innerHTML = `<h3>The notes could not be combined</h3><p class="upload-error">${esc(e.message)}</p>${sheetButtons(null, { cancel: 'Close' })}`;
    return;
  }
  if ($('.scrim .combine')) closeSheet();
  if (job.parts > LONG_COMBINE) longCombine(job);
  else toast('Combining in the background. Its progress is in the Background panel; you’ll review it before anything changes.');
  checkActivity();
  watchCombine(job.id);
}

const LONG_COMBINE = 20;

const combinePart = (job) => `Part ${Math.min((job.done || 0) + 1, job.parts)} of ${job.parts}`;

function longCombine(job) {
  const box = sheet(`<div class="combine combine-long">
    <h3>This combine takes ${job.parts} requests to the AI</h3>
    <p>The AI works through the notes one part at a time. You can carry on meanwhile: its progress is in the Background panel, and you’ll review it before anything changes.</p>
    <p class="hint" id="combine-part" role="status">${combinePart(job)}</p>
    ${sheetButtons(['Stop combining', 'id="combine-stop"'], { cancel: 'Keep going', kind: 'danger' })}
  </div>`);
  $('#combine-stop', box).addEventListener('click', () => stopCombine(job.id).catch(fail));
}

async function stopCombine(id) {
  await api(`/api/combines/${enc(id)}`, { method: 'DELETE' });
  closeSheet();
  checkActivity();
  toast('Combining was stopped. Neither note was changed.');
}

const combinesWatched = new Set();
const combinesPaused = new Set();

function watchCombine(id) {
  if (combinesWatched.has(id)) return;
  combinesWatched.add(id);
  const look = async () => {
    if (!appVisible()) { combinesWatched.delete(id); combinesPaused.add(id); return; }
    let job;
    try {
      job = await api(`/api/combines/${enc(id)}`);
    } catch (e) {
      combinesWatched.delete(id);
      return;
    }
    const line = $('#combine-part');
    if (job.state === 'working') {
      if (line) line.textContent = combinePart(job);
      setTimeout(look, 1500);
      return;
    }
    if (line) closeSheet();
    combinesWatched.delete(id);
    checkActivity();
    if (job.state === 'ready') toast(`“${job.with_title}” and “${job.into_title}” are ready to review`, { action: 'Review', run: () => reviewCombine(id).catch(fail) });
    else toast(job.error || 'The notes could not be combined.', { bad: true });
  };
  setTimeout(look, 1500);
}

async function forgetCombine(id) {
  await api(`/api/combines/${enc(id)}`, { method: 'DELETE' }).catch(() => {});
  checkActivity();
}

async function reviewCombine(id) {
  const job = await api(`/api/combines/${enc(id)}`);
  if (job.state === 'working') { toast('Still combining. Its progress is in the Background panel.'); return; }
  if (job.state !== 'ready') {
    await forgetCombine(id);
    toast(job.error || 'The notes could not be combined.', { bad: true });
    return;
  }
  const made = job.result;
  const from = { id: job.with, title: job.with_title };
  const into = { id: job.into, title: job.into_title };
  const putBack = made.added.length
    ? `<p>leo put back ${made.added.length === 1 ? 'one thing' : `${made.added.length} things`} the AI left out (pictures, links, code, math or checkboxes), at the end under “Also in the original notes”.</p>`
    : '';
  const thin = made.kept_enough
    ? ''
    : `<p class="set-note warn">Some wording from the originals may be missing (about ${Math.round(made.kept * 100)}% of their words are here). Read it through before you save.</p>`;
  const shown = sheet(`<div class="combine"><h3>${esc(made.title)}</h3>
    <p class="hint">Everything from “${esc(made.with_title)}” and “${esc(made.title)}”, combined. Nothing changes until you save.</p>
    ${putBack}${thin}
    <div class="prose combine-preview">${md.render(made.body)}</div>
    <div class="buttons"><button class="btn plain" id="combine-discard">Discard</button><button class="btn primary" id="combine-save">Save combined note</button></div></div>`);
  $('#combine-discard', shown).addEventListener('click', () => { closeSheet(); forgetCombine(id); });
  $('#combine-save', shown).addEventListener('click', () => saveCombined(made, from, into, id).catch(fail));
}

async function saveCombined(made, from, into, id) {
  const saved = await api(`/api/notes/${enc(into.id)}/combined`, {
    method: 'POST',
    body: { with: from.id, body: made.body, base: made.base, with_base: made.with_base },
  });
  closeSheet();
  forgetCombine(id);
  await afterCombine(into.id);
  toast(`Combined into “${into.title}”`, {
    action: 'Undo',
    run: async () => {
      try {
        await api(`/api/notes/${enc(into.id)}`, { method: 'PATCH', body: { body: saved.before, base: saved.note.version } });
        await api('/api/trash/restore', { method: 'POST', body: { ids: [saved.removed] } });
        await api(`/api/notes/${enc(into.id)}/uncombine`, { method: 'POST', body: { from: saved.removed } });
        await afterCombine(into.id);
        toast('Both notes are back as they were.');
      } catch (e) {
        fail(e);
      }
    },
  });
}

async function afterCombine(id) {
  if (state.view === 'note' && state.session && state.session.note.id === id && !saving.unsaved()) await showNote(id);
  else await showLatest();
  loadSideFolders();
}

async function moveNote(id, dir) {
  return api(`/api/notes/${enc(id)}/move`, { method: 'POST', body: { directory: dir } });
}

async function dropNote(note, into) {
  if (state.session?.note.id === note.id) await ready();
  await moveNote(note.id, into);
  await afterMove(null, null, note.id);
  toast(`Moved “${note.title}” to ${folderLabel(into)}`, {
    action: 'Undo',
    run: () => moveNote(note.id, note.dir).then(() => afterMove(null, null, note.id)).catch(fail),
  });
}

async function dropFolder(from, into) {
  if (state.view === 'note' && (state.dir === from || state.dir.startsWith(`${from}/`))) await ready();
  let moved;
  try {
    moved = await api('/api/dirs/move', { method: 'POST', body: { from, into } });
  } catch (e) {
    if (e.status !== 409) throw e;
    throw new Error(`${folderLabel(into)} already has a folder named ${folderLabel(from)}. Choose another folder.`);
  }
  await afterMove(from, moved.path);
  toast(`Moved ${folderLabel(from)} into ${folderLabel(into)}`, {
    action: 'Undo',
    run: () =>
      api('/api/dirs/move', { method: 'POST', body: { from: moved.path, into: parentOf(from) } })
        .then(() => afterMove(moved.path, from))
        .catch(fail),
  });
}

async function afterMove(from, to, noteId = null) {
  const dir = state.dir || '';
  if (from) {
    moveOpenFolders(from, to);
    if (side.target === from || side.target?.startsWith(`${from}/`)) side.target = to + side.target.slice(from.length);
  }
  if (from && state.view === 'note' && (dir === from || dir.startsWith(`${from}/`))) state.dir = to + dir.slice(from.length);
  const affected = noteId === state.session?.note.id || from && (dir === from || dir.startsWith(`${from}/`));
  if (affected && state.view === 'note' && state.session?.note.id && !saving.get(state.session.key)) await showNote(state.session.note.id);
  else await showLatest();
  await loadSideFolders();
}

const PAGE_STEP = 20;

function pageWidthLive(article, px, edge = 'right') {
  const [least, most] = window.leoTheme.PAGE_RANGE;
  const room = article.closest('.note-columns')?.clientWidth || most;
  const width = Math.round(Math.max(least, Math.min(most, room, marginMostColumn(room), px)));
  document.documentElement.style.setProperty('--note-width', `${width}px`);
  const label = $('.page-width-label', article);
  label.textContent = `${width} px`;
  label.dataset.edge = edge;
  label.hidden = false;
  for (const mark of article.querySelectorAll('.page-mark')) mark.setAttribute('aria-valuenow', String(width));
  marginRedraw();
  return width;
}

function pageWidthDone(article, width) {
  $('.page-width-label', article).hidden = true;
  window.leoTheme.setPageWidth(width);
  marginRedraw();
}

document.addEventListener('pointerdown', (e) => {
  const mark = e.target.closest?.('.page-mark');
  if (!mark || e.button !== 0) return;
  e.preventDefault();
  const article = mark.closest('article.note');
  const box = article.getBoundingClientRect();
  const middle = box.left + box.width / 2;
  let width = Math.round(box.width);
  mark.setPointerCapture(e.pointerId);
  mark.classList.add('dragging');
  const move = (ev) => { width = pageWidthLive(article, 2 * Math.abs(ev.clientX - middle), mark.dataset.edge); };
  const up = () => {
    mark.removeEventListener('pointermove', move);
    mark.classList.remove('dragging');
    pageWidthDone(article, width);
  };
  mark.addEventListener('pointermove', move);
  mark.addEventListener('pointerup', up, { once: true });
  mark.addEventListener('pointercancel', up, { once: true });
});

document.addEventListener('dblclick', (e) => {
  if (!e.target.closest?.('.page-mark')) return;
  window.leoTheme.setPageWidth(null);
  marginRedraw();
});

document.addEventListener('keydown', (e) => {
  const mark = e.target.closest?.('.page-mark');
  if (!mark) return;
  const article = mark.closest('article.note');
  const now = Math.round(article.getBoundingClientRect().width);
  const grow = { ArrowRight: 1, ArrowUp: 1, ArrowLeft: -1, ArrowDown: -1 }[e.key];
  if (e.key === 'Enter' || e.key === 'Home') {
    e.preventDefault();
    window.leoTheme.setPageWidth(null);
    return marginRedraw();
  }
  if (!grow) return;
  e.preventDefault();
  const width = pageWidthLive(article, now + grow * (mark.dataset.edge === 'left' ? -1 : 1) * PAGE_STEP, mark.dataset.edge);
  pageWidthDone(article, width);
});
