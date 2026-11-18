const KEEPING_ROWS = [
  ['trash_days', 'Trash', 'notes in the trash', 'keeping-trash', 'Notes you deleted. Once their time is up they cannot be restored.', 'note'],
  ['chat_days', 'Chats with Felix', 'chats with Felix', 'keeping-chat', 'Conversations in Felix’s chat panel. Your notes are not touched.', 'chat'],
  ['upload_days', 'Uploaded files', 'uploaded files', 'keeping-upload', 'The PDFs, slides and documents you turned into notes. The notes stay; opening the original stops working once it has gone.', 'file'],
  ['transcript_days', 'Recording transcripts', 'recording transcripts', 'keeping-transcript', 'What was said in a recording, kept beside its note so Felix can quote it. The note stays.', 'transcript'],
  ['audio_days', 'Unfinished recording audio', 'unfinished recording audio', 'keeping-audio', 'Audio from a recording that stopped before it became a note. Recover it on the Record page before its time is up.', 'recording'],
];

async function showKeeping() {
  const mine = ++seq;
  state = { view: 'keeping', dir: '' };
  chrome();
  document.title = 'Keeping · leo';
  refreshPanel(app, settingsFrame('keeping', skeleton(4, 'keeping')));
  let keep;
  try { keep = await api('/api/keep'); }
  catch (error) {
    if (mine === seq) refreshPanel(app, settingsFrame('keeping', empty(DRAWN.broken, 'Keeping could not be opened', 'leo did not answer. Open this page again in a moment.')));
    throw error;
  }
  if (mine !== seq) return;
  state.keep = keep;
  drawKeeping();
}

const keepValue = (days) => (days === null || days === undefined ? 'forever' : String(days));
const keepDays = (value) => (value === 'forever' ? null : Number(value));

function keepCard() {
  const keep = state.keep;
  if (!keep) return '';
  const busy = state.keepBusy ? ' disabled' : '';
  const select = (field, label) =>
    `<span class="select-control"><select data-keep="${field}" aria-label="Keep ${esc(label.toLowerCase())} for"${busy}>${keep.choices.map((c) => `<option value="${keepValue(c.days)}"${keepValue(c.days) === keepValue(keep[field]) ? ' selected' : ''}>${esc(c.label === 'forever' ? 'Forever' : c.label === 'a year' ? '1 year' : c.label)}</option>`).join('')}</select>${ICON.chevron}</span>`;
  return `<section class="set-card">
      ${KEEPING_ROWS.map(([field, label, , find, about]) => `<div class="set-row" data-find="${find}"><span class="keep-text">${label}<span class="hint keep-about">${about}</span></span>${select(field, label)}</div>`).join('')}
      <p class="hint keep-note">leo never deletes a note on its own. Things past their time go when you change a choice here, when leo starts, and once a day.</p>
    </section>`;
}

async function changeKeep(el) {
  if (state.keepBusy) return drawKeeping();
  const before = state.keep;
  const field = el.dataset.keep;
  const next = Object.fromEntries(KEEPING_ROWS.map(([name]) => [name, before[name]]));
  next[field] = keepDays(el.value);
  const shorter = (a, b) => b !== null && (a === null || b < a);
  if (!shorter(before[field], next[field])) return saveKeep(next);
  const [, , what, , , noun] = KEEPING_ROWS.find(([name]) => name === field);
  const period = el.selectedOptions[0].textContent.toLowerCase();
  state.keepBusy = true;
  drawKeeping();
  let count = 0;
  try { ({ count } = await api('/api/keep/preview', { method: 'POST', body: { field, days: next[field] } })); }
  finally {
    state.keepBusy = false;
    if (state.view === 'keeping') drawKeeping();
  }
  if (state.view !== 'keeping') return;
  const goes = count
    ? `${plural(count, noun)} older than that ${count === 1 ? 'is' : 'are'} deleted now, and others follow as they age. This cannot be undone, even if you choose a longer time afterwards.`
    : 'Nothing is old enough to be deleted now. From then on, each one is deleted as it reaches that age.';
  const box = confirmSheet({ title: `Keep ${what} for ${esc(period)}?`, text: esc(goes), yes: count ? `Delete ${esc(plural(count, noun))}` : `Keep for ${esc(period)}`, action: 'keep-now', danger: Boolean(count), cancel: 'keep-cancel' });
  box.returnFocus = app.querySelector(`select[data-keep="${field}"]`);
  box.querySelector('[data-action="keep-cancel"]').focus();
  state.keepPending = { next, goes: count ? plural(count, noun) : '' };
}

async function saveKeep(next, goes = '') {
  if (state.keepBusy) return;
  const changed = Object.keys(next).find((key) => state.keep?.[key] !== next[key]);
  state.keepBusy = true;
  drawKeeping();
  try {
    state.keep = await api('/api/keep', { method: 'POST', body: next });
    toast(goes ? `Saved. ${goes} deleted.` : 'Saved');
  } finally {
    state.keepBusy = false;
    if (state.view === 'keeping') drawKeeping();
  }
  pulse(app.querySelector(`select[data-keep="${changed}"]`)?.closest('.select-control'));
}

function keepDrafts() {
  const drafts = saving.drafts();
  if (!drafts.length) return '';
  const titles = drafts.slice(0, 3).map((d) => `“${d.edit.title || 'Untitled'}”`).join(', ') + (drafts.length > 3 ? ` and ${drafts.length - 3} more` : '');
  return `<section class="set-card" data-find="keeping-drafts">
      <div class="set-row"><span class="keep-text">Edits not saved yet<span class="hint keep-about">${esc(`leo could not save your latest edits to ${titles}. They are kept in this window until they save or you discard them.`)}</span></span><button type="button" class="btn plain sm" data-action="drafts-retry">Try again</button><button type="button" class="btn plain sm" data-action="drafts-discard-ask">Discard</button></div>
    </section>`;
}

async function draftsRetry() {
  await saving.retry();
  if (state.view === 'keeping') drawKeeping();
  const left = saving.drafts().length;
  toast(left ? `${plural(left, 'edit')} still could not be saved.` : 'Saved', { bad: Boolean(left) });
}

function draftsDiscardAsk(el) {
  const count = saving.drafts().length;
  if (!count) return drawKeeping();
  const box = confirmSheet({ title: `Discard ${esc(plural(count, 'unsaved edit'))}?`, text: 'Each note keeps what leo last saved. The edits kept in this window are gone for good.', yes: 'Discard', action: 'drafts-discard' });
  box.returnFocus = el;
  box.querySelector('[data-action="close"]').focus();
}

function draftsDiscard() {
  const count = saving.discardAll();
  closeSheet();
  if (state.view === 'keeping') drawKeeping();
  toast(`Discarded ${plural(count, 'unsaved edit')}`);
}

function drawKeeping() {
  const exports = [...app.querySelectorAll('[data-export]')].map(el => [el.dataset.export, el.checked]);
  refreshPanel(app, settingsFrame('keeping', `<div class="settings">
    ${keepCard()}
    ${keepDrafts()}
    <section class="set-card store-export" data-find="keeping-export">
      <div class="set-row"><span class="keep-text">Export notes<span class="hint keep-about">Your notes and tasks, plus the parts you choose below: uploads with recording transcripts, chats with margin notes, and trash. Settings and keys are never included.</span></span><a class="btn plain sm" id="export-link" download href="${exportHref()}">Download zip</a></div>
      <div class="store-export-parts">
        <label class="set-row"><span class="grow">Uploaded files</span>${settingsSwitch('data-export="uploads"', true)}</label>
        <label class="set-row"><span class="grow">Chats</span>${settingsSwitch('data-export="chats"', true)}</label>
        <label class="set-row"><span class="grow">Trash</span>${settingsSwitch('data-export="trash"', false)}</label>
      </div>
      <div class="set-row" data-find="keeping-restore"><span class="keep-text">Restore tasks<span class="hint keep-about">Brings tasks back from the tasks.json inside an exported zip. Unzip it first.</span></span><button type="button" class="btn plain sm" data-action="tasks-restore-pick">Choose tasks.json</button><input type="file" id="tasks-restore" accept=".json,application/json" hidden></div>
    </section>
  </div>`));
  for (const [part, checked] of exports) { const box = app.querySelector(`[data-export="${CSS.escape(part)}"]`); if (box) box.checked = checked; }
  $('#export-link').href = exportHref();
}

const TASKS_RESTORE_PIECE = 500;

async function tasksRestoreAsk(file) {
  let tasks;
  try {
    const data = JSON.parse(await file.text());
    tasks = Array.isArray(data) ? data : data && data.tasks;
    if (!Array.isArray(tasks) || tasks.some((task) => !task || typeof task.id !== 'string' || typeof task.title !== 'string')) throw new Error('no tasks');
  } catch (e) {
    const zip = /\.zip$/i.test(file.name);
    return toast(zip ? 'Unzip the export first, then choose the tasks.json inside it.' : `“${file.name}” is not a tasks file. Choose the tasks.json inside an exported zip.`, { bad: true });
  }
  if (!tasks.length) return toast('That file has no tasks in it.', { bad: true });
  state.tasksRestore = tasks;
  confirmSheet({ title: `Restore ${esc(plural(tasks.length, 'task'))}?`, text: [`<b>${esc(file.name)}</b>`, 'Tasks that are not here come back, including ones you deleted since. A task you already have stays as it is unless the file\'s copy is newer, which then replaces yours.'], yes: 'Restore', action: 'tasks-restore-go', danger: false });
}

async function tasksRestoreGo() {
  const tasks = state.tasksRestore;
  closeSheet();
  state.tasksRestore = null;
  if (!tasks) return;
  const done = { added: 0, kept: 0 };
  for (let at = 0; at < tasks.length; at += TASKS_RESTORE_PIECE) {
    try {
      const piece = await api('/api/tasks/import', { method: 'POST', body: { tasks: tasks.slice(at, at + TASKS_RESTORE_PIECE) } });
      done.added += piece.added;
      done.kept += piece.kept;
    } catch (e) {
      toast(`Restored ${plural(done.added, 'task')} before it stopped: ${e.message || 'leo could not reach the notes folder'}`, { bad: true });
      loadTasksWindow().catch(() => {});
      return;
    }
  }
  toast(done.added ? `Restored ${plural(done.added, 'task')}${done.kept ? ` · ${done.kept} already here` : ''}` : 'Every task in that file is already here');
  pulse($('.toast'));
  loadTasksWindow().catch(() => {});
}

function exportHref() {
  const on = (part, fallback) => {
    const box = app.querySelector(`[data-export="${part}"]`);
    return box ? box.checked : fallback;
  };
  return `/api/export?uploads=${on('uploads', true)}&chats=${on('chats', true)}&trash=${on('trash', false)}`;
}

app.addEventListener('change', (e) => {
  if (state.view !== 'keeping') return;
  if (e.target.closest('[data-export]')) $('#export-link').href = exportHref();
  const keep = e.target.closest('select[data-keep]');
  if (keep) changeKeep(keep).catch(fail);
  const restore = e.target.closest('#tasks-restore');
  if (restore && restore.files && restore.files[0]) {
    tasksRestoreAsk(restore.files[0]).catch(fail);
    restore.value = '';
  }
});
