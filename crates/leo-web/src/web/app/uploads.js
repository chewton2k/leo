const UPLOAD_ACCEPT = '.pdf,.docx,.pptx,.txt,.md,image/*';
const UPLOAD_PARALLEL = 2;
let picked = [];
let uploadPoll = 0;

async function shrink(file) {
  if (!/^image\//.test(file.type) || file.type === 'image/gif') return { name: file.name, type: file.type, blob: file };
  try {
    const bitmap = await createImageBitmap(file);
    const scale = Math.min(1, 2000 / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(bitmap.width * scale);
    canvas.height = Math.round(bitmap.height * scale);
    canvas.getContext('2d').drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/jpeg', 0.86));
    if (!blob) throw new Error('no blob');
    return { name: file.name.replace(/\.[^.]+$/, '') + '.jpg', type: 'image/jpeg', blob };
  } catch (e) {
    return { name: file.name, type: file.type, blob: file };
  }
}

function base64(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(',')[1] || '');
    reader.onerror = () => reject(new Error(`${blob.name || 'A file'} could not be read.`));
    reader.readAsDataURL(blob);
  });
}

async function pictureFor(file) {
  if (file.type === 'image/png' && file.size <= 3 * 1024 * 1024) return { name: file.name, type: file.type, blob: file };
  return shrink(file);
}

function altFor(name) {
  const stem = String(name || '').replace(/\.[^.]+$/, '').replace(/[[\]()]/g, ' ').replace(/\s+/g, ' ').trim();
  return !stem || /^image$/i.test(stem) ? 'Pasted picture' : stem;
}

const pastedName = (file, i) => (file.name && !/^image\.\w+$/i.test(file.name) ? file.name : `pasted-${i + 1}.${(file.type.split('/')[1] || 'png').replace('jpeg', 'jpg')}`);
const pastedFiles = (files, from = 0) => files.map((f, i) => (pastedName(f, from + i) === f.name ? f : new File([f], pastedName(f, from + i), { type: f.type })));

async function addPictures(s, files) {
  const list = [...files].filter((f) => /^image\//.test(f.type));
  if (!list.length || !s || !s.doc) return;
  toast(list.length === 1 ? 'Adding the picture…' : `Adding ${list.length} pictures…`);
  let added = 0;
  for (const [i, file] of list.entries()) {
    try {
      const ready = await pictureFor(file);
      const saved = await putBody(`/api/pictures/${enc(pastedName(ready, i))}`, ready.blob);
      if (state.session !== s || !s.doc) return;
      s.doc.insert(`![${altFor(file.name)}](${saved.path})`);
      added += 1;
    } catch (e) {
      fail(e);
      return;
    }
  }
  toast(added === 1 ? 'Picture added' : `${added} pictures added`);
}

function choosePictures() {
  const s = state.session;
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = 'image/*';
  input.multiple = true;
  input.addEventListener('change', () => addPictures(s, input.files));
  input.click();
}

const UPLOAD_READS = 2000;

document.addEventListener('input', (e) => {
  const area = e.target.closest ? e.target.closest('[data-reads]') : null;
  const hint = area && document.getElementById(`${area.id}-hint`);
  if (hint) hint.hidden = Array.from(area.value.trim()).length <= Number(area.dataset.reads);
});

function noteFields({ id, folders, here, reads, before = '' }) {
  const options = ['', ...folders].map((name) => `<option value="${esc(name)}"${name === here ? ' selected' : ''}>${esc(name ? folderLabel(name) + (name.includes('/') ? ` (${name})` : '') : 'Top level')}</option>`).join('');
  return `<div class="note-fields">
    <div class="field">${ICON.folder}<span class="select-control"><select id="${id}-dir" aria-label="Folder">${options}</select>${ICON.chevron}</span></div>${before}
    <p class="note-fields-optional">Optional</p>
    <label class="field">${ICON.note}<input id="${id}-title" placeholder="Title" aria-label="Title (optional)" autocomplete="off"></label>
    <label class="field field-area"><textarea id="${id}-wants" rows="2"${reads ? ` data-reads="${reads}"` : ''} aria-label="What you want from it (optional)" placeholder="What do you want from it? e.g. the main ideas, step by step"></textarea></label>${reads ? `\n    <p class="set-desc" id="${id}-wants-hint" hidden>Felix reads the first ${reads.toLocaleString('en')} characters of this.</p>` : ''}
  </div>`;
}

function dropChoice(files) {
  const list = [...files];
  const box = sheet(`<h3>What should leo do with these?</h3>
    ${sheetButtons(['Ask Felix about these', 'id="drop-ask"'], { middle: '<button type="button" class="btn plain" id="drop-make">Make notes from them</button>' })}`);
  $('#drop-make', box).addEventListener('click', () => uploadSheet(list).catch(fail));
  $('#drop-ask', box).addEventListener('click', () => {
    closeSheet();
    chat.attachFiles(list);
  });
}

// The note a sheet opened on (files dropped on an open note can be added to it).
let uploadNote = null;
const addingToNote = () => Boolean(uploadNote) && $('#upload-where')?.value === 'add';

function drawPicked() {
  const list = $('#upload-list');
  if (!list) return;
  const adding = addingToNote();
  list.innerHTML = picked
    .map((f, i) => `<div class="upload-file">${/^image\//.test(f.type) ? ICON.image : ICON.note}<span class="grow">${esc(f.name)}<span class="sub">${speechBytes(f.size)}</span></span><button class="icon-btn" data-action="upload-drop" data-i="${i}" aria-label="Remove ${esc(f.name)}">${ICON.close}</button></div>`)
    .join('');
  $('#upload-go').disabled = !picked.length;
  $('#upload-pick-label').textContent = picked.length ? 'Add more files' : 'Choose, drop or paste files';
  $('#upload-zone').classList.toggle('has-files', picked.length > 0);
  $('#upload-vision').hidden = !picked.some((f) => /^image\//.test(f.type));
  const several = picked.length > 1;
  $('#upload-together-row').hidden = !several || adding;
  const together = adding || !several || $('#upload-together').checked;
  $('#upload-title').closest('.field').hidden = !together || adding;
  $('#upload-dir').closest('.field').hidden = adding;
  $('#upload-heading').textContent = adding ? 'Add a file to this note' : 'Make a note from a file';
  $('#upload-go').textContent = adding ? 'Add to the note' : together ? 'Make the note' : `Make ${picked.length} notes`;
}

// `add`: the files were dropped on the open note, so adding to it is the
// default; from the rail or a folder a new note is. Either way, on a note the
// sheet offers both.
async function uploadSheet(files, into, { add = false } = {}) {
  const here = creationDir(into);
  const open = state.view === 'note' && state.session && state.session.note && state.session.note.id ? state.session.note : null;
  uploadNote = open ? { id: open.id, title: open.title || 'Untitled' } : null;
  const where = uploadNote ? `<div class="upload-where">${segmented({ label: 'Where it goes', choices: [['add', 'Add to this note'], ['new', 'New note']], current: add ? 'add' : 'new', field: 'id="upload-where"' })}</div>` : '';
  picked = files ? [...files] : [];
  let folders = [];
  try {
    folders = await api('/api/folders');
  } catch (e) {
    folders = [];
  }
  const kinds = [['PDF', 'pdf'], ['Word', 'doc'], ['PowerPoint', 'slides'], ['Text', 'text'], ['Photo', 'image']].map(([name, tone]) => `<span class="upload-kind" style="--tone:var(--tone-${tone})">${name}</span>`).join('');
  sheet(`<h3 id="upload-heading">Make a note from a file</h3>
    <label class="upload-drop" id="upload-zone">${ICON.upload}<span id="upload-pick-label">Choose, drop or paste files</span><span class="upload-kinds">${kinds}</span><input type="file" id="upload-input" multiple accept="${UPLOAD_ACCEPT}"></label>
    <div class="upload-list" id="upload-list"></div>
    <p class="hint upload-hint" id="upload-vision" hidden>Photos and scans need a writing AI that can see images.</p>
    ${where}
    <label class="source-keep" id="upload-together-row" hidden><span class="grow">Make one note from all of them</span>${settingsSwitch('id="upload-together"', false)}</label>
    ${noteFields({ id: 'upload', folders: folders.map((d) => d.name), here, reads: UPLOAD_READS })}
    ${sheetButtons(['Make the note', 'id="upload-go" data-action="upload-go" disabled'])}`);
  $('#upload-together').addEventListener('change', drawPicked);
  $('#upload-where')?.addEventListener('change', drawPicked);
  $('#upload-input').addEventListener('change', (e) => {
    picked.push(...e.target.files);
    e.target.value = '';
    drawPicked();
  });
  const zone = $('#upload-zone');
  zone.addEventListener('dragover', (e) => {
    e.preventDefault();
    zone.classList.add('over');
  });
  zone.addEventListener('dragleave', () => zone.classList.remove('over'));
  zone.addEventListener('drop', (e) => {
    e.preventDefault();
    zone.classList.remove('over');
    picked.push(...e.dataTransfer.files);
    drawPicked();
  });
  drawPicked();
}

async function sendFile(upload, item, ready) {
  await putBody(`/api/import/files/${enc(upload)}/${enc(item)}?name=${enc(ready.name)}`, ready.blob);
}

async function putBody(url, blob) {
  let response;
  try {
    response = await fetch(url, { method: 'PUT', credentials: 'same-origin', body: blob });
  } catch (e) {
    throw new Offline();
  }
  if (response.status === 401) throw new Locked();
  if (response.status === 204) return null;
  let data = null;
  try {
    data = await response.json();
  } catch (e) {
    data = null;
  }
  if (response.ok) return data;
  throw new Error((data && data.error) || `leo answered ${response.status}.`);
}

// Each file travels in its own request, two at a time, so nothing holds the
// whole upload; a file that fails is named and the others still go on.
async function sendFiles(upload, chosen) {
  const results = new Array(chosen.length);
  const unsent = [];
  let next = 0;
  let done = 0;
  const lane = async () => {
    while (next < chosen.length) {
      const at = next++;
      const file = chosen[at];
      uploadProgress(`Sending ${file.name}`, done, chosen.length);
      try {
        const ready = await shrink(file);
        const item = crypto.randomUUID();
        await sendFile(upload, item, ready);
        results[at] = { item, name: ready.name, type: ready.type };
      } catch (e) {
        if (e instanceof Locked || e instanceof Offline) throw e;
        unsent.push(file.name);
      }
      done += 1;
    }
  };
  await Promise.all(Array.from({ length: Math.min(UPLOAD_PARALLEL, chosen.length) }, lane));
  return { files: results.filter(Boolean), unsent };
}

const uploadClock = { since: 0, step: '', stepSince: 0, steps: [] };

function uploadProgress(text, done, total) {
  const now = Date.now();
  if (!uploadClock.since) Object.assign(uploadClock, { since: now, stepSince: now, steps: [] });
  if (uploadClock.step !== `${done}/${total}`) {
    if (uploadClock.step) uploadClock.steps = [...uploadClock.steps, (now - uploadClock.stepSince) / 1000].slice(-6);
    uploadClock.step = `${done}/${total}`;
    uploadClock.stepSince = now;
  }
  const typical = uploadClock.steps.length ? uploadClock.steps.reduce((a, b) => a + b, 0) / uploadClock.steps.length : 20;
  const creep = 1 - Math.exp(-((now - uploadClock.stepSince) / 1000) / Math.max(3, typical));
  const bar = total ? Math.round(((Math.min(done, total) + (done < total ? 0.9 * creep : 0)) / total) * 100) : Math.round(60 * creep);
  const box = $('#upload-progress');
  if (!box) return;
  box.innerHTML = `<div class="upload-step">${esc(text)} <span class="hint">· ${leoChat.tookFor(now - uploadClock.since)}</span></div><div class="upload-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.max(6, Math.min(99, bar))}"><i style="width:${Math.max(6, Math.min(99, bar))}%"></i></div>`;
}

async function uploadGo() {
  if (!picked.length) return;
  const into = addingToNote() ? uploadNote : null;
  const dir = into ? '' : $('#upload-dir').value;
  const each = !into && picked.length > 1 && !$('#upload-together').checked;
  const title = each || into ? '' : $('#upload-title').value.trim();
  const wants = $('#upload-wants').value.trim();
  const names = picked.map((f) => f.name);
  sheet(`<div class="upload-working">${felix.felix(72, 'idle think')}<h3>${into ? `Adding to “${esc(into.title)}”` : each ? `Making ${names.length} notes` : 'Making your note'}</h3><p>${esc(names.length === 1 ? names[0] : `${names.length} files`)}</p><div id="upload-progress"></div><p class="hint">This can take a minute for long files. You can close this; the note appears in the folder when it is ready.</p></div>`);
  Object.assign(uploadClock, { since: Date.now(), step: '', stepSince: Date.now(), steps: [] });
  const upload = crypto.randomUUID();
  let sent;
  try {
    sent = await sendFiles(upload, picked);
  } catch (e) {
    closeSheet();
    throw e;
  }
  if (!sent.files.length) {
    closeSheet();
    return toast(`${sent.unsent.join(', ')} could not be sent. Try again.`, { bad: true });
  }
  let started;
  try {
    started = await api('/api/import', { method: 'POST', body: { upload, directory: dir, title: title || null, wants, files: sent.files, each, into: into ? into.id : null } });
  } catch (e) {
    closeSheet();
    throw e;
  }
  if (sent.unsent.length) toast(`${sent.unsent.join(', ')} could not be sent; leo is making notes from the rest.`, { bad: true });
  picked = [];
  watchUpload(started.id, dir, into);
  checkActivity();
}

function watchUpload(id, dir, into = null) {
  clearTimeout(uploadPoll);
  uploadPoll = setTimeout(async () => {
    let job;
    try {
      job = await api(`/api/import/${enc(id)}`);
    } catch (e) {
      return watchUpload(id, dir, into);
    }
    if (job.state === 'waiting') {
      uploadProgress('Waiting for another upload to finish…', 0, 0);
      return watchUpload(id, dir, into);
    }
    if (job.state === 'working') {
      uploadProgress(job.step, job.done, job.total);
      return watchUpload(id, dir, into);
    }
    const open = $('.upload-working');
    if (job.state === 'failed') {
      if (open) {
        open.innerHTML = `${felix.felix(72, 'droop')}<h3>${into ? 'The file could not be added' : 'The note could not be made'}</h3><p class="upload-error">${esc(job.error || 'Something went wrong.')}</p>${sheetButtons(['Try again', `data-action="upload"${into ? ' data-add="yes"' : ''}`], { cancel: 'Close', middle: noAi(job.error) ? '<a class="btn plain" href="#/settings/writing">Set up writing AI</a>' : '' })}`;
      } else {
        toast(job.error || 'The upload failed.', { bad: true });
      }
      return;
    }
    if (into) {
      if (open) closeSheet();
      if (job.error) toast(job.error, { bad: true });
      await addSection(into, job.section_chars ? id : '').catch(fail);
      return;
    }
    const many = (job.notes || []).length > 1;
    if (job.error) toast(`Some files could not be made into notes. ${job.error}`, { bad: true });
    if (open) {
      closeSheet();
      go(many ? folderHash(dir) : noteHash(job.note));
    } else {
      await showLatest().catch(() => {});
      if (many) toast(`Your ${job.notes.length} notes from the upload are ready.`, { action: 'Open folder', run: () => go(folderHash(dir)) });
      else toast('Your note from the upload is ready.', { action: 'Open', run: () => go(noteHash(job.note)) });
    }
    if (many && open && state.view === 'workspace' && (state.dir || '') === dir) await showLatest().catch(() => {});
  }, 900);
}

// Puts an upload's section (named by its job, so it never travels back through
// the page) at the end of the note through the version-checked
// insert (as Add to note does), using the open editor's own version after
// saving its draft, so typing during the upload never makes a conflict copy.
async function addSection(into, job) {
  if (!job) return;
  const isHere = () => state.view === 'note' && state.session && state.session.note && state.session.note.id === into.id;
  let done = null;
  try {
    for (let attempt = 0; attempt < 2 && !done; attempt++) {
      let base = null;
      if (isHere()) {
        const s = await ready();
        base = s ? s.note.version : null;
      }
      try {
        if (!base) base = (await api(`/api/notes/${enc(into.id)}`)).version;
        done = await api(`/api/notes/${enc(into.id)}/insert`, { method: 'POST', body: { job, line: null, base } });
      } catch (e) {
        if (e.status !== 409 || attempt) throw e;
      }
    }
  } finally {
    // ready() paused the editor; drawing the note again restarts it either way.
    if (isHere()) await showNote(into.id);
  }
  toast(`Added to “${into.title}”`, {
    action: 'Undo',
    run: async () => {
      try {
        await api(`/api/notes/${enc(into.id)}`, { method: 'PATCH', body: { body: done.before, base: done.version } });
        if (isHere()) await showNote(into.id);
        toast(`Took it out of “${into.title}”`);
      } catch (e) {
        toast(e.status === 409 ? 'That note has changed since, so leo left it as it is.' : e.message, { bad: true });
      }
    },
  });
}

async function showLatest() {
  if ($('.scrim')) return;
  if (state.view === 'workspace') await loadSideFolders();
  else if (state.view === 'search') await render();
}

async function drawOriginals(note) {
  if (!note.id) return;
  let files = [];
  try {
    files = await api(`/api/notes/${enc(note.id)}/originals`);
  } catch (e) {
    return;
  }
  const row = $('.note-originals');
  if (!files.length || !row || state.view !== 'note' || state.session.note.id !== note.id) return;
  const all = files.length > 1 ? `<a class="chip original all" href="/api/notes/${enc(note.id)}/originals.zip" download>${ICON.paperclip}Download all ${files.length}</a>` : '';
  row.insertAdjacentHTML('beforeend', files.map((f) => {
    const look = viewKind(f.name) ? ` data-action="view-original" data-id="${esc(note.id)}" data-name="${esc(f.name)}" title="Open ${esc(f.name)}"` : '';
    return `<a class="chip original" href="/api/notes/${enc(note.id)}/originals/${enc(f.name)}" download="${esc(f.name)}"${look}>${ICON.paperclip}${esc(f.name)}</a>`;
  }).join('') + all);
}

function viewKind(name) {
  if (/\.pdf$/i.test(name)) return 'pdf';
  if (/\.(png|jpe?g|gif|webp)$/i.test(name)) return 'image';
  if (/\.(txt|md)$/i.test(name)) return 'text';
  return null;
}

async function viewOriginal(id, name, page = null) {
  const kind = viewKind(name);
  if (!kind) return;
  if (state.view === 'note' && state.session && state.session.note.id === id && sourceFits()) return openSource(id, name, page);
  const url = `/api/notes/${enc(id)}/originals/${enc(name)}`;
  const tab = kind === 'text' ? '' : `<a class="btn sm plain" href="${url}?view=1" target="_blank" rel="noopener">Open in a new tab</a>`;
  const links = `<a class="btn sm plain" href="${url}" download="${esc(name)}">Download</a>${tab}`;
  if (kind === 'image') {
    viewPicture({ title: name, links, body: `<img class="viewer-img" src="${url}?view=1" alt="${esc(name)}">` });
    return;
  }
  const body = kind === 'pdf'
    ? `<iframe class="viewer-frame" src="${url}?view=1${page ? `#page=${Number(page)}` : ''}" title="${esc(name)}"></iframe>`
    : '<div class="viewer-text prose">Loading…</div>';
  const scrim = sheet(`<div class="viewer-head"><b title="${esc(name)}">${esc(name)}</b><span class="viewer-tools">${links}<button class="icon-btn" data-action="close" aria-label="Close">${ICON.close}</button></span></div>${body}`);
  scrim.querySelector('.sheet').classList.add('viewer');
  state.viewerZoom = 1;
  if (kind === 'text') {
    await fillTextFile(scrim.querySelector('.viewer-text'), url, name);
  }
}

async function fillTextFile(box, url, name) {
  try {
    const response = await fetch(url, { credentials: 'same-origin' });
    if (response.status === 401) throw new Locked();
    if (!response.ok) throw new Error();
    const text = await response.text();
    if (!box.isConnected) return;
    box.innerHTML = /\.md$/i.test(name) ? md.render(text) : `<pre>${esc(text)}</pre>`;
  } catch (e) {
    if (e instanceof Locked) showLocked();
    else box.textContent = 'This file could not be shown. Download it instead.';
  }
}

function viewPicture({ title, body = '', links = '', nodes = [] }) {
  const zoom = '<button class="btn sm plain" data-action="viewer-zoom" data-step="-1" aria-label="Zoom out">−</button><button class="btn sm plain" data-action="viewer-zoom" data-step="1" aria-label="Zoom in">+</button><button class="btn sm plain" data-action="viewer-zoom" data-step="0">Fit</button>';
  const scrim = sheet(`<div class="viewer-head"><b title="${esc(title)}">${esc(title)}</b><span class="viewer-tools">${zoom}${links}<button class="icon-btn" data-action="close" aria-label="Close">${ICON.close}</button></span></div><div class="viewer-pan">${body}</div>`);
  scrim.querySelector('.sheet').classList.add('viewer');
  scrim.querySelector('.viewer-pan').append(...nodes);
  state.viewerZoom = 1;
  scrim.querySelectorAll('.viewer-img, .viewer-diagram').forEach((el) => el.addEventListener('dblclick', () => setViewerZoom(state.viewerZoom > 1 ? 1 : 2)));
  return scrim;
}

function setViewerZoom(level) {
  state.viewerZoom = Math.min(4, Math.max(0.5, level));
  document.querySelectorAll('.viewer-img, .viewer-diagram').forEach((el) => el.style.setProperty('--zoom', String(state.viewerZoom)));
}
