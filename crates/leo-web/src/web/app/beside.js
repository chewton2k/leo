const besidePane = { el: $('#beside'), s: null, seq: 0, margin: marginFresh($('#beside')), reading: readingFresh() };
panes.push(besidePane);
besidePane.el.addEventListener('scroll', () => readingScrolled(besidePane), { passive: true });
const NOTE_PAGE = /^#\/(n|new)(\/|$)/;

function besideRoom() {
  const body = getComputedStyle(document.body);
  return document.documentElement.clientWidth - (parseFloat(body.paddingLeft) || 0) - (parseFloat(body.paddingRight) || 0);
}

const besideGrip = document.createElement('div');
besideGrip.className = 'pane-resize beside-resize';
besideGrip.tabIndex = 0;
besideGrip.hidden = true;
besideGrip.title = 'Drag to resize the notes; double-click for half and half';
besideGrip.setAttribute('role', 'separator');
besideGrip.setAttribute('aria-orientation', 'vertical');
besideGrip.setAttribute('aria-label', 'Note panes width');
besideGrip.setAttribute('aria-controls', 'beside');
document.body.appendChild(besideGrip);
const besideResize = window.leoPaneResize.create({
  handle: besideGrip,
  key: 'leo-beside-width',
  docked: 'right',
  measure: () => besidePane.el.getBoundingClientRect().width,
  limits: () => [SIDE_PAGE_ROOM, Math.max(SIDE_PAGE_ROOM, besideRoom() - SIDE_PAGE_ROOM)],
  apply: (width) => document.body.style.setProperty('--beside-w', `${width ?? Math.round(besideRoom() / 2)}px`),
  enabled: () => document.body.classList.contains('split'),
  onChange: () => window.dispatchEvent(new Event('resize')),
});

function usePane(p) {
  if (!p.s && p !== panes[0]) return;
  const was = state.session;
  state.session = p.s;
  marginPage = p.margin;
  if (p.s && was !== p.s && state.view === 'note') noteContext(p.s.note);
  for (const q of panes) q.el.classList.toggle('pane-on', split() && q === p);
  document.body.classList.toggle('beside-on', p === besidePane);
}

for (const p of panes) for (const type of ['pointerdown', 'focusin']) p.el.addEventListener(type, () => { if (split()) usePane(p); }, true);

function besideClose() {
  const s = besidePane.s;
  besidePane.seq++;
  if (s) {
    withPane(besidePane, marginLeave);
    s.doc.destroy();
  }
  besidePane.s = null;
  besidePane.el.innerHTML = '';
  besidePane.el.hidden = true;
  besideGrip.hidden = true;
  document.body.classList.remove('split', 'beside-on');
  app.classList.remove('pane-on');
  if (marginPage === besidePane.margin || state.session === s) usePane(panes[0]);
  pageBarLater();
  drawSide();
  window.dispatchEvent(new Event('resize'));
}

async function besideShut() {
  const s = besidePane.s;
  if (!s) return true;
  s.doc.stop();
  try {
    await flush(s);
  } catch (error) {
    if (besidePane.s !== s) return true;
    toast(error.message || 'leo could not save this note.', { bad: true, ...(s.persisted ? { action: 'Close anyway', run: () => { if (besidePane.s === s) besideClose(); } } : {}) });
    return false;
  }
  if (besidePane.s === s) besideClose();
  return true;
}

function besideLeave(hash) {
  return NOTE_PAGE.test(hash || '') ? true : besideShut();
}

function besideTake(id) {
  const s = besidePane.s;
  if (!s || s.note.id !== id) return null;
  s.doc.stop();
  besideClose();
  return { fresh: s.note, draft: s };
}

function paneGone(p, home = '#/') {
  if (p !== panes[0]) return besideClose();
  leaveMain();
  go(split() ? noteHash(besidePane.s.note.id) : home, { replace: true });
}

function paneClose(p) {
  if (p === besidePane) return besideShut();
  if (split()) return go(noteHash(besidePane.s.note.id), { replace: true });
}

async function besideOpen(id, into = besidePane) {
  const held = paneWith(id);
  if (held) {
    usePane(held);
    if (held !== into) toast('That note is already open in the other pane.');
    return;
  }
  if (into !== besidePane || state.view !== 'note' || !panes[0].s) return go(noteHash(id));
  if (!besideRoomy()) return toast('Not enough room for two notes');
  if (split()) {
    besidePane.s.doc.stop();
    try { await flush(besidePane.s); } catch (error) { return fail(error); }
  }
  document.body.classList.add('split');
  besidePane.el.hidden = false;
  besideGrip.hidden = false;
  besideResize.fit();
  try {
    await showNote(id, { pane: besidePane });
  } catch (error) {
    if (!besidePane.s) besideClose();
    throw error;
  }
  if (!besidePane.s) return;
  usePane(besidePane);
  pageBarLater();
  drawSide();
}

function besideRoomy() {
  if (sideWide() && !side.mini && besideRoom() < SPLIT_ROOM) {
    side.mini = true;
    document.body.classList.add('side-mini');
    drawSide();
  }
  return besideRoom() >= SPLIT_ROOM;
}

function besideFits() {
  const roomy = !split() || besideRoomy();
  besideResize.fit();
  if (roomy) return;
  const id = besidePane.s.note.id;
  besideShut().then((closed) => { if (closed) toast('Not enough room for two notes', id ? { action: 'Open', run: () => go(noteHash(id)) } : {}); }).catch(fail);
}

addEventListener('resize', besideFits);
