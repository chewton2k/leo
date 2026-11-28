const ASK_PRESETS = { this: 'Respond to this passage.', draw: 'Draw this.' };
const askFloat = document.createElement('div');
askFloat.className = 'float-act ask-float';
askFloat.setAttribute('role', 'toolbar');
askFloat.setAttribute('aria-label', 'Felix and this passage');
askFloat.innerHTML = `<p class="ask-long"></p><button type="button" data-ask="this"><svg aria-hidden="true"><use href="#leo-mark"/></svg><span>Felix this</span><kbd>${ON_MAC ? '⌘↵' : 'Ctrl+Enter'}</kbd></button><i class="ask-sep" aria-hidden="true"></i><button type="button" data-ask="ask">Ask Felix</button><i class="ask-sep" aria-hidden="true"></i><button type="button" data-ask="draw">Visualize</button>`;
askFloat.hidden = true;
askFloat.dataset.keepSelection = '';
askFloat.addEventListener('mousedown', (e) => e.preventDefault());
askFloat.addEventListener('click', (e) => {
  const kind = e.target.closest('[data-ask]')?.dataset.ask;
  if (kind === 'ask') openAskField();
  else if (kind) askAboutSelection(ASK_PRESETS[kind]);
});
document.body.appendChild(askFloat);

function hideAskFloat() {
  askFloat.hidden = true;
}

function placeFloat(el, point) {
  const doc = inPane('#doc');
  if (!point || !doc) return false;
  el.hidden = false;
  const width = el.offsetWidth;
  const area = doc.getBoundingClientRect();
  const left = Math.max(area.left, Math.min(point.x - width / 2, area.right - width));
  el.style.left = `${Math.max(8, left)}px`;
  const above = point.top - el.offsetHeight - 8;
  el.style.top = `${above >= readingTop() ? above : Math.min(point.y + 6, window.innerHeight - el.offsetHeight - 12)}px`;
  return true;
}

function placeAskFloat(has) {
  const s = state.view === 'note' ? state.session : null;
  if (has) askFloat.querySelector('[data-ask="ask"]').title = `Ask Felix (${shortcutText(shortcutKeys('ask'))})`;
  const box = has && s && s.doc ? s.doc.selectionBox() : null;
  const long = Boolean(box && marginPage.layer && !chat.isOpen()) && Array.from(s.doc.selection()?.text || '').length > MARGIN_PASSAGE_READS;
  askFloat.toggleAttribute('data-long', long);
  if (long) askFloat.querySelector('.ask-long').textContent = `Felix reads the first ${MARGIN_PASSAGE_READS.toLocaleString('en')} characters of the selected passage.`;
  const columns = inPane('.note-columns');
  const beside = Boolean(box && columns && ['right', 'both'].includes(columns.dataset.margin) && !marginTaken(s, s.doc.selection()));
  askFloat.classList.toggle('beside', beside);
  if (!beside) {
    if (!placeFloat(askFloat, box)) askFloat.hidden = true;
    return;
  }
  askFloat.hidden = false;
  const gap = parseFloat(getComputedStyle(columns).getPropertyValue('--margin-gap')) || 24;
  const left = inPane('#doc').getBoundingClientRect().right + gap;
  askFloat.style.left = `${Math.min(left, window.innerWidth - askFloat.offsetWidth - 8)}px`;
  askFloat.style.top = `${Math.max(8, Math.min(box.top - 4, window.innerHeight - askFloat.offsetHeight - 12))}px`;
}

function openAskField() {
  const s = state.view === 'note' ? state.session : null;
  const picked = s && s.doc ? s.doc.selection() : null;
  if (!picked || !s.note.id || !marginAskFirst(s, picked)) return askAboutSelection();
  askFloat.hidden = true;
}

function askAboutSelection(question = '') {
  const s = state.view === 'note' ? state.session : null;
  const picked = s && s.doc ? s.doc.selection() : null;
  hideAskFloat();
  if (!picked) return toast('Select some text in the note first.');
  if (!s.note.id) return toast('Felix can read this note once it has been saved.');
  if (marginAsk(s, picked, question)) return;
  chat.attachSelection({ note: s.note.id, title: cleanTitle(s.title.textContent) || s.note.title || 'Untitled', ...picked });
}

document.addEventListener('mousedown', (e) => {
  if (e.target.closest && e.target.closest('[data-keep-selection]')) e.preventDefault();
});

const KEEP_SELECTION = '[data-keep-selection], a, button, input, textarea, select, summary, [role="button"], [contenteditable], .pane-resize, .chat-resize, .cal-grip';

document.addEventListener('pointerdown', (e) => {
  if (!(e.target instanceof Element) || (e.target.closest(KEEP_SELECTION) && !e.target.closest('#margin-mode, #margin-mode-menu, #margin-persona, #margin-persona-menu'))) return;
  for (let node = e.target; node; node = node.parentElement) {
    const pick = getComputedStyle(node).webkitUserSelect;
    if (pick && pick !== 'auto') {
      if (pick === 'none') {
        getSelection().removeAllRanges();
        hideAskFloat();
      }
      return;
    }
  }
});

document.addEventListener('keydown', (e) => {
  if (state.view !== 'note' || !pressed(e, 'ask')) return;
  e.preventDefault();
  askAboutSelection();
});

document.addEventListener('keydown', (e) => {
  if (state.view !== 'note' || e.key !== 'Enter' || !(ON_MAC ? e.metaKey : e.ctrlKey) || e.shiftKey || e.altKey || askFloat.hidden || e.target.closest?.('.margin-layer, .chat, .scrim')) return;
  e.preventDefault();
  e.stopPropagation();
  askAboutSelection(ASK_PRESETS.this);
}, true);

window.addEventListener('scroll', (e) => {
  const doc = inPane('#doc');
  if (!(e.target instanceof Element) || !doc || e.target.contains(doc) || doc.contains(e.target)) hideAskFloat();
}, { passive: true, capture: true });

function requestId() {
  if (window.crypto && typeof window.crypto.randomUUID === 'function') return window.crypto.randomUUID();
  return `r${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}`;
}

function addSheet({ text }) {
  const open = state.view === 'note' && state.session && state.session.note.id ? state.session : null;
  const caret = open && open.doc ? open.doc.caretLine() : null;
  const pick = { target: open ? { id: open.note.id, title: cleanTitle(open.title.textContent) || open.note.title || 'Untitled' } : null, where: 'end', searching: !open, found: [], query: '' };
  const request = requestId();
  const box = sheet(`<h3>Add to a note</h3>
    <div class="add-target" id="add-target"></div>
    <div class="add-preview prose">${md.render(text)}</div>
    <div class="add-where" id="add-where"></div>
    ${sheetButtons(['Add', 'id="add-go"'])}`);
  const targetBox = $('#add-target', box);
  const whereBox = $('#add-where', box);
  const go = $('#add-go', box);
  let looking = 0;

  const canCursor = () => Boolean(open && pick.target && pick.target.id === open.note.id && caret !== null);

  function drawWhere() {
    if (!canCursor()) pick.where = 'end';
    whereBox.innerHTML = segmented({ label: 'Where to add it', action: 'add-where', key: 'where', current: pick.where, choices: [['end', 'End of note'], ...(canCursor() ? [['cursor', 'At the cursor']] : [])] });
  }

  function drawTarget() {
    if (!pick.searching && pick.target) {
      targetBox.innerHTML = `<div class="set-row"><span class="set-label">Into</span><span class="set-value"><span class="add-title">${esc(pick.target.title)}</span><button type="button" class="btn plain sm" id="add-change">Change</button></span></div>`;
      $('#add-change', box).addEventListener('click', () => {
        pick.searching = true;
        drawTarget();
      });
    } else {
      const rows = pick.found.map((n, i) => `<button type="button" class="list-row add-found" data-i="${i}">${ICON.note}<span class="grow">${esc(n.title || 'Untitled')}${n.directory ? `<span class="sub">${esc(folderLabel(n.directory))}</span>` : ''}</span></button>`).join('');
      targetBox.innerHTML = `<input class="set-input" id="add-search" type="search" placeholder="Find the note to add it to" autocomplete="off" value="${esc(pick.query)}">
        <div class="add-results">${rows || `<p class="hint">${pick.query.trim() ? 'No notes match.' : 'Type part of a note’s title.'}</p>`}</div>`;
      const input = $('#add-search', box);
      input.focus();
      input.setSelectionRange(input.value.length, input.value.length);
      input.addEventListener('input', () => {
        pick.query = input.value;
        const mine = ++looking;
        setTimeout(async () => {
          if (mine !== looking) return;
          const query = pick.query.trim();
          let found = [];
          if (query) {
            try {
              found = await api(`/api/search?q=${enc(query)}&brief=true&limit=8`);
            } catch (e) {
              found = [];
            }
          }
          if (mine !== looking) return;
          pick.found = (found || []).slice(0, 8);
          drawTarget();
        }, 180);
      });
      for (const row of targetBox.querySelectorAll('.add-found')) {
        row.addEventListener('click', () => {
          const n = pick.found[Number(row.dataset.i)];
          pick.target = { id: n.id, title: n.title || 'Untitled' };
          pick.searching = false;
          drawTarget();
          drawWhere();
        });
      }
    }
    go.disabled = !pick.target;
  }

  whereBox.addEventListener('click', (e) => {
    const chip = e.target.closest('[data-where]');
    if (!chip) return;
    pick.where = chip.dataset.where;
    drawWhere();
  });

  go.addEventListener('click', async () => {
    if (!pick.target || go.disabled) return;
    go.disabled = true;
    const target = pick.target;
    try {
      let base = null;
      const isOpen = paneWith(target.id);
      if (isOpen) {
        const s = await ready(isOpen.s);
        base = s ? s.note.version : null;
      }
      if (!base) base = (await api(`/api/notes/${enc(target.id)}`)).version;
      const line = pick.where === 'cursor' && caret !== null ? caret + 1 : null;
      const done = await api(`/api/notes/${enc(target.id)}/insert`, { method: 'POST', body: { text, line, base, request } });
      closeSheet();
      if (paneWith(target.id)) await showNote(target.id, { pane: paneWith(target.id) });
      toast(`Added to “${target.title}”`, {
        action: 'Undo',
        run: async () => {
          try {
            await api(`/api/notes/${enc(target.id)}`, { method: 'PATCH', body: { body: done.before, base: done.version } });
            if (paneWith(target.id)) await showNote(target.id, { pane: paneWith(target.id) });
            toast(`Took it out of “${target.title}”`);
          } catch (e) {
            toast(e.status === 409 ? 'That note has changed since, so leo left it as it is.' : e.message, { bad: true });
          }
        },
      });
    } catch (e) {
      go.disabled = false;
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
    }
  });

  drawTarget();
  drawWhere();
}

async function drawRelated(id, current, p = here(), limit = 4) {
  const box = inPane('#related', p);
  if (!box || !id) return;
  let found;
  try {
    found = await api(`/api/notes/${enc(id)}/related?limit=${limit}`);
  } catch (e) {
    return;
  }
  if (!current() || !box.isConnected) return;
  const notes = (found && found.notes) || [];
  if (!notes.length) {
    box.hidden = true;
    box.innerHTML = '';
    return;
  }
  box.hidden = false;
  const rows = notes.map((n) => `<a class="list-row related-row" href="${noteHash(n.id)}">${ICON.note}<span class="grow"><span class="related-title">${esc(n.title || 'Untitled')}</span>${n.why ? `<span class="sub">${esc(n.why)}</span>` : ''}</span></a>`).join('');
  box.innerHTML = `<h2 class="related-head">Related notes</h2><div class="related-list">${rows}</div>${found.more ? '<button type="button" class="btn plain sm related-more">More</button>' : ''}`;
  const more = box.querySelector('.related-more');
  if (more) more.addEventListener('click', () => drawRelated(id, current, p, 20));
}

const readingFresh = () => ({ id: null, timer: 0, user: false, quietUntil: 0 });
const reading = readingFresh();
panes[0].reading = reading;
const READ_SAVE_MS = 2000;

function readingTop() {
  const bar = $('#bar');
  return (bar ? bar.getBoundingClientRect().bottom : 0) + 8;
}

function blockText(block) {
  return (block.textContent || '').replace(/\u200b/g, '').replace(/\s+/g, ' ').trim().slice(0, 40);
}

function readingAnchor(p) {
  const top = readingTop();
  const first = [...p.el.querySelectorAll('#doc .blk')].find((b) => blockText(b) && b.getBoundingClientRect().bottom > top);
  if (!first) return null;
  return { line: Number(first.dataset.start) || 0, text: blockText(first), dy: Math.round(top - first.getBoundingClientRect().top) };
}

function readingScrolled(p) {
  const at = p.reading;
  if (state.view !== 'note' || !at.id || !at.user || Date.now() < at.quietUntil || asideOpen(p)) return;
  clearTimeout(at.timer);
  const id = at.id;
  at.timer = setTimeout(() => {
    if (state.view !== 'note' || at.id !== id || asideOpen(p)) return;
    const anchor = paneScroll(p).top < 40 ? { line: 0, text: '', dy: 0 } : readingAnchor(p);
    if (anchor) api(`/api/notes/${enc(id)}/reading`, { method: 'PUT', body: { anchor } }).catch(() => {});
  }, READ_SAVE_MS);
}

window.addEventListener('scroll', () => readingScrolled(panes[0]), { passive: true });

for (const kind of ['wheel', 'touchmove', 'keydown', 'pointerdown']) {
  window.addEventListener(kind, () => {
    for (const p of panes) p.reading.user = true;
  }, { passive: true, capture: true });
}

async function restoreReading(id, mine, p = panes[0]) {
  const at = p.reading;
  clearTimeout(at.timer);
  at.id = id;
  at.user = false;
  if (!id || (citedLines && citedLines.id === id)) return;
  let got;
  try {
    got = await api(`/api/notes/${enc(id)}/reading`);
  } catch (e) {
    return;
  }
  const anchor = got && got.anchor;
  const { box, top } = paneScroll(p);
  if (mine !== (p === panes[0] ? seq : p.seq) || at.id !== id || at.user || !anchor || (!anchor.line && !anchor.text)) return;
  if (top > 4 || p.s?.doc?.isEditing()) return;
  const blocks = [...p.el.querySelectorAll('#doc .blk')];
  const block = (anchor.text && blocks.find((b) => blockText(b) === anchor.text)) || blocks.find((b) => Number(b.dataset.start) >= Number(anchor.line));
  if (!block) return;
  at.quietUntil = Date.now() + 1500;
  box.scrollTo(0, Math.max(0, top + block.getBoundingClientRect().top - readingTop() + (Number(anchor.dy) || 0)));
}
