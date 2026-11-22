let sourcePage;
let generatedPreview;
let sourceSearch;
let fileAside = null;

function sourceAt(source, cursor) {
  const at = (v) => (v === null || v === undefined ? 'end' : v);
  return source ? `&at=${enc(source)}&passages=${at(cursor.passages)}&points=${at(cursor.points)}` : '';
}

async function fetchSources(note, query, source, cursor) {
  return api(`/api/notes/${enc(note)}/recording?q=${enc(query)}${sourceAt(source, cursor)}`);
}

function asideOpen() {
  return Boolean($('.note-layout.aside-open'));
}

let asideReturn = null;

function openAside(kind, facts, body) {
  const layout = $('.note-layout');
  const box = $('#note-aside');
  if (!layout || !box) return null;
  const arriving = !layout.classList.contains('aside-open');
  if (arriving) asideReturn = { focus: document.activeElement, top: window.scrollY };
  layout.classList.add('aside-open');
  layout.dataset.aside = kind;
  $('#note-aside-facts').innerHTML = facts;
  box.innerHTML = body;
  if (arriving) {
    window.scrollTo(0, 0);
    arriveOnPage(box);
  }
  return box;
}

function closeAside() {
  const layout = $('.note-layout');
  if (!layout || !layout.classList.contains('aside-open')) return;
  if (transcriptOpen() && changedSources().length) return toast('Save your fixes first.');
  layout.classList.remove('aside-open');
  delete layout.dataset.aside;
  $('#note-aside').innerHTML = '';
  $('#note-aside-facts').innerHTML = '';
  sourcePage = null;
  fileAside = null;
  if (typeof marginDraw === 'function') marginDraw();
  const back = asideReturn;
  asideReturn = null;
  if (!back) return;
  window.scrollTo(0, back.top);
  if (back.focus && back.focus.isConnected && back.focus !== document.body) back.focus({ preventScroll: true });
}

const backLink = '<button type="button" class="facts-link" data-action="note-back">Back to note</button>';

function transcriptOpen() {
  const layout = $('.note-layout');
  return Boolean(layout && layout.dataset.aside === 'transcript' && sourcePage);
}

function holdAside(id) {
  if (!asideOpen()) return null;
  const kind = $('.note-layout').dataset.aside;
  if (kind === 'file' && fileAside) return { kind, file: fileAside, back: asideReturn };
  if (kind === 'transcript' && sourcePage && sourcePage.note === id) {
    const wants = $('#regenerate-wants');
    return { kind, fixes: changedSources(), wants: wants ? wants.value : null, back: asideReturn };
  }
  return null;
}

function restoreAside(held) {
  if (!held) return;
  if (held.kind === 'file') {
    const { url, kind, name, page } = held.file;
    openFileAside(url, kind, name, page).catch(fail);
    asideReturn = held.back;
    return;
  }
  drawRecordingSources();
  asideReturn = held.back;
  held.fixes.forEach((fix) => {
    const box = document.querySelector(`[data-${fix.kind}="${fix.seq}"]`);
    if (box) box.textContent = fix.text;
  });
  if (held.wants !== null && $('#regenerate-wants')) $('#regenerate-wants').value = held.wants;
}

async function showRecordingSources(query = '') {
  if (transcriptOpen() && changedSources().length) return toast('Save your fixes first.');
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
  if (list) list.scrollIntoView({ block: 'start' });
}

async function sourceEarlier() {
  if (changedSources().length) return toast('Save your fixes first.');
  const back = sourcePage.back.slice();
  const cursor = back.pop();
  if (cursor) return loadSourcePage(sourcePage.page.head.id, cursor, back);
}

async function sourceLater() {
  if (changedSources().length) return toast('Save your fixes first.');
  const { page } = sourcePage;
  if (page.next) return loadSourcePage(page.head.id, page.next, [...sourcePage.back, page.at]);
}

async function sourcePick(source) {
  if (changedSources().length) {
    $('#source-pick').value = sourcePage.page.head.id;
    return toast('Save your fixes first.');
  }
  return loadSourcePage(source, { passages: 0, points: 0 }, []);
}

async function sourceMatch(source, at) {
  if (changedSources().length) return toast('Save your fixes first.');
  await loadSourcePage(source, { passages: Number(at), points: 0 }, Number(at) > 0 ? [{ passages: 0, points: 0 }] : []);
  const row = document.querySelector(`[data-passage="${at}"]`);
  if (row) row.scrollIntoView({ block: 'center' });
}

function sourceClock(secs) {
  return window.leoRecording.clock(secs);
}

function sourceSpan(kind, [at, p]) {
  return `<span class="source-text" contenteditable="plaintext-only" spellcheck="true" data-${kind}="${at}">${esc(p.text || '')}</span>`;
}

function sourceItems(items) {
  let run = [];
  const out = [];
  const flush = () => {
    if (run.length) out.push(`<p class="spoken">${run.join(' ')}</p>`);
    run = [];
  };
  for (const item of items) {
    if (item.passage) {
      run.push(sourceSpan('passage', item.passage));
      continue;
    }
    flush();
    if (item.point) out.push(`<p class="typed-point"><span class="typed-label">Your point · </span>${sourceSpan('point', item.point)}</p>`);
    else out.push(`<p class="echo-fold">Hidden: ${plural(item.echo.length, 'echo line')} · <button type="button" class="facts-link" data-echo-show>Show</button></p><p class="spoken echo-lines" hidden>${item.echo.map((row) => sourceSpan('passage', row)).join(' ')}</p>`);
  }
  flush();
  return out.join('');
}

function sourceRows(page) {
  const R = window.leoRecording;
  const { speakers } = page.head;
  const { meeting, groups } = R.transcriptFlow(page.passages, page.points, speakers);
  return groups.map((g) => {
    const text = esc(g.items.flatMap((i) => (i.echo ? i.echo : [i.passage || i.point])).map(([, p]) => p.text || '').join(' ').toLowerCase());
    const said = sourceItems(g.items);
    if (meeting) {
      const who = g.speaker ? `<div class="turn-who"><span class="tone-dot" style="--tone: ${R.toneOf(speakers, g.speaker)}"></span><span class="turn-name">${esc(g.speaker)}</span><span class="rec-at">${sourceClock(g.at)}</span></div>` : '';
      return `<div class="source-row turn" data-text="${text}">${who}${said}</div>`;
    }
    return `<div class="source-row minute" data-text="${text}"><span class="rec-at">${sourceClock(g.at)}</span><div class="minute-said">${said}</div></div>`;
  }).join('');
}

function speakerRow(head) {
  const R = window.leoRecording;
  const names = R.speakersOf(head.speakers);
  if (!names.length) return '';
  return `<div class="speaker-row" aria-label="Speakers">${names.map((name) => `<span class="speaker-pill"><span class="tone-dot" style="--tone: ${R.toneOf(head.speakers, name)}"></span><button type="button" class="speaker-name" data-speaker="${esc(name)}" aria-label="Rename ${esc(name)}" title="Rename ${esc(name)}">${esc(name)}</button></span>`).join('')}</div>`;
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

function sourceLength(page) {
  if (page.next || !page.passages.length) return '';
  return ` · ${sourceClock(Math.max(...page.passages.map(([, p]) => p.end_secs || 0)))}`;
}

function drawRecordingSources(query = '') {
  const active = document.activeElement;
  const focus = active && active.closest && active.closest('#note-aside') ? { id: active.id, passage: active.dataset.passage, point: active.dataset.point } : null;
  query = query || ($('#source-query') && $('#source-query').value) || '';
  const typedWants = $('#regenerate-wants') ? $('#regenerate-wants').value : null;
  const page = sourcePage;
  const { head } = page.page;
  const many = page.list.length > 1;
  const current = page.list.findIndex((s) => s.id === head.id);
  const pick = many ? `<label class="select-control"><select id="source-pick" aria-label="Recording">${page.list.map((s, i) => `<option value="${esc(s.id)}"${s.id === head.id ? ' selected' : ''}>${esc(sourceLabel(s, i))}</option>`).join('')}</select>${ICON.chevron}</label>` : '';
  const paging = page.back.length || page.page.next ? `<div class="source-paging">${page.back.length ? '<button type="button" class="btn plain" data-action="source-earlier">Earlier</button>' : ''}${page.page.next ? '<button type="button" class="btn plain" data-action="source-later">Later</button>' : ''}</div>` : '';
  const box = openAside('transcript', `Transcript${sourceLength(page.page)} · ${backLink}`, `<div class="note-transcript">
    ${pick}
    <label class="field">${ICON.search}<input id="source-query" value="${esc(query)}" placeholder="Find in transcript" aria-label="Find in transcript" autocomplete="off"></label>
    <div id="source-found">${sourceMatches(page)}</div>
    ${speakerRow(head)}
    <div id="source-passages" class="source-list transcript-flow">${sourceRows(page.page)}<p class="hint source-none" hidden>Nothing on this page matches.</p></div>
    ${paging}
    ${sourceDetails(head)}
    <label class="source-wants"><span>What you want from the note</span><textarea id="regenerate-wants" class="set-input" rows="2" data-reads="4000" placeholder="For example: focus on what will be on the exam, keep it short">${esc(page.wants || '')}</textarea></label>
    <p class="set-desc" id="regenerate-wants-hint"${Array.from((typedWants !== null ? typedWants : page.wants || '').trim()).length > 4000 ? '' : ' hidden'}>Felix reads the first 4,000 characters of this.</p>
    <p class="hint">Fix a misheard word and save; writing the note again uses your fixes.</p>
    <div class="buttons source-buttons"><button type="button" class="btn plain" data-action="source-save">Save fixes</button><button type="button" class="btn primary" data-action="source-regenerate">Write the note again</button></div>
  </div>`);
  if (!box) return;
  if (typedWants !== null) $('#regenerate-wants', box).value = typedWants;
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
  box.onkeydown = (e) => {
    if (e.key === 'Enter' && e.target.matches && e.target.matches('.source-text')) {
      e.preventDefault();
      e.target.blur();
    }
  };
  box.onclick = (e) => {
    const show = e.target.closest && e.target.closest('[data-echo-show]');
    if (show) {
      const fold = show.closest('.echo-fold');
      fold.nextElementSibling.hidden = false;
      fold.remove();
    }
    const name = e.target.closest && e.target.closest('.speaker-name');
    if (name) renameSpeakerField(name);
  };
  if (many) $('#source-pick', box).addEventListener('change', (e) => sourcePick(e.target.value).catch(fail));
  filter();
  if (focus) {
    const field = focus.id ? document.getElementById(focus.id) : [...box.querySelectorAll('[data-passage],[data-point]')].find((x) => x.dataset.passage === focus.passage && x.dataset.point === focus.point);
    if (field) field.focus({ preventScroll: true });
  }
}

function renameSpeakerField(button) {
  const from = button.dataset.speaker;
  const input = document.createElement('input');
  input.className = 'speaker-input';
  input.value = from;
  input.setAttribute('aria-label', `New name for ${from}`);
  input.size = Math.max(6, from.length + 2);
  button.replaceWith(input);
  input.focus();
  input.select();
  let done = false;
  const finish = (save) => {
    if (done) return;
    done = true;
    const to = input.value.trim();
    if (save && to && to !== from) renameSpeaker(from, to).catch((e) => { fail(e); if (input.isConnected) input.replaceWith(button); });
    else if (input.isConnected) input.replaceWith(button);
  };
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      finish(true);
    } else if (e.key === 'Escape') finish(false);
  });
  input.addEventListener('blur', () => finish(true));
}

async function renameSpeaker(from, to) {
  if (changedSources().length) throw Object.assign(new Error('Save your fixes first.'), { shown: true });
  if (window.leoRecording.NOT_SPEAKERS.includes(to)) throw Object.assign(new Error(`“${to}” names a recording, not a speaker. Choose another name.`), { shown: true });
  const page = sourcePage;
  const mine = seq;
  const { head } = page.page;
  try {
    const saved = await api(`/api/notes/${enc(page.note)}/recording`, { method: 'PUT', body: { source: head.id, base: head.revision, rename: { from, to } } });
    head.revision = saved.revision;
  } catch (e) {
    if (e.status === 409) throw new Error('This recording changed while you were renaming. Close the transcript and open it again.');
    throw e;
  }
  if (mine !== seq || sourcePage !== page || !$('#source-query')) return;
  await loadSourcePage(head.id, page.page.at, page.back);
  toast(`${from} is now ${to} in this recording.`);
}

function changedSources() {
  const { page } = sourcePage;
  const changed = (rows, kind) =>
    rows
      .map(([at, p]) => {
        const box = document.querySelector(`[data-${kind}="${at}"]`);
        const text = box ? box.textContent : null;
        return box && text !== p.text ? { kind, seq: at, text } : null;
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
  const { head } = page.page;
  try {
    for (const batch of sourceBatches(changes)) {
      const saved = await api(`/api/notes/${enc(page.note)}/recording`, { method: 'PUT', body: { source: head.id, base: head.revision, changes: batch } });
      head.revision = saved.revision;
    }
  } catch (e) {
    if (e.status === 409) throw new Error('This recording changed while you were fixing it. Your fixes are still on this page: copy them, then close the transcript and open it again.');
    throw e;
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
    button.disabled = false;
    button.textContent = 'Write the note again';
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
