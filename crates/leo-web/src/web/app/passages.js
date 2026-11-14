const ASK_PRESETS = { this: 'Respond to this passage.', draw: 'Draw this step by step.' };
const askFloat = document.createElement('div');
askFloat.className = 'float-act ask-float';
askFloat.setAttribute('role', 'toolbar');
askFloat.setAttribute('aria-label', 'Felix and this passage');
askFloat.innerHTML = `<button type="button" data-ask="this">${ICON.chat}<span>Felix this</span><kbd>${ON_MAC ? '⌘↵' : 'Ctrl+Enter'}</kbd></button><button type="button" data-ask="ask">Ask Felix</button><button type="button" data-ask="draw">Visualize</button>`;
askFloat.hidden = true;
askFloat.dataset.keepSelection = '';
askFloat.addEventListener('mousedown', (e) => e.preventDefault());
askFloat.addEventListener('click', (e) => {
  const kind = e.target.closest('[data-ask]')?.dataset.ask;
  if (kind === 'ask') openAskField();
  else if (kind) askAboutSelection(ASK_PRESETS[kind]);
});
document.body.appendChild(askFloat);

const askField = document.createElement('form');
askField.className = 'field ask-field';
askField.hidden = true;
askField.innerHTML = `${ICON.chat}<input type="text" maxlength="1000" placeholder="Ask about this passage…" aria-label="Ask Felix about this passage" autocomplete="off" enterkeyhint="send">`;
const askInput = askField.querySelector('input');
let askHeld = null;
askField.addEventListener('submit', (e) => {
  e.preventDefault();
  const held = askHeld;
  const question = askInput.value.trim();
  closeAskField();
  if (held && held.s === state.session) askAboutSelection(question, held);
});
askInput.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  e.preventDefault();
  e.stopPropagation();
  closeAskField();
});
askInput.addEventListener('blur', () => closeAskField());
document.body.appendChild(askField);

function closeAskField() {
  askHeld = null;
  askField.hidden = true;
}

function hideAskFloat() {
  askFloat.hidden = true;
  closeAskField();
}

function placeFloat(el, point) {
  const doc = $('#doc');
  if (!point || !doc) return false;
  el.hidden = false;
  const width = el.offsetWidth;
  const area = doc.getBoundingClientRect();
  const left = Math.max(area.left, Math.min(point.x - width / 2, area.right - width));
  el.style.left = `${Math.max(8, left)}px`;
  el.style.top = `${Math.min(point.y + 6, window.innerHeight - el.offsetHeight - 12)}px`;
  return true;
}

function placeAskFloat(has) {
  const s = state.view === 'note' ? state.session : null;
  if (!placeFloat(askFloat, has && s && s.doc ? s.doc.selectionBox() : null)) askFloat.hidden = true;
}

function openAskField() {
  const s = state.view === 'note' ? state.session : null;
  const picked = s && s.doc ? s.doc.selection() : null;
  if (!picked || !s.note.id || !marginBeside(s, picked)) return askAboutSelection();
  askFloat.hidden = true;
  askInput.value = '';
  if (!placeFloat(askField, s.doc.selectionBox())) return askAboutSelection();
  askHeld = { s, picked };
  askInput.focus({ preventScroll: true });
}

function askAboutSelection(question = '', held = null) {
  const s = held ? held.s : state.view === 'note' ? state.session : null;
  const picked = held ? held.picked : s && s.doc ? s.doc.selection() : null;
  hideAskFloat();
  if (!picked) return toast('Select some text in the note first.');
  if (!s.note.id) return toast('Felix can read this note once it has been saved.');
  if (marginAsk(s, picked, question)) return;
  chat.attachSelection({ note: s.note.id, title: cleanTitle(s.title.textContent) || s.note.title || 'Untitled', ...picked });
}

document.addEventListener('mousedown', (e) => {
  if (e.target.closest && e.target.closest('[data-keep-selection]')) e.preventDefault();
});

const KEEP_SELECTION = '[data-keep-selection], a, button, input, textarea, select, summary, [role="button"], [contenteditable], .pane-resize, .chat-resize, .source-grip, .cal-grip';

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
  if (state.view !== 'note' || e.key !== 'Enter' || !(ON_MAC ? e.metaKey : e.ctrlKey) || e.shiftKey || e.altKey || askFloat.hidden || e.target.closest?.('.margin-layer, .chat, .scrim, .ask-field')) return;
  e.preventDefault();
  e.stopPropagation();
  askAboutSelection(ASK_PRESETS.this);
}, true);

window.addEventListener('scroll', hideAskFloat, { passive: true, capture: true });

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
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" id="add-go">Add</button></div>`);
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
              found = await api(`/api/search?q=${enc(query)}&brief=true`);
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
      const isOpen = state.view === 'note' && state.session && state.session.note.id === target.id;
      if (isOpen) {
        const s = await ready();
        base = s ? s.note.version : null;
      }
      if (!base) base = (await api(`/api/notes/${enc(target.id)}`)).version;
      const line = pick.where === 'cursor' && caret !== null ? caret + 1 : null;
      const done = await api(`/api/notes/${enc(target.id)}/insert`, { method: 'POST', body: { text, line, base, request } });
      closeSheet();
      if (state.view === 'note' && state.session && state.session.note.id === target.id) await showNote(target.id);
      toast(`Added to “${target.title}”`, {
        action: 'Undo',
        run: async () => {
          try {
            await api(`/api/notes/${enc(target.id)}`, { method: 'PATCH', body: { body: done.before, base: done.version } });
            if (state.view === 'note' && state.session && state.session.note.id === target.id) await showNote(target.id);
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

async function drawRelated(id, mine, limit = 4) {
  const box = $('#related');
  if (!box || !id) return;
  let found;
  try {
    found = await api(`/api/notes/${enc(id)}/related?limit=${limit}`);
  } catch (e) {
    return;
  }
  if (mine !== seq || !box.isConnected) return;
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
  if (more) more.addEventListener('click', () => drawRelated(id, mine, 20));
}

const reading = { id: null, timer: 0, user: false, quietUntil: 0 };
const READ_SAVE_MS = 2000;

function readingTop() {
  const bar = $('#bar');
  return (bar ? bar.getBoundingClientRect().bottom : 0) + 8;
}

function blockText(block) {
  return (block.textContent || '').replace(/\u200b/g, '').replace(/\s+/g, ' ').trim().slice(0, 40);
}

function readingAnchor() {
  const top = readingTop();
  const first = [...document.querySelectorAll('#doc .blk')].find((b) => blockText(b) && b.getBoundingClientRect().bottom > top);
  if (!first) return null;
  return { line: Number(first.dataset.start) || 0, text: blockText(first), dy: Math.round(top - first.getBoundingClientRect().top) };
}

window.addEventListener('scroll', () => {
  if (state.view !== 'note' || !reading.id || !reading.user || Date.now() < reading.quietUntil) return;
  clearTimeout(reading.timer);
  const id = reading.id;
  reading.timer = setTimeout(() => {
    if (state.view !== 'note' || reading.id !== id) return;
    const anchor = window.scrollY < 40 ? { line: 0, text: '', dy: 0 } : readingAnchor();
    if (anchor) api(`/api/notes/${enc(id)}/reading`, { method: 'PUT', body: { anchor } }).catch(() => {});
  }, READ_SAVE_MS);
}, { passive: true });

for (const kind of ['wheel', 'touchmove', 'keydown', 'pointerdown']) {
  window.addEventListener(kind, () => {
    reading.user = true;
  }, { passive: true, capture: true });
}

async function restoreReading(id, mine) {
  clearTimeout(reading.timer);
  reading.id = id;
  reading.user = false;
  if (!id || (citedLines && citedLines.id === id)) return;
  let got;
  try {
    got = await api(`/api/notes/${enc(id)}/reading`);
  } catch (e) {
    return;
  }
  const anchor = got && got.anchor;
  if (mine !== seq || reading.id !== id || reading.user || !anchor || (!anchor.line && !anchor.text)) return;
  if (window.scrollY > 4 || (state.session && state.session.doc && state.session.doc.isEditing())) return;
  const blocks = [...document.querySelectorAll('#doc .blk')];
  const block = (anchor.text && blocks.find((b) => blockText(b) === anchor.text)) || blocks.find((b) => Number(b.dataset.start) >= Number(anchor.line));
  if (!block) return;
  reading.quietUntil = Date.now() + 1500;
  window.scrollTo(0, Math.max(0, window.scrollY + block.getBoundingClientRect().top - readingTop() + (Number(anchor.dy) || 0)));
}
