let sourcePage;
let generatedPreview;

async function showRecordingSources(query = '') {
  const mine = seq;
  const s = await ready();
  if (!s || !s.note.id) return;
  const page = await api(`/api/notes/${enc(s.note.id)}/recording?q=${enc(query)}`);
  if (mine !== seq) return;
  if (!page.sources.length) return toast('This note was not made from a recording leo kept.');
  sourcePage = { ...page, note: s.note.id, title: s.note.title };
  generatedPreview = null;
  drawRecordingSources(query);
}

function sourceClock(secs) {
  return window.leoRecording.clock(secs);
}

function sourceRows(source, i, kind) {
  const list = kind === 'passage' ? source.passages : source.points;
  return list
    .map((p, j) => {
      const text = p.text || '';
      const at = kind === 'passage' ? p.start_secs : p.at_secs;
      const rows = Math.min(8, Math.max(1, Math.ceil(text.length / 80)));
      return `<div class="source-row" data-text="${esc(text.toLowerCase())}"><span class="rec-at">${sourceClock(at)}</span><textarea class="source-text" data-${kind}="${i}" data-i="${j}" rows="${rows}" spellcheck="true">${esc(text)}</textarea></div>`;
    })
    .join('');
}

function sourceDetails(source) {
  if (!source.warnings.length && !source.trace.length) return '';
  const warnings = source.warnings.map((w) => `<p class="set-note warn">${esc(w)}</p>`).join('');
  const trace = source.trace.map((t) => `<li>${esc(new Date(t.at).toLocaleTimeString())} · ${esc(t.stage)} · ${esc(t.detail)}</li>`).join('');
  return `<details class="source-more"><summary>${ICON.chevron}<span>How this recording went</span></summary>${warnings}<ul class="source-trace">${trace}</ul></details>`;
}

function drawRecordingSources(query = '') {
  const active = document.activeElement;
  const focus = active && active.closest('.source-sheet') ? { id: active.id, passage: active.dataset.passage, point: active.dataset.point, i: active.dataset.i, start: active.selectionStart, end: active.selectionEnd } : null;
  query = query || ($('#source-query') && $('#source-query').value) || '';
  const typedWants = $('#regenerate-wants') ? $('#regenerate-wants').value : null;
  const page = sourcePage;
  const many = page.sources.length > 1;
  const body = page.sources
    .map((source, i) => {
      const label = many ? `<div class="source-label">Recording ${i + 1}${source.from ? ` · from “${esc(source.from)}”` : ''} · ${esc(new Date(source.started).toLocaleString(undefined, { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }))}</div>` : '';
      const points = source.points.length ? `<div class="source-label">Your points</div>${sourceRows(source, i, 'point')}` : '';
      return `<section class="recording-source">${label}${sourceRows(source, i, 'passage')}${points}${sourceDetails(source)}</section>`;
    })
    .join('');
  const wants = page.sources.map((x) => x.wants || '').filter(Boolean).join('\n');
  const box = sheet(`<h3>Transcript</h3>
    <p>What was said, with times. Fix a misheard word and save; writing the note again uses your fixes.</p>
    <label class="field">${ICON.search}<input id="source-query" value="${esc(query)}" placeholder="Find a word, name or phrase" autocomplete="off"></label>
    <div id="source-passages" class="source-list">${body}<p class="hint source-none" hidden>Nothing in the transcript matches.</p></div>
    <label class="source-wants"><span>What you want from the note</span><textarea id="regenerate-wants" class="set-input" rows="2" maxlength="4000" placeholder="For example: focus on what will be on the exam, keep it short">${esc(wants)}</textarea></label>
    ${sheetButtons(['Write the note again', 'data-action="source-regenerate"'], { cancel: 'Close', middle: '<button type="button" class="btn plain" data-action="source-save">Save fixes</button>' })}`);
  $('.sheet', box).classList.add('source-sheet');
  if (typedWants !== null) $('#regenerate-wants', box).value = typedWants;
  const grow = (area) => {
    area.style.height = 'auto';
    area.style.height = `${area.scrollHeight + 2}px`;
  };
  box.querySelectorAll('.source-text').forEach((area) => {
    grow(area);
    area.addEventListener('input', () => grow(area));
  });
  const filter = () => {
    const q = $('#source-query', box).value.toLowerCase().trim();
    let shown = 0;
    box.querySelectorAll('.source-row').forEach((row) => {
      row.hidden = Boolean(q) && !row.dataset.text.includes(q);
      if (!row.hidden) shown += 1;
    });
    $('.source-none', box).hidden = !q || shown > 0;
  };
  $('#source-query', box).addEventListener('input', filter);
  filter();
  if (focus) {
    const field = focus.id ? document.getElementById(focus.id) : [...box.querySelectorAll('[data-passage],[data-point]')].find((x) => x.dataset.passage === focus.passage && x.dataset.point === focus.point && x.dataset.i === focus.i);
    if (field) {
      field.focus({ preventScroll: true });
      if (focus.start !== null && typeof field.setSelectionRange === 'function') field.setSelectionRange(focus.start, focus.end);
    }
  }
}

function changedSources() {
  return sourcePage.sources
    .map((source, i) => {
      const passages = source.passages.map((p, j) => {
        const box = document.querySelector(`[data-passage="${i}"][data-i="${j}"]`);
        return box ? { ...p, text: box.value } : p;
      });
      const points = source.points.map((p, j) => {
        const box = document.querySelector(`[data-point="${i}"][data-i="${j}"]`);
        return box ? { ...p, text: box.value } : p;
      });
      const changed = passages.some((p, j) => p.text !== source.passages[j].text) || points.some((p, j) => p.text !== source.points[j].text);
      return changed ? { source, passages, points } : null;
    })
    .filter(Boolean);
}

async function saveSource() {
  const page = sourcePage;
  const mine = seq;
  const changes = changedSources();
  if (!changes.length) return toast('Nothing has changed.');
  for (const { source, passages, points } of changes) {
    await api(`/api/notes/${enc(page.note)}/recording`, { method: 'PUT', body: { source: source.id, base: page.versions[source.id], points, passages } });
  }
  const updated = await api(`/api/notes/${enc(page.note)}/recording`);
  if (mine !== seq || sourcePage !== page || !$('#source-query')) return;
  sourcePage = { ...page, ...updated };
  drawRecordingSources();
  toast(`Saved your fixes to “${page.title || 'Untitled'}”`);
}

async function regenerateSource() {
  if (changedSources().length) throw Object.assign(new Error('Save your fixes first, so the new note uses them.'), { shown: true });
  const page = sourcePage;
  const mine = seq;
  const button = $('[data-action="source-regenerate"]');
  button.disabled = true;
  button.textContent = 'Writing…';
  try {
    const preview = await api(`/api/notes/${enc(page.note)}/regenerate`, { method: 'POST', body: { wants: $('#regenerate-wants').value.trim() } });
    const previous = await api(`/api/notes/${enc(page.note)}`);
    if (mine !== seq || sourcePage !== page || !$('#regenerate-wants')) return;
    generatedPreview = { ...preview, note: page.note, previous };
    const box = sheet(`<h3>The note, written again</h3>
      <p>Your current note stays as it is until you use this one.</p>
      <label class="source-keep"><span class="grow">Keep the current title</span>${settingsSwitch('id="regenerate-title"', true)}</label>
      <div class="prose combine-preview">${md.render(preview.body)}</div>
      ${sheetButtons(['Use this version', 'data-action="source-apply"'], { cancel: 'Back', action: 'source-back' })}`);
    $('.sheet', box).classList.add('source-sheet');
  } catch (e) {
    if (button.isConnected) {
      button.disabled = false;
      button.textContent = 'Write the note again';
    }
    throw e;
  }
}

async function applyGenerated() {
  const p = generatedPreview;
  const mine = seq;
  const note = await api(`/api/notes/${enc(p.note)}`, { method: 'PATCH', body: { body: p.body, title: $('#regenerate-title').checked ? p.previous.title : p.title, base: p.base, written: true } });
  if (mine !== seq) return;
  closeSheet();
  await showNote(p.note);
  toast('The note was written again.', {
    action: 'Undo',
    run: async () => {
      try {
        await api(`/api/notes/${enc(p.note)}`, { method: 'PATCH', body: { title: p.previous.title, body: p.previous.body, base: note.version, written: !p.previous.writeup_pending } });
        await showNote(p.note);
        toast('The previous note is back.');
      } catch (e) {
        fail(e);
      }
    },
  });
}
