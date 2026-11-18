let sourcePage;
let generatedPreview;
let sourceSearch;

function sourceAt(source, cursor) {
  const at = (v) => (v === null || v === undefined ? 'end' : v);
  return source ? `&at=${enc(source)}&passages=${at(cursor.passages)}&points=${at(cursor.points)}` : '';
}

async function fetchSources(note, query, source, cursor) {
  return api(`/api/notes/${enc(note)}/recording?q=${enc(query)}${sourceAt(source, cursor)}`);
}

async function showRecordingSources(query = '') {
  const mine = seq;
  const s = await ready();
  if (!s || !s.note.id) return;
  const page = await fetchSources(s.note.id, query);
  if (mine !== seq) return;
  if (!page.list.length || !page.page) return toast('This note was not made from a recording leo kept.');
  sourcePage = { ...page, note: s.note.id, title: s.note.title, back: [] };
  generatedPreview = null;
  drawRecordingSources(query);
}

async function loadSourcePage(source, cursor, back) {
  const page = sourcePage;
  const mine = seq;
  const query = ($('#source-query') && $('#source-query').value) || '';
  const answer = await fetchSources(page.note, query, source, cursor);
  if (mine !== seq || sourcePage !== page || !$('#source-query')) return;
  sourcePage = { ...page, ...answer, back };
  drawRecordingSources();
  const list = $('#source-passages');
  if (list) list.scrollTop = 0;
}

function sourceEarlier() {
  if (changedSources().length) return toast('Save your fixes first.');
  const back = sourcePage.back.slice();
  const cursor = back.pop();
  if (cursor) return loadSourcePage(sourcePage.page.head.id, cursor, back);
}

function sourceLater() {
  if (changedSources().length) return toast('Save your fixes first.');
  const { page } = sourcePage;
  if (page.next) return loadSourcePage(page.head.id, page.next, [...sourcePage.back, page.at]);
}

function sourcePick(source) {
  if (changedSources().length) return toast('Save your fixes first.');
  return loadSourcePage(source, { passages: 0, points: 0 }, []);
}

async function sourceMatch(source, at) {
  if (changedSources().length) return toast('Save your fixes first.');
  await loadSourcePage(source, { passages: Number(at), points: 0 }, []);
  const row = document.querySelector(`[data-passage="${at}"]`);
  if (row) row.scrollIntoView({ block: 'center' });
}

function sourceClock(secs) {
  return window.leoRecording.clock(secs);
}

function sourceRows(rows, kind) {
  return rows
    .map(([at, p]) => {
      const text = p.text || '';
      const time = kind === 'passage' ? p.start_secs : p.at_secs;
      const lines = Math.min(8, Math.max(1, Math.ceil(text.length / 80)));
      return `<div class="source-row" data-text="${esc(text.toLowerCase())}"><span class="rec-at">${sourceClock(time)}</span><textarea class="source-text" data-${kind}="${at}" rows="${lines}" spellcheck="true">${esc(text)}</textarea></div>`;
    })
    .join('');
}

function sourceDetails(source) {
  if (!source.warnings.length && !source.trace.length) return '';
  const warnings = source.warnings.map((w) => `<p class="set-note warn">${esc(w)}</p>`).join('');
  const trace = source.trace.map((t) => `<li>${esc(new Date(t.at).toLocaleTimeString())} · ${esc(t.stage)} · ${esc(t.detail)}</li>`).join('');
  return `<details class="source-more"><summary>${ICON.chevron}<span>How this recording went</span></summary>${warnings}<ul class="source-trace">${trace}</ul></details>`;
}

function sourceLabel(source, i) {
  return `Recording ${i + 1}${source.from ? ` · from “${source.from}”` : ''} · ${new Date(source.started).toLocaleString(undefined, { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })}`;
}

function sourceMatches(page) {
  if (!page.matches.length) return '';
  const index = new Map(page.list.map((s, i) => [s.id, i]));
  const many = page.list.length > 1;
  return `<div class="source-label">Found</div>${page.matches
    .map((m) => `<button type="button" class="btn plain source-match" data-action="source-match" data-source="${esc(m.source)}" data-at="${m.seq}"><span class="rec-at">${many ? `${index.get(m.source) + 1} · ` : ''}${sourceClock(m.passage.start_secs)}</span>${esc([...(m.passage.text || '')].slice(0, 120).join(''))}</button>`)
    .join('')}`;
}

function drawRecordingSources(query = '') {
  const active = document.activeElement;
  const focus = active && active.closest('.source-sheet') ? { id: active.id, passage: active.dataset.passage, point: active.dataset.point, start: active.selectionStart, end: active.selectionEnd } : null;
  query = query || ($('#source-query') && $('#source-query').value) || '';
  const typedWants = $('#regenerate-wants') ? $('#regenerate-wants').value : null;
  const page = sourcePage;
  const { head } = page.page;
  const many = page.list.length > 1;
  const current = page.list.findIndex((s) => s.id === head.id);
  const pick = many ? `<label class="select-control"><select id="source-pick" aria-label="Recording">${page.list.map((s, i) => `<option value="${esc(s.id)}"${s.id === head.id ? ' selected' : ''}>${esc(sourceLabel(s, i))}</option>`).join('')}</select>${ICON.chevron}</label>` : '';
  const points = page.page.points.length ? `<div class="source-label">Your points</div>${sourceRows(page.page.points, 'point')}` : '';
  const paging = page.back.length || page.page.next ? `<div class="source-paging">${page.back.length ? '<button type="button" class="btn plain" data-action="source-earlier">Earlier</button>' : ''}${page.page.next ? '<button type="button" class="btn plain" data-action="source-later">Later</button>' : ''}</div>` : '';
  const body = `<section class="recording-source">${many ? `<div class="source-label">${esc(sourceLabel(head, current))}</div>` : ''}${sourceRows(page.page.passages, 'passage')}${points}${sourceDetails(head)}</section>${paging}`;
  const box = sheet(`<h3>Transcript</h3>
    <p>What was said, with times. Fix a misheard word and save; writing the note again uses your fixes.</p>
    ${pick}
    <label class="field">${ICON.search}<input id="source-query" value="${esc(query)}" placeholder="Find a word, name or phrase" autocomplete="off"></label>
    <div id="source-found">${sourceMatches(page)}</div>
    <div id="source-passages" class="source-list">${body}<p class="hint source-none" hidden>Nothing on this page matches.</p></div>
    <label class="source-wants"><span>What you want from the note</span><textarea id="regenerate-wants" class="set-input" rows="2" maxlength="4000" placeholder="For example: focus on what will be on the exam, keep it short">${esc(page.wants || '')}</textarea></label>
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
  const find = () => {
    clearTimeout(sourceSearch);
    sourceSearch = setTimeout(async () => {
      const page = sourcePage;
      const mine = seq;
      const q = $('#source-query') ? $('#source-query').value : '';
      const answer = await fetchSources(page.note, q, page.page.head.id, page.page.at).catch(() => null);
      if (!answer || mine !== seq || sourcePage !== page || !$('#source-found')) return;
      sourcePage = { ...page, matches: answer.matches };
      $('#source-found').innerHTML = sourceMatches(sourcePage);
    }, 300);
  };
  $('#source-query', box).addEventListener('input', () => {
    filter();
    find();
  });
  if (many) $('#source-pick', box).addEventListener('change', (e) => sourcePick(e.target.value).catch(fail));
  filter();
  if (focus) {
    const field = focus.id ? document.getElementById(focus.id) : [...box.querySelectorAll('[data-passage],[data-point]')].find((x) => x.dataset.passage === focus.passage && x.dataset.point === focus.point);
    if (field) {
      field.focus({ preventScroll: true });
      if (focus.start !== null && typeof field.setSelectionRange === 'function') field.setSelectionRange(focus.start, focus.end);
    }
  }
}

function changedSources() {
  const { page } = sourcePage;
  const changed = (rows, kind) =>
    rows
      .map(([at, p]) => {
        const box = document.querySelector(`[data-${kind}="${at}"]`);
        return box && box.value !== p.text ? { kind, seq: at, text: box.value } : null;
      })
      .filter(Boolean);
  return [...changed(page.passages, 'passage'), ...changed(page.points, 'point')];
}

function sourceBatches(changes, most = 1000000) {
  const batches = [];
  let batch = [];
  let size = 0;
  for (const change of changes) {
    const bytes = new TextEncoder().encode(JSON.stringify(change)).length;
    if (batch.length && size + bytes > most) {
      batches.push(batch);
      batch = [];
      size = 0;
    }
    batch.push(change);
    size += bytes;
  }
  if (batch.length) batches.push(batch);
  return batches;
}

async function saveSource() {
  const page = sourcePage;
  const mine = seq;
  const changes = changedSources();
  if (!changes.length) return toast('Nothing has changed.');
  let base = page.page.head.revision;
  for (const batch of sourceBatches(changes)) {
    const saved = await api(`/api/notes/${enc(page.note)}/recording`, { method: 'PUT', body: { source: page.page.head.id, base, changes: batch } });
    base = saved.revision;
  }
  if (mine !== seq || sourcePage !== page || !$('#source-query')) return;
  await loadSourcePage(page.page.head.id, page.page.at, page.back);
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
