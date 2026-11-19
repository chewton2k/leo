async function showTrash() {
  const mine = ++seq;
  const before = state.view === 'trash' ? state : null;
  state = { view: 'trash', dir: '', selecting: before ? before.selecting : false, picked: new Set() };
  chrome();
  document.title = 'Trash · leo';
  if (!before) refreshPanel(app, settingsFrame('trash', skeleton(3, 'trash')));
  const [items, keep] = await Promise.all([api('/api/trash'), api('/api/keep').catch(() => null)]);
  if (mine !== seq) return;
  state.items = items;
  state.trashDays = keep ? keep.trash_days : 30;
  if (!items.length) state.selecting = false;
  drawTrash();
  if (!before) cascade(app.querySelector('.trash-row')?.parentElement);
}

function trashCounted(items) {
  const pictures = items.filter((t) => t.kind === 'picture' || t.kind === 'unused').length;
  const files = items.filter((t) => t.kind === 'held').length;
  const notes = items.length - pictures - files;
  const parts = [];
  if (notes || (!pictures && !files)) parts.push(plural(notes, 'note'));
  if (pictures) parts.push(plural(pictures, 'picture'));
  if (files) parts.push(plural(files, 'set of files', 'sets of files'));
  return parts.length > 1 ? `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}` : parts[0];
}

function trashRevisions(items, ids, all) {
  const chosen = all ? items : items.filter((t) => ids.includes(t.id));
  return Object.fromEntries(chosen.filter((t) => t.revision).map((t) => [t.id, t.revision]));
}

function trashRow(t, sel, picked) {
  const unreadable = t.readable === false;
  const picture = t.kind === 'picture';
  const unused = t.kind === 'unused';
  const held = t.kind === 'held';
  const name = unreadable ? `${t.title}.md` : t.title;
  const box = sel ? `<input type="checkbox" class="trash-box" data-trash-pick value="${esc(t.id)}"${picked.has(t.id) ? ' checked' : ''} aria-label="Select ${esc(name)}">` : '';
  const restore = unreadable || unused ? '' : picture || held ? `<button class="btn sm plain" data-trash-keep data-id="${esc(t.id)}">Restore</button>` : `<button class="btn sm plain" data-action="restore" data-id="${esc(t.id)}">Restore</button>`;
  const actions = sel ? '' : `${restore}<button class="icon-btn trash-forget" data-action="trash-forget" data-id="${esc(t.id)}" aria-label="Delete ${esc(name)} for good" title="Delete for good">${ICON.trash}</button>`;
  const carried = t.pictures ? ` · ${plural(t.pictures, 'picture')}` : '';
  const sub = unreadable ? 'leo cannot read this file' : picture ? 'Picture · removed from its note' : unused ? 'Picture no note uses' : held ? 'Kept for a while after its note left' : `${t.directory ? '/' + t.directory : 'Top level'} · deleted ${rel(t.deleted_at)}${carried}`;
  const row = sel ? 'label' : 'div';
  return `<${row} data-refresh-key="${esc(t.id)}" class="list-row trash-row${picked.has(t.id) ? ' picked' : ''}">${box}<span class="grow"><div>${esc(name)}</div><div class="sub">${esc(sub)}</div></span>${actions}</${row}>`;
}

function drawTrash() {
  const items = state.items || [];
  if (!items.length) {
    refreshPanel(app, settingsFrame('trash', empty(DRAWN.trash, 'Nothing thrown away.', `Deleted notes wait here ${state.trashDays === null ? 'until you empty the trash' : `for ${state.trashDays === 365 ? 'a year' : plural(state.trashDays, 'day')}`}, so a mistake can be undone.`)));
    floating.innerHTML = '';
    return;
  }
  const sel = state.selecting;
  const picked = state.picked;
  const rows = items.map((t) => trashRow(t, sel, picked)).join('');
  const all = picked.size === items.length;
  const restorable = items.some((t) => picked.has(t.id) && t.readable !== false && t.kind !== 'unused');
  refreshPanel(app, settingsFrame('trash', `<div class="trash-head" data-find="trash-notes"><div class="section-title">Trash · ${trashCounted(items)}</div>
      <div class="trash-tools">${sel ? '<button class="btn sm plain" data-action="trash-select">Done</button>' : `<button class="btn sm plain" data-action="trash-select">Select</button><button class="btn sm plain danger-text" data-action="trash-empty">Empty trash</button>`}</div></div>
    <div class="panel">${rows}</div>
    <p class="hint" style="margin:14px 4px">${state.trashDays === null ? 'Deleted notes stay here until you empty the trash.' : `Deleted notes stay here for ${state.trashDays === 365 ? 'a year' : plural(state.trashDays, 'day')}, then leave on their own.`} <a href="#/settings/keeping">Change how long</a></p>`));
  refreshPanel(floating, sel
    ? `<div class="select-bar" role="toolbar" aria-label="Selected notes">
        <label class="select-all"><input type="checkbox" data-trash-all${all ? ' checked' : ''}><span>All</span></label>
        <span class="select-count">${picked.size} selected</span>
        <button class="btn sm plain" data-action="trash-restore-picked"${restorable ? '' : ' disabled'}>Restore</button>
        <button class="btn sm danger" data-action="trash-delete-picked"${picked.size ? '' : ' disabled'}>Delete</button>
      </div>`
    : '');
}

function trashAsk(ids, all) {
  const items = state.items || [];
  const names = all ? items : items.filter((t) => ids.includes(t.id));
  const what = all ? `all ${trashCounted(items)} in the trash` : names.length === 1 ? `“${names[0].title}”` : trashCounted(names);
  state.trashPending = { ids, all, revisions: trashRevisions(items, ids, all), noun: names.some((t) => t.kind && t.kind !== 'note') ? 'item' : 'note' };
  confirmSheet({ title: `Delete ${esc(what)} for good?`, text: `${all || names.length > 1 ? 'They' : 'It'} cannot be restored afterwards.`, yes: 'Delete for good', action: 'trash-delete-now' });
}

async function trashDeleteNow() {
  const pending = state.trashPending;
  closeSheet();
  if (!pending) return;
  state.trashPending = null;
  let done;
  try {
    done = await api('/api/trash/delete', { method: 'POST', body: { ids: pending.ids, all: pending.all, revisions: pending.revisions } });
  } catch (e) {
    if (e.status !== 409) throw e;
    toast(e.message);
    await showTrash();
    const still = pending.ids.filter((id) => (state.items || []).some((t) => t.id === id));
    if (pending.all ? (state.items || []).length : still.length) trashAsk(still, pending.all);
    return;
  }
  const noun = pending.noun || 'note';
  toast(done.failed ? `Deleted ${plural(done.deleted, noun)} for good. ${done.failed} could not be deleted: check the permissions of leo's notes folder.` : `Deleted ${plural(done.deleted, noun)} for good`);
  state.picked = new Set();
  await showTrash();
}

async function trashRestorePicked(chosen) {
  const ids = chosen || [...state.picked];
  if (!ids.length) return;
  const noun = (state.items || []).some((t) => ids.includes(t.id) && t.kind && t.kind !== 'note') ? 'item' : 'note';
  const done = await api('/api/trash/restore', { method: 'POST', body: { ids } });
  toast(`Restored ${plural(done.restored, noun)}${done.unreadable ? `. ${done.unreadable} could not be restored: leo cannot read the file.` : ''}${done.files_failed ? `. ${plural(done.files_failed, 'item')} could not be fully restored. Check Trash for files still there.` : ''}`);
  pulse($('.toast'));
  state.picked = new Set();
  state.selecting = false;
  await Promise.all([showTrash(), refreshSideTree()]);
}

app.addEventListener('click', (e) => {
  if (state.view !== 'trash') return;
  const keep = e.target.closest('[data-trash-keep]');
  if (keep) trashRestorePicked([keep.dataset.id]).catch(fail);
});

app.addEventListener('change', (e) => {
  if (state.view !== 'trash') return;
  const box = e.target.closest('[data-trash-pick]');
  if (!box) return;
  if (box.checked) state.picked.add(box.value);
  else state.picked.delete(box.value);
  drawTrash();
});

floating.addEventListener('change', (e) => {
  if (state.view !== 'trash' || !e.target.closest('[data-trash-all]')) return;
  state.picked = e.target.checked ? new Set((state.items || []).map((t) => t.id)) : new Set();
  drawTrash();
});
